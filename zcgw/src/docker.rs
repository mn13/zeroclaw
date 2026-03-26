use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DockerConfig {
    pub image: String,
    pub network: String,
    pub grpc_port: u16,
    pub memory_limit: String,
    pub env_vars: HashMap<String, String>,
    pub config_template_path: String,
    /// When true, the gateway is running outside Docker and needs host port
    /// mappings to reach agent containers. When false (gateway in Docker),
    /// agents are reached via container hostname on the Docker network.
    pub host_mode: bool,
    /// Base directory for agent data (gateway-local path).
    /// Each agent gets `<agents_dir>/<id>/`.
    /// Contains config.toml and data/ subdirectory.
    pub agents_dir: PathBuf,
    /// Host-side path to the agents directory, used for `docker run -v` bind mounts.
    /// When the gateway runs inside Docker, `agents_dir` is the container-internal
    /// path while `host_agents_dir` is the actual host filesystem path.
    /// When running on the host directly, this equals `agents_dir`.
    pub host_agents_dir: PathBuf,
    /// Base port for sequential host port assignment (default: 50051).
    pub base_port: u16,
    /// Directory containing workspace template files (TOOLS.md, AGENTS.md, etc.).
    /// Copied into new agent workspaces on first creation only.
    pub workspace_templates_dir: PathBuf,
}

/// Result of creating an agent container.
#[allow(dead_code)]
pub struct CreateAgentResult {
    pub container_id: String,
    /// The gRPC address the gateway should use to reach this agent.
    pub grpc_address: String,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image: "zeroclaw:latest".to_string(),
            network: "zeroclaw-net".to_string(),
            grpc_port: 50051,
            memory_limit: "512m".to_string(),
            env_vars: HashMap::new(),
            config_template_path: String::new(),
            host_mode: false,
            agents_dir: PathBuf::from("docker/agents"),
            host_agents_dir: PathBuf::from("docker/agents"),
            base_port: 50051,
            workspace_templates_dir: PathBuf::from("docker/config/workspace-templates"),
        }
    }
}

/// Information about a Docker container, parsed from `docker ps` JSON output.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ContainerInfo {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Names")]
    pub names: String,
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Image")]
    pub image: String,
}

/// List containers matching a label filter.
#[allow(dead_code)]
pub async fn list_containers(label_filter: &str) -> anyhow::Result<Vec<ContainerInfo>> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("label={}", label_filter),
            "--format",
            "{{json .}}",
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker ps failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut containers = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let info: ContainerInfo = serde_json::from_str(line)?;
        containers.push(info);
    }
    Ok(containers)
}

/// Resolve a container name/id from an instance id.
/// First tries the id directly, then falls back to label-based lookup
/// (needed for Docker Compose containers which have project-prefixed names).
async fn resolve_container(id: &str) -> anyhow::Result<String> {
    // Try direct name first
    let check = Command::new("docker")
        .args(["inspect", "--format", "{{.Name}}", id])
        .output()
        .await;
    if let Ok(out) = check {
        if out.status.success() {
            return Ok(id.to_string());
        }
    }

    // Fall back to label lookup
    let output = Command::new("docker")
        .args([
            "ps",
            "-aq",
            "--filter",
            &format!("label=zeroclaw.instance={}", id),
        ])
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("docker ps lookup failed for instance {}", id);
    }

    let container_id = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if container_id.is_empty() {
        anyhow::bail!("no container found for instance {}", id);
    }

    Ok(container_id)
}

/// Pick the next available sequential port starting from base_port.
/// Checks both the known `used_ports` list and whether the port is
/// actually free on the host (not bound by stale containers or other processes).
pub fn next_available_port(base_port: u16, used_ports: &[u16]) -> u16 {
    let mut port = base_port;
    loop {
        if !used_ports.contains(&port) && is_port_free(port) {
            return port;
        }
        port = port.checked_add(1).expect("port range exhausted");
    }
}

/// Check whether a TCP port is available by attempting to bind it.
fn is_port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Extract port number from a grpc_address like "http://localhost:50051".
pub fn port_from_address(addr: &str) -> Option<u16> {
    addr.rsplit(':').next()?.parse().ok()
}

/// Ensure the agent directory exists and write its config.
/// On first creation (workspace dir doesn't exist yet), copies template files
/// from `templates_dir` into the workspace. Existing workspaces are never overwritten.
async fn setup_agent_dir(
    agents_dir: &Path,
    id: &str,
    config_toml: &str,
    templates_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let agent_dir = agents_dir.join(id);
    let data_dir = agent_dir.join("data");
    let zc_dir = data_dir.join(".zeroclaw").join("workspace");
    let config_path = agent_dir.join("config.toml");

    let is_new = !zc_dir.exists();
    tokio::fs::create_dir_all(&zc_dir).await?;

    // Copy workspace templates only for brand-new agents.
    if is_new {
        if let Ok(mut entries) = tokio::fs::read_dir(templates_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let dest = zc_dir.join(entry.file_name());
                    if let Err(e) = tokio::fs::copy(&path, &dest).await {
                        warn!(
                            src = %path.display(),
                            dest = %dest.display(),
                            error = %e,
                            "failed to copy workspace template"
                        );
                    } else {
                        debug!(file = %entry.file_name().to_string_lossy(), "copied workspace template");
                    }
                }
            }
        } else {
            warn!(path = %templates_dir.display(), "workspace templates directory not found — skipping");
        }

        // Create standard subdirectories
        for subdir in &["sessions", "memory", "state", "cron", "skills"] {
            let _ = tokio::fs::create_dir_all(zc_dir.join(subdir)).await;
        }

        info!(agent = id, "provisioned new agent workspace from templates");
    }

    // Guard: if config.toml was auto-created as a directory by a stale Docker
    // bind mount, remove it so we can write the actual file.
    if let Ok(meta) = tokio::fs::metadata(&config_path).await {
        if meta.is_dir() {
            warn!(path = %config_path.display(), "config.toml is a directory — removing stale mount artifact");
            tokio::fs::remove_dir_all(&config_path).await?;
        }
    }

    tokio::fs::write(&config_path, config_toml).await?;
    debug!(path = %agent_dir.display(), "set up agent directory");

    Ok(agent_dir)
}

/// Create and start a new agent container.
pub async fn create_agent(
    id: &str,
    agent_config_toml: &str,
    host_port: u16,
    docker_config: &DockerConfig,
) -> anyhow::Result<CreateAgentResult> {
    let container_name = id.to_string();

    // Remove any stale container with the same name (stopped or dead)
    let _ = Command::new("docker")
        .args(["rm", "-f", &container_name])
        .output()
        .await;

    // Set up agent directory with config and data (templates copied only for new agents)
    let _agent_dir = setup_agent_dir(
        &docker_config.agents_dir,
        id,
        agent_config_toml,
        &docker_config.workspace_templates_dir,
    )
    .await?;

    // Use host-side paths for bind mounts (required when gateway runs inside Docker)
    let host_agent_dir = docker_config.host_agents_dir.join(id);
    let host_config = host_agent_dir.join("config.toml");
    let host_data = host_agent_dir.join("data");

    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        container_name.clone(),
        "--network".to_string(),
        docker_config.network.clone(),
        "--memory".to_string(),
        docker_config.memory_limit.clone(),
        "--label".to_string(),
        "zeroclaw.role=agent".to_string(),
        "--label".to_string(),
        "zeroclaw.managed=true".to_string(),
        "--label".to_string(),
        format!("zeroclaw.instance={}", id),
        "-v".to_string(),
        format!("{}:/data", host_data.display()),
        "-v".to_string(),
        format!("{}:/etc/zc/config.toml", host_config.display()),
    ];

    // Publish host port
    if docker_config.host_mode {
        args.push("-p".to_string());
        args.push(format!("{}:{}", host_port, docker_config.grpc_port));
    }

    // Pass through env vars (API keys, etc.)
    for (key, val) in &docker_config.env_vars {
        args.push("-e".to_string());
        args.push(format!("{}={}", key, val));
    }

    args.push(docker_config.image.clone());

    debug!(cmd = %format!("docker {}", args.join(" ")), "creating agent container");

    let output = Command::new("docker").args(&args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker run failed: {}", stderr);
    }

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Determine the gRPC address the gateway should use
    let grpc_address = if docker_config.host_mode {
        format!("http://localhost:{}", host_port)
    } else {
        format!("http://{}:{}", container_name, docker_config.grpc_port)
    };

    Ok(CreateAgentResult {
        container_id,
        grpc_address,
    })
}

/// Start an existing stopped container.
pub async fn start_agent(id: &str) -> anyhow::Result<()> {
    let container = resolve_container(id).await?;
    let output = Command::new("docker")
        .args(["start", &container])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker start failed: {}", stderr);
    }
    Ok(())
}

/// Stop a running container.
pub async fn stop_agent(id: &str) -> anyhow::Result<()> {
    let container = resolve_container(id).await?;
    let output = Command::new("docker")
        .args(["stop", &container])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker stop failed: {}", stderr);
    }
    Ok(())
}

/// Restart a running container, syncing the host config into the data directory first.
pub async fn restart_agent(id: &str, agents_dir: &Path) -> anyhow::Result<()> {
    // Sync host config → data dir so the agent picks up changes
    sync_agent_config(id, agents_dir).await;

    let container = resolve_container(id).await?;
    let output = Command::new("docker")
        .args(["restart", &container])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker restart failed: {}", stderr);
    }
    Ok(())
}

/// Stop and remove a container and clean up its agent directory.
pub async fn destroy_agent(id: &str, agents_dir: &Path) -> anyhow::Result<()> {
    let container = match resolve_container(id).await {
        Ok(c) => c,
        Err(_) => return Ok(()), // container doesn't exist, nothing to destroy
    };

    // Stop first (ignore errors — container may already be stopped).
    let _ = Command::new("docker")
        .args(["stop", &container])
        .output()
        .await;

    let output = Command::new("docker")
        .args(["rm", "-f", &container])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker rm failed: {}", stderr);
    }

    // Remove the agent directory
    let agent_dir = agents_dir.join(id);
    if agent_dir.exists() {
        if let Err(e) = tokio::fs::remove_dir_all(&agent_dir).await {
            warn!(id, error = %e, "failed to remove agent directory");
        }
    }

    // Also remove any leftover Docker volume
    let volume_name = format!("{}-data", id);
    let _ = Command::new("docker")
        .args(["volume", "rm", "-f", &volume_name])
        .output()
        .await;

    Ok(())
}

/// Get the status of a container (e.g. "running", "exited").
pub async fn get_container_status(id: &str) -> anyhow::Result<String> {
    let container = resolve_container(id).await?;
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &container])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker inspect failed: {}", stderr);
    }

    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(status)
}

/// Sync the host config into the agent's data directory so the container
/// picks up changes on next start/restart.
pub async fn sync_agent_config(id: &str, agents_dir: &Path) {
    let host_config = agents_dir.join(id).join("config.toml");
    let data_config = agents_dir
        .join(id)
        .join("data")
        .join(".zeroclaw")
        .join("config.toml");
    if host_config.exists() {
        if let Some(parent) = data_config.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::copy(&host_config, &data_config).await {
            warn!(id, error = %e, "failed to sync agent config");
        } else {
            debug!(id, "synced host config to data dir");
        }
    }
}

/// Ensure all instances in the config have Docker containers matching their
/// desired state. Creates missing containers, starts stopped ones (if desired
/// state is "running"), and leaves stopped ones alone (if desired state is
/// "stopped"). Updates grpc_address in config if a new container is created,
/// then persists the config.
pub async fn ensure_agents_from_config(
    config: &mut crate::config::GatewayConfig,
    config_path: &str,
    docker_config: &DockerConfig,
) {
    let ids: Vec<String> = config.instances.keys().cloned().collect();
    let mut config_changed = false;

    // Collect used ports for sequential allocation.
    let used_ports: Vec<u16> = config
        .instances
        .values()
        .filter_map(|c| port_from_address(&c.grpc_address))
        .collect();
    let mut allocated_ports = used_ports;

    for id in &ids {
        let instance = match config.instances.get(id) {
            Some(i) => i.clone(),
            None => continue,
        };

        // Sync host config → data dir
        sync_agent_config(id, &docker_config.agents_dir).await;

        let desired = &instance.desired_state;

        match resolve_container(id).await {
            Ok(container) => {
                // Container exists — check its state
                let status = Command::new("docker")
                    .args(["inspect", "--format", "{{.State.Status}}", &container])
                    .output()
                    .await;
                let state = status
                    .ok()
                    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                    .unwrap_or_default();

                match desired {
                    crate::config::DesiredState::Running if state != "running" => {
                        info!(id = %id, state = %state, "starting agent container");
                        let _ = start_agent(id).await;
                    }
                    crate::config::DesiredState::Stopped if state == "running" => {
                        info!(id = %id, "stopping agent container (desired: stopped)");
                        let _ = stop_agent(id).await;
                    }
                    _ => {
                        debug!(id = %id, state = %state, desired = %desired, "container state matches desired");
                    }
                }
            }
            Err(_) => {
                // No container exists
                if *desired == crate::config::DesiredState::Stopped {
                    debug!(id = %id, "no container, desired stopped — skipping");
                    continue;
                }

                // Create the container
                info!(id = %id, "creating missing agent container");

                // Read agent config from the agents dir, or fall back to template
                let agent_config_toml = {
                    let host_config = docker_config.agents_dir.join(id).join("config.toml");
                    if host_config.exists() {
                        tokio::fs::read_to_string(&host_config)
                            .await
                            .unwrap_or_default()
                    } else if !docker_config.config_template_path.is_empty() {
                        tokio::fs::read_to_string(&docker_config.config_template_path)
                            .await
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                };

                let host_port = next_available_port(docker_config.base_port, &allocated_ports);
                allocated_ports.push(host_port);

                match create_agent(id, &agent_config_toml, host_port, docker_config).await {
                    Ok(result) => {
                        info!(id = %id, addr = %result.grpc_address, "created agent container");
                        // Update grpc_address if it changed
                        if let Some(inst) = config.instances.get_mut(id) {
                            if inst.grpc_address != result.grpc_address {
                                inst.grpc_address = result.grpc_address;
                                config_changed = true;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(id = %id, error = %e, "failed to create agent container");
                    }
                }
            }
        }
    }

    if config_changed {
        if let Err(e) = config.save(config_path) {
            warn!(error = %e, "failed to persist updated config");
        }
    }
}
