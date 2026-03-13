use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tonic::transport::Server;
use zeroclaw::config::Config;
use zeroclaw::serve::grpc_server::ClawAgentService;
use zeroclaw::serve::proto::claw_agent_server::ClawAgentServer;
use zeroclaw::serve::session::SessionManager;

#[derive(Parser, Debug)]
#[command(name = "zc", about = "ZeroClaw gRPC agent server")]
struct Cli {
    /// gRPC listen port
    #[arg(long, default_value = "50051", env = "ZC_GRPC_PORT")]
    grpc_port: u16,

    /// Data directory for history and state
    #[arg(long, env = "ZC_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Path to config.toml override
    #[arg(long, short, env = "ZC_CONFIG")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing (respects RUST_LOG env).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load config.
    let config = Config::load_or_init()
        .await
        .context("failed to load ZeroClaw config")?;

    // Determine data directory: CLI flag > workspace_dir/zc-data > ~/.zeroclaw/zc-data
    let data_dir = cli
        .data_dir
        .unwrap_or_else(|| config.workspace_dir.join("zc-data"));
    tokio::fs::create_dir_all(&data_dir)
        .await
        .context("failed to create data directory")?;

    tracing::info!(
        data_dir = %data_dir.display(),
        grpc_port = cli.grpc_port,
        model = config.default_model.as_deref().unwrap_or("default"),
        provider = config.default_provider.as_deref().unwrap_or("openrouter"),
        "starting zc server"
    );

    // Create the session manager (spawns the agent actor).
    let session = SessionManager::new(config, data_dir).context("failed to create session")?;

    // Build the gRPC server.
    let service = ClawAgentService::new(session);
    let addr = format!("0.0.0.0:{}", cli.grpc_port)
        .parse()
        .context("invalid listen address")?;

    tracing::info!(%addr, "gRPC server listening");

    // Serve with graceful shutdown on SIGTERM / SIGINT.
    Server::builder()
        .add_service(ClawAgentServer::new(service))
        .serve_with_shutdown(addr, shutdown_signal())
        .await
        .context("gRPC server error")?;

    tracing::info!("zc server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => tracing::info!("received SIGINT"),
            _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
        tracing::info!("received SIGINT");
    }
}
