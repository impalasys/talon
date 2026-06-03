use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

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
    pub grpc_port: Option<u16>,
    pub ui_port: Option<u16>,
    pub keep_temp_dir: bool,
    pub env: HashMap<String, String>,
    pub startup_timeout: Duration,
    pub provider: Option<Provider>,
    pub jwt_secret: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            talon_node_path: None,
            grpc_port: None,
            ui_port: None,
            keep_temp_dir: false,
            env: HashMap::new(),
            startup_timeout: Duration::from_secs(30),
            provider: None,
            jwt_secret: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct JwtOptions {
    pub subject: String,
    pub ttl: Duration,
    pub namespace: Option<String>,
    pub agent: Option<String>,
    pub session: Option<String>,
    pub channel: Option<String>,
}

impl Default for JwtOptions {
    fn default() -> Self {
        Self {
            subject: "talon-sdk".to_string(),
            ttl: Duration::from_secs(3600),
            namespace: None,
            agent: None,
            session: None,
            channel: None,
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
        let talon_node = resolve_talon_node(options.talon_node_path.as_deref())?;
        let grpc_port = options.grpc_port.unwrap_or(free_port()?);
        let ui_port = options.ui_port.unwrap_or(free_port()?);
        let temp_dir = tempfile::Builder::new().prefix("talon-server-").tempdir()?;
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir)?;
        let config_path = temp_dir.path().join("talon.yaml");
        std::fs::write(&config_path, config_yaml(options.provider.as_ref()))?;

        let mut command = Command::new(talon_node);
        command
            .env("GRPC_ADDR", format!("127.0.0.1:{grpc_port}"))
            .env("GATEWAY_UI_ADDR", format!("127.0.0.1:{ui_port}"))
            .env("TALON_CONFIG_PATH", &config_path)
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(secret) = &options.jwt_secret {
            if !secret.is_empty() {
                command.env("GATEWAY_JWT_SECRET", secret);
            }
        }
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

pub fn mint_jwt(secret: &str, options: JwtOptions) -> Result<String> {
    if secret.is_empty() {
        return Err(anyhow!("secret is required"));
    }
    if options.subject.trim().is_empty() {
        return Err(anyhow!("subject is required"));
    }
    if options.ttl.is_zero() {
        return Err(anyhow!("ttl must be positive"));
    }
    if options.channel.is_some() && options.namespace.is_none() {
        return Err(anyhow!("channel-scoped JWTs require namespace"));
    }

    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .checked_add(options.ttl)
        .ok_or_else(|| anyhow!("ttl is too large"))?
        .as_secs();
    let mut claims = Map::new();
    claims.insert("sub".to_string(), json!(options.subject));
    claims.insert("aud".to_string(), json!("talon"));
    claims.insert("exp".to_string(), json!(exp));
    add_jwt_claim(&mut claims, "talon:ns", options.namespace)?;
    add_jwt_claim(&mut claims, "talon:agent", options.agent)?;
    add_jwt_claim(&mut claims, "talon:session", options.session)?;
    add_jwt_claim(&mut claims, "talon:channel", options.channel)?;

    let header = jwt_segment(&json!({"alg": "HS256", "typ": "JWT"}))?;
    let payload = jwt_segment(&Value::Object(claims))?;
    let message = format!("{header}.{payload}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(message.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{message}.{signature}"))
}

pub fn authorization_header(token: &str) -> Result<String> {
    if token.trim().is_empty() {
        return Err(anyhow!("token is required"));
    }
    Ok(format!("Bearer {token}"))
}

fn add_jwt_claim(claims: &mut Map<String, Value>, key: &str, value: Option<String>) -> Result<()> {
    if let Some(value) = value {
        if value.trim().is_empty() {
            return Err(anyhow!("{key} must not be empty"));
        }
        claims.insert(key.to_string(), json!(value));
    }
    Ok(())
}

fn jwt_segment(value: &Value) -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?))
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

fn config_yaml(provider: Option<&Provider>) -> String {
    let mut yaml = String::new();
    if let Some(provider) = provider {
        let name = if provider.name.is_empty() {
            "mock"
        } else {
            provider.name.as_str()
        };
        yaml.push_str(&format!(
            "providers:\n  {name}:\n    type: openai_compatible\n    base_url: {:?}\n    model: {:?}\n    api_key: {:?}\ndefault_provider: {:?}\n",
            provider.base_url, provider.model, provider.api_key, name
        ));
    }
    yaml.push_str(
        "control_plane:\n  database:\n    driver: sqlite\n    data_dir: ./data\n  message_broker:\n    driver: local_socket\n",
    );
    yaml
}
