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

    // If a config path is provided, point ZEROCLAW_CONFIG_DIR at its parent
    // so that Config::load_or_init() picks it up.
    if let Some(ref config_path) = cli.config {
        if let Some(parent) = config_path.parent() {
            std::env::set_var("ZEROCLAW_CONFIG_DIR", parent);
        }
    }

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

    // ── Spawn daemon components (channels, cron, heartbeat) ──
    // These run alongside the gRPC server to provide the full agent runtime.
    let daemon_handle = {
        let daemon_cfg = config.clone();
        tokio::spawn(async move {
            run_daemon_components(daemon_cfg).await;
        })
    };

    // ── gRPC server ──
    let session = SessionManager::new(config, data_dir).context("failed to create session")?;
    let service = ClawAgentService::new(session);
    let addr = format!("0.0.0.0:{}", cli.grpc_port)
        .parse()
        .context("invalid listen address")?;

    tracing::info!(%addr, "gRPC server listening");

    Server::builder()
        .add_service(ClawAgentServer::new(service))
        .serve_with_shutdown(addr, shutdown_signal())
        .await
        .context("gRPC server error")?;

    // Clean up daemon components on shutdown.
    daemon_handle.abort();
    let _ = daemon_handle.await;

    tracing::info!("zc server shut down gracefully");
    Ok(())
}

/// Run all daemon components that don't conflict with the gRPC server.
/// This gives Docker containers the same runtime capabilities as `zeroclaw daemon`:
/// supervised channel listeners, cron scheduler, and heartbeat worker.
async fn run_daemon_components(config: Config) {
    let initial_backoff = config.reliability.channel_initial_backoff_secs.max(1);
    let max_backoff = config
        .reliability
        .channel_max_backoff_secs
        .max(initial_backoff);

    zeroclaw::health::mark_component_ok("daemon");

    let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    // Channel listeners (Telegram, Discord, Slack, etc.)
    if has_channels(&config) {
        let channels_cfg = config.clone();
        tracing::info!("channels configured — starting supervised channel listeners");
        handles.push(spawn_supervised(
            "channels",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = channels_cfg.clone();
                async move { zeroclaw::channels::start_channels(cfg).await }
            },
        ));
    } else {
        zeroclaw::health::mark_component_ok("channels");
        tracing::info!("no channels configured — channel supervisor disabled");
    }

    // Cron scheduler
    if config.cron.enabled {
        let scheduler_cfg = config.clone();
        tracing::info!("cron enabled — starting scheduler");
        handles.push(spawn_supervised(
            "scheduler",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = scheduler_cfg.clone();
                async move { zeroclaw::cron::scheduler::run(cfg).await }
            },
        ));
    } else {
        zeroclaw::health::mark_component_ok("scheduler");
        tracing::info!("cron disabled — scheduler not started");
    }

    // Heartbeat worker
    if config.heartbeat.enabled {
        let heartbeat_cfg = config.clone();
        tracing::info!("heartbeat enabled — starting heartbeat worker");
        handles.push(spawn_supervised(
            "heartbeat",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = heartbeat_cfg.clone();
                async move { run_heartbeat(cfg).await }
            },
        ));
    } else {
        zeroclaw::health::mark_component_ok("heartbeat");
    }

    // Wait for all handles (they loop forever unless aborted).
    for handle in handles {
        let _ = handle.await;
    }
}

fn has_channels(config: &Config) -> bool {
    config
        .channels_config
        .channels_except_webhook()
        .iter()
        .any(|(_, ok)| *ok)
}

/// Spawn a supervised component that auto-restarts on failure with exponential backoff.
fn spawn_supervised<F, Fut>(
    name: &'static str,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
    mut run_component: F,
) -> tokio::task::JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);

        loop {
            zeroclaw::health::mark_component_ok(name);
            match run_component().await {
                Ok(()) => {
                    zeroclaw::health::mark_component_error(name, "component exited unexpectedly");
                    tracing::warn!("Component '{name}' exited unexpectedly");
                    backoff = initial_backoff_secs.max(1);
                }
                Err(e) => {
                    zeroclaw::health::mark_component_error(name, e.to_string());
                    tracing::error!("Component '{name}' failed: {e}");
                }
            }

            zeroclaw::health::bump_component_restart(name);
            tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
            backoff = backoff.saturating_mul(2).min(max_backoff);
        }
    })
}

/// Heartbeat worker — runs periodic tasks from HEARTBEAT.md or config fallback.
async fn run_heartbeat(config: Config) -> anyhow::Result<()> {
    let observer: std::sync::Arc<dyn zeroclaw::observability::Observer> =
        std::sync::Arc::from(zeroclaw::observability::create_observer(
            &config.observability,
        ));
    let engine = zeroclaw::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
        observer,
    );

    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval =
        tokio::time::interval(tokio::time::Duration::from_secs(u64::from(interval_mins) * 60));

    loop {
        interval.tick().await;

        let file_tasks = engine.collect_tasks().await?;
        let tasks = heartbeat_tasks(file_tasks, config.heartbeat.message.as_deref());
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = config.default_temperature;
            match zeroclaw::agent::run(
                config.clone(),
                Some(prompt),
                None,
                None,
                temp,
                vec![],
                false,
            )
            .await
            {
                Ok(output) => {
                    zeroclaw::health::mark_component_ok("heartbeat");
                    let announcement = if output.trim().is_empty() {
                        "heartbeat task executed".to_string()
                    } else {
                        output
                    };
                    if let Some((channel, target)) = heartbeat_delivery(&config) {
                        if let Err(e) = zeroclaw::cron::scheduler::deliver_announcement(
                            &config,
                            &channel,
                            &target,
                            &announcement,
                        )
                        .await
                        {
                            zeroclaw::health::mark_component_error(
                                "heartbeat",
                                format!("delivery failed: {e}"),
                            );
                            tracing::warn!("Heartbeat delivery failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    zeroclaw::health::mark_component_error("heartbeat", e.to_string());
                    tracing::warn!("Heartbeat task failed: {e}");
                }
            }
        }
    }
}

fn heartbeat_tasks(file_tasks: Vec<String>, fallback_message: Option<&str>) -> Vec<String> {
    if !file_tasks.is_empty() {
        return file_tasks;
    }
    fallback_message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| vec![m.to_string()])
        .unwrap_or_default()
}

fn heartbeat_delivery(config: &Config) -> Option<(String, String)> {
    let channel = config
        .heartbeat
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    let target = config
        .heartbeat
        .to
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    Some((channel.to_string(), target.to_string()))
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
