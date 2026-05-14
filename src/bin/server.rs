use anyhow::Result;
use std::sync::Arc;
use talon::config::{Config, ConfigExt};
use talon::control::build_control_plane;
use talon::gateway::auth::AuthConfig;
use talon::gateway::server::Gateway;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    talon::security::install_jwt_crypto_provider();
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Talon Gateway Server...");
    let config = Config::load_default()?;
    let config = Arc::new(config);

    let auth_config = if let Ok(secret) = std::env::var("GATEWAY_JWT_SECRET") {
        AuthConfig::jwt(secret)
    } else if let Ok(token) = std::env::var("GATEWAY_TOKEN") {
        AuthConfig::tokens(vec![token])
    } else if let Ok(password) = std::env::var("GATEWAY_PASSWORD") {
        AuthConfig::password(password)
    } else {
        AuthConfig::open()
    };

    let cp = build_control_plane(&config).await?;
    let gateway = Gateway::new(Some(auth_config), cp.kv, cp.pubsub, cp.scheduler);

    let rpc_addr = std::env::var("GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    gateway.start_rpc_server(&rpc_addr).await?;

    tokio::signal::ctrl_c().await?;
    println!("Shutting down...");
    Ok(())
}
