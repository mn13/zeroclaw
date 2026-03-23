//! signal-cli daemon lifecycle management.
//!
//! The gateway bundles the signal-cli native binary and manages it as a child
//! process.  Agents connect to its HTTP API (SSE + JSON-RPC) over the Docker
//! network at `http://gateway:<port>`.

use crate::app_state::{SignalCliConfig, SignalConnectionsStore};
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Start the signal-cli daemon as a child process.
///
/// Binds to `0.0.0.0:<port>` so agent containers on the Docker network can
/// reach it.  Account data is stored in `config.data_dir`.
pub async fn start_daemon(config: &SignalCliConfig, handle: &Arc<Mutex<Option<Child>>>) {
    let mut guard = handle.lock().await;

    // If already running, don't start another.
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => {
                info!("signal-cli daemon already running");
                return;
            }
            _ => {
                // Process exited or errored — we'll restart below.
            }
        }
    }

    let listen_addr = format!("0.0.0.0:{}", config.http_port);
    info!(
        cli = %config.cli_path,
        listen = %listen_addr,
        data_dir = %config.data_dir.display(),
        "starting signal-cli daemon"
    );

    match tokio::process::Command::new(&config.cli_path)
        .args([
            "--config",
            &config.data_dir.to_string_lossy(),
            "daemon",
            "--http",
            &listen_addr,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => {
            info!(pid = child.id(), "signal-cli daemon started");
            *guard = Some(child);
        }
        Err(e) => {
            error!(error = %e, "failed to start signal-cli daemon — is the binary installed?");
        }
    }
}

/// Stop the signal-cli daemon if running.
pub async fn stop_daemon(handle: &Arc<Mutex<Option<Child>>>) {
    let mut guard = handle.lock().await;
    if let Some(ref mut child) = *guard {
        info!(pid = child.id(), "stopping signal-cli daemon");
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    *guard = None;
}

/// Supervisor loop: checks every 10 seconds if the daemon should be running
/// (i.e. connections exist) and restarts it if it has crashed.
pub async fn supervise_daemon(
    config: &SignalCliConfig,
    handle: &Arc<Mutex<Option<Child>>>,
    connections: &Arc<tokio::sync::RwLock<SignalConnectionsStore>>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    let mut consecutive_failures: u32 = 0;

    loop {
        interval.tick().await;

        let has_connections = {
            let store = connections.read().await;
            !store.connections.is_empty()
        };

        if !has_connections {
            // No connections — daemon should not be running.
            let guard = handle.lock().await;
            if guard.is_some() {
                drop(guard);
                stop_daemon(handle).await;
            }
            consecutive_failures = 0;
            continue;
        }

        // Check if the daemon is still alive.
        let needs_restart = {
            let mut guard = handle.lock().await;
            match *guard {
                None => true,
                Some(ref mut child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        warn!(status = %status, "signal-cli daemon exited unexpectedly");
                        *guard = None;
                        true
                    }
                    Ok(None) => {
                        // Still running.
                        consecutive_failures = 0;
                        false
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to check signal-cli daemon status");
                        false
                    }
                },
            }
        };

        if needs_restart {
            consecutive_failures += 1;
            // Exponential backoff: 2^failures seconds, capped at 60.
            let backoff_secs = (2u64.pow(consecutive_failures)).min(60);
            if consecutive_failures > 1 {
                warn!(
                    attempt = consecutive_failures,
                    backoff_secs, "restarting signal-cli daemon with backoff"
                );
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            }
            start_daemon(config, handle).await;
        }
    }
}

/// Check whether the signal-cli HTTP daemon is reachable.
pub async fn health_check(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/api/v1/check", port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let Ok(client) = client else { return false };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}
