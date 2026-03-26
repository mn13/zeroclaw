use crate::app_state::AppState;
use crate::config::{DesiredState, GatewayConfig, InstanceConfig};
use crate::docker;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::error;

/// Persist the desired state of an instance to zcgw.toml.
async fn persist_desired_state(
    config_path: &str,
    id: &str,
    state: DesiredState,
) -> Result<(), StatusCode> {
    let mut gw_config =
        GatewayConfig::load(config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(instance) = gw_config.instances.get_mut(id) {
        instance.desired_state = state;
    }
    gw_config
        .save(config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

// ---------- Stats ----------

pub async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = chrono::Utc::now()
        .signed_duration_since(state.started_at)
        .num_seconds();
    let (total, healthy, unhealthy) = state.registry.instance_count().await;
    Json(serde_json::json!({
        "uptime_secs": uptime,
        "started_at": state.started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "total_instances": total,
        "healthy_count": healthy,
        "unhealthy_count": unhealthy,
    }))
}

// ---------- Gateway Config ----------

pub async fn get_config(State(state): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let content =
        tokio::fs::read_to_string(&state.config_path)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "raw": content })))
}

#[derive(Deserialize)]
pub struct UpdateConfigBody {
    pub raw: String,
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(body): Json<UpdateConfigBody>,
) -> Result<impl IntoResponse, StatusCode> {
    // Validate by parsing
    let _parsed: GatewayConfig =
        toml::from_str(&body.raw).map_err(|_| StatusCode::BAD_REQUEST)?;

    tokio::fs::write(&state.config_path, &body.raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true, "requires_restart": true })))
}

// ---------- Instance List ----------

pub async fn list_instances(State(state): State<AppState>) -> impl IntoResponse {
    let instances = state.registry.instances().await;
    let health = state.registry.all_health().await;

    let mut result = Vec::new();
    for (id, cfg) in &instances {
        let container_status = docker::get_container_status(id).await.unwrap_or_default();
        let h = health.get(id).cloned().unwrap_or(crate::registry::InstanceHealth::Unknown);
        result.push(serde_json::json!({
            "id": id,
            "display_name": cfg.display_name,
            "grpc_address": cfg.grpc_address,
            "health": h,
            "container_status": container_status,
            "desired_state": cfg.desired_state.to_string(),
        }));
    }

    result.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    Json(result)
}

// ---------- Create Instance ----------

#[derive(Deserialize)]
pub struct CreateInstanceBody {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub config_toml: String,
}

pub async fn create_instance(
    State(state): State<AppState>,
    Json(body): Json<CreateInstanceBody>,
) -> Result<impl IntoResponse, StatusCode> {
    // Determine agent config: use provided TOML, fall back to template, or empty
    let agent_config = if !body.config_toml.is_empty() {
        body.config_toml.clone()
    } else if !state.docker_config.config_template_path.is_empty() {
        tokio::fs::read_to_string(&state.docker_config.config_template_path)
            .await
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Determine host port: find next available sequential port
    let instances = state.registry.instances().await;
    let used_ports: Vec<u16> = instances
        .values()
        .filter_map(|c| docker::port_from_address(&c.grpc_address))
        .collect();
    let host_port = docker::next_available_port(state.docker_config.base_port, &used_ports);

    // Always create a Docker container for the new agent
    let result = docker::create_agent(&body.id, &agent_config, host_port, &state.docker_config)
        .await
        .map_err(|e| {
            error!(id = %body.id, error = %e, "create_agent failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let config = InstanceConfig {
        grpc_address: result.grpc_address,
        display_name: body.display_name,
        desired_state: DesiredState::Running,
    };

    // Add to registry
    state
        .registry
        .add_instance(body.id.clone(), config.clone())
        .await;

    // Persist to config file
    let mut gw_config = GatewayConfig::load(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    gw_config.instances.insert(body.id.clone(), config);
    gw_config
        .save(&state.config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Spawn background health check (container needs a moment to start gRPC)
    let registry = state.registry.clone();
    let id_clone = body.id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        registry.check_health_one(&id_clone).await;
    });

    Ok(Json(serde_json::json!({ "ok": true, "id": body.id })))
}

// ---------- Instance Actions ----------

#[derive(Deserialize)]
pub struct InstanceActionBody {
    pub action: String,
}

pub async fn instance_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InstanceActionBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let needs_health_check = match body.action.as_str() {
        "start" => {
            docker::start_agent(&id).await.map_err(|e| {
                error!(%id, error = %e, "start_agent failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            persist_desired_state(&state.config_path, &id, DesiredState::Running).await?;
            true
        }
        "stop" => {
            docker::stop_agent(&id).await.map_err(|e| {
                error!(%id, error = %e, "stop_agent failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            state.registry.invalidate_client(&id).await;
            persist_desired_state(&state.config_path, &id, DesiredState::Stopped).await?;
            false
        }
        "destroy" => {
            docker::destroy_agent(&id, &state.docker_config.agents_dir)
                .await
                .map_err(|e| {
                    error!(%id, error = %e, "destroy_agent failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            state.registry.remove_instance(&id).await;

            // Remove from persisted config
            if let Ok(mut gw_config) = GatewayConfig::load(&state.config_path) {
                gw_config.instances.remove(&id);
                let _ = gw_config.save(&state.config_path);
            }
            false
        }
        "restart" => {
            docker::restart_agent(&id, &state.docker_config.agents_dir)
                .await
                .map_err(|e| {
                    error!(%id, error = %e, "restart_agent failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            state.registry.invalidate_client(&id).await;
            persist_desired_state(&state.config_path, &id, DesiredState::Running).await?;
            true
        }
        "reconnect" => {
            state.registry.invalidate_client(&id).await;
            true
        }
        _ => {
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Trigger background health check for start/reconnect
    if needs_health_check {
        let registry = state.registry.clone();
        let id_clone = id.clone();
        tokio::spawn(async move {
            // Small delay for start (container needs to boot)
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            registry.check_health_one(&id_clone).await;
        });
    }

    Ok(Json(
        serde_json::json!({ "ok": true, "action": body.action, "id": id }),
    ))
}

// ---------- Template ----------

pub async fn get_template(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    if state.docker_config.config_template_path.is_empty() {
        return Ok(Json(serde_json::json!({ "raw": "" })));
    }

    let content = tokio::fs::read_to_string(&state.docker_config.config_template_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({ "raw": content })))
}

/// Return all workspace template files (SOUL.md, IDENTITY.md, TOOLS.md, etc.)
/// so the admin UI can pre-populate identity fields when creating a new agent.
pub async fn get_workspace_templates(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let dir = &state.docker_config.workspace_templates_dir;
    let mut files: Vec<serde_json::Value> = Vec::new();

    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        files.push(serde_json::json!({ "filename": filename, "content": content }));
    }

    Ok(Json(serde_json::json!({ "files": files })))
}
