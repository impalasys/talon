// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use base64::engine::general_purpose;
use serde_json::Number;
use std::collections::HashSet;
use std::io::Read;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    OnceLock,
};
use std::time::Instant;

pub(super) fn register(registry: &mut ToolRegistry, spec: &manifests::AgentSpec) {
    if has_capability_action(spec, "code", "run") {
        registry.register_builtin(
            super::RUN_PYTHON_CODE_TOOL,
            "Run Python code in Monty, a restricted interpreter for agent-written code. Monty runs in an isolated subprocess of the current Talon binary; set TALON_MONTY_BIN to use an external runtime instead. Host filesystem, environment, and network access are not available; declared Talon files/artifacts may be mounted under /talon/input and outputs written under /talon/output are persisted as session artifacts. Code may call Talon native tools with talon_tool(name, args), except run_python_code itself.",
            json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Python code to execute. The value of the final expression is returned when present." },
                    "inputs": {
                        "type": "object",
                        "description": "Optional JSON object exposed as Python globals before execution.",
                        "additionalProperties": true
                    },
                    "mounts": {
                        "type": "array",
                        "description": "Optional Talon file:// or artifact:// handles to materialize read-only under /talon/input.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "uri": { "type": "string", "description": "file:// or artifact:// URI to mount." },
                                "mount_path": { "type": "string", "description": "Relative path under /talon/input. Defaults to a safe name derived from the URI." }
                            },
                            "required": ["uri"]
                        }
                    },
                    "persist_outputs": {
                        "type": "boolean",
                        "description": "Whether files written under /talon/output should be persisted as session artifacts. Defaults to true."
                    },
                    "timeout_ms": { "type": "integer", "description": "Maximum execution time in milliseconds. Defaults to 1000 and is capped at 30000." },
                    "memory_bytes": { "type": "integer", "description": "Approximate heap memory cap in bytes. Defaults to 16777216 and is capped at 134217728." }
                },
                "required": ["code"]
            }),
        );
    }
}

pub(super) async fn execute(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    name: &str,
    args: &Value,
) -> Result<Option<ToolOutput>> {
    if name != super::RUN_PYTHON_CODE_TOOL {
        return Ok(None);
    }
    require_capability(spec, "code", "run")?;
    run_python_code_tool(
        cp,
        current_namespace,
        current_agent,
        current_session,
        spec,
        args,
    )
    .await
    .map(ToolOutput::text)
    .map(Some)
}

const TALON_TOOL_FUNCTION: &str = "talon_tool";
const CODE_INPUT_MOUNT: &str = "/talon/input";
const CODE_OUTPUT_MOUNT: &str = "/talon/output";
const MAX_CODE_MOUNTS: usize = 25;
const MAX_CODE_INPUT_BYTES: u64 = 25 * 1024 * 1024;
const MAX_CODE_OUTPUT_FILES: usize = 100;
const MAX_CODE_OUTPUT_BYTES: u64 = 25 * 1024 * 1024;
const MAX_CODE_OUTPUT_ENTRIES: usize = 100;
const MAX_CODE_STDOUT_BYTES: usize = 1024 * 1024;
const MAX_CODE_STDERR_BYTES: usize = 1024 * 1024;
const CODE_CAPTURE_OVERHEAD_BYTES: usize = MAX_CODE_STDOUT_BYTES + MAX_CODE_STDERR_BYTES;
const DEFAULT_CODE_MAX_CONCURRENT_RUNS: usize = 1;
const DEFAULT_CODE_MEMORY_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_CODE_QUEUE_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CODE_MAX_QUEUED_RUNS: usize = 8;
const DEFAULT_CODE_MAX_TOOL_CALLS: usize = 16;
const CODE_RUNTIME_OVERHEAD_BYTES: usize = 64 * 1024 * 1024;
const MAX_CODE_CONCURRENT_RUNS: usize = 1_024;
const MAX_CODE_MEMORY_BUDGET_BYTES: usize = 1024 * 1024 * 1024;
const MAX_CODE_QUEUED_RUNS: usize = 10_000;

static CODE_EXECUTION_LIMITER: OnceLock<CodeExecutionLimiter> = OnceLock::new();

struct CodeMountBundle {
    _temp_dir: tempfile::TempDir,
    output_dir: PathBuf,
    mounts: Vec<monty_pool::MountSpec>,
    mounted_inputs: Vec<Value>,
}

struct CodeRunContext {
    deadline: Instant,
    tool_calls: Arc<AtomicUsize>,
    max_tool_calls: usize,
}

#[derive(Clone)]
struct CodeOutputCapture {
    stdout: String,
    stderr: String,
    stdout_bytes: usize,
    stderr_bytes: usize,
    truncated_stdout: bool,
    truncated_stderr: bool,
}

impl CodeOutputCapture {
    fn new() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 0,
            truncated_stdout: false,
            truncated_stderr: false,
        }
    }

    fn push(&mut self, stream: monty_types::PrintStream, text: &str) {
        let (output, used, limit, truncated) = match stream {
            monty_types::PrintStream::Stdout => (
                &mut self.stdout,
                &mut self.stdout_bytes,
                MAX_CODE_STDOUT_BYTES,
                &mut self.truncated_stdout,
            ),
            monty_types::PrintStream::Stderr => (
                &mut self.stderr,
                &mut self.stderr_bytes,
                MAX_CODE_STDERR_BYTES,
                &mut self.truncated_stderr,
            ),
        };
        let remaining = limit.saturating_sub(*used);
        if remaining == 0 {
            *truncated = true;
            return;
        }
        let mut end = text.len().min(remaining);
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&text[..end]);
        *used += end;
        if end < text.len() {
            *truncated = true;
        }
    }

    fn truncated(&self) -> bool {
        self.truncated_stdout || self.truncated_stderr
    }

    fn finish(mut self) -> (String, String) {
        if self.truncated_stdout {
            self.stdout.push_str("\n...[stdout truncated at 1 MiB]");
        }
        if self.truncated_stderr {
            self.stderr.push_str("\n...[stderr truncated at 1 MiB]");
        }
        (self.stdout, self.stderr)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeExecutionConfig {
    max_concurrent_runs: usize,
    memory_budget_bytes: usize,
    queue_timeout: Duration,
    max_queued_runs: usize,
    max_tool_calls: usize,
}

impl CodeExecutionConfig {
    fn from_env() -> Self {
        Self::from_getter(|name| std::env::var(name).ok())
    }

    fn from_getter<F>(mut get: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        Self {
            max_concurrent_runs: env_positive_usize(
                &mut get,
                "TALON_CODE_MAX_CONCURRENT_RUNS",
                DEFAULT_CODE_MAX_CONCURRENT_RUNS,
            )
            .min(MAX_CODE_CONCURRENT_RUNS),
            memory_budget_bytes: env_positive_usize(
                &mut get,
                "TALON_CODE_MEMORY_BUDGET_BYTES",
                DEFAULT_CODE_MEMORY_BUDGET_BYTES,
            )
            .min(MAX_CODE_MEMORY_BUDGET_BYTES),
            queue_timeout: Duration::from_millis(env_positive_u64(
                &mut get,
                "TALON_CODE_QUEUE_TIMEOUT_MS",
                DEFAULT_CODE_QUEUE_TIMEOUT_MS,
            )),
            max_queued_runs: env_usize(
                &mut get,
                "TALON_CODE_MAX_QUEUED_RUNS",
                DEFAULT_CODE_MAX_QUEUED_RUNS,
            )
            .min(MAX_CODE_QUEUED_RUNS),
            max_tool_calls: env_positive_usize(
                &mut get,
                "TALON_CODE_MAX_TOOL_CALLS",
                DEFAULT_CODE_MAX_TOOL_CALLS,
            ),
        }
    }
}

struct CodeExecutionLimiter {
    config: CodeExecutionConfig,
    cpu_slots: Arc<tokio::sync::Semaphore>,
    memory_bytes: Arc<tokio::sync::Semaphore>,
    queued_or_active: Arc<AtomicUsize>,
}

impl CodeExecutionLimiter {
    fn new(config: CodeExecutionConfig) -> Self {
        Self {
            cpu_slots: Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_runs)),
            memory_bytes: Arc::new(tokio::sync::Semaphore::new(config.memory_budget_bytes)),
            queued_or_active: Arc::new(AtomicUsize::new(0)),
            config,
        }
    }

    async fn acquire(&self, requested_memory_bytes: usize) -> Result<CodeExecutionReservation> {
        let reservation_bytes = code_memory_reservation_bytes(requested_memory_bytes)?;
        if reservation_bytes > self.config.memory_budget_bytes {
            tracing::warn!(
                requested_memory_bytes,
                reservation_bytes,
                memory_budget_bytes = self.config.memory_budget_bytes,
                "rejected Monty code execution that exceeds the worker memory budget"
            );
            return Err(anyhow!(
                "code execution capacity unavailable: requested {} bytes exceeds the worker's {} byte code memory budget",
                reservation_bytes,
                self.config.memory_budget_bytes
            ));
        }

        let max_occupancy = self
            .config
            .max_concurrent_runs
            .saturating_add(self.config.max_queued_runs);
        self.queued_or_active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < max_occupancy).then_some(current + 1)
            })
            .map_err(|_| {
                tracing::warn!(
                    max_occupancy,
                    "rejected Monty code execution because the worker queue is full"
                );
                anyhow!("code execution capacity unavailable: worker queue is full; retry later")
            })?;
        let occupancy_guard = CodeOccupancyGuard {
            counter: self.queued_or_active.clone(),
        };
        let started = Instant::now();
        let cpu_slot = match tokio::time::timeout(
            self.config.queue_timeout,
            self.cpu_slots.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(
                    queue_timeout_ms = self.config.queue_timeout.as_millis() as u64,
                    "timed out waiting for a Monty code execution slot"
                );
                return Err(anyhow!(
                    "code execution capacity unavailable: waited {} ms for a worker slot; retry later",
                    self.config.queue_timeout.as_millis()
                ));
            }
        };
        let remaining = self.config.queue_timeout.saturating_sub(started.elapsed());
        let memory_permit = match tokio::time::timeout(
            remaining,
            self.memory_bytes
                .clone()
                .acquire_many_owned(reservation_bytes as u32),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) | Err(_) => {
                drop(cpu_slot);
                tracing::warn!(
                    queue_timeout_ms = self.config.queue_timeout.as_millis() as u64,
                    "timed out waiting for a Monty code execution memory reservation"
                );
                return Err(anyhow!(
                    "code execution capacity unavailable: waited {} ms for worker memory; retry later",
                    self.config.queue_timeout.as_millis()
                ));
            }
        };
        let queue_wait_ms = started.elapsed().as_millis() as u64;
        tracing::info!(
            requested_memory_bytes,
            reservation_bytes,
            queue_wait_ms,
            queued_or_active = self.queued_or_active.load(Ordering::Acquire),
            "admitted Monty code execution"
        );
        Ok(CodeExecutionReservation {
            _cpu_slot: cpu_slot,
            _memory_permit: memory_permit,
            _occupancy_guard: occupancy_guard,
            queue_wait_ms,
            reservation_bytes,
        })
    }
}

struct CodeOccupancyGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for CodeOccupancyGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

struct CodeExecutionReservation {
    _cpu_slot: tokio::sync::OwnedSemaphorePermit,
    _memory_permit: tokio::sync::OwnedSemaphorePermit,
    _occupancy_guard: CodeOccupancyGuard,
    queue_wait_ms: u64,
    reservation_bytes: usize,
}

fn code_execution_limiter() -> &'static CodeExecutionLimiter {
    CODE_EXECUTION_LIMITER
        .get_or_init(|| CodeExecutionLimiter::new(CodeExecutionConfig::from_env()))
}

fn code_memory_reservation_bytes(requested_memory_bytes: usize) -> Result<usize> {
    requested_memory_bytes
        .checked_add(CODE_RUNTIME_OVERHEAD_BYTES)
        .and_then(|value| value.checked_add(MAX_CODE_INPUT_BYTES as usize))
        .and_then(|value| value.checked_add(MAX_CODE_OUTPUT_BYTES as usize))
        .and_then(|value| value.checked_add(CODE_CAPTURE_OVERHEAD_BYTES))
        .ok_or_else(|| anyhow!("code memory reservation overflowed"))
}

fn env_usize<F>(get: &mut F, name: &str, default: usize) -> usize
where
    F: FnMut(&str) -> Option<String>,
{
    get(name)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_positive_usize<F>(get: &mut F, name: &str, default: usize) -> usize
where
    F: FnMut(&str) -> Option<String>,
{
    env_usize(get, name, default).max(1)
}

fn env_positive_u64<F>(get: &mut F, name: &str, default: u64) -> u64
where
    F: FnMut(&str) -> Option<String>,
{
    get(name)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Debug)]
struct CodeDeadlineExceeded;

impl std::fmt::Display for CodeDeadlineExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("code execution deadline exceeded")
    }
}

impl std::error::Error for CodeDeadlineExceeded {}

struct CodeExecutionTelemetry {
    started: Instant,
    requested_memory_bytes: usize,
    reserved_memory_bytes: Option<usize>,
    queue_wait_ms: Option<u64>,
    mounted_input_bytes: u64,
    output_bytes: u64,
    output_files: usize,
    finished: bool,
}

impl CodeExecutionTelemetry {
    fn new(requested_memory_bytes: usize) -> Self {
        Self {
            started: Instant::now(),
            requested_memory_bytes,
            reserved_memory_bytes: None,
            queue_wait_ms: None,
            mounted_input_bytes: 0,
            output_bytes: 0,
            output_files: 0,
            finished: false,
        }
    }

    fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        let fields = tracing::info_span!(
            "monty_code_execution",
            outcome,
            duration_ms = self.started.elapsed().as_millis() as u64,
            requested_memory_bytes = self.requested_memory_bytes,
            reserved_memory_bytes = self.reserved_memory_bytes.unwrap_or_default(),
            queue_wait_ms = self.queue_wait_ms.unwrap_or_default(),
            mounted_input_bytes = self.mounted_input_bytes,
            output_bytes = self.output_bytes,
            output_files = self.output_files,
        );
        let _entered = fields.enter();
        if outcome == "success" {
            tracing::info!("completed Monty code execution");
        } else {
            tracing::warn!("Monty code execution ended without success");
        }
    }
}

impl Drop for CodeExecutionTelemetry {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("cancelled");
        }
    }
}

fn code_error_outcome(error: &anyhow::Error) -> &'static str {
    let message = error.to_string();
    if message.contains("deadline exceeded") || message.contains("timed out") {
        "timeout"
    } else if message.contains("capacity unavailable") {
        "capacity_rejected"
    } else if message.contains("mount") || message.contains("mounted") {
        "mount_failed"
    } else if message.contains("output") || message.contains("artifact") {
        "output_failed"
    } else if message.contains("tool bridge") || message.contains("Talon tool") {
        "tool_bridge_failed"
    } else {
        "monty_failed"
    }
}

async fn run_python_code_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
) -> Result<String> {
    let code = req_str(args, "code")?.to_string();
    let mut inputs = monty_inputs_from_args(args)?;
    if inputs.iter().any(|(name, _)| name == TALON_TOOL_FUNCTION) {
        return Err(anyhow!(
            "input name '{}' is reserved for Talon tool calls",
            TALON_TOOL_FUNCTION
        ));
    }
    inputs.push((
        TALON_TOOL_FUNCTION.to_string(),
        monty_types::MontyObject::Function {
            name: TALON_TOOL_FUNCTION.to_string(),
            docstring: Some(
                "Call a permitted Talon native tool: talon_tool(name, args={}).".to_string(),
            ),
        },
    ));
    let timeout_ms = opt_u64(args, "timeout_ms").unwrap_or(1000).clamp(1, 30_000);
    let memory_bytes = opt_u64(args, "memory_bytes")
        .unwrap_or(16 * 1024 * 1024)
        .clamp(1, 128 * 1024 * 1024) as usize;
    let persist_outputs = args
        .get("persist_outputs")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut telemetry = CodeExecutionTelemetry::new(memory_bytes);
    let limiter = code_execution_limiter();
    let reservation = match limiter.acquire(memory_bytes).await {
        Ok(reservation) => reservation,
        Err(error) => {
            telemetry.finish(code_error_outcome(&error));
            return Err(error);
        }
    };
    telemetry.queue_wait_ms = Some(reservation.queue_wait_ms);
    telemetry.reserved_memory_bytes = Some(reservation.reservation_bytes);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let timeout_duration = Duration::from_millis(timeout_ms);

    let run = async {
        let CodeMountBundle {
            _temp_dir,
            output_dir,
            mounts,
            mounted_inputs,
        } = prepare_code_mounts(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            args,
        )
        .await
        .map_err(|error| anyhow!("Monty mount preparation failed: {error}"))?;
        telemetry.mounted_input_bytes = mounted_inputs
            .iter()
            .filter_map(|input| input.get("sizeBytes").and_then(Value::as_u64))
            .sum();
        let context = CodeRunContext {
            deadline,
            tool_calls: Arc::new(AtomicUsize::new(0)),
            max_tool_calls: limiter.config.max_tool_calls,
        };
        let mut result = run_monty_python(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            code,
            inputs,
            mounts,
            &output_dir,
            context,
            timeout_ms,
            memory_bytes,
        )
        .await
        .map_err(|error| anyhow!("Monty execution failed: {error}"))?;
        let (output_bytes, output_files) = code_output_stats(&output_dir)?;
        let outputs = if persist_outputs {
            persist_code_outputs(
                cp,
                current_namespace,
                current_agent,
                current_session,
                &output_dir,
            )
            .await
            .map_err(|error| anyhow!("output persistence failed: {error}"))?
        } else {
            Vec::new()
        };
        telemetry.output_bytes = output_bytes;
        telemetry.output_files = output_files;
        if let Some(object) = result.as_object_mut() {
            object.insert("mountedInputs".to_string(), json!(mounted_inputs));
            object.insert("outputMount".to_string(), json!(CODE_OUTPUT_MOUNT));
            object.insert("outputs".to_string(), json!(outputs));
        }
        Ok::<_, anyhow::Error>(serde_json::to_string_pretty(&result)?)
    };

    let result = match tokio::time::timeout(timeout_duration, run).await {
        Ok(Ok(result)) => {
            telemetry.finish("success");
            Ok(result)
        }
        Ok(Err(error)) => {
            telemetry.finish(code_error_outcome(&error));
            Err(error)
        }
        Err(_) => {
            let error = anyhow!(CodeDeadlineExceeded);
            telemetry.finish("timeout");
            Err(error)
        }
    };
    drop(reservation);
    result
}

impl CodeRunContext {
    fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| anyhow::Error::new(CodeDeadlineExceeded))
    }

    fn reserve_tool_call(&self) -> Result<()> {
        self.tool_calls
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < self.max_tool_calls).then_some(current + 1)
            })
            .map_err(|_| {
                anyhow!(
                    "Talon tool bridge exceeded the {} call limit for one code execution",
                    self.max_tool_calls
                )
            })?;
        self.remaining()?;
        Ok(())
    }
}

async fn run_monty_python(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    code: String,
    inputs: Vec<(String, monty_types::MontyObject)>,
    mounts: Vec<monty_pool::MountSpec>,
    output_dir: &Path,
    context: CodeRunContext,
    timeout_ms: u64,
    memory_bytes: usize,
) -> Result<Value> {
    let input_names = inputs
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let limits = monty_types::ResourceLimits::default()
        .max_duration(Duration::from_millis(timeout_ms))
        .max_memory(memory_bytes);
    let monty_binary = std::env::var_os("TALON_MONTY_BIN")
        .map(std::path::PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| {
            std::env::current_exe().map_err(|err| {
                anyhow!(
                    "unable to locate the current Talon executable for the embedded Monty worker: {err}; set TALON_MONTY_BIN to an external monty runtime"
                )
            })
        })?;
    let mut config = monty_pool::PoolConfig::subprocess(monty_binary);
    config.min_processes = 0;
    config.max_processes = 1;
    config.request_timeout =
        Some(Duration::from_millis(timeout_ms).saturating_add(Duration::from_secs(1)));
    config.checkout_timeout = Some(Duration::from_secs(5));
    let pool = monty_pool::Pool::new(config).await.map_err(|err| {
        anyhow!(
            "failed to start Monty runtime: {err}. The current Talon binary contains the embedded worker; set TALON_MONTY_BIN to an external monty runtime to override it"
        )
    })?;
    let repl = monty_pool::ReplConfig {
        script_name: "agent_code.py".to_string(),
        limits: Some(limits),
        ..Default::default()
    };
    let mut session = pool.checkout(&repl).await?;
    let capture = Arc::new(std::sync::Mutex::new(CodeOutputCapture::new()));
    let capture_for_print = Arc::clone(&capture);
    let mut on_print = monty_pool::on_print_sync(move |stream, text| {
        if let Ok(mut capture) = capture_for_print.lock() {
            capture.push(stream, text);
        }
    });
    let mut event = session
        .feed(&code, inputs, mounts, true, &mut on_print)
        .await?;
    if capture
        .lock()
        .map(|capture| capture.truncated())
        .unwrap_or(true)
    {
        return Err(anyhow!(
            "Monty stdout/stderr capture exceeded 1 MiB per stream; [stdout truncated at 1 MiB] or [stderr truncated at 1 MiB]"
        ));
    }
    let result = loop {
        match event {
            monty_pool::TurnEvent::Complete(value) => break value,
            monty_pool::TurnEvent::FunctionCall {
                function_name,
                args,
                kwargs,
                method_call,
                ..
            } => {
                let value = match call_talon_tool(
                    cp,
                    current_namespace,
                    current_agent,
                    current_session,
                    spec,
                    &context,
                    &function_name,
                    args,
                    kwargs,
                    method_call,
                )
                .await
                {
                    Ok(value) => monty_pool::ResumeValue::Return(value),
                    Err(error) if error.downcast_ref::<CodeDeadlineExceeded>().is_some() => {
                        return Err(error);
                    }
                    Err(error) => monty_pool::ResumeValue::Error(monty_types::MontyException::new(
                        monty_types::ExcType::RuntimeError,
                        Some(error.to_string()),
                    )),
                };
                event = session.resume(value, &mut on_print).await?;
                if capture
                    .lock()
                    .map(|capture| capture.truncated())
                    .unwrap_or(true)
                {
                    return Err(anyhow!(
                        "Monty stdout/stderr capture exceeded 1 MiB per stream; [stdout truncated at 1 MiB] or [stderr truncated at 1 MiB]"
                    ));
                }
            }
            monty_pool::TurnEvent::OsCall { function_name, .. } => {
                if let Some(next) = session.resume_from_mounts(&mut on_print).await? {
                    event = next;
                } else {
                    return Err(anyhow!(
                        "Monty code requested OS operation '{function_name}', but only filesystem access under {CODE_INPUT_MOUNT} and {CODE_OUTPUT_MOUNT} is available"
                    ));
                }
                enforce_code_output_entry_limit(output_dir)?;
                if capture
                    .lock()
                    .map(|capture| capture.truncated())
                    .unwrap_or(true)
                {
                    return Err(anyhow!(
                        "Monty stdout/stderr capture exceeded 1 MiB per stream; [stdout truncated at 1 MiB] or [stderr truncated at 1 MiB]"
                    ));
                }
            }
            monty_pool::TurnEvent::NameLookup { name } => {
                let known_inputs = if input_names.is_empty() {
                    "no inputs were provided".to_string()
                } else {
                    format!("available inputs: {}", input_names.join(", "))
                };
                return Err(anyhow!(
                    "Monty code requested unknown name '{name}'; {known_inputs}"
                ));
            }
            monty_pool::TurnEvent::ResolveFutures { .. } => {
                return Err(anyhow!(
                    "Monty code suspended on external futures, but external futures are disabled for this tool"
                ));
            }
        }
    };
    drop(on_print);
    let (stdout, stderr) = capture
        .lock()
        .map_err(|_| anyhow!("Monty output capture mutex was poisoned"))?
        .clone()
        .finish();
    session.finish().await?;
    Ok(json!({
        "ok": true,
        "runtime": "monty",
        "language": "python",
        "stdout": stdout,
        "stderr": stderr,
        "result": result.to_string(),
        "value": serde_json::to_value(&result)?,
        "inputMount": CODE_INPUT_MOUNT,
    }))
}

async fn prepare_code_mounts(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
) -> Result<CodeMountBundle> {
    let temp_dir = tempfile::Builder::new()
        .prefix("talon-code-")
        .tempdir()
        .map_err(|err| anyhow!("failed to create code mount tempdir: {err}"))?;
    let input_dir = temp_dir.path().join("input");
    let output_dir = temp_dir.path().join("output");
    fs::create_dir_all(&input_dir)?;
    fs::create_dir_all(&output_dir)?;

    let requested = args
        .get("mounts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if requested.len() > MAX_CODE_MOUNTS {
        return Err(anyhow!(
            "mounts exceeds maximum of {} entries",
            MAX_CODE_MOUNTS
        ));
    }

    let mut total_input_bytes = 0_u64;
    let mut mounted_inputs = Vec::new();
    let mut seen_mount_paths = HashSet::new();
    for (index, mount) in requested.iter().enumerate() {
        let object = mount
            .as_object()
            .ok_or_else(|| anyhow!("mounts[{index}] must be an object"))?;
        let uri = object
            .get("uri")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("mounts[{index}].uri is required"))?;
        let mount_path = object
            .get("mount_path")
            .and_then(Value::as_str)
            .map(validate_code_mount_path)
            .transpose()?
            .unwrap_or_else(|| default_code_mount_path(uri, index));
        if !seen_mount_paths.insert(mount_path.clone()) {
            return Err(anyhow!(
                "mount_path '{}' is used more than once",
                mount_path.display()
            ));
        }
        let remaining_input_bytes = MAX_CODE_INPUT_BYTES.saturating_sub(total_input_bytes);
        let (bytes, media_type, source_kind) = read_code_mount_source(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            uri,
            remaining_input_bytes,
        )
        .await?;
        total_input_bytes = total_input_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("mounted input byte count overflowed"))?;
        if total_input_bytes > MAX_CODE_INPUT_BYTES {
            return Err(anyhow!(
                "mounted inputs exceed {} byte limit",
                MAX_CODE_INPUT_BYTES
            ));
        }
        let target = input_dir.join(&mount_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &bytes)?;
        mounted_inputs.push(json!({
            "uri": uri,
            "mountPath": code_virtual_mount_path(CODE_INPUT_MOUNT, &mount_path),
            "mediaType": media_type,
            "sizeBytes": bytes.len(),
            "source": source_kind,
        }));
    }

    let input_mount = monty_pool::MountSpec::new(
        CODE_INPUT_MOUNT,
        input_dir,
        monty_pool::MountSpecMode::ReadOnly,
    )
    .map_err(|error| anyhow!("failed to configure Monty input mount: {error}"))?;
    let mut output_mount = monty_pool::MountSpec::new(
        CODE_OUTPUT_MOUNT,
        output_dir.clone(),
        monty_pool::MountSpecMode::ReadWrite,
    )
    .map_err(|error| anyhow!("failed to configure Monty output mount: {error}"))?;
    output_mount.write_bytes_limit = Some(MAX_CODE_OUTPUT_BYTES);

    Ok(CodeMountBundle {
        _temp_dir: temp_dir,
        output_dir,
        mounts: vec![input_mount, output_mount],
        mounted_inputs,
    })
}

async fn read_code_mount_source(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    uri: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, String, &'static str)> {
    if uri.starts_with("file://") {
        require_file_read(spec)?;
        let (namespace, path) = parse_file_uri(uri)?;
        let namespace = if namespace == "current" {
            current_namespace.to_string()
        } else {
            namespace
        };
        let file = find_file_by_path(cp, &namespace, &path)
            .await?
            .ok_or_else(|| anyhow!("File '{}' not found", path))?;
        let object_key = file
            .status
            .as_ref()
            .and_then(|status| status.object_ref.as_ref())
            .map(|object_ref| object_ref.key.as_str())
            .ok_or_else(|| anyhow!("File has no objectRef"))?;
        preflight_code_mount_size(cp, object_key, max_bytes).await?;
        let media_type = file
            .spec
            .as_ref()
            .map(|spec| spec.media_type.clone())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        Ok((read_file_bytes(cp, &file).await?, media_type, "file"))
    } else if uri.starts_with("artifact://") {
        let (_, artifact) =
            resolve_artifact_uri(cp, current_agent, current_session, uri, OP_READ).await?;
        let object_ref = artifact
            .object_ref
            .as_ref()
            .ok_or_else(|| anyhow!("Artifact has no objectRef"))?;
        preflight_code_mount_size(cp, &object_ref.key, max_bytes).await?;
        let object = cp
            .objects
            .get(&object_ref.key)
            .await?
            .ok_or_else(|| anyhow!("Artifact object not found"))?;
        Ok((object.bytes, artifact.media_type, "artifact"))
    } else {
        Err(anyhow!("mount uri must start with file:// or artifact://"))
    }
}

async fn preflight_code_mount_size(
    cp: &ControlPlane,
    object_key: &str,
    max_bytes: u64,
) -> Result<()> {
    let metadata = cp
        .objects
        .head(object_key)
        .await?
        .ok_or_else(|| anyhow!("mounted object '{}' not found", object_key))?;
    if metadata.size_bytes > max_bytes {
        return Err(anyhow!(
            "mounted object '{}' is {} bytes, exceeding remaining {} byte input budget",
            object_key,
            metadata.size_bytes,
            max_bytes
        ));
    }
    Ok(())
}

fn validate_code_mount_path(value: &str) -> Result<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("mount_path must not be empty"));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(anyhow!("mount_path must be relative to {CODE_INPUT_MOUNT}"));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return Err(anyhow!("mount_path must not contain '..' or prefixes")),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(anyhow!("mount_path must include a file name"));
    }
    Ok(clean)
}

fn default_code_mount_path(uri: &str, index: usize) -> PathBuf {
    let tail = uri
        .rsplit('/')
        .next()
        .map(safe_code_path_segment)
        .filter(|value| !value.is_empty() && value != "." && value != "..")
        .unwrap_or_else(|| format!("mount-{index}"));
    PathBuf::from(tail)
}

fn safe_code_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

async fn read_file_bytes(cp: &ControlPlane, file: &resources_proto::File) -> Result<Vec<u8>> {
    let object_ref = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .ok_or_else(|| anyhow!("File has no objectRef"))?;
    let object = crate::control::cas::CasStore::new(cp.objects.clone())
        .get_object_decoded(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("File object '{}' not found", object_ref.key))?;
    Ok(object.bytes)
}

async fn persist_code_outputs(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    output_dir: &Path,
) -> Result<Vec<Value>> {
    let mut paths = Vec::new();
    collect_code_output_files(output_dir, output_dir, &mut paths)?;
    if paths.len() > MAX_CODE_OUTPUT_FILES {
        return Err(anyhow!(
            "code output contains {} files, exceeding limit of {}",
            paths.len(),
            MAX_CODE_OUTPUT_FILES
        ));
    }
    let mut total_bytes = 0_u64;
    let mut outputs = Vec::new();
    for relative in paths {
        let path = output_dir.join(&relative);
        let remaining_bytes = MAX_CODE_OUTPUT_BYTES
            .checked_sub(total_bytes)
            .ok_or_else(|| anyhow!("output byte count overflowed"))?;
        let bytes = read_code_output_bytes(&path, remaining_bytes)?;
        total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("output byte count overflowed"))?;
        if total_bytes > MAX_CODE_OUTPUT_BYTES {
            return Err(anyhow!(
                "code outputs exceed {} byte limit",
                MAX_CODE_OUTPUT_BYTES
            ));
        }
        let relative_virtual_path = code_relative_virtual_path(&relative);
        let output_mount_path = format!("{CODE_OUTPUT_MOUNT}/{relative_virtual_path}");
        let title = relative_virtual_path.clone();
        let media_type = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        let artifact_output = create_artifact(
            cp,
            current_namespace,
            current_agent,
            current_session,
            &json!({
                "title": title,
                "media_type": media_type,
                "content_base64": general_purpose::STANDARD.encode(&bytes),
                "metadata": {
                    "source": "run_python_code",
                    "mountPath": output_mount_path.clone()
                }
            }),
        )
        .await?;
        let artifact_value: Value = serde_json::from_str(&artifact_output)?;
        outputs.push(json!({
            "path": output_mount_path,
            "sizeBytes": bytes.len(),
            "mediaType": media_type,
            "artifactUri": artifact_value.get("artifactUri").cloned().unwrap_or(Value::Null),
            "artifact": artifact_value.get("artifact").cloned().unwrap_or(Value::Null),
        }));
    }
    Ok(outputs)
}

fn read_code_output_bytes(path: &Path, remaining_bytes: u64) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "code output '{}' is not a regular file",
            path.display()
        ));
    }
    if metadata.len() > remaining_bytes {
        return Err(anyhow!(
            "code outputs exceed {} byte limit",
            MAX_CODE_OUTPUT_BYTES
        ));
    }

    let file = fs::File::open(path)?;
    let read_limit = remaining_bytes.saturating_add(1);
    let capacity = metadata.len().min(read_limit) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > remaining_bytes {
        return Err(anyhow!(
            "code outputs exceed {} byte limit",
            MAX_CODE_OUTPUT_BYTES
        ));
    }
    Ok(bytes)
}

fn code_output_stats(output_dir: &Path) -> Result<(u64, usize)> {
    enforce_code_output_entry_limit(output_dir)?;
    let mut paths = Vec::new();
    collect_code_output_files(output_dir, output_dir, &mut paths)?;
    if paths.len() > MAX_CODE_OUTPUT_FILES {
        return Err(anyhow!(
            "code output contains {} files, exceeding limit of {}",
            paths.len(),
            MAX_CODE_OUTPUT_FILES
        ));
    }
    let mut total_bytes = 0_u64;
    for relative in &paths {
        let size = fs::metadata(output_dir.join(relative))?.len();
        total_bytes = total_bytes
            .checked_add(size)
            .ok_or_else(|| anyhow!("output byte count overflowed"))?;
        if total_bytes > MAX_CODE_OUTPUT_BYTES {
            return Err(anyhow!(
                "code outputs exceed {} byte limit",
                MAX_CODE_OUTPUT_BYTES
            ));
        }
    }
    Ok((total_bytes, paths.len()))
}

fn code_virtual_mount_path(root: &str, relative: &Path) -> String {
    format!("{root}/{}", code_relative_virtual_path(relative))
}

fn code_relative_virtual_path(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

fn collect_code_output_files(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_code_output_files(root, &path, paths)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root)?.to_path_buf();
            paths.push(relative);
        }
    }
    paths.sort();
    Ok(())
}

fn count_code_output_entries(dir: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        count += 1;
        if count > MAX_CODE_OUTPUT_ENTRIES {
            return Ok(count);
        }
        if entry.file_type()?.is_dir() {
            count += count_code_output_entries(&entry.path())?;
            if count > MAX_CODE_OUTPUT_ENTRIES {
                return Ok(count);
            }
        }
    }
    Ok(count)
}

fn enforce_code_output_entry_limit(output_dir: &Path) -> Result<()> {
    let entries = count_code_output_entries(output_dir)?;
    if entries > MAX_CODE_OUTPUT_ENTRIES {
        return Err(anyhow!(
            "code output contains {} files or directories, exceeding limit of {} entries",
            entries,
            MAX_CODE_OUTPUT_ENTRIES
        ));
    }
    Ok(())
}

async fn call_talon_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    context: &CodeRunContext,
    function_name: &str,
    args: Vec<monty_types::MontyObject>,
    kwargs: Vec<(monty_types::MontyObject, monty_types::MontyObject)>,
    method_call: bool,
) -> Result<monty_types::MontyObject> {
    if function_name != TALON_TOOL_FUNCTION || method_call {
        return Err(anyhow!(
            "host function '{}' is not available; use {}(name, args={{}})",
            function_name,
            TALON_TOOL_FUNCTION
        ));
    }
    if !kwargs.is_empty() {
        return Err(anyhow!(
            "{TALON_TOOL_FUNCTION} does not accept keyword arguments"
        ));
    }
    let tool_name = args
        .first()
        .ok_or_else(|| anyhow!("{TALON_TOOL_FUNCTION} requires a tool name"))?;
    let tool_name = String::try_from(tool_name)
        .map_err(|_| anyhow!("{TALON_TOOL_FUNCTION} first argument must be a string tool name"))?;
    if tool_name == RUN_PYTHON_CODE_TOOL {
        return Err(anyhow!("run_python_code cannot be called from code"));
    }
    context.reserve_tool_call()?;
    let tool_args = match args.get(1) {
        Some(value) => monty_object_to_json(value)?,
        None => json!({}),
    };
    if !tool_args.is_object() {
        return Err(anyhow!(
            "{TALON_TOOL_FUNCTION} second argument must be a dict/object"
        ));
    }
    let remaining = context.remaining()?;
    let output = tokio::time::timeout(
        remaining,
        Box::pin(execute_tool_for_session(
            cp,
            current_namespace,
            current_agent,
            current_session,
            spec,
            &tool_name,
            &tool_args,
        )),
    )
    .await
    .map_err(|_| anyhow::Error::new(CodeDeadlineExceeded))?
    .map_err(|error| {
        if error.downcast_ref::<CodeDeadlineExceeded>().is_some() {
            error
        } else {
            anyhow!("Talon tool bridge failed: {error}")
        }
    })?
    .ok_or_else(|| anyhow!("unknown Talon tool '{}'", tool_name))?;
    match serde_json::from_str::<Value>(&output) {
        Ok(value) => json_to_monty_object(&value),
        Err(_) => Ok(monty_types::MontyObject::String(output)),
    }
}

fn monty_object_to_json(value: &monty_types::MontyObject) -> Result<Value> {
    Ok(match value {
        monty_types::MontyObject::None => Value::Null,
        monty_types::MontyObject::Bool(value) => Value::Bool(*value),
        monty_types::MontyObject::Int(value) => json!(value),
        monty_types::MontyObject::Float(value) => Value::Number(
            Number::from_f64(*value)
                .ok_or_else(|| anyhow!("non-finite float not supported in talon_tool args"))?,
        ),
        monty_types::MontyObject::String(value) => Value::String(value.clone()),
        monty_types::MontyObject::Bytes(value) => {
            json!({ "content_base64": general_purpose::STANDARD.encode(value) })
        }
        monty_types::MontyObject::List(values)
        | monty_types::MontyObject::Tuple(values)
        | monty_types::MontyObject::Set(values) => values
            .iter()
            .map(monty_object_to_json)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array)?,
        monty_types::MontyObject::Dict(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, value) in pairs {
                let key = match key {
                    monty_types::MontyObject::String(value) => value.clone(),
                    other => other.to_string(),
                };
                map.insert(key, monty_object_to_json(value)?);
            }
            Value::Object(map)
        }
        other => {
            return Err(anyhow!(
                "cannot pass {} value from Python to Talon tool",
                other.type_name()
            ));
        }
    })
}

fn monty_inputs_from_args(args: &Value) -> Result<Vec<(String, monty_types::MontyObject)>> {
    let Some(inputs) = args.get("inputs") else {
        return Ok(Vec::new());
    };
    let object = inputs
        .as_object()
        .ok_or_else(|| anyhow!("inputs must be a JSON object"))?;
    object
        .iter()
        .map(|(name, value)| {
            if !is_python_identifier(name) {
                return Err(anyhow!(
                    "input name '{}' is not a valid Python identifier",
                    name
                ));
            }
            Ok((name.clone(), json_to_monty_object(value)?))
        })
        .collect()
}

fn json_to_monty_object(value: &Value) -> Result<monty_types::MontyObject> {
    Ok(match value {
        Value::Null => monty_types::MontyObject::None,
        Value::Bool(value) => monty_types::MontyObject::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                monty_types::MontyObject::Int(value)
            } else if let Some(value) = number.as_u64() {
                let value = i64::try_from(value)
                    .map_err(|_| anyhow!("integer input exceeds Monty's i64 boundary"))?;
                monty_types::MontyObject::Int(value)
            } else {
                monty_types::MontyObject::Float(
                    number
                        .as_f64()
                        .ok_or_else(|| anyhow!("invalid JSON number input"))?,
                )
            }
        }
        Value::String(value) => monty_types::MontyObject::String(value.clone()),
        Value::Array(values) => monty_types::MontyObject::List(
            values
                .iter()
                .map(json_to_monty_object)
                .collect::<Result<Vec<_>>>()?,
        ),
        Value::Object(object) => monty_types::MontyObject::Dict(
            object
                .iter()
                .map(|(key, value)| {
                    Ok((
                        monty_types::MontyObject::String(key.clone()),
                        json_to_monty_object(value)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .into(),
        ),
    })
}

fn is_python_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{InMemoryObjectStore, ObjectMetadata, ObjectStore};
    use crate::control::scheduler::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend};
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockScheduler {
        scheduled: Mutex<Vec<ScheduleWakeupRequest>>,
        cancelled: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl SchedulerBackend for MockScheduler {
        async fn schedule(&self, req: ScheduleWakeupRequest) -> anyhow::Result<ScheduledWakeup> {
            self.scheduled.lock().await.push(req);
            Ok(ScheduledWakeup {
                handle: Some("handle-1".to_string()),
                armed: true,
            })
        }

        async fn cancel(&self, handle: &str) -> anyhow::Result<()> {
            self.cancelled.lock().await.push(handle.to_string());
            Ok(())
        }
    }

    fn control_plane(kv: Arc<MockKvStore>, scheduler: Arc<MockScheduler>) -> ControlPlane {
        ControlPlane::builder(kv, Arc::new(EmptyPubSub))
            .scheduler(scheduler)
            .build()
    }

    fn code_spec(capabilities: &[&str]) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([(
                "code".to_string(),
                crate::gateway::rpc::protobuf_value::ListValue {
                    values: capabilities
                        .iter()
                        .map(|action| crate::gateway::rpc::protobuf_value::Value {
                            kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                        })
                        .collect(),
                },
            )]),
            ..manifests::AgentSpec::default()
        }
    }

    fn code_and_file_spec(
        code_capabilities: &[&str],
        file_capabilities: &[&str],
    ) -> manifests::AgentSpec {
        manifests::AgentSpec {
            capabilities: HashMap::from([
                (
                    "code".to_string(),
                    crate::gateway::rpc::protobuf_value::ListValue {
                        values: code_capabilities
                            .iter()
                            .map(|action| crate::gateway::rpc::protobuf_value::Value {
                                kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                            })
                            .collect(),
                    },
                ),
                (
                    "files".to_string(),
                    crate::gateway::rpc::protobuf_value::ListValue {
                        values: file_capabilities
                            .iter()
                            .map(|action| crate::gateway::rpc::protobuf_value::Value {
                                kind: Some(ProtoValueKind::StringValue((*action).to_string())),
                            })
                            .collect(),
                    },
                ),
            ]),
            ..manifests::AgentSpec::default()
        }
    }

    #[test]
    fn code_execution_config_uses_serverless_defaults_and_ignores_invalid_values() {
        let config = CodeExecutionConfig::from_getter(|name| match name {
            "TALON_CODE_MAX_CONCURRENT_RUNS" => Some("0".to_string()),
            "TALON_CODE_MEMORY_BUDGET_BYTES" => Some("invalid".to_string()),
            "TALON_CODE_QUEUE_TIMEOUT_MS" => Some("0".to_string()),
            "TALON_CODE_MAX_QUEUED_RUNS" => Some("3".to_string()),
            "TALON_CODE_MAX_TOOL_CALLS" => Some("2".to_string()),
            _ => None,
        });
        assert_eq!(config.max_concurrent_runs, 1);
        assert_eq!(config.memory_budget_bytes, DEFAULT_CODE_MEMORY_BUDGET_BYTES);
        assert_eq!(config.queue_timeout, Duration::from_secs(30));
        assert_eq!(config.max_queued_runs, 3);
        assert_eq!(config.max_tool_calls, 2);
    }

    #[tokio::test]
    async fn code_execution_limiter_bounds_queue_and_releases_reservations() {
        let limiter = CodeExecutionLimiter::new(CodeExecutionConfig {
            max_concurrent_runs: 1,
            memory_budget_bytes: code_memory_reservation_bytes(1).unwrap(),
            queue_timeout: Duration::from_millis(10),
            max_queued_runs: 0,
            max_tool_calls: 1,
        });
        let reservation = limiter.acquire(1).await.unwrap();
        let error = match limiter.acquire(1).await {
            Ok(_) => panic!("second code execution should not be admitted"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("worker queue is full"), "{error}");
        drop(reservation);
        assert!(limiter.acquire(1).await.is_ok());
    }

    #[tokio::test]
    async fn cancelled_limiter_waits_release_queue_occupancy() {
        let reservation_bytes = code_memory_reservation_bytes(1).unwrap();
        let limiter = CodeExecutionLimiter::new(CodeExecutionConfig {
            max_concurrent_runs: 1,
            memory_budget_bytes: reservation_bytes,
            queue_timeout: Duration::from_secs(30),
            max_queued_runs: 1,
            max_tool_calls: 1,
        });
        let reservation = limiter.acquire(1).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(1), limiter.acquire(1))
                .await
                .is_err()
        );
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 1);
        drop(reservation);
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 0);
        assert!(limiter.acquire(1).await.is_ok());
    }

    #[tokio::test]
    async fn cancelled_memory_wait_releases_queue_occupancy() {
        let reservation_bytes = code_memory_reservation_bytes(1).unwrap();
        let limiter = CodeExecutionLimiter::new(CodeExecutionConfig {
            max_concurrent_runs: 2,
            memory_budget_bytes: reservation_bytes,
            queue_timeout: Duration::from_secs(30),
            max_queued_runs: 1,
            max_tool_calls: 1,
        });
        let reservation = limiter.acquire(1).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(1), limiter.acquire(1))
                .await
                .is_err()
        );
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 1);
        drop(reservation);
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 0);
        assert!(limiter.acquire(1).await.is_ok());
    }

    #[tokio::test]
    async fn internal_queue_timeout_releases_occupancy_once() {
        let reservation_bytes = code_memory_reservation_bytes(1).unwrap();
        let limiter = CodeExecutionLimiter::new(CodeExecutionConfig {
            max_concurrent_runs: 1,
            memory_budget_bytes: reservation_bytes,
            queue_timeout: Duration::from_millis(1),
            max_queued_runs: 1,
            max_tool_calls: 1,
        });
        let reservation = limiter.acquire(1).await.unwrap();
        let error = match limiter.acquire(1).await {
            Ok(_) => panic!("second code execution should time out"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("worker slot"), "{error}");
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 1);
        drop(reservation);
        assert_eq!(limiter.queued_or_active.load(Ordering::Acquire), 0);
        assert!(limiter.acquire(1).await.is_ok());
    }

    #[test]
    fn code_output_capture_is_bounded_per_stream() {
        let mut capture = CodeOutputCapture::new();
        capture.push(
            monty_types::PrintStream::Stdout,
            &"x".repeat(MAX_CODE_STDOUT_BYTES + 32),
        );
        capture.push(
            monty_types::PrintStream::Stderr,
            &"y".repeat(MAX_CODE_STDERR_BYTES + 32),
        );
        assert!(capture.truncated());
        let (stdout, stderr) = capture.finish();
        assert!(stdout.len() <= MAX_CODE_STDOUT_BYTES + 64);
        assert!(stderr.len() <= MAX_CODE_STDERR_BYTES + 64);
        assert!(stdout.contains("stdout truncated"));
        assert!(stderr.contains("stderr truncated"));
    }

    #[test]
    fn code_output_entry_limit_rejects_excessive_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        for index in 0..=MAX_CODE_OUTPUT_ENTRIES {
            fs::create_dir(temp_dir.path().join(format!("entry-{index}"))).unwrap();
        }
        let error = enforce_code_output_entry_limit(temp_dir.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeding limit"), "{error}");
    }

    #[test]
    fn code_memory_reservation_covers_runtime_and_mount_bounds() {
        assert_eq!(
            code_memory_reservation_bytes(128 * 1024 * 1024).unwrap(),
            244 * 1024 * 1024
        );
    }
    #[test]
    fn register_code_tools_respects_capabilities() {
        let mut registry = ToolRegistry::new();
        register_tools(&mut registry, &code_spec(&["run"]));
        assert!(registry.get_tool(RUN_PYTHON_CODE_TOOL).is_some());

        let mut empty_registry = ToolRegistry::new();
        register_tools(&mut empty_registry, &code_spec(&[]));
        assert!(empty_registry.get_tool(RUN_PYTHON_CODE_TOOL).is_none());
    }

    #[test]
    fn code_tool_schema_is_openai_compatible() {
        let mut registry = ToolRegistry::new();
        register(&mut registry, &code_spec(&["run"]));
        let tool = registry
            .get_tool(RUN_PYTHON_CODE_TOOL)
            .expect("run_python_code should be registered");
        assert_eq!(tool.input_schema["type"], "object");
        for keyword in ["anyOf", "oneOf", "allOf", "not"] {
            assert!(
                tool.input_schema.get(keyword).is_none(),
                "top-level {keyword} is not OpenAI-compatible"
            );
        }
    }

    #[tokio::test]
    async fn run_python_code_executes_with_monty() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let output = execute_tool_for_session(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            &code_spec(&["run"]),
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "print('checking')\nsum(values) + offset",
                "inputs": {
                    "values": [1, 2, 3],
                    "offset": 4
                },
                "timeout_ms": 1000,
                "memory_bytes": 1048576
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["runtime"], "monty");
        assert_eq!(value["stdout"], "checking\n");
        assert_eq!(value["result"], "10");
    }

    #[tokio::test]
    async fn run_python_code_enforces_hard_wall_deadline() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let started = Instant::now();
        let error = execute_tool_for_session(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            &code_spec(&["run"]),
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "while True: pass",
                "timeout_ms": 100,
                "memory_bytes": 1048576,
                "persist_outputs": false
            }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            error.contains("deadline") || error.contains("timeout"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn run_python_code_rejects_oversized_output_without_persistence() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let error = execute_tool_for_session(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            &code_spec(&["run"]),
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "from pathlib import Path\nPath('/talon/output/too-large.bin').write_bytes(b'x' * (25 * 1024 * 1024 + 1))",
                "timeout_ms": 3000,
                "memory_bytes": 128 * 1024 * 1024,
                "persist_outputs": false
            }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("output") || error.contains("write"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn run_python_code_rejects_excessive_output_entries_during_execution() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let error = execute_tool_for_session(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            &code_spec(&["run"]),
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "from pathlib import Path\nroot = Path('/talon/output')\nfor index in range(101):\n    (root / f'dir-{index}').mkdir()",
                "timeout_ms": 3000,
                "memory_bytes": 1048576,
                "persist_outputs": false
            }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("exceeding limit") || error.contains("entries"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn run_python_code_requires_capability() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let err = execute_tool_for_session(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            &code_spec(&[]),
            RUN_PYTHON_CODE_TOOL,
            &json!({ "code": "1 + 1" }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("code:run"));
    }

    #[tokio::test]
    async fn run_python_code_mounts_files_and_persists_outputs() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let namespace = "Tenant:test:Workspace:main";
        let spec = code_and_file_spec(&["run"], &["read", "create"]);
        execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            CREATE_FILE_TOOL,
            &json!({
                "path": "/datasets/input.txt",
                "content": "hello mount",
                "media_type": "text/plain"
            }),
        )
        .await
        .unwrap()
        .unwrap();

        let output = execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "from pathlib import Path\ntext = Path('/talon/input/input.txt').read_text()\nPath('/talon/output/summary.txt').write_text(text.upper())\ntext",
                "mounts": [
                    {
                        "uri": file_uri(namespace, "/datasets/input.txt"),
                        "mount_path": "input.txt"
                    }
                ],
                "timeout_ms": 1000
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["value"], "hello mount");
        assert_eq!(
            value["mountedInputs"][0]["mountPath"],
            "/talon/input/input.txt"
        );
        let artifact_uri = value["outputs"][0]["artifactUri"].as_str().unwrap();
        let read = execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            READ_ARTIFACT_TOOL,
            &json!({ "artifact_uri": artifact_uri }),
        )
        .await
        .unwrap()
        .unwrap();
        let read: Value = serde_json::from_str(&read).unwrap();
        assert_eq!(read["content"], "HELLO MOUNT");
    }

    #[tokio::test]
    async fn run_python_code_rejects_duplicate_mount_paths_before_runtime() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let namespace = "Tenant:test:Workspace:main";
        let spec = code_and_file_spec(&["run"], &["read", "create"]);
        execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            CREATE_FILE_TOOL,
            &json!({
                "path": "/datasets/one.txt",
                "content": "one",
                "media_type": "text/plain"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let error = execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "1",
                "mounts": [
                    {
                        "uri": file_uri(namespace, "/datasets/one.txt"),
                        "mount_path": "input.txt"
                    },
                    {
                        "uri": file_uri(namespace, "/datasets/two.txt"),
                        "mount_path": "input.txt"
                    }
                ]
            }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("mount_path 'input.txt' is used more than once"),
            "{error}"
        );
    }

    #[test]
    fn default_code_mount_paths_do_not_use_dot_segments() {
        assert_eq!(
            default_code_mount_path("file://Tenant:test:Workspace:main/..", 7),
            PathBuf::from("mount-7")
        );
        assert_eq!(
            default_code_mount_path("file://Tenant:test:Workspace:main/.", 8),
            PathBuf::from("mount-8")
        );
        assert_eq!(
            default_code_mount_path("file://Tenant:test:Workspace:main/data.txt", 9),
            PathBuf::from("data.txt")
        );
    }

    #[test]
    fn code_virtual_mount_paths_use_forward_slashes() {
        let path = PathBuf::from("nested\\windows-name.txt");
        assert_eq!(
            code_virtual_mount_path(CODE_OUTPUT_MOUNT, &path),
            "/talon/output/nested/windows-name.txt"
        );
    }

    #[test]
    fn monty_object_to_json_rejects_non_finite_floats() {
        let error = monty_object_to_json(&monty_types::MontyObject::Float(f64::NAN))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("non-finite float not supported in talon_tool args"),
            "{error}"
        );
        assert!(monty_object_to_json(&monty_types::MontyObject::Float(f64::INFINITY)).is_err());
    }

    #[tokio::test]
    async fn code_artifact_mounts_use_existing_artifact_access_grants() {
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let namespace = "Tenant:test:Workspace:main";
        let writer = "writer";
        let writer_session = "session-1";
        let reader = "reader";
        let reader_session = "session-2";
        let artifact_output = execute_tool_for_session(
            &cp,
            namespace,
            writer,
            writer_session,
            &manifests::AgentSpec::default(),
            CREATE_ARTIFACT_TOOL,
            &json!({
                "title": "mountable.txt",
                "media_type": "text/plain",
                "content": "artifact contents"
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let artifact_output: Value = serde_json::from_str(&artifact_output).unwrap();
        let artifact_uri = artifact_output["artifactUri"].as_str().unwrap();
        let denied = read_code_mount_source(
            &cp,
            namespace,
            reader,
            reader_session,
            &code_spec(&["run"]),
            artifact_uri,
            MAX_CODE_INPUT_BYTES,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(denied.contains("artifact access denied"), "{denied}");

        execute_tool_for_session(
            &cp,
            namespace,
            writer,
            writer_session,
            &manifests::AgentSpec::default(),
            GRANT_ARTIFACT_TOOL,
            &json!({
                "artifact_uri": artifact_uri,
                "target_agent": reader,
                "target_session_id": reader_session,
                "operations": ["read"]
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let (bytes, media_type, source_kind) = read_code_mount_source(
            &cp,
            namespace,
            reader,
            reader_session,
            &code_spec(&["run"]),
            artifact_uri,
            MAX_CODE_INPUT_BYTES,
        )
        .await
        .unwrap();
        assert_eq!(bytes, b"artifact contents");
        assert_eq!(media_type, "text/plain");
        assert_eq!(source_kind, "artifact");
    }

    #[tokio::test]
    async fn code_mount_preflight_enforces_remaining_input_budget() {
        let store = Arc::new(InMemoryObjectStore::default());
        store
            .put(
                "mount-object",
                &[0_u8; 2],
                ObjectMetadata {
                    media_type: "application/octet-stream".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let cp = ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub))
            .objects(store)
            .build();

        let error = preflight_code_mount_size(&cp, "mount-object", 1)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("remaining 1 byte input budget"), "{error}");
    }

    #[tokio::test]
    async fn persist_code_outputs_rejects_oversized_file_before_reading() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("oversized.bin");
        let file = fs::File::create(&output_path).unwrap();
        file.set_len(MAX_CODE_OUTPUT_BYTES + 1).unwrap();

        let cp = control_plane(
            Arc::new(MockKvStore::default()),
            Arc::new(MockScheduler::default()),
        );
        let error = persist_code_outputs(
            &cp,
            "Tenant:test:Workspace:main",
            "analyst",
            "session-1",
            temp_dir.path(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("code outputs exceed"), "{error}");
    }

    #[tokio::test]
    async fn run_python_code_can_call_talon_tools_but_not_itself() {
        if !monty_runtime_available_for_test() {
            return;
        }
        let kv = Arc::new(MockKvStore::default());
        let scheduler = Arc::new(MockScheduler::default());
        let cp = control_plane(kv, scheduler);
        let namespace = "Tenant:test:Workspace:main";
        let spec = code_and_file_spec(&["run"], &["read", "create"]);
        execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            CREATE_FILE_TOOL,
            &json!({
                "path": "/datasets/tool.txt",
                "content": "hello tool",
                "media_type": "text/plain"
            }),
        )
        .await
        .unwrap()
        .unwrap();

        let output = execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "talon_tool('read_file', {'path': '/datasets/tool.txt'})['content']",
                "timeout_ms": 1000
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["value"], "hello tool");

        let error = execute_tool_for_session(
            &cp,
            namespace,
            "analyst",
            "session-1",
            &spec,
            RUN_PYTHON_CODE_TOOL,
            &json!({
                "code": "talon_tool('run_python_code', {'code': '1 + 1'})",
                "timeout_ms": 1000
            }),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("run_python_code cannot be called from code"));
    }

    fn monty_runtime_available_for_test() -> bool {
        if std::env::var_os("TALON_MONTY_BIN").is_some() {
            return true;
        }
        std::process::Command::new("sh")
            .arg("-c")
            .arg("command -v monty")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}
