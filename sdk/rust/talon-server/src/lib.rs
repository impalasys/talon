// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub talon_node_path: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub config: Option<Value>,
    pub data_dir: Option<PathBuf>,
    pub grpc_port: Option<u16>,
    pub ui_port: Option<u16>,
    pub keep_temp_dir: bool,
    pub env: HashMap<String, String>,
    pub startup_timeout: Duration,
    pub provider: Option<Provider>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            talon_node_path: None,
            config_path: None,
            config: None,
            data_dir: None,
            grpc_port: None,
            ui_port: None,
            keep_temp_dir: false,
            env: HashMap::new(),
            startup_timeout: Duration::from_secs(30),
            provider: None,
        }
    }
}

pub struct Server {
    child: Child,
    temp_dir: Option<tempfile::TempDir>,
    persisted_temp_dir: Option<PathBuf>,
    config_path: PathBuf,
    grpc_port: u16,
    ui_port: u16,
    logs: Arc<Mutex<String>>,
}

impl Server {
    pub fn start(options: Options) -> Result<Self> {
        if options.config_path.is_some()
            && (options.config.is_some()
                || options.data_dir.is_some()
                || options.provider.is_some())
        {
            return Err(anyhow!(
                "config_path cannot be combined with config, data_dir, or provider; put those settings in the config file"
            ));
        }
        if options.config.is_some() && options.provider.is_some() {
            return Err(anyhow!(
                "config cannot be combined with provider; put providers in the config object"
            ));
        }
        let talon_node = resolve_talon_node(options.talon_node_path.as_deref())?;
        let grpc_port = options.grpc_port.unwrap_or(free_port()?);
        let ui_port = options.ui_port.unwrap_or(free_port()?);
        let temp_dir = tempfile::Builder::new().prefix("talon-server-").tempdir()?;
        let config_path = if let Some(path) = options.config_path {
            absolute_path(path)?
        } else {
            let data_dir = options.data_dir.map(absolute_path).transpose()?;
            let config = if let Some(config) = options.config {
                config_with_data_dir(config, data_dir.as_deref())
            } else {
                let default_data_dir;
                let data_dir = match data_dir.as_deref() {
                    Some(data_dir) => data_dir,
                    None => {
                        default_data_dir = temp_dir.path().join("data");
                        &default_data_dir
                    }
                };
                default_config(options.provider.as_ref(), data_dir)
            };
            if let Some(data_dir) = control_plane_data_dir(&config) {
                let data_dir = Path::new(data_dir);
                let data_dir = if data_dir.is_absolute() {
                    data_dir.to_path_buf()
                } else {
                    temp_dir.path().join(data_dir)
                };
                std::fs::create_dir_all(data_dir)?;
            }
            let config_path = temp_dir.path().join("talon.json");
            std::fs::write(&config_path, serde_json::to_string_pretty(&config)? + "\n")?;
            config_path
        };

        let mut command = Command::new(talon_node);
        command
            .env("GRPC_ADDR", format!("127.0.0.1:{grpc_port}"))
            .env("GATEWAY_UI_ADDR", format!("127.0.0.1:{ui_port}"))
            .env("TALON_CONFIG_PATH", &config_path)
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &options.env {
            command.env(key, value);
        }

        let mut child = command.spawn().context("failed to start talon-node")?;
        let logs = Arc::new(Mutex::new(String::new()));
        capture_logs(child.stdout.take(), Arc::clone(&logs));
        capture_logs(child.stderr.take(), Arc::clone(&logs));
        wait_for_port(grpc_port, options.startup_timeout).with_context(|| {
            format!(
                "talon-node did not become ready; logs:\n{}",
                read_logs(&logs)
            )
        })?;

        let (temp_dir, persisted_temp_dir) = if options.keep_temp_dir {
            #[allow(deprecated)]
            let path = temp_dir.into_path();
            (None, Some(path))
        } else {
            (Some(temp_dir), None)
        };
        Ok(Self {
            child,
            temp_dir,
            persisted_temp_dir,
            config_path,
            grpc_port,
            ui_port,
            logs,
        })
    }

    pub fn grpc_endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.grpc_port)
    }

    pub fn ui_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.ui_port)
    }

    pub fn temp_dir(&self) -> &Path {
        self.persisted_temp_dir
            .as_deref()
            .unwrap_or_else(|| self.temp_dir.as_ref().expect("temp dir").path())
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn logs(&self) -> String {
        read_logs(&self.logs)
    }

    pub fn stop(mut self) -> Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        Ok(())
    }
}

pub fn authorization_header(token: &str) -> Result<String> {
    if token.trim().is_empty() {
        return Err(anyhow!("token is required"));
    }
    Ok(format!("Bearer {token}"))
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn resolve_talon_node(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Ok(path) = std::env::var("TALON_NODE_PATH") {
        return Ok(PathBuf::from(path));
    }
    let platform = platform_name()?;
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bin")
        .join(platform)
        .join("talon-node");
    if bundled.exists() {
        return Ok(bundled);
    }
    Err(anyhow!(
        "talon-node binary not found; set TALON_NODE_PATH or bundle {}",
        bundled.display()
    ))
}

fn platform_name() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("linux-x64"),
        ("macos", "aarch64") => Ok("darwin-arm64"),
        (os, arch) => Err(anyhow!("unsupported talon-node platform: {os}-{arch}")),
    }
}

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn wait_for_port(port: u16, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!("timeout waiting for 127.0.0.1:{port}"))
}

fn capture_logs<R: Read + Send + 'static>(reader: Option<R>, logs: Arc<Mutex<String>>) {
    if let Some(mut reader) = reader {
        std::thread::spawn(move || {
            let mut buffer = String::new();
            let _ = reader.read_to_string(&mut buffer);
            if let Ok(mut logs) = logs.lock() {
                logs.push_str(&buffer);
            }
        });
    }
}

fn read_logs(logs: &Arc<Mutex<String>>) -> String {
    logs.lock().map(|logs| logs.clone()).unwrap_or_default()
}

fn default_config(provider: Option<&Provider>, data_dir: &Path) -> Value {
    let mut config = json!({
        "control_plane": {
            "database": {
                "driver": "sqlite",
                "data_dir": data_dir.display().to_string(),
            },
            "message_broker": {
                "driver": "local_socket",
            },
        },
    });
    if let Some(provider) = provider {
        let name = if provider.name.is_empty() {
            "mock"
        } else {
            provider.name.as_str()
        };
        if let Some(object) = config.as_object_mut() {
            object.insert(
                "providers".to_string(),
                json!({
                    name: {
                        "type": "openai_compatible",
                        "base_url": provider.base_url,
                        "model": provider.model,
                        "api_key": provider.api_key,
                    },
                }),
            );
            object.insert("default_provider".to_string(), json!(name));
        }
    }
    config
}

fn config_with_data_dir(mut config: Value, data_dir: Option<&Path>) -> Value {
    let Some(data_dir) = data_dir else {
        return config;
    };
    if !config.is_object() {
        config = json!({});
    }
    let object = config.as_object_mut().expect("object config");
    let control_plane = object.entry("control_plane").or_insert_with(|| json!({}));
    if !control_plane.is_object() {
        *control_plane = json!({});
    }
    let control_plane = control_plane.as_object_mut().expect("object control_plane");
    let database = control_plane.entry("database").or_insert_with(|| json!({}));
    if !database.is_object() {
        *database = json!({});
    }
    database.as_object_mut().expect("object database").insert(
        "data_dir".to_string(),
        json!(data_dir.display().to_string()),
    );
    config
}

fn control_plane_data_dir(config: &Value) -> Option<&str> {
    config
        .get("control_plane")?
        .get("database")?
        .get("data_dir")?
        .as_str()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_config_uses_requested_data_dir() {
        let config = default_config(None, Path::new("/tmp/talon-data"));
        assert_eq!(
            config["control_plane"]["database"]["driver"].as_str(),
            Some("sqlite")
        );
        assert_eq!(
            config["control_plane"]["database"]["data_dir"].as_str(),
            Some("/tmp/talon-data")
        );
        assert_eq!(
            config["control_plane"]["message_broker"]["driver"].as_str(),
            Some("local_socket")
        );
    }

    #[test]
    fn config_can_specify_general_talon_settings() {
        let config = config_with_data_dir(
            json!({
                "workspace_dir": "/tmp/workspace",
                "default_provider": "openai",
                "control_plane": {
                    "database": {"driver": "sqlite"},
                    "message_broker": {"driver": "local_socket"},
                },
            }),
            None,
        );
        assert_eq!(config["workspace_dir"].as_str(), Some("/tmp/workspace"));
        assert_eq!(config["default_provider"].as_str(), Some("openai"));
    }
}
