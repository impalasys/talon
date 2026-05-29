// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
#[cfg(feature = "heap-profile")]
use std::path::PathBuf;

fn env_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(feature = "heap-profile")]
fn program_name() -> String {
    std::env::args()
        .next()
        .and_then(|arg| {
            std::path::Path::new(&arg)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "talon".to_string())
}

#[cfg(feature = "cpu-profile")]
pub fn init_cpu_profiler_from_env<F>(mut get: F) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
{
    if !get("TALON_CPU_PROFILE_ENABLED")
        .as_deref()
        .map(env_enabled)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let path = get("TALON_CPU_PROFILE_PATH")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/tmp/talon-worker-cpu.svg".to_string());
    let seconds = get("TALON_CPU_PROFILE_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(20);
    let delay_seconds = get("TALON_CPU_PROFILE_DELAY_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let frequency_hz = get("TALON_CPU_PROFILE_FREQUENCY_HZ")
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(99);

    std::thread::Builder::new()
        .name("talon-cpu-profiler".to_string())
        .spawn(move || {
            if delay_seconds > 0 {
                std::thread::sleep(std::time::Duration::from_secs(delay_seconds));
            }

            let guard = match pprof::ProfilerGuard::new(frequency_hz) {
                Ok(guard) => guard,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to start CPU profiler");
                    return;
                }
            };

            tracing::info!(
                path = %path,
                seconds,
                frequency_hz,
                "Started CPU profiler"
            );
            std::thread::sleep(std::time::Duration::from_secs(seconds));

            let report = match guard.report().build() {
                Ok(report) => report,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to build CPU profile report");
                    return;
                }
            };

            if let Some(parent) = std::path::Path::new(&path).parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    tracing::warn!(path = %path, error = %err, "Failed to create CPU profile directory");
                    return;
                }
            }

            match std::fs::File::create(&path) {
                Ok(file) => {
                    if let Err(err) = report.flamegraph(file) {
                        tracing::warn!(path = %path, error = %err, "Failed to write CPU flamegraph");
                    } else {
                        tracing::info!(path = %path, "Wrote CPU flamegraph");
                    }
                }
                Err(err) => {
                    tracing::warn!(path = %path, error = %err, "Failed to create CPU flamegraph");
                }
            }
        })?;

    Ok(())
}

#[cfg(feature = "heap-profile")]
pub fn init_heap_profiler_from_env<F>(mut get: F) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
{
    if !get("TALON_HEAP_PROFILE_ENABLED")
        .as_deref()
        .map(env_enabled)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let dir = get("TALON_HEAP_PROFILE_DIR")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/tmp".to_string());
    let delay_seconds = get("TALON_HEAP_PROFILE_DELAY_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let label = get("TALON_HEAP_PROFILE_LABEL")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "heap".to_string());
    let program = program_name();
    let heap_path = PathBuf::from(&dir).join(format!("{program}-{label}.heap"));
    let stats_path = PathBuf::from(dir).join(format!("{program}-{label}.json"));

    std::thread::Builder::new()
        .name("talon-heap-profiler".to_string())
        .spawn(move || {
            if delay_seconds > 0 {
                std::thread::sleep(std::time::Duration::from_secs(delay_seconds));
            }

            if let Some(parent) = heap_path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    tracing::warn!(
                        path = %heap_path.display(),
                        error = %err,
                        "Failed to create heap profile directory"
                    );
                    return;
                }
            }

            match write_heap_profile(&heap_path) {
                Ok(()) => tracing::info!(path = %heap_path.display(), "Wrote heap profile"),
                Err(err) => {
                    tracing::warn!(
                        path = %heap_path.display(),
                        error = %err,
                        "Failed to write heap profile"
                    );
                    return;
                }
            }

            match allocator_stats_json() {
                Ok(stats) => {
                    if let Err(err) = std::fs::write(&stats_path, stats) {
                        tracing::warn!(
                            path = %stats_path.display(),
                            error = %err,
                            "Failed to write heap profile stats"
                        );
                    } else {
                        tracing::info!(path = %stats_path.display(), "Wrote heap profile stats");
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to read heap profile stats");
                }
            }
        })?;

    Ok(())
}

#[cfg(feature = "heap-profile")]
fn write_heap_profile(path: &std::path::Path) -> Result<()> {
    let path = std::ffi::CString::new(path.to_string_lossy().as_bytes())?;
    unsafe {
        tikv_jemalloc_ctl::raw::write::<*const std::ffi::c_char>(b"prof.dump\0", path.as_ptr())
            .map_err(|err| anyhow::anyhow!("{err:?}"))?;
    }
    Ok(())
}

#[cfg(feature = "heap-profile")]
fn allocator_stats_json() -> Result<Vec<u8>> {
    use tikv_jemalloc_ctl::{epoch, stats};

    epoch::advance().map_err(|err| anyhow::anyhow!("{err:?}"))?;
    let payload = serde_json::json!({
        "allocated_bytes": stats::allocated::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
        "active_bytes": stats::active::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
        "metadata_bytes": stats::metadata::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
        "resident_bytes": stats::resident::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
        "mapped_bytes": stats::mapped::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
        "retained_bytes": stats::retained::read().map_err(|err| anyhow::anyhow!("{err:?}"))?,
    });
    Ok(serde_json::to_vec_pretty(&payload)?)
}

#[cfg(not(feature = "heap-profile"))]
pub fn init_heap_profiler_from_env<F>(mut get: F) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
{
    if get("TALON_HEAP_PROFILE_ENABLED")
        .as_deref()
        .map(env_enabled)
        .unwrap_or(false)
    {
        tracing::warn!(
            "TALON_HEAP_PROFILE_ENABLED is set, but Talon was built without heap-profile"
        );
    }
    Ok(())
}

#[cfg(not(feature = "cpu-profile"))]
pub fn init_cpu_profiler_from_env<F>(mut get: F) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
{
    if get("TALON_CPU_PROFILE_ENABLED")
        .as_deref()
        .map(env_enabled)
        .unwrap_or(false)
    {
        tracing::warn!("TALON_CPU_PROFILE_ENABLED is set, but Talon was built without cpu-profile");
    }
    Ok(())
}
