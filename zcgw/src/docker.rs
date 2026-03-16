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
    /// Base directory for agent data. Each agent gets `<agents_dir>/<id>/`.
    /// Contains config.toml and data/ subdirectory (bind-mounted into container).
    pub agents_dir: PathBuf,
    /// Base port for sequential host port assignment (default: 50051).
    pub base_port: u16,
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
            base_port: 50051,
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
/// Scans used_ports to find the first gap.
pub fn next_available_port(base_port: u16, used_ports: &[u16]) -> u16 {
    let mut port = base_port;
    loop {
        if !used_ports.contains(&port) {
            return port;
        }
        port += 1;
    }
}

/// Extract port number from a grpc_address like "http://localhost:50051".
pub fn port_from_address(addr: &str) -> Option<u16> {
    addr.rsplit(':').next()?.parse().ok()
}

/// Ensure the agent directory exists and write its config.
async fn setup_agent_dir(agents_dir: &Path, id: &str, config_toml: &str) -> anyhow::Result<PathBuf> {
    let agent_dir = agents_dir.join(id);
    let data_dir = agent_dir.join("data");
    let config_path = agent_dir.join("config.toml");

    tokio::fs::create_dir_all(&data_dir).await?;
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

    // Set up agent directory with config and data
    let agent_dir = setup_agent_dir(&docker_config.agents_dir, id, agent_config_toml).await?;
    let config_path = agent_dir.join("config.toml");
    let data_dir = agent_dir.join("data");

    // Use absolute paths for bind mounts
    let config_abs = tokio::fs::canonicalize(&config_path).await?;
    let data_abs = tokio::fs::canonicalize(&data_dir).await?;

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
        format!("{}:/data", data_abs.display()),
        "-v".to_string(),
        format!("{}:/etc/zc/config.toml:ro", config_abs.display()),
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

/// Ensure all registered agents have running containers.
/// Tries to start stopped containers. Skips agents without containers
/// (e.g. compose-managed agents that are already running).
pub async fn ensure_agents_running(instance_ids: &[String]) {
    for id in instance_ids {
        match resolve_container(id).await {
            Ok(container) => {
                // Check if it's running
                let status = Command::new("docker")
                    .args(["inspect", "--format", "{{.State.Status}}", &container])
                    .output()
                    .await;
                if let Ok(out) = status {
                    let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if state != "running" {
                        info!(id, state = %state, "starting stopped agent container");
                        let _ = start_agent(id).await;
                    }
                }
            }
            Err(_) => {
                // No container exists — might be a compose-managed agent, skip
                debug!(id, "no container found, skipping auto-start");
            }
        }
    }
}
