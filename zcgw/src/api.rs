use crate::app_state::AppState;
use crate::proto;
use crate::registry::InstanceHealth;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio_stream::StreamExt;
use tonic::metadata::MetadataValue;

/// Attach gRPC auth metadata to a request.
fn authed_request<T>(body: T, secret: &str) -> tonic::Request<T> {
    let mut req = tonic::Request::new(body);
    let val: MetadataValue<_> = format!("Bearer {}", secret).parse().unwrap();
    req.metadata_mut().insert("authorization", val);
    req
}

// ── Input Validation Helpers ────────────────────────────────────────

/// Maximum number of concurrent pending Signal link sessions.
const MAX_PENDING_SIGNAL_LINKS: usize = 3;
/// Maximum age of a pending Signal link session before it's considered stale.
const PENDING_SIGNAL_LINK_TTL: std::time::Duration = std::time::Duration::from_secs(600);

/// Validate an agent ID to prevent path traversal.
///
/// Agent IDs must be non-empty, contain only alphanumeric characters, hyphens,
/// and underscores, and must not contain path separators or `..`.
fn validate_agent_id(id: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if id.is_empty() || id.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Invalid agent ID: must be 1-128 characters" })),
        ));
    }
    if id.contains('/') || id.contains('\\') || id.contains('\0') || id.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Invalid agent ID: must not contain path separators or '..'" }),
            ),
        ));
    }
    // Allow alphanumeric, hyphens, underscores, dots (for docker container names)
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Invalid agent ID: only alphanumeric, hyphens, underscores, and dots allowed" }),
            ),
        ));
    }
    Ok(())
}

/// Validate a Signal device name (used in `signal-cli link -n <name>`).
///
/// Prevents command injection by restricting to safe characters.
fn validate_device_name(name: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if name.is_empty() || name.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Device name must be 1-64 characters" })),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Device name must contain only letters, numbers, spaces, hyphens, and underscores" }),
            ),
        ));
    }
    Ok(())
}

/// Validate a Signal connection name.
fn validate_connection_name(name: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if name.is_empty() || name.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Connection name must be 1-64 characters" })),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Connection name must contain only letters, numbers, hyphens, and underscores" }),
            ),
        ));
    }
    Ok(())
}

/// Validate an E.164 phone number (e.g. "+1234567890").
fn validate_e164(number: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // E.164: starts with +, followed by 1-15 digits
    let digits = number.strip_prefix('+').unwrap_or(number);
    if digits.is_empty() || digits.len() > 15 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Invalid phone number: must be E.164 format (e.g. +1234567890)" }),
            ),
        ));
    }
    Ok(())
}

/// Evict expired pending Signal link sessions and return the current count.
fn evict_stale_pending_links(
    pending: &mut std::collections::HashMap<String, crate::app_state::SignalLinkPendingState>,
) -> usize {
    let now = std::time::Instant::now();
    pending.retain(|_, state| now.duration_since(state.created_at) < PENDING_SIGNAL_LINK_TTL);
    pending.len()
}

// ---------- Health ----------

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

// ---------- Instances ----------

#[derive(Serialize)]
struct InstanceInfo {
    id: String,
    display_name: String,
    grpc_address: String,
    health: InstanceHealth,
}

pub async fn list_instances(State(state): State<AppState>) -> impl IntoResponse {
    let health = state.registry.all_health().await;
    let instance_map = state.registry.instances().await;
    let mut instances: Vec<InstanceInfo> = instance_map
        .iter()
        .map(|(id, cfg)| InstanceInfo {
            id: id.clone(),
            display_name: cfg.display_name.clone(),
            grpc_address: cfg.grpc_address.clone(),
            health: health.get(id).cloned().unwrap_or(InstanceHealth::Unknown),
        })
        .collect();
    instances.sort_by(|a, b| a.id.cmp(&b.id));
    Json(instances)
}

// ---------- Status ----------

pub async fn get_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .get_status(authed_request(
            proto::GetStatusRequest {},
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let s = resp.into_inner();
    Ok(Json(serde_json::json!({
        "state": s.state,
        "uptime_secs": s.uptime_secs,
        "total_turns": s.total_turns,
        "history_length": s.history_length,
        "model": s.model,
        "provider": s.provider,
    })))
}

// ---------- History ----------

#[derive(Deserialize)]
pub struct HistoryQuery {
    offset: Option<u64>,
    limit: Option<u64>,
}

pub async fn get_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .get_history(authed_request(
            proto::HistoryRequest {
                offset: q.offset.unwrap_or(0),
                limit: q.limit.unwrap_or(50),
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let h = resp.into_inner();
    let messages: Vec<serde_json::Value> = h
        .messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "turn_index": m.turn_index,
                "role": m.role,
                "content": m.content,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "total": h.total,
        "offset": h.offset,
        "limit": h.limit,
        "messages": messages,
    })))
}

pub async fn clear_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .clear_history(authed_request(
            proto::ClearHistoryRequest { confirm: true },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let c = resp.into_inner();
    Ok(Json(
        serde_json::json!({"messages_cleared": c.messages_cleared}),
    ))
}

// ---------- Config ----------

pub async fn get_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .get_config(authed_request(
            proto::GetConfigRequest {},
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let c = resp.into_inner();
    // Return the raw JSON string as a JSON value
    let val: serde_json::Value =
        serde_json::from_str(&c.config_json).unwrap_or(serde_json::Value::String(c.config_json));
    Ok(Json(val))
}

#[derive(Deserialize)]
pub struct UpdateConfigBody {
    #[serde(flatten)]
    fields: serde_json::Value,
}

pub async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfigBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .update_config(authed_request(
            proto::UpdateConfigRequest {
                partial_json: body.fields.to_string(),
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|e| {
            tracing::error!(instance = %id, error = %e, "update_config gRPC call failed");
            StatusCode::BAD_GATEWAY
        })?;

    let u = resp.into_inner();
    Ok(Json(serde_json::json!({
        "updated_fields": u.updated_fields,
        "requires_restart": u.requires_restart,
    })))
}

// ---------- Tools ----------

pub async fn list_tools(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .list_tools(authed_request(
            proto::ListToolsRequest {},
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let t = resp.into_inner();
    let tools: Vec<serde_json::Value> = t
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": serde_json::from_str::<serde_json::Value>(&tool.parameters_json).unwrap_or_default(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"tools": tools})))
}

// ---------- Memory ----------

#[derive(Deserialize)]
pub struct ListMemoryQuery {
    category: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
}

pub async fn list_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListMemoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .list_memory(authed_request(
            proto::ListMemoryRequest {
                category: q.category.unwrap_or_default(),
                offset: q.offset.unwrap_or(0),
                limit: q.limit.unwrap_or(50),
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let m = resp.into_inner();
    Ok(Json(memory_list_json(&m)))
}

#[derive(Deserialize)]
pub struct SearchMemoryQuery {
    query: String,
    limit: Option<u64>,
}

pub async fn search_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SearchMemoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .search_memory(authed_request(
            proto::SearchMemoryRequest {
                query: q.query,
                limit: q.limit.unwrap_or(10),
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let m = resp.into_inner();
    Ok(Json(memory_list_json(&m)))
}

fn memory_list_json(m: &proto::MemoryEntryList) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = m
        .entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "key": e.key,
                "content": e.content,
                "category": e.category,
                "timestamp": e.timestamp,
                "score": e.score,
            })
        })
        .collect();
    serde_json::json!({"entries": entries, "total": m.total})
}

#[derive(Deserialize)]
pub struct StoreMemoryBody {
    key: String,
    content: String,
    #[serde(default)]
    category: String,
}

pub async fn store_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<StoreMemoryBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    client
        .store_memory(authed_request(
            proto::StoreMemoryRequest {
                key: body.key,
                content: body.content,
                category: body.category,
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn forget_memory(
    State(state): State<AppState>,
    Path((id, key)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    client
        .forget_memory(authed_request(
            proto::ForgetMemoryRequest { key },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------- Identity Files ----------

/// Well-known identity markdown files that ZeroClaw agents load from their workspace.
const KNOWN_IDENTITY_FILES: &[&str] = &[
    "SOUL.md",
    "IDENTITY.md",
    "AGENTS.md",
    "TOOLS.md",
    "USER.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "MEMORY.md",
];

/// Resolve the agent workspace directory on the host filesystem.
/// Inside the container: /data/.zeroclaw/workspace/
/// On the host: <agents_dir>/<id>/data/.zeroclaw/workspace/
fn agent_workspace_dir(state: &AppState, id: &str) -> std::path::PathBuf {
    state
        .docker_config
        .agents_dir
        .join(id)
        .join("data")
        .join(".zeroclaw")
        .join("workspace")
}

#[derive(Serialize)]
struct IdentityFile {
    filename: String,
    content: String,
}

pub async fn list_identity(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let data_dir = agent_workspace_dir(&state, &id);
    if !data_dir.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut files: Vec<IdentityFile> = Vec::new();

    // Read all .md files in the data directory
    let mut entries = tokio::fs::read_dir(&data_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".md") {
            if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                files.push(IdentityFile {
                    filename: name,
                    content,
                });
            }
        }
    }

    // Sort: known files first (in canonical order), then custom files alphabetically
    files.sort_by(|a, b| {
        let a_idx = KNOWN_IDENTITY_FILES.iter().position(|f| *f == a.filename);
        let b_idx = KNOWN_IDENTITY_FILES.iter().position(|f| *f == b.filename);
        match (a_idx, b_idx) {
            (Some(ai), Some(bi)) => ai.cmp(&bi),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.filename.cmp(&b.filename),
        }
    });

    Ok(Json(serde_json::json!({
        "files": files,
        "known_files": KNOWN_IDENTITY_FILES,
    })))
}

pub async fn get_identity_file(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    if !filename.ends_with(".md") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = agent_workspace_dir(&state, &id).join(&filename);
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(
        serde_json::json!({ "filename": filename, "content": content }),
    ))
}

#[derive(Deserialize)]
pub struct UpdateIdentityBody {
    content: String,
}

pub async fn update_identity_file(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
    Json(body): Json<UpdateIdentityBody>,
) -> Result<impl IntoResponse, StatusCode> {
    if !filename.ends_with(".md") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let data_dir = agent_workspace_dir(&state, &id);
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = data_dir.join(&filename);
    tokio::fs::write(&path, &body.content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_identity_file(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    if !filename.ends_with(".md") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = agent_workspace_dir(&state, &id).join(&filename);
    tokio::fs::remove_file(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct BatchIdentityBody {
    files: Vec<BatchIdentityFile>,
}

#[derive(Deserialize)]
struct BatchIdentityFile {
    filename: String,
    content: String,
}

pub async fn batch_update_identity(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<BatchIdentityBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let data_dir = agent_workspace_dir(&state, &id);
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut saved = 0;
    for file in &body.files {
        if !file.filename.ends_with(".md") {
            continue;
        }
        let path = data_dir.join(&file.filename);
        if file.content.is_empty() {
            // Delete empty files
            let _ = tokio::fs::remove_file(&path).await;
        } else {
            tokio::fs::write(&path, &file.content)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            saved += 1;
        }
    }

    Ok(Json(serde_json::json!({ "ok": true, "saved": saved })))
}

// ---------- Agent Config Helpers ----------

/// Primary config path: host-level file written by the gateway.
fn agent_config_path(state: &AppState, id: &str) -> std::path::PathBuf {
    state.docker_config.agents_dir.join(id).join("config.toml")
}

/// Fallback config path: inside the agent's data directory (used by
/// pre-existing or Docker Compose-managed agents that don't have a
/// host-level config file).
fn agent_config_fallback_path(state: &AppState, id: &str) -> std::path::PathBuf {
    state
        .docker_config
        .agents_dir
        .join(id)
        .join("data")
        .join(".zeroclaw")
        .join("config.toml")
}

async fn read_agent_config(state: &AppState, id: &str) -> Result<toml::Value, StatusCode> {
    let primary = agent_config_path(state, id);
    let path = if primary.exists() {
        primary
    } else {
        // Fall back to the data-dir config for pre-existing agents
        let fallback = agent_config_fallback_path(state, id);
        if fallback.exists() {
            fallback
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    };
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    toml::from_str(&content).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn write_agent_config(
    state: &AppState,
    id: &str,
    val: &toml::Value,
) -> Result<(), StatusCode> {
    let path = agent_config_path(state, id);
    // Ensure the host-level directory exists (for pre-existing agents
    // that only had a data-dir config).
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let content = toml::to_string_pretty(val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, &content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also sync to the data directory so changes are visible inside the container
    // immediately (the entrypoint only copies on start, not during runtime).
    let data_config = agent_config_fallback_path(state, id);
    if let Some(parent) = data_config.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&data_config, &content).await;

    // Push the change to the running agent via gRPC for live hot-reload.
    // Convert TOML → JSON for the gRPC UpdateConfigRequest.
    // Best-effort: if the agent is unreachable, the host file is still updated
    // and will take effect on next restart.
    if let Ok(json_val) = toml_value_to_json(val) {
        if let Ok(mut client) = state.registry.get_client(id).await {
            let _ = client
                .update_config(authed_request(
                    proto::UpdateConfigRequest {
                        partial_json: json_val.to_string(),
                    },
                    &state.grpc_secret,
                ))
                .await;
        }
    }

    Ok(())
}

/// Convert a TOML value tree to a serde_json::Value.
fn toml_value_to_json(val: &toml::Value) -> Result<serde_json::Value, ()> {
    // Serialize TOML → string → JSON is lossy for datetimes, but config
    // values are strings/numbers/bools/arrays/tables which round-trip fine.
    let json_str = serde_json::to_string(val).map_err(|_| ())?;
    serde_json::from_str(&json_str).map_err(|_| ())
}

// ---------- Connectors ----------

/// Channel field descriptor for the frontend schema.
#[derive(Serialize)]
struct ChannelField {
    name: &'static str,
    label: &'static str,
    field_type: &'static str,
    required: bool,
    sensitive: bool,
    help: &'static str,
}

/// Channel type descriptor.
#[derive(Serialize)]
struct ChannelDescriptor {
    channel_type: &'static str,
    label: &'static str,
    fields: Vec<ChannelField>,
}

fn channel_schema() -> Vec<ChannelDescriptor> {
    vec![
        ChannelDescriptor {
            channel_type: "telegram",
            label: "Telegram",
            fields: vec![
                ChannelField {
                    name: "bot_token",
                    label: "Bot Token",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Telegram bot token from @BotFather",
                },
                ChannelField {
                    name: "allowed_users",
                    label: "Allowed Users",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Telegram user IDs or usernames. Empty = deny all",
                },
                ChannelField {
                    name: "stream_mode",
                    label: "Stream Mode",
                    field_type: "select:off,partial",
                    required: false,
                    sensitive: false,
                    help: "off = single message, partial = progressive edits",
                },
                ChannelField {
                    name: "draft_update_interval_ms",
                    label: "Draft Update Interval (ms)",
                    field_type: "u64",
                    required: false,
                    sensitive: false,
                    help: "Min interval between draft edits (default: 1000)",
                },
                ChannelField {
                    name: "interrupt_on_new_message",
                    label: "Interrupt on New Message",
                    field_type: "bool",
                    required: false,
                    sensitive: false,
                    help: "Cancel in-flight request on new message from same sender",
                },
                ChannelField {
                    name: "mention_only",
                    label: "Mention Only",
                    field_type: "bool",
                    required: false,
                    sensitive: false,
                    help: "Only respond to @-mentions in groups (DMs always processed)",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "discord",
            label: "Discord",
            fields: vec![
                ChannelField {
                    name: "bot_token",
                    label: "Bot Token",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Discord bot token",
                },
                ChannelField {
                    name: "guild_id",
                    label: "Guild ID",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Restrict to a specific guild",
                },
                ChannelField {
                    name: "allowed_users",
                    label: "Allowed Users",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed user IDs",
                },
                ChannelField {
                    name: "listen_to_bots",
                    label: "Listen to Bots",
                    field_type: "bool",
                    required: false,
                    sensitive: false,
                    help: "Process messages from other bots",
                },
                ChannelField {
                    name: "mention_only",
                    label: "Mention Only",
                    field_type: "bool",
                    required: false,
                    sensitive: false,
                    help: "Only respond when mentioned",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "slack",
            label: "Slack",
            fields: vec![
                ChannelField {
                    name: "bot_token",
                    label: "Bot Token",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Slack bot token (xoxb-...)",
                },
                ChannelField {
                    name: "app_token",
                    label: "App Token",
                    field_type: "string",
                    required: false,
                    sensitive: true,
                    help: "Socket mode app token (xapp-...)",
                },
                ChannelField {
                    name: "channel_id",
                    label: "Channel ID",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Default channel ID",
                },
                ChannelField {
                    name: "allowed_users",
                    label: "Allowed Users",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed user IDs",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "whatsapp",
            label: "WhatsApp",
            fields: vec![
                ChannelField {
                    name: "access_token",
                    label: "Access Token",
                    field_type: "string",
                    required: false,
                    sensitive: true,
                    help: "Cloud API access token",
                },
                ChannelField {
                    name: "phone_number_id",
                    label: "Phone Number ID",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Cloud API phone number ID",
                },
                ChannelField {
                    name: "session_path",
                    label: "Session Path",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Web client session path (alternative to Cloud)",
                },
                ChannelField {
                    name: "allowed_numbers",
                    label: "Allowed Numbers",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed phone numbers",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "email",
            label: "Email",
            fields: vec![
                ChannelField {
                    name: "imap_host",
                    label: "IMAP Host",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "IMAP server hostname",
                },
                ChannelField {
                    name: "smtp_host",
                    label: "SMTP Host",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "SMTP server hostname",
                },
                ChannelField {
                    name: "username",
                    label: "Username",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Email account username",
                },
                ChannelField {
                    name: "password",
                    label: "Password",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Email account password",
                },
                ChannelField {
                    name: "from_address",
                    label: "From Address",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Sender email address",
                },
                ChannelField {
                    name: "allowed_senders",
                    label: "Allowed Senders",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed sender addresses",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "signal",
            label: "Signal",
            fields: vec![
                ChannelField {
                    name: "http_url",
                    label: "HTTP URL",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "signal-cli REST API URL",
                },
                ChannelField {
                    name: "account",
                    label: "Account",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Signal account phone number",
                },
                ChannelField {
                    name: "group_id",
                    label: "Group ID",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Signal group ID",
                },
                ChannelField {
                    name: "allowed_from",
                    label: "Allowed From",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed sender numbers",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "matrix",
            label: "Matrix",
            fields: vec![
                ChannelField {
                    name: "homeserver",
                    label: "Homeserver",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Matrix homeserver URL",
                },
                ChannelField {
                    name: "access_token",
                    label: "Access Token",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Matrix access token",
                },
                ChannelField {
                    name: "room_id",
                    label: "Room ID",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Room to join",
                },
                ChannelField {
                    name: "allowed_users",
                    label: "Allowed Users",
                    field_type: "string_list",
                    required: true,
                    sensitive: false,
                    help: "Allowed Matrix user IDs",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "irc",
            label: "IRC",
            fields: vec![
                ChannelField {
                    name: "server",
                    label: "Server",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "IRC server address",
                },
                ChannelField {
                    name: "nickname",
                    label: "Nickname",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Bot nickname",
                },
                ChannelField {
                    name: "channels",
                    label: "Channels",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Channels to join",
                },
                ChannelField {
                    name: "server_password",
                    label: "Server Password",
                    field_type: "string",
                    required: false,
                    sensitive: true,
                    help: "Server password",
                },
            ],
        },
        ChannelDescriptor {
            channel_type: "mattermost",
            label: "Mattermost",
            fields: vec![
                ChannelField {
                    name: "url",
                    label: "URL",
                    field_type: "string",
                    required: true,
                    sensitive: false,
                    help: "Mattermost server URL",
                },
                ChannelField {
                    name: "bot_token",
                    label: "Bot Token",
                    field_type: "string",
                    required: true,
                    sensitive: true,
                    help: "Mattermost bot token",
                },
                ChannelField {
                    name: "channel_id",
                    label: "Channel ID",
                    field_type: "string",
                    required: false,
                    sensitive: false,
                    help: "Default channel ID",
                },
                ChannelField {
                    name: "allowed_users",
                    label: "Allowed Users",
                    field_type: "string_list",
                    required: false,
                    sensitive: false,
                    help: "Allowed user IDs",
                },
            ],
        },
    ]
}

pub async fn get_connectors(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let config = read_agent_config(&state, &id).await?;
    let mut channels_config = config
        .get("channels_config")
        .cloned()
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));

    // Inject "enabled: true" into each existing channel sub-table so the
    // frontend knows which channels are currently active in the TOML.
    if let toml::Value::Table(ref mut channels) = channels_config {
        for (_key, val) in channels.iter_mut() {
            if let toml::Value::Table(ref mut ch) = val {
                ch.entry("enabled").or_insert(toml::Value::Boolean(true));
            }
        }
    }

    // Convert toml::Value to serde_json::Value
    let channels_json: serde_json::Value =
        serde_json::to_value(&channels_config).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "channels_config": channels_json,
        "channel_schema": channel_schema(),
    })))
}

#[derive(Deserialize)]
pub struct UpdateConnectorsBody {
    channels_config: serde_json::Value,
}

pub async fn update_connectors(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConnectorsBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut config = read_agent_config(&state, &id).await?;

    // Convert JSON to toml, then clean up:
    // - Strip the UI-only "enabled" field from each channel sub-table
    // - Remove channel sub-tables where enabled was false (i.e. disabled channels)
    let mut channels_toml: toml::Value =
        serde_json::from_value(body.channels_config).map_err(|_| StatusCode::BAD_REQUEST)?;

    if let toml::Value::Table(ref mut channels) = channels_toml {
        let keys: Vec<String> = channels.keys().cloned().collect();
        for key in keys {
            let remove = if let Some(toml::Value::Table(ref mut ch)) = channels.get_mut(&key) {
                // Check if enabled is false — if so, remove the whole channel
                let enabled = ch.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                // Always strip the "enabled" field — not part of the agent schema
                ch.remove("enabled");
                !enabled
            } else {
                false
            };
            if remove {
                channels.remove(&key);
            }
        }
    }

    // Ensure required `cli` field is present (defaults to true).
    if let toml::Value::Table(ref mut channels) = channels_toml {
        channels.entry("cli").or_insert(toml::Value::Boolean(true));
    }

    if let toml::Value::Table(ref mut t) = config {
        t.insert("channels_config".to_string(), channels_toml);
    }

    write_agent_config(&state, &id, &config).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "requires_restart": true }),
    ))
}

// ---------- MCP Servers ----------

pub async fn get_mcp_servers(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let config = read_agent_config(&state, &id).await?;
    let mcp_servers = config
        .get("mcp_servers")
        .cloned()
        .unwrap_or(toml::Value::Array(vec![]));

    let mcp_json: serde_json::Value =
        serde_json::to_value(&mcp_servers).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "mcp_servers": mcp_json })))
}

#[derive(Deserialize)]
pub struct UpdateMcpServersBody {
    mcp_servers: serde_json::Value,
}

pub async fn update_mcp_servers(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateMcpServersBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut config = read_agent_config(&state, &id).await?;

    let mcp_toml: toml::Value =
        serde_json::from_value(body.mcp_servers).map_err(|_| StatusCode::BAD_REQUEST)?;

    if let toml::Value::Table(ref mut t) = config {
        t.insert("mcp_servers".to_string(), mcp_toml);
    }

    write_agent_config(&state, &id, &config).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "requires_restart": true }),
    ))
}

// ---------- Integrations: Composio ----------

/// Shared HTTP client for Composio API calls.
fn composio_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client")
}

/// Persist the Composio store to disk.
async fn persist_composio_store(state: &AppState) -> Result<(), StatusCode> {
    let store = state.composio_store.read().await;
    let path = state
        .docker_config
        .agents_dir
        .join(".composio")
        .join("store.json");
    let content =
        serde_json::to_string_pretty(&*store).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Return the prefix used for all Composio identifiers owned by this gateway.
///
/// Without a tenant: `"zcgw-"`. With tenant `"acme"`: `"zcgw-acme-"`.
fn composio_prefix(tenant_id: Option<&str>) -> String {
    match tenant_id {
        Some(tid) => format!("zcgw-{}-", tid),
        None => "zcgw-".to_string(),
    }
}

/// Generate a Composio user_id for a given instance.
fn composio_user_id(tenant_id: Option<&str>, instance_id: &str) -> String {
    format!("{}{}", composio_prefix(tenant_id), instance_id)
}

/// Generate a Composio MCP server name for a given toolkit slug.
fn composio_mcp_server_name(tenant_id: Option<&str>, slug: &str) -> String {
    format!("{}{}", composio_prefix(tenant_id), slug)
}

/// Parsed fields from a Composio connected account API response.
struct ComposioAccountFields {
    toolkit_slug: String,
    display_name: String,
    user_id: String,
    status: String,
    connected_at: String,
}

/// Extract connection fields from a Composio v3 API connected_account object.
///
/// The v3 API nests the toolkit slug under `toolkit.slug` and uses `user_id`
/// directly (not `clientUniqueUserId`). The `created_at` field replaces `createdAt`.
fn parse_composio_account(item: &serde_json::Value) -> ComposioAccountFields {
    let toolkit_slug = item
        .get("toolkit")
        .and_then(|t| t.get("slug"))
        .and_then(|v| v.as_str())
        // Fallback for single-account fetch endpoint which may use flat fields
        .or_else(|| item.get("appName").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();

    let display_name = item
        .get("connectionParams")
        .and_then(|v| v.get("user_email"))
        .and_then(|v| v.as_str())
        .unwrap_or(&toolkit_slug)
        .to_string();

    let user_id = item
        .get("user_id")
        .or_else(|| item.get("clientUniqueUserId"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let status = item
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("ACTIVE")
        .to_string();

    let connected_at = item
        .get("created_at")
        .or_else(|| item.get("createdAt"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    ComposioAccountFields {
        toolkit_slug,
        display_name,
        user_id,
        status,
        connected_at,
    }
}

/// Resolve an app name (e.g. "gmail") to a Composio auth_config_id.
async fn resolve_composio_auth_config_id(
    api_key: &str,
    app_name: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let client = composio_client();
    let resp = client
        .get("https://backend.composio.dev/api/v3/auth_configs")
        .header("x-api-key", api_key)
        .query(&[("toolkit_slug", app_name)])
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to query Composio auth_configs");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Composio API error: {}", e) })),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::error!(status = %status, body = %body, "Composio auth_configs request failed");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Composio API returned {}", status) })),
        ));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Invalid Composio response: {}", e) })),
        )
    })?;

    // Extract the first auth_config_id from the response
    let items = data.get("items").and_then(|v| v.as_array());
    if let Some(items) = items {
        if let Some(first) = items.first() {
            if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                return Ok(id.to_string());
            }
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(
            serde_json::json!({ "error": format!("No auth config found for app '{}'", app_name) }),
        ),
    ))
}

/// Fetch details of a connected account from Composio.
async fn fetch_composio_connected_account(
    api_key: &str,
    account_id: &str,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let client = composio_client();
    let resp = client
        .get(format!(
            "https://backend.composio.dev/api/v3/connected_accounts/{}",
            account_id
        ))
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Composio API error: {}", e) })),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Composio API returned {}", status) })),
        ));
    }

    resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Invalid Composio response: {}", e) })),
        )
    })
}

// ---- Gateway-level handlers ----

pub async fn get_composio_gateway_config(State(state): State<AppState>) -> impl IntoResponse {
    let store = state.composio_store.read().await;
    Json(serde_json::json!({
        "has_api_key": state.composio_api_key.is_some(),
        "total_connections": store.connections.len(),
        "mcp_servers": store.mcp_servers.len(),
        "tenant_id": state.tenant_id,
    }))
}

#[derive(Deserialize)]
pub struct UpdateComposioGatewayConfigBody {
    #[allow(dead_code)]
    api_key: Option<String>,
}

pub async fn update_composio_gateway_config(
    State(state): State<AppState>,
    Json(_body): Json<UpdateComposioGatewayConfigBody>,
) -> impl IntoResponse {
    // API key is currently read from env var at startup — runtime updates
    // would require wrapping composio_api_key in Arc<RwLock<>>.
    // For now, just note that a restart is needed.
    Json(serde_json::json!({
        "ok": true,
        "has_api_key": state.composio_api_key.is_some(),
        "note": "API key is read from COMPOSIO_API_KEY env var. Restart gateway after changing.",
    }))
}

pub async fn list_composio_connections(State(state): State<AppState>) -> impl IntoResponse {
    let store = state.composio_store.read().await;
    Json(serde_json::json!({
        "connections": store.connections,
    }))
}

/// Sync connections from the Composio API into the local store.
///
/// Queries Composio for all connected accounts, merges new/updated ones into
/// the gateway store, and returns the updated connection list. This is the
/// primary mechanism for discovering connections — the OAuth callback is a
/// best-effort supplement.
pub async fn sync_composio_connections(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let api_key = state.composio_api_key.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "COMPOSIO_API_KEY not configured" })),
        )
    })?;

    let client = composio_client();
    let resp = client
        .get("https://backend.composio.dev/api/v3/connected_accounts")
        .header("x-api-key", api_key)
        .query(&[("showActiveOnly", "true")])
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Composio API error: {}", e) })),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                serde_json::json!({ "error": format!("Composio API returned {}: {}", status, body) }),
            ),
        ));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Invalid Composio response: {}", e) })),
        )
    })?;

    // Parse connected accounts from response
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut synced = 0usize;
    let mut store = state.composio_store.write().await;
    let prefix = composio_prefix(state.tenant_id.as_deref());

    for item in &items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if id.is_empty() {
            continue;
        }

        let fields = parse_composio_account(item);

        // Only import connections with our prefix (tenant-scoped when configured)
        if !fields.user_id.starts_with(&prefix) {
            continue;
        }

        // Check if connection already exists in store
        if let Some(existing) = store.connections.iter_mut().find(|c| c.id == id) {
            // Update fields from live Composio data
            existing.status = fields.status;
            existing.display_name = fields.display_name.clone();
            existing.toolkit_slug = fields.toolkit_slug.clone();
            // Fix name if it was stored as "unknown" from old parsing
            if existing.name == "unknown" || existing.name.is_empty() {
                existing.name = fields.display_name;
            }
        } else {
            // New connection — add it (unassigned; user assigns via UI)
            store.connections.push(crate::app_state::ComposioConnection {
                id: id.to_string(),
                name: fields.toolkit_slug.clone(),
                toolkit_slug: fields.toolkit_slug,
                display_name: fields.display_name,
                user_id: fields.user_id,
                assigned_to: vec![],
                status: fields.status,
                connected_at: if fields.connected_at.is_empty() {
                    chrono::Utc::now().to_rfc3339()
                } else {
                    fields.connected_at
                },
            });
            synced += 1;
        }
    }

    drop(store);
    persist_composio_store(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to persist store" })),
        )
    })?;

    let store = state.composio_store.read().await;
    Ok(Json(serde_json::json!({
        "ok": true,
        "synced": synced,
        "total_from_composio": items.len(),
        "connections": store.connections,
    })))
}

pub async fn list_composio_apps(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let api_key = state.composio_api_key.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "COMPOSIO_API_KEY not configured" })),
        )
    })?;

    let client = composio_client();
    let resp = client
        .get("https://backend.composio.dev/api/v3/auth_configs")
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Composio API error: {}", e) })),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                serde_json::json!({ "error": format!("Composio API returned {}: {}", status, body) }),
            ),
        ));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Invalid Composio response: {}", e) })),
        )
    })?;

    // Extract the items array and map to a simplified list
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let apps: Vec<serde_json::Value> = items
        .iter()
        .map(|item| {
            serde_json::json!({
                "id": item.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "toolkit_slug": item.get("toolkit_slug").or_else(|| item.get("appName")).and_then(|v| v.as_str()).unwrap_or(""),
                "name": item.get("name").or_else(|| item.get("appName")).and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "apps": apps })))
}

#[derive(Deserialize)]
pub struct ComposioConnectInitBody {
    instance_id: Option<String>,
    name: Option<String>,
    app: Option<String>,
    auth_config_id: Option<String>,
}

pub async fn composio_connect_init(
    State(state): State<AppState>,
    Json(body): Json<ComposioConnectInitBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref id) = body.instance_id {
        validate_agent_id(id)?;
    }

    let api_key = state.composio_api_key.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "COMPOSIO_API_KEY not configured" })),
        )
    })?;

    let redirect_host = state.composio_redirect_host.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "COMPOSIO_REDIRECT_HOST not configured (set COMPOSIO_REDIRECT_HOST or ZEROCLAW_GOOGLE_REDIRECT_HOST)" })),
        )
    })?;

    // Resolve auth_config_id
    let auth_config_id = if let Some(ref id) = body.auth_config_id {
        id.clone()
    } else if let Some(ref app) = body.app {
        resolve_composio_auth_config_id(api_key, app).await?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Either 'app' or 'auth_config_id' is required" })),
        ));
    };

    let tid = state.tenant_id.as_deref();
    // All connections are created under the gateway's identity.
    // The instance_id is only used for local assigned_to tracking.
    let user_id = composio_user_id(tid, "gateway");
    let toolkit_slug = body.app.clone().unwrap_or_else(|| "unknown".to_string());
    let host = redirect_host.trim_end_matches('/');
    let callback_url = if host.starts_with("http://") || host.starts_with("https://") {
        format!("{}/composio/callback", host)
    } else {
        format!("http://{}/composio/callback", host)
    };

    let client = composio_client();
    let link_body = serde_json::json!({
        "auth_config_id": auth_config_id,
        "user_id": user_id,
        "redirect_url": callback_url,
    });

    let resp = client
        .post("https://backend.composio.dev/api/v3/connected_accounts/link")
        .header("x-api-key", api_key)
        .json(&link_body)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Composio API error: {}", e) })),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        tracing::error!(status = %status, body = %body_text, "Composio link request failed");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                serde_json::json!({ "error": format!("Composio link API returned {}: {}", status, body_text) }),
            ),
        ));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Invalid Composio response: {}", e) })),
        )
    })?;

    let redirect_url = data
        .get("redirect_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let connected_account_id = data
        .get("connected_account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Store pending state
    let pending_id = uuid::Uuid::new_v4().to_string();
    let pending_name = body.name.clone().unwrap_or_default();
    let pending_instance_id = body.instance_id.clone();
    if let Ok(mut pending) = state.composio_oauth_pending.lock() {
        pending.insert(
            pending_id.clone(),
            crate::app_state::ComposioOAuthPendingState {
                user_id: user_id.clone(),
                toolkit_slug,
                connected_account_id: connected_account_id.clone(),
                name: pending_name,
                instance_id: pending_instance_id,
                created_at: std::time::Instant::now(),
            },
        );
    }

    Ok(Json(serde_json::json!({
        "redirect_url": redirect_url,
        "connected_account_id": connected_account_id,
        "pending_id": pending_id,
        "user_id": user_id,
    })))
}

#[derive(Deserialize)]
pub struct ComposioCallbackQuery {
    connected_account_id: Option<String>,
}

pub async fn composio_oauth_callback(
    State(state): State<AppState>,
    Query(query): Query<ComposioCallbackQuery>,
) -> impl IntoResponse {
    let Some(account_id) = query.connected_account_id else {
        return axum::response::Html(
            "<html><body><h2>Composio Connection</h2><p>Authorization flow completed. You may close this window and refresh the dashboard.</p></body></html>".to_string()
        );
    };

    // Look up pending state by connected_account_id to get name and instance_id
    let pending_info: Option<(String, Option<String>)> = state
        .composio_oauth_pending
        .lock()
        .ok()
        .and_then(|pending| {
            pending
                .values()
                .find(|p| p.connected_account_id.as_deref() == Some(&account_id))
                .map(|p| (p.name.clone(), p.instance_id.clone()))
        });

    // Fetch connection details from Composio if we have an API key
    if let Some(ref api_key) = state.composio_api_key {
        match fetch_composio_connected_account(api_key, &account_id).await {
            Ok(data) => {
                let fields = parse_composio_account(&data);

                // Use pending state for name and instance_id; fallback to parsed fields
                let (conn_name, pending_instance_id) =
                    pending_info.unwrap_or_else(|| (String::new(), None));
                let name = if conn_name.is_empty() {
                    fields.display_name.clone()
                } else {
                    conn_name
                };
                let instance_id = pending_instance_id.unwrap_or_default();

                let connection = crate::app_state::ComposioConnection {
                    id: account_id.clone(),
                    name,
                    toolkit_slug: fields.toolkit_slug,
                    display_name: fields.display_name,
                    user_id: fields.user_id,
                    assigned_to: if instance_id.is_empty() {
                        vec![]
                    } else {
                        vec![instance_id]
                    },
                    status: fields.status,
                    connected_at: if fields.connected_at.is_empty() {
                        chrono::Utc::now().to_rfc3339()
                    } else {
                        fields.connected_at
                    },
                };

                let mut store = state.composio_store.write().await;
                // Remove any existing connection with same ID
                store.connections.retain(|c| c.id != account_id);
                store.connections.push(connection);
                drop(store);

                // Persist
                if let Err(e) = persist_composio_store(&state).await {
                    tracing::error!(error = ?e, "failed to persist composio store after callback");
                }
            }
            Err(e) => {
                tracing::warn!(error = ?e, account_id = %account_id, "failed to fetch composio account details in callback");
            }
        }
    }

    axum::response::Html(
        "<html><body><h2>Composio Connected</h2><p>Authorization successful! You may close this window and refresh the dashboard.</p></body></html>".to_string()
    )
}

pub async fn delete_composio_connection_global(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let mut store = state.composio_store.write().await;
    let before = store.connections.len();
    store.connections.retain(|c| c.id != connection_id);
    if store.connections.len() == before {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Connection not found" })),
        ));
    }
    drop(store);

    persist_composio_store(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to persist store" })),
        )
    })?;

    // Best-effort delete from Composio API
    if let Some(ref api_key) = state.composio_api_key {
        let client = composio_client();
        let _ = client
            .delete(format!(
                "https://backend.composio.dev/api/v3/connected_accounts/{}",
                connection_id
            ))
            .header("x-api-key", api_key)
            .send()
            .await;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---- Per-instance handlers ----

pub async fn get_composio(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let config = read_agent_config(&state, &id).await?;
    let composio = config.get("composio");

    let enabled = composio
        .and_then(|c| c.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let entity_id = composio
        .and_then(|c| c.get("entity_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let has_api_key = composio
        .and_then(|c| c.get("api_key"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    // Enrich with gateway info
    let gateway_user_id = composio_user_id(state.tenant_id.as_deref(), "gateway");
    let store = state.composio_store.read().await;
    let connections: Vec<&crate::app_state::ComposioConnection> = store
        .connections
        .iter()
        .filter(|c| c.assigned_to.contains(&id))
        .collect();

    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "entity_id": entity_id,
        "has_api_key": has_api_key,
        "has_gateway_api_key": state.composio_api_key.is_some(),
        "user_id": gateway_user_id,
        "connections": connections,
    })))
}

#[derive(Deserialize)]
pub struct UpdateComposioBody {
    enabled: Option<bool>,
    api_key: Option<String>,
    entity_id: Option<String>,
    sync_gateway: Option<bool>,
}

pub async fn update_composio(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateComposioBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut config = read_agent_config(&state, &id).await?;

    if let toml::Value::Table(ref mut t) = config {
        let composio = t
            .entry("composio")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let toml::Value::Table(ref mut ct) = composio {
            if let Some(enabled) = body.enabled {
                ct.insert("enabled".to_string(), toml::Value::Boolean(enabled));
            }
            if let Some(api_key) = body.api_key {
                ct.insert("api_key".to_string(), toml::Value::String(api_key));
            }
            if let Some(entity_id) = body.entity_id {
                ct.insert("entity_id".to_string(), toml::Value::String(entity_id));
            }

            // sync_gateway: write gateway API key, gateway entity_id, and connected_accounts
            if body.sync_gateway.unwrap_or(false) {
                if let Some(ref gw_api_key) = state.composio_api_key {
                    ct.insert(
                        "api_key".to_string(),
                        toml::Value::String(gw_api_key.clone()),
                    );
                    ct.insert(
                        "entity_id".to_string(),
                        toml::Value::String(composio_user_id(
                            state.tenant_id.as_deref(),
                            "gateway",
                        )),
                    );
                    ct.insert("enabled".to_string(), toml::Value::Boolean(true));

                    // Write connected_accounts: toolkit_slug → connected_account_id
                    let store = state.composio_store.read().await;
                    let mut accts = toml::map::Map::new();
                    for conn in store
                        .connections
                        .iter()
                        .filter(|c| c.assigned_to.contains(&id))
                    {
                        accts
                            .entry(conn.toolkit_slug.clone())
                            .or_insert_with(|| toml::Value::String(conn.id.clone()));
                    }
                    ct.insert(
                        "connected_accounts".to_string(),
                        toml::Value::Table(accts),
                    );
                }
            }
        }
    }

    write_agent_config(&state, &id, &config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn list_instance_composio_connections(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = state.composio_store.read().await;
    let connections: Vec<&crate::app_state::ComposioConnection> = store
        .connections
        .iter()
        .filter(|c| c.assigned_to.contains(&id))
        .collect();
    Json(serde_json::json!({ "connections": connections }))
}

#[derive(Deserialize)]
pub struct ComposioInstanceConnectBody {
    app: Option<String>,
    auth_config_id: Option<String>,
    name: Option<String>,
}

pub async fn composio_instance_connect(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ComposioInstanceConnectBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    validate_agent_id(&id)?;
    composio_connect_init(
        State(state),
        Json(ComposioConnectInitBody {
            instance_id: Some(id),
            name: body.name,
            app: body.app,
            auth_config_id: body.auth_config_id,
        }),
    )
    .await
}

pub async fn unassign_composio_connection(
    State(state): State<AppState>,
    Path((id, connection_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let mut store = state.composio_store.write().await;
    let conn = store.connections.iter_mut().find(|c| c.id == connection_id);
    if let Some(conn) = conn {
        conn.assigned_to.retain(|a| a != &id);
    } else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Connection not found" })),
        ));
    }
    drop(store);

    persist_composio_store(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to persist store" })),
        )
    })?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AssignComposioConnectionBody {
    connection_id: String,
}

pub async fn assign_composio_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AssignComposioConnectionBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    validate_agent_id(&id)?;

    let mut store = state.composio_store.write().await;
    let conn = store
        .connections
        .iter_mut()
        .find(|c| c.id == body.connection_id);
    if let Some(conn) = conn {
        if !conn.assigned_to.contains(&id) {
            conn.assigned_to.push(id);
        }
    } else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Connection not found" })),
        ));
    }
    drop(store);

    persist_composio_store(&state).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to persist store" })),
        )
    })?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn composio_mcp_sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    validate_agent_id(&id)?;

    let api_key = state.composio_api_key.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "COMPOSIO_API_KEY not configured" })),
        )
    })?;

    // Use gateway identity for Composio API calls (MCP URL user_id)
    let user_id = composio_user_id(state.tenant_id.as_deref(), "gateway");

    // Get connected toolkits assigned to this instance
    let store = state.composio_store.read().await;
    let instance_connections: Vec<crate::app_state::ComposioConnection> = store
        .connections
        .iter()
        .filter(|c| c.assigned_to.contains(&id))
        .cloned()
        .collect();
    let existing_mcp = store.mcp_servers.clone();
    drop(store);

    if instance_connections.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No Composio connections found for this instance" })),
        ));
    }

    // Collect unique toolkit slugs
    let mut toolkit_slugs: Vec<String> = instance_connections
        .iter()
        .map(|c| c.toolkit_slug.clone())
        .collect();
    toolkit_slugs.sort();
    toolkit_slugs.dedup();

    let client = composio_client();
    let mut mcp_urls: Vec<(String, String)> = Vec::new(); // (slug, url)
    let mut new_mcp_entries: std::collections::HashMap<
        String,
        crate::app_state::ComposioMcpServerEntry,
    > = existing_mcp;

    // Pre-fetch existing MCP servers from Composio so we can reuse them
    let existing_remote_servers: Vec<serde_json::Value> = match client
        .get("https://backend.composio.dev/api/v3/mcp/servers")
        .header("x-api-key", api_key)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await.unwrap_or_default();
            data.get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    for slug in &toolkit_slugs {
        let server_id = if let Some(entry) = new_mcp_entries.get(slug) {
            entry.server_id.clone()
        } else {
            let server_name = composio_mcp_server_name(state.tenant_id.as_deref(), slug);

            // Check if a server with this name already exists remotely
            let existing_sid = existing_remote_servers.iter().find_map(|s| {
                let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name == server_name {
                    s.get("id").and_then(|v| v.as_str()).map(String::from)
                } else {
                    None
                }
            });

            let sid = if let Some(sid) = existing_sid {
                tracing::info!(slug = %slug, server_id = %sid, "Reusing existing Composio MCP server");
                sid
            } else {
                // Resolve toolkit slug to auth_config_id first
                let auth_config_id =
                    match resolve_composio_auth_config_id(api_key, slug).await {
                        Ok(id) => id,
                        Err(_) => {
                            tracing::warn!(
                                slug = %slug,
                                "Could not resolve auth_config_id for toolkit"
                            );
                            continue;
                        }
                    };

                // Create MCP server via Composio API (POST /api/v3/mcp/servers)
                let create_body = serde_json::json!({
                    "name": server_name,
                    "auth_config_ids": [auth_config_id],
                });
                let resp = client
                    .post("https://backend.composio.dev/api/v3/mcp/servers")
                    .header("x-api-key", api_key)
                    .json(&create_body)
                    .send()
                    .await;

                match resp {
                    Ok(r) if r.status().is_success() => {
                        let data: serde_json::Value = r.json().await.unwrap_or_default();
                        data.get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string()
                    }
                    Ok(r) => {
                        let status = r.status();
                        let body = r.text().await.unwrap_or_default();
                        tracing::warn!(slug = %slug, status = %status, body = %body, "Composio MCP create failed");
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(slug = %slug, error = %e, "Composio MCP create request failed");
                        continue;
                    }
                }
            };

            if sid.is_empty() {
                tracing::warn!(slug = %slug, "Composio MCP returned no server ID");
                continue;
            }

            new_mcp_entries.insert(
                slug.clone(),
                crate::app_state::ComposioMcpServerEntry {
                    server_id: sid.clone(),
                    toolkit_slug: slug.clone(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            );
            sid
        };

        // Composio MCP uses Streamable HTTP transport at /mcp endpoint
        let url = format!(
            "https://backend.composio.dev/v3/mcp/{}/mcp?user_id={}",
            server_id, user_id
        );
        mcp_urls.push((slug.clone(), url));
    }

    // Update store with new MCP entries
    {
        let mut store = state.composio_store.write().await;
        store.mcp_servers = new_mcp_entries;
    }
    let _ = persist_composio_store(&state).await;

    // Write MCP server entries to instance config
    let mut config = read_agent_config(&state, &id).await.map_err(|s| {
        (
            s,
            Json(serde_json::json!({ "error": "Failed to read instance config" })),
        )
    })?;

    if let toml::Value::Table(ref mut t) = config {
        // Read existing mcp_servers array
        let mut servers: Vec<toml::Value> = t
            .get("mcp_servers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Remove stale composio-* entries
        servers.retain(|s| {
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !name.starts_with("composio-")
        });

        // Add new composio MCP entries (Streamable HTTP transport)
        for (slug, url) in &mcp_urls {
            let mut entry = toml::map::Map::new();
            entry.insert(
                "name".to_string(),
                toml::Value::String(format!("composio-{}", slug)),
            );
            entry.insert(
                "transport".to_string(),
                toml::Value::String("streamable-http".to_string()),
            );
            entry.insert("url".to_string(), toml::Value::String(url.clone()));
            entry.insert("enabled".to_string(), toml::Value::Boolean(true));
            // Composio MCP servers require API key authentication
            let mut headers = toml::map::Map::new();
            headers.insert(
                "x-api-key".to_string(),
                toml::Value::String(api_key.clone()),
            );
            entry.insert("headers".to_string(), toml::Value::Table(headers));
            servers.push(toml::Value::Table(entry));
        }

        let mcp_toml = toml::Value::Array(servers);
        t.insert("mcp_servers".to_string(), mcp_toml);
    }

    write_agent_config(&state, &id, &config)
        .await
        .map_err(|s| {
            (
                s,
                Json(serde_json::json!({ "error": "Failed to write instance config" })),
            )
        })?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "synced_toolkits": toolkit_slugs,
        "mcp_urls": mcp_urls.iter().map(|(s, u)| serde_json::json!({ "toolkit": s, "url": u })).collect::<Vec<_>>(),
        "note": "MCP server URLs have been written to the instance config. The agent will discover and load these tools on next startup.",
    })))
}

// ---------- Integrations: Google (GOGCLI) ----------

/// Run a GOG CLI command locally on the gateway (not via docker exec).
async fn run_gog_command(
    gog_home: &std::path::Path,
    args: &[&str],
) -> Result<std::process::Output, std::io::Error> {
    let xdg_config_home = gog_home
        .parent()
        .expect("gog_home must have a parent (.google/)");
    tokio::process::Command::new("gog")
        .args(args)
        .env("GOG_KEYRING_BACKEND", "file")
        .env("GOG_KEYRING_PASSWORD", "zeroclaw")
        .env("XDG_CONFIG_HOME", xdg_config_home)
        .output()
        .await
}

/// Persist the Google accounts store to disk.
async fn persist_google_accounts(state: &AppState) -> Result<(), StatusCode> {
    let store = state.google_accounts.read().await;
    let accounts_path = state
        .docker_config
        .agents_dir
        .join(".google")
        .join("accounts.json");
    let json =
        serde_json::to_string_pretty(&*store).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&accounts_path, json).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to persist google accounts store");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// Copy the gateway's GOG keyring files to an agent's data directory.
async fn copy_keyring_to_agent(
    agents_dir: &std::path::Path,
    instance_id: &str,
    gog_home: &std::path::Path,
) -> Result<(), StatusCode> {
    let src = gog_home;
    let dst = agents_dir
        .join(instance_id)
        .join("data")
        .join(".zeroclaw")
        .join("gogcli");

    tokio::fs::create_dir_all(dst.join("keyring"))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to create agent gogcli dir");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Copy credentials.json (client creds)
    let src_creds = src.join("credentials.json");
    if src_creds.exists() {
        tokio::fs::copy(&src_creds, dst.join("credentials.json"))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to copy credentials.json to agent");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    // Copy all keyring files (encrypted tokens)
    let src_keyring = src.join("keyring");
    if src_keyring.exists() {
        let mut entries = tokio::fs::read_dir(&src_keyring)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let dest_file = dst.join("keyring").join(entry.file_name());
            tokio::fs::copy(entry.path(), dest_file)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "Failed to copy keyring file to agent");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
        }
    }

    Ok(())
}

/// Append the Google Workspace section to an agent's TOOLS.md.
/// Only appends if the section doesn't already exist. Never overwrites
/// existing content — the agent's own edits are always preserved.
/// If TOOLS.md doesn't exist yet (no template was provisioned), creates
/// a minimal one with the Google section.
async fn write_tools_md(agents_dir: &std::path::Path, instance_id: &str) -> Result<(), StatusCode> {
    let workspace_dir = agents_dir
        .join(instance_id)
        .join("data")
        .join(".zeroclaw")
        .join("workspace");
    tokio::fs::create_dir_all(&workspace_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tools_path = workspace_dir.join("TOOLS.md");
    let google_section = r#"## Google Workspace (`gog` CLI)
The `gog` command-line tool provides full access to Google Workspace services.
It is pre-installed and whitelisted — call it directly via the shell tool.

### Quick reference
| Task | Command |
|------|---------|
| List recent emails | `gog gmail list "in:inbox" --max 10` |
| Search emails | `gog gmail list "from:name@example.com newer_than:7d"` |
| Read an email | `gog gmail read <message-id>` |
| Send an email | `gog gmail send --to user@example.com --subject "Subject" --body "Body text"` |
| Reply to email | `gog gmail send --reply-to-message-id <id> --body "Reply text"` |
| List Drive files | `gog drive list` |
| Read Drive file | `gog drive read <file-id>` |
| Upload to Drive | `gog drive upload <local-path>` |
| List calendar events | `gog calendar list --from 2026-03-18 --to 2026-03-25` |
| Create calendar event | `gog calendar create --title "Meeting" --start "..." --end "..."` |

### Rules
- Do NOT use shell redirections (`>`, `<`, `2>&1`) with gog commands.
- Do NOT attempt to locate the binary with `which`, `ls`, or path lookups.
- If you get an account error, pass `--account <email>`.
- Use `gog <command> --help` for full flag reference."#;

    if tools_path.exists() {
        let content = tokio::fs::read_to_string(&tools_path)
            .await
            .unwrap_or_default();
        if content.contains("## Google Workspace") {
            // Already has the section — do not touch the file
            return Ok(());
        }
        // Append to existing content (preserving agent edits)
        let updated = format!("{content}\n\n{google_section}\n");
        tokio::fs::write(&tools_path, updated)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        // No TOOLS.md at all — create a minimal fallback with just the Google section
        let content = format!("# TOOLS.md\n\n{google_section}\n");
        tokio::fs::write(&tools_path, content)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(())
}

/// Ensure the google section in a TOML config has `enabled`, `auto_whitelist_gog`,
/// and the given email in `accounts`.
fn ensure_google_account_in_config(config: &mut toml::Value, email: &str) {
    if let toml::Value::Table(ref mut t) = config {
        let google = t
            .entry("google")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let toml::Value::Table(ref mut gt) = google {
            gt.entry("enabled")
                .or_insert_with(|| toml::Value::Boolean(true));
            gt.entry("auto_whitelist_gog")
                .or_insert_with(|| toml::Value::Boolean(true));
            let accounts = gt
                .entry("accounts")
                .or_insert_with(|| toml::Value::Array(vec![]));
            if let toml::Value::Array(ref mut arr) = accounts {
                let email_val = toml::Value::String(email.to_string());
                if !arr.contains(&email_val) {
                    arr.push(email_val);
                }
            }
        }
    }
}

/// Remove an email from the google.accounts array in a TOML config.
fn remove_google_account_from_config(config: &mut toml::Value, email: &str) {
    if let toml::Value::Table(ref mut t) = config {
        if let Some(toml::Value::Table(ref mut gt)) = t.get_mut("google") {
            if let Some(toml::Value::Array(ref mut arr)) = gt.get_mut("accounts") {
                arr.retain(|v| v.as_str() != Some(email));
            }
        }
    }
}

// ── Gateway-level Google endpoints ──

/// List all gateway-authenticated Google accounts.
pub async fn list_google_accounts(State(state): State<AppState>) -> impl IntoResponse {
    let store = state.google_accounts.read().await;
    Json(serde_json::json!({ "accounts": store.accounts }))
}

#[derive(Deserialize)]
pub struct GoogleAuthInitBody {
    email: String,
}

/// Start OAuth flow at the gateway level (not per-instance).
pub async fn google_auth_init(
    State(state): State<AppState>,
    Json(body): Json<GoogleAuthInitBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let _credentials_json = state.google_credentials_json.as_deref().ok_or_else(|| {
        tracing::error!("ZEROCLAW_GOOGLE_CREDENTIALS_JSON not set");
        StatusCode::BAD_REQUEST
    })?;
    let redirect_host = state.google_redirect_host.as_deref().ok_or_else(|| {
        tracing::error!("ZEROCLAW_GOOGLE_REDIRECT_HOST not set");
        StatusCode::BAD_REQUEST
    })?;

    // Run `gog auth add <email> --services user --remote --step 1 --redirect-uri <uri>` locally
    let redirect_uri_arg = format!("http://{redirect_host}/oauth2/callback");
    let output = run_gog_command(
        &state.gog_home,
        &[
            "auth",
            "add",
            &body.email,
            "--services",
            "user",
            "--remote",
            "--step",
            "1",
            "--redirect-uri",
            &redirect_uri_arg,
        ],
    )
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to run gog auth init");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    // Parse the auth URL from gog output
    let auth_url = combined.lines().find_map(|line| {
        if line.starts_with("http") {
            Some(line.trim().to_string())
        } else {
            line.strip_prefix("auth_url\t")
                .map(|url| url.trim().to_string())
        }
    });

    match auth_url {
        Some(url) => {
            let parsed = url::Url::parse(&url).map_err(|e| {
                tracing::error!(error = %e, "Failed to parse auth URL: {url}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let oauth_state = parsed
                .query_pairs()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.to_string())
                .ok_or_else(|| {
                    tracing::error!("No state param found in auth URL: {url}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let redirect_uri = parsed
                .query_pairs()
                .find(|(k, _)| k == "redirect_uri")
                .map(|(_, v)| v.to_string())
                .ok_or_else(|| {
                    tracing::error!("No redirect_uri param found in auth URL: {url}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            if let Ok(mut pending) = state.oauth_pending.lock() {
                pending.insert(
                    oauth_state,
                    crate::app_state::OAuthPendingState {
                        email: body.email.clone(),
                        redirect_uri,
                        created_at: std::time::Instant::now(),
                    },
                );
            }

            Ok(Json(serde_json::json!({
                "auth_url": url,
                "email": body.email,
            })))
        }
        None => {
            tracing::error!(stdout = %stdout, stderr = %stderr, "No auth URL found in gog output");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// OAuth2 callback handler — Google redirects the browser here after consent.
/// This is an unauthenticated endpoint (protected by the unguessable `state` param).
#[derive(Deserialize)]
pub struct OAuthCallbackParams {
    #[allow(dead_code)]
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn google_oauth_callback(
    State(app_state): State<AppState>,
    Query(params): Query<OAuthCallbackParams>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    use axum::response::Html;

    // If Google returned an error, show error page
    if let Some(ref err) = params.error {
        return Html(format!(
            r#"<!DOCTYPE html><html><head><title>OAuth Error</title>
<style>body{{font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#fef2f2}}
.card{{text-align:center;padding:2rem;border-radius:12px;background:#fff;box-shadow:0 2px 8px rgba(0,0,0,.1)}}
h2{{color:#dc2626;margin:0 0 .5rem}}p{{color:#666;margin:0}}</style></head>
<body><div class="card"><h2>OAuth Error</h2><p>{}</p>
</div></body></html>"#,
            err.replace('<', "&lt;").replace('>', "&gt;")
        )).into_response();
    }

    let oauth_state = match params.state {
        Some(ref s) if !s.is_empty() => s.clone(),
        _ => {
            return Html("<h2>OAuth Error</h2><p>Missing state parameter.</p>").into_response();
        }
    };

    // Look up and consume the pending state (one-time use)
    let pending = {
        let mut map = match app_state.oauth_pending.lock() {
            Ok(m) => m,
            Err(_) => {
                return Html("<h2>OAuth Error</h2><p>Internal error.</p>").into_response();
            }
        };
        map.remove(&oauth_state)
    };

    let pending = match pending {
        Some(p) => {
            if p.created_at.elapsed() > std::time::Duration::from_secs(600) {
                return Html("<h2>OAuth Error</h2><p>OAuth session expired. Please try again.</p>")
                    .into_response();
            }
            p
        }
        None => {
            return Html(
                "<h2>OAuth Error</h2><p>Unknown or expired OAuth session. Please try again.</p>",
            )
            .into_response();
        }
    };

    let email = &pending.email;

    // Reconstruct the full callback URL using the redirect_uri from step 1
    let raw_query = req.uri().query().unwrap_or_default();
    let full_callback_url = format!("{}?{raw_query}", pending.redirect_uri);

    // Run step 2 locally on the gateway
    let output = match run_gog_command(
        &app_state.gog_home,
        &[
            "auth",
            "add",
            email,
            "--remote",
            "--step",
            "2",
            "--auth-url",
            &full_callback_url,
            "--redirect-uri",
            &pending.redirect_uri,
        ],
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(error = %e, "Failed to run gog auth step 2");
            let msg = urlencoding::encode("Failed to complete token exchange");
            return Redirect::temporary(&format!("/integrations?google=error&message={msg}"))
                .into_response();
        }
    };

    if output.status.success() {
        // Add account to the gateway store
        {
            let mut store = app_state.google_accounts.write().await;
            if !store.accounts.iter().any(|a| a.email == *email) {
                store.accounts.push(crate::app_state::GoogleAccount {
                    email: email.clone(),
                    assigned_to: vec![],
                    authenticated_at: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                });
            }
        }
        let _ = persist_google_accounts(&app_state).await;

        Html(format!(
            r#"<!DOCTYPE html><html><head><title>Google Account Linked</title>
<style>body{{font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#f0fdf4}}
.card{{text-align:center;padding:2rem;border-radius:12px;background:#fff;box-shadow:0 2px 8px rgba(0,0,0,.1)}}
h2{{color:#16a34a;margin:0 0 .5rem}}p{{color:#666;margin:0}}</style></head>
<body><div class="card"><h2>Account Linked</h2>
<p><strong>{email}</strong> connected to the gateway.</p>
<p style="margin-top:1rem;font-size:.9rem">You can close this tab and return to the dashboard.</p>
</div></body></html>"#
        ))
        .into_response()
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(stderr = %stderr, "gog auth step 2 failed");
        Html(format!(
            r#"<!DOCTYPE html><html><head><title>OAuth Error</title>
<style>body{{font-family:system-ui;display:flex;justify-content:center;align-items:center;height:100vh;margin:0;background:#fef2f2}}
.card{{text-align:center;padding:2rem;border-radius:12px;background:#fff;box-shadow:0 2px 8px rgba(0,0,0,.1);max-width:500px}}
h2{{color:#dc2626;margin:0 0 .5rem}}p{{color:#666;margin:0}}</style></head>
<body><div class="card"><h2>OAuth Error</h2>
<p>Token exchange failed. Please try again from the Integrations page.</p>
<details style="margin-top:1rem;text-align:left;font-size:.85rem"><summary>Details</summary><pre>{}</pre></details>
</div></body></html>"#,
            stderr.replace('<', "&lt;").replace('>', "&gt;")
        ))
        .into_response()
    }
}

/// Manual callback URL submission — user pastes the full redirect URL.
#[derive(Deserialize)]
pub struct GoogleAuthCompleteBody {
    callback_url: String,
    email: String,
}

pub async fn google_auth_complete(
    State(app_state): State<AppState>,
    Json(body): Json<GoogleAuthCompleteBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let redirect_host = app_state.google_redirect_host.as_deref().ok_or_else(|| {
        tracing::error!("ZEROCLAW_GOOGLE_REDIRECT_HOST not set");
        StatusCode::BAD_REQUEST
    })?;
    let redirect_uri = format!("http://{redirect_host}/oauth2/callback");

    let parsed = url::Url::parse(&body.callback_url).map_err(|e| {
        tracing::error!(error = %e, "Invalid callback URL");
        StatusCode::BAD_REQUEST
    })?;

    let full_callback_url = format!("{redirect_uri}?{}", parsed.query().unwrap_or_default());

    // Consume the in-memory pending state (best-effort cleanup)
    if let Some(oauth_state) = parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
    {
        if let Ok(mut map) = app_state.oauth_pending.lock() {
            map.remove(&oauth_state);
        }
    }

    // Run step 2 locally on the gateway
    let output = run_gog_command(
        &app_state.gog_home,
        &[
            "auth",
            "add",
            &body.email,
            "--remote",
            "--step",
            "2",
            "--auth-url",
            &full_callback_url,
            "--redirect-uri",
            &redirect_uri,
        ],
    )
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to run gog auth step 2");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if output.status.success() {
        // Add to gateway store
        {
            let mut store = app_state.google_accounts.write().await;
            if !store.accounts.iter().any(|a| a.email == body.email) {
                store.accounts.push(crate::app_state::GoogleAccount {
                    email: body.email.clone(),
                    assigned_to: vec![],
                    authenticated_at: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                });
            }
        }
        persist_google_accounts(&app_state).await?;

        Ok(Json(serde_json::json!({
            "ok": true,
            "email": body.email,
        })))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(stderr = %stderr, "gog auth step 2 failed");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// Delete a Google account from the gateway and all assigned agents.
pub async fn delete_google_account_global(
    State(state): State<AppState>,
    Path(email): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let assigned_to = {
        let store = state.google_accounts.read().await;
        store
            .accounts
            .iter()
            .find(|a| a.email == email)
            .map(|a| a.assigned_to.clone())
            .unwrap_or_default()
    };

    // Remove from each assigned agent's config
    for agent_id in &assigned_to {
        if let Ok(mut config) = read_agent_config(&state, agent_id).await {
            remove_google_account_from_config(&mut config, &email);
            let _ = write_agent_config(&state, agent_id, &config).await;
        }
    }

    // Remove from gateway store
    {
        let mut store = state.google_accounts.write().await;
        store.accounts.retain(|a| a.email != email);
    }
    persist_google_accounts(&state).await?;

    // Clean from gateway keyring (best-effort)
    let _ = run_gog_command(&state.gog_home, &["auth", "remove", &email]).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Per-instance Google endpoints ──

pub async fn get_google(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let config = read_agent_config(&state, &id).await?;
    let google = config.get("google");

    let enabled = google
        .and_then(|c| c.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_credentials = state.google_credentials_json.is_some();
    let has_redirect_host = state.google_redirect_host.is_some();
    let accounts: Vec<String> = google
        .and_then(|c| c.get("accounts"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let auto_whitelist_gog = google
        .and_then(|c| c.get("auto_whitelist_gog"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let gateway_accounts = {
        let store = state.google_accounts.read().await;
        store.accounts.clone()
    };

    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "has_credentials": has_credentials,
        "has_redirect_host": has_redirect_host,
        "accounts": accounts,
        "auto_whitelist_gog": auto_whitelist_gog,
        "gateway_accounts": gateway_accounts,
    })))
}

#[derive(Deserialize)]
pub struct UpdateGoogleBody {
    enabled: Option<bool>,
    auto_whitelist_gog: Option<bool>,
    assign_accounts: Option<Vec<String>>,
}

pub async fn update_google(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateGoogleBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut config = read_agent_config(&state, &id).await?;

    if let toml::Value::Table(ref mut t) = config {
        let google = t
            .entry("google")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let toml::Value::Table(ref mut gt) = google {
            if let Some(enabled) = body.enabled {
                gt.insert("enabled".to_string(), toml::Value::Boolean(enabled));
            }
            if let Some(auto_wl) = body.auto_whitelist_gog {
                gt.insert(
                    "auto_whitelist_gog".to_string(),
                    toml::Value::Boolean(auto_wl),
                );
            }
        }
    }

    // Handle account assignment from gateway
    if let Some(ref assign_accounts) = body.assign_accounts {
        // Validate all emails exist in gateway store
        {
            let store = state.google_accounts.read().await;
            for email in assign_accounts {
                if !store.accounts.iter().any(|a| &a.email == email) {
                    tracing::error!(email = %email, "Account not found in gateway store");
                    return Err(StatusCode::BAD_REQUEST);
                }
            }
        }

        // Copy keyring to agent
        copy_keyring_to_agent(&state.docker_config.agents_dir, &id, &state.gog_home).await?;

        // Write GOG config.json with default_account
        let gogcli_dir = state
            .docker_config
            .agents_dir
            .join(&id)
            .join("data")
            .join(".zeroclaw")
            .join("gogcli");
        if let Some(first_email) = assign_accounts.first() {
            let gog_config = serde_json::json!({ "default_account": first_email });
            let _ = tokio::fs::write(
                gogcli_dir.join("config.json"),
                serde_json::to_string_pretty(&gog_config).unwrap_or_default(),
            )
            .await;
        }

        // Update agent config with all accounts
        for email in assign_accounts {
            ensure_google_account_in_config(&mut config, email);
        }

        // Write TOOLS.md
        write_tools_md(&state.docker_config.agents_dir, &id).await?;

        // Update assigned_to in gateway store
        {
            let mut store = state.google_accounts.write().await;
            for account in &mut store.accounts {
                if assign_accounts.contains(&account.email) && !account.assigned_to.contains(&id) {
                    account.assigned_to.push(id.clone());
                }
            }
        }
        persist_google_accounts(&state).await?;
    }

    write_agent_config(&state, &id, &config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Unassign a Google account from a specific agent (does NOT remove from gateway).
pub async fn delete_google_account(
    State(state): State<AppState>,
    Path((id, email)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut config = read_agent_config(&state, &id).await?;
    remove_google_account_from_config(&mut config, &email);
    write_agent_config(&state, &id, &config).await?;

    // Update assigned_to in gateway store
    {
        let mut store = state.google_accounts.write().await;
        if let Some(account) = store.accounts.iter_mut().find(|a| a.email == email) {
            account.assigned_to.retain(|aid| aid != &id);
        }
    }
    persist_google_accounts(&state).await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Gateway-level Signal endpoints ──────────────────────────────────

/// Persist the Signal connections store to disk.
async fn persist_signal_connections(state: &AppState) -> Result<(), StatusCode> {
    let store = state.signal_connections.read().await;
    let dir = state.docker_config.agents_dir.join(".signal");
    let _ = tokio::fs::create_dir_all(&dir).await;
    let json =
        serde_json::to_string_pretty(&*store).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(dir.join("connections.json"), json)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to persist signal connections store");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// Determine the signal-cli HTTP URL that an agent container should use.
fn signal_http_url_for_agent(state: &AppState) -> String {
    let port = state.signal_cli_config.http_port;
    if state.docker_config.host_mode {
        format!("http://localhost:{port}")
    } else {
        // In Docker mode, agents reach the gateway container by its service name.
        format!("http://gateway:{port}")
    }
}

/// List all gateway-level Signal connections.
pub async fn list_signal_connections(State(state): State<AppState>) -> impl IntoResponse {
    let store = state.signal_connections.read().await;
    let daemon_running = crate::signal_cli::health_check(state.signal_cli_config.http_port).await;
    Json(serde_json::json!({
        "connections": store.connections,
        "daemon_running": daemon_running,
    }))
}

/// Request body for starting a Signal device-link flow.
#[derive(Deserialize)]
pub struct SignalLinkStartBody {
    /// Human-friendly device name (e.g. "ZeroClaw Gateway").
    #[serde(default = "default_signal_device_name")]
    device_name: String,
}

fn default_signal_device_name() -> String {
    "ZeroClaw".to_string()
}

/// Start a Signal device-link flow.
///
/// Spawns `signal-cli link -n <device_name>` which outputs a `tsdevice://` URI
/// on stdout and blocks until the user scans the QR code with their phone.
/// Returns the URI (to be rendered as a QR code by the frontend) and a
/// pending-link ID to poll/complete later.
pub async fn signal_link_start(
    State(state): State<AppState>,
    Json(body): Json<SignalLinkStartBody>,
) -> impl IntoResponse {
    // Validate device name to prevent argument injection.
    if let Err(e) = validate_device_name(&body.device_name) {
        return e;
    }

    // Enforce limit on concurrent pending link sessions.
    {
        if let Ok(mut pending) = state.signal_link_pending.lock() {
            let active = evict_stale_pending_links(&mut pending);
            if active >= MAX_PENDING_SIGNAL_LINKS {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(
                        serde_json::json!({ "error": "Too many pending link sessions. Complete or wait for existing ones to expire." }),
                    ),
                );
            }
        }
    }

    // We need to stop the daemon temporarily because signal-cli locks its data
    // directory — the `link` command can't run while the daemon holds the lock.
    crate::signal_cli::stop_daemon(&state.signal_cli_handle).await;

    let child = tokio::process::Command::new(&state.signal_cli_config.cli_path)
        .args([
            "--config",
            &state.signal_cli_config.data_dir.to_string_lossy(),
            "link",
            "-n",
            &body.device_name,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            // Log internal details for operators; return sanitized message to client.
            tracing::error!(error = %e, path = %state.signal_cli_config.cli_path, "Failed to spawn signal-cli link");
            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                "signal-cli binary not found. Check gateway configuration."
            } else {
                "Failed to start signal-cli. Check gateway logs for details."
            };
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": msg })),
            );
        }
    };

    // Read the tsdevice:// URI from stdout.  signal-cli prints the URI on the
    // first line and then blocks waiting for the phone scan.
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to capture signal-cli stdout" })),
            );
        }
    };

    let mut reader = tokio::io::BufReader::new(stdout);
    let mut uri_line = String::new();

    // Wait up to 30 seconds for the URI to appear.
    let read_result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut uri_line),
    )
    .await;

    match read_result {
        Ok(Ok(0)) | Err(_) => {
            // EOF or timeout — process likely failed. Read stderr for logging only.
            let stderr_msg = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = String::new();
                let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                buf
            } else {
                String::new()
            };
            let _ = child.kill().await;
            // Restart daemon since we stopped it.
            crate::signal_cli::start_daemon(&state.signal_cli_config, &state.signal_cli_handle)
                .await;
            // Log full details internally; return sanitized message to client.
            tracing::error!(stderr = %stderr_msg.trim(), "signal-cli link failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "error": "signal-cli link did not produce a URI. Check gateway logs for details." }),
                ),
            );
        }
        Ok(Err(e)) => {
            let _ = child.kill().await;
            crate::signal_cli::start_daemon(&state.signal_cli_config, &state.signal_cli_handle)
                .await;
            tracing::error!(error = %e, "Failed to read signal-cli link output");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "error": "Failed to read signal-cli output. Check gateway logs for details." }),
                ),
            );
        }
        Ok(Ok(_)) => {}
    }

    let device_link_uri = uri_line.trim().to_string();
    if !device_link_uri.starts_with("tsdevice:") && !device_link_uri.starts_with("sgnl:") {
        let _ = child.kill().await;
        crate::signal_cli::start_daemon(&state.signal_cli_config, &state.signal_cli_handle).await;
        // Log the raw output internally; don't expose to client.
        tracing::error!(output = %device_link_uri, "Unexpected signal-cli output");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({ "error": "signal-cli produced unexpected output. Check gateway logs." }),
            ),
        );
    }

    // Store the pending link (child process keeps running, waiting for QR scan).
    let link_id = uuid::Uuid::new_v4().to_string();
    if let Ok(mut pending) = state.signal_link_pending.lock() {
        pending.insert(
            link_id.clone(),
            crate::app_state::SignalLinkPendingState {
                device_name: body.device_name,
                child,
                created_at: std::time::Instant::now(),
            },
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "link_id": link_id,
            "device_link_uri": device_link_uri,
        })),
    )
}

/// Request body for completing a Signal device-link flow.
#[derive(Deserialize)]
pub struct SignalLinkFinishBody {
    /// The link_id returned by `/link/start`.
    link_id: String,
    /// User-chosen name for this connection (e.g. "support-line").
    name: String,
    /// The E.164 phone number that was linked.
    account: String,
}

/// Complete a Signal device-link flow.
///
/// The user has scanned the QR code — the `signal-cli link` process should
/// have exited successfully.  This saves the connection to the store and
/// restarts the daemon.
pub async fn signal_link_finish(
    State(state): State<AppState>,
    Json(body): Json<SignalLinkFinishBody>,
) -> impl IntoResponse {
    // Validate inputs.
    if let Err(e) = validate_connection_name(&body.name) {
        return e;
    }
    if let Err(e) = validate_e164(&body.account) {
        return e;
    }

    // Validate name is unique.
    {
        let store = state.signal_connections.read().await;
        if store.connections.iter().any(|c| c.name == body.name) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "Connection name already exists" })),
            );
        }
    }

    // Consume the pending link.
    let mut pending_state = {
        let pending = state.signal_link_pending.lock();
        match pending {
            Ok(mut p) => match p.remove(&body.link_id) {
                Some(s) => s,
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(
                            serde_json::json!({ "error": "Link session not found or expired. Please start a new link." }),
                        ),
                    );
                }
            },
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Internal lock error" })),
                );
            }
        }
    };

    // Wait for the link process to complete (up to 5 seconds — it should
    // already be done once the user has scanned the QR code).
    let wait_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        pending_state.child.wait(),
    )
    .await;

    let success = match wait_result {
        Ok(Ok(status)) => status.success(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "signal-cli link process error");
            false
        }
        Err(_) => {
            // Still running — the user may not have scanned yet.  Kill and fail.
            let _ = pending_state.child.kill().await;
            tracing::warn!("signal-cli link timed out waiting for completion");
            false
        }
    };

    // Restart the daemon regardless of success.
    crate::signal_cli::start_daemon(&state.signal_cli_config, &state.signal_cli_handle).await;

    if !success {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Signal linking failed. Make sure you scanned the QR code with your Signal app before completing." }),
            ),
        );
    }

    // Normalize account: ensure E.164 format.
    let account = if body.account.starts_with('+') {
        body.account.clone()
    } else {
        format!("+{}", body.account)
    };

    // Save the connection.
    {
        let mut store = state.signal_connections.write().await;
        store.connections.push(crate::app_state::SignalConnection {
            name: body.name.clone(),
            account,
            linked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            assigned_to: vec![],
        });
    }
    if let Err(e) = persist_signal_connections(&state).await {
        tracing::error!(error = ?e, "Failed to persist signal connections");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to save connection to disk" })),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "name": body.name,
        })),
    )
}

/// Delete a Signal connection from the gateway and all assigned agents.
pub async fn delete_signal_connection(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_connection_name(&name) {
        return e;
    }
    let assigned_to = {
        let store = state.signal_connections.read().await;
        store
            .connections
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.assigned_to.clone())
            .unwrap_or_default()
    };

    // Remove signal config from each assigned agent.
    for agent_id in &assigned_to {
        if let Ok(mut config) = read_agent_config(&state, agent_id).await {
            remove_signal_from_config(&mut config);
            let _ = write_agent_config(&state, agent_id, &config).await;
        }
    }

    // Remove from gateway store.
    {
        let mut store = state.signal_connections.write().await;
        store.connections.retain(|c| c.name != name);
    }
    if let Err(sc) = persist_signal_connections(&state).await {
        return (
            sc,
            Json(serde_json::json!({ "error": "Failed to persist connections" })),
        );
    }

    // If no connections remain, stop the daemon.
    {
        let store = state.signal_connections.read().await;
        if store.connections.is_empty() {
            crate::signal_cli::stop_daemon(&state.signal_cli_handle).await;
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

// ── Per-instance Signal endpoints ───────────────────────────────────

/// Get Signal integration status for an agent instance.
pub async fn get_signal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_agent_id(&id) {
        return e;
    }
    let config = match read_agent_config(&state, &id).await {
        Ok(c) => c,
        Err(sc) => return (sc, Json(serde_json::json!({ "error": "Agent not found" }))),
    };
    let signal = config.get("channels_config").and_then(|c| c.get("signal"));

    let enabled = signal.is_some();
    let account = signal
        .and_then(|s| s.get("account"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let http_url = signal
        .and_then(|s| s.get("http_url"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // Find the connection name from our store by matching the account.
    let connection_name = {
        let store = state.signal_connections.read().await;
        store
            .connections
            .iter()
            .find(|c| c.account == account)
            .map(|c| c.name.clone())
    };

    let gateway_connections = {
        let store = state.signal_connections.read().await;
        store.connections.clone()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "enabled": enabled,
            "account": account,
            "http_url": http_url,
            "connection_name": connection_name,
            "gateway_connections": gateway_connections,
        })),
    )
}

/// Request body for assigning a Signal connection to an agent.
#[derive(Deserialize)]
pub struct AssignSignalBody {
    /// Name of the gateway-level Signal connection to assign.
    connection: String,
    /// Optional group ID filter ("dm" for DMs only, or a specific group ID).
    #[serde(default)]
    group_id: Option<String>,
    /// Allowed sender numbers. Defaults to ["*"] (all).
    #[serde(default)]
    allowed_from: Option<Vec<String>>,
    /// Skip attachment-only messages.
    #[serde(default)]
    ignore_attachments: Option<bool>,
    /// Skip story messages.
    #[serde(default)]
    ignore_stories: Option<bool>,
}

/// Assign a named Signal connection to an agent instance.
pub async fn assign_signal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AssignSignalBody>,
) -> impl IntoResponse {
    if let Err(e) = validate_agent_id(&id) {
        return e;
    }

    // Validate allowed_from entries are valid E.164 numbers or "*".
    if let Some(ref entries) = body.allowed_from {
        for entry in entries {
            if entry != "*" {
                if let Err(e) = validate_e164(entry) {
                    return e;
                }
            }
        }
    }

    // Look up the connection.
    let connection = {
        let store = state.signal_connections.read().await;
        match store
            .connections
            .iter()
            .find(|c| c.name == body.connection)
            .cloned()
        {
            Some(c) => c,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "Signal connection not found" })),
                )
            }
        }
    };

    let http_url = signal_http_url_for_agent(&state);
    let allowed_from = body.allowed_from.unwrap_or_else(|| vec!["*".to_string()]);
    let ignore_attachments = body.ignore_attachments.unwrap_or(false);
    let ignore_stories = body.ignore_stories.unwrap_or(true);

    // Write signal config into the agent's config.toml.
    let mut config = match read_agent_config(&state, &id).await {
        Ok(c) => c,
        Err(sc) => return (sc, Json(serde_json::json!({ "error": "Agent not found" }))),
    };
    ensure_signal_in_config(
        &mut config,
        &http_url,
        &connection.account,
        body.group_id.as_deref(),
        &allowed_from,
        ignore_attachments,
        ignore_stories,
    );
    if let Err(sc) = write_agent_config(&state, &id, &config).await {
        return (
            sc,
            Json(serde_json::json!({ "error": "Failed to write agent config" })),
        );
    }

    // Update assigned_to in the gateway store.
    {
        let mut store = state.signal_connections.write().await;
        if let Some(conn) = store
            .connections
            .iter_mut()
            .find(|c| c.name == body.connection)
        {
            if !conn.assigned_to.contains(&id) {
                conn.assigned_to.push(id.clone());
            }
        }
    }
    if let Err(sc) = persist_signal_connections(&state).await {
        return (
            sc,
            Json(serde_json::json!({ "error": "Failed to persist connections" })),
        );
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

/// Unassign a Signal connection from an agent instance.
pub async fn unassign_signal(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = validate_agent_id(&id) {
        return e;
    }
    let mut config = match read_agent_config(&state, &id).await {
        Ok(c) => c,
        Err(sc) => return (sc, Json(serde_json::json!({ "error": "Agent not found" }))),
    };
    remove_signal_from_config(&mut config);
    if let Err(sc) = write_agent_config(&state, &id, &config).await {
        return (
            sc,
            Json(serde_json::json!({ "error": "Failed to write agent config" })),
        );
    }

    // Update assigned_to in the gateway store.
    {
        let mut store = state.signal_connections.write().await;
        if let Some(conn) = store.connections.iter_mut().find(|c| c.name == name) {
            conn.assigned_to.retain(|aid| aid != &id);
        }
    }
    if let Err(sc) = persist_signal_connections(&state).await {
        return (
            sc,
            Json(serde_json::json!({ "error": "Failed to persist connections" })),
        );
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

// ── Signal config helpers ───────────────────────────────────────────

/// Write the `[channels_config.signal]` section into an agent's TOML config.
fn ensure_signal_in_config(
    config: &mut toml::Value,
    http_url: &str,
    account: &str,
    group_id: Option<&str>,
    allowed_from: &[String],
    ignore_attachments: bool,
    ignore_stories: bool,
) {
    if let toml::Value::Table(ref mut root) = config {
        let channels = root
            .entry("channels_config")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let toml::Value::Table(ref mut ct) = channels {
            // Ensure the required `cli` field exists (defaults to false for Docker agents).
            ct.entry("cli").or_insert(toml::Value::Boolean(false));
            let mut signal_table = toml::map::Map::new();
            signal_table.insert(
                "http_url".to_string(),
                toml::Value::String(http_url.to_string()),
            );
            signal_table.insert(
                "account".to_string(),
                toml::Value::String(account.to_string()),
            );
            if let Some(gid) = group_id {
                signal_table.insert("group_id".to_string(), toml::Value::String(gid.to_string()));
            }
            signal_table.insert(
                "allowed_from".to_string(),
                toml::Value::Array(
                    allowed_from
                        .iter()
                        .map(|s| toml::Value::String(s.clone()))
                        .collect(),
                ),
            );
            signal_table.insert(
                "ignore_attachments".to_string(),
                toml::Value::Boolean(ignore_attachments),
            );
            signal_table.insert(
                "ignore_stories".to_string(),
                toml::Value::Boolean(ignore_stories),
            );
            ct.insert("signal".to_string(), toml::Value::Table(signal_table));
        }
    }
}

/// Remove the `[channels_config.signal]` section from an agent's TOML config.
fn remove_signal_from_config(config: &mut toml::Value) {
    if let toml::Value::Table(ref mut root) = config {
        if let Some(toml::Value::Table(ref mut ct)) = root.get_mut("channels_config") {
            ct.remove("signal");
        }
    }
}

// ---------- Skills ----------

fn agent_skills_dir(state: &AppState, id: &str) -> std::path::PathBuf {
    agent_workspace_dir(state, id).join("skills")
}

pub async fn list_skills(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let skills_dir = agent_skills_dir(&state, &id);
    if !skills_dir.exists() {
        return Ok(Json(serde_json::json!({ "skills": [] })));
    }

    let mut entries = tokio::fs::read_dir(&skills_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut skills = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".md") {
            if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                skills.push(serde_json::json!({ "name": name, "content": content }));
            }
        }
    }

    skills.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(Json(serde_json::json!({ "skills": skills })))
}

#[derive(Deserialize)]
pub struct UpdateSkillBody {
    content: String,
}

pub async fn update_skill(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
    Json(body): Json<UpdateSkillBody>,
) -> Result<impl IntoResponse, StatusCode> {
    if !name.ends_with(".md") || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let skills_dir = agent_skills_dir(&state, &id);
    tokio::fs::create_dir_all(&skills_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = skills_dir.join(&name);
    tokio::fs::write(&path, &body.content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_skill(
    State(state): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    if !name.ends_with(".md") || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = agent_skills_dir(&state, &id).join(&name);
    tokio::fs::remove_file(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- Cron Jobs ----------

/// Path to the agent's cron SQLite database on the host filesystem.
fn agent_cron_db_path(state: &AppState, id: &str) -> std::path::PathBuf {
    agent_workspace_dir(state, id).join("cron").join("jobs.db")
}

pub async fn list_cron_jobs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let db_path = agent_cron_db_path(&state, &id);
    if !db_path.exists() {
        return Ok(Json(serde_json::json!({ "jobs": [] })));
    }

    let jobs = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, expression, command, schedule, job_type, prompt, name, \
             enabled, next_run, last_run, last_status, last_output, created_at, \
             session_target, model, delivery, delete_after_run \
             FROM cron_jobs ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "expression": row.get::<_, String>(1).unwrap_or_default(),
                "command": row.get::<_, String>(2).unwrap_or_default(),
                "schedule": row.get::<_, String>(3).unwrap_or_default(),
                "job_type": row.get::<_, String>(4).unwrap_or_default(),
                "prompt": row.get::<_, String>(5).unwrap_or_default(),
                "name": row.get::<_, String>(6).unwrap_or_default(),
                "enabled": row.get::<_, bool>(7).unwrap_or(true),
                "next_run": row.get::<_, String>(8).unwrap_or_default(),
                "last_run": row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                "last_status": row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                "last_output": row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                "created_at": row.get::<_, String>(12).unwrap_or_default(),
                "session_target": row.get::<_, String>(13).unwrap_or_default(),
                "model": row.get::<_, Option<String>>(14)?.unwrap_or_default(),
                "delivery": row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                "delete_after_run": row.get::<_, bool>(16).unwrap_or(false),
            }))
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "jobs": jobs })))
}

pub async fn get_cron_runs(
    State(state): State<AppState>,
    Path((id, job_id)): Path<(String, String)>,
    Query(q): Query<CronRunsQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let db_path = agent_cron_db_path(&state, &id);
    if !db_path.exists() {
        return Ok(Json(serde_json::json!({ "runs": [] })));
    }

    let limit = q.limit.unwrap_or(20);
    let runs = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, job_id, started_at, finished_at, status, output, duration_ms \
             FROM cron_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![job_id, limit], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "job_id": row.get::<_, String>(1)?,
                "started_at": row.get::<_, String>(2).unwrap_or_default(),
                "finished_at": row.get::<_, String>(3).unwrap_or_default(),
                "status": row.get::<_, String>(4).unwrap_or_default(),
                "output": row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                "duration_ms": row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            }))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "runs": runs })))
}

#[derive(Deserialize)]
pub struct CronRunsQuery {
    limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateCronJobBody {
    name: String,
    expression: String,
    job_type: String,
    command: Option<String>,
    prompt: Option<String>,
}

pub async fn create_cron_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateCronJobBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let db_path = agent_cron_db_path(&state, &id);
    // Ensure cron directory exists
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let job_id_clone = job_id.clone();
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // Compute next_run from the cron expression.
    let next_run_str = {
        let expr = &body.expression;
        // Normalize 5-field cron to 6-field (prepend seconds=0) for the cron crate.
        let normalized = if expr.split_whitespace().count() == 5 {
            format!("0 {expr}")
        } else {
            expr.to_string()
        };
        match cron::Schedule::from_str(&normalized) {
            Ok(schedule) => match schedule.after(&now).next() {
                Some(next) => next
                    .with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                None => now_str.clone(),
            },
            Err(_) => now_str.clone(),
        }
    };

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = rusqlite::Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cron_jobs (
                id               TEXT PRIMARY KEY,
                expression       TEXT NOT NULL,
                command          TEXT NOT NULL,
                schedule         TEXT,
                job_type         TEXT NOT NULL DEFAULT 'shell',
                prompt           TEXT,
                name             TEXT,
                session_target   TEXT NOT NULL DEFAULT 'isolated',
                model            TEXT,
                enabled          INTEGER NOT NULL DEFAULT 1,
                delivery         TEXT,
                delete_after_run INTEGER NOT NULL DEFAULT 0,
                created_at       TEXT NOT NULL,
                next_run         TEXT NOT NULL,
                last_run         TEXT,
                last_status      TEXT,
                last_output      TEXT
            );
            CREATE TABLE IF NOT EXISTS cron_runs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id      TEXT NOT NULL,
                started_at  TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                status      TEXT NOT NULL,
                output      TEXT,
                duration_ms INTEGER,
                FOREIGN KEY (job_id) REFERENCES cron_jobs(id) ON DELETE CASCADE
            );",
        )?;
        conn.execute(
            "INSERT INTO cron_jobs (id, name, expression, command, prompt, job_type, enabled, created_at, next_run) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
            rusqlite::params![
                job_id_clone,
                body.name,
                body.expression,
                body.command.unwrap_or_default(),
                body.prompt.unwrap_or_default(),
                body.job_type,
                now_str,
                next_run_str,
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true, "id": job_id })))
}

pub async fn update_cron_job(
    State(state): State<AppState>,
    Path((id, job_id)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, StatusCode> {
    let db_path = agent_cron_db_path(&state, &id);
    if !db_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = rusqlite::Connection::open(&db_path)?;
        // Build SET clauses from the JSON body
        let allowed = [
            "name",
            "expression",
            "command",
            "prompt",
            "enabled",
            "job_type",
        ];
        for field in &allowed {
            if let Some(val) = body.get(field) {
                let sql = format!("UPDATE cron_jobs SET {} = ?1 WHERE id = ?2", field);
                match val {
                    serde_json::Value::Bool(b) => {
                        conn.execute(&sql, rusqlite::params![*b as i32, job_id])?;
                    }
                    serde_json::Value::String(s) => {
                        conn.execute(&sql, rusqlite::params![s, job_id])?;
                    }
                    serde_json::Value::Number(n) => {
                        conn.execute(&sql, rusqlite::params![n.as_i64().unwrap_or(0), job_id])?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_cron_job(
    State(state): State<AppState>,
    Path((id, job_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let db_path = agent_cron_db_path(&state, &id);
    if !db_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = rusqlite::Connection::open(&db_path)?;
        conn.execute(
            "DELETE FROM cron_runs WHERE job_id = ?1",
            rusqlite::params![job_id],
        )?;
        conn.execute(
            "DELETE FROM cron_jobs WHERE id = ?1",
            rusqlite::params![job_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------- Chat (non-streaming REST) ----------

#[derive(Deserialize)]
pub struct ChatBody {
    message: String,
}

pub async fn chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ChatBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut client = state
        .registry
        .get_client(&id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let resp = client
        .send_message(authed_request(
            proto::SendMessageRequest {
                message: body.message,
            },
            &state.grpc_secret,
        ))
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut stream = resp.into_inner();
    let mut content = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut turn_id = String::new();
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;

    while let Some(msg) = stream.next().await {
        let msg = msg.map_err(|_| StatusCode::BAD_GATEWAY)?;
        turn_id = msg.turn_id.clone();
        if let Some(output) = msg.output {
            use proto::chat_output::Output;
            match output {
                Output::Delta(d) => content.push_str(&d),
                Output::ToolStart(ts) => {
                    tool_calls.push(serde_json::json!({
                        "type": "tool_start",
                        "tool": ts.tool,
                        "arguments": ts.arguments,
                    }));
                }
                Output::ToolResult(tr) => {
                    tool_calls.push(serde_json::json!({
                        "type": "tool_result",
                        "tool": tr.tool,
                        "success": tr.success,
                        "output": tr.output,
                    }));
                }
                Output::Done(d) => {
                    if !d.content.is_empty() {
                        content = d.content;
                    }
                    input_tokens = d.input_tokens;
                    output_tokens = d.output_tokens;
                }
                Output::Error(e) => {
                    return Ok(Json(serde_json::json!({
                        "error": e.message,
                        "turn_id": turn_id,
                    })));
                }
                _ => {}
            }
        }
    }

    Ok(Json(serde_json::json!({
        "turn_id": turn_id,
        "content": content,
        "tool_calls": tool_calls,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composio_prefix_without_tenant() {
        assert_eq!(composio_prefix(None), "zcgw-");
    }

    #[test]
    fn composio_prefix_with_tenant() {
        assert_eq!(composio_prefix(Some("acme")), "zcgw-acme-");
    }

    #[test]
    fn composio_user_id_without_tenant() {
        assert_eq!(composio_user_id(None, "bot-1"), "zcgw-bot-1");
    }

    #[test]
    fn composio_user_id_with_tenant() {
        assert_eq!(composio_user_id(Some("acme"), "bot-1"), "zcgw-acme-bot-1");
    }

    #[test]
    fn composio_user_id_gateway_fallback() {
        assert_eq!(composio_user_id(None, "gateway"), "zcgw-gateway");
        assert_eq!(
            composio_user_id(Some("acme"), "gateway"),
            "zcgw-acme-gateway"
        );
    }

    #[test]
    fn composio_mcp_server_name_without_tenant() {
        assert_eq!(composio_mcp_server_name(None, "gmail"), "zcgw-gmail");
    }

    #[test]
    fn composio_mcp_server_name_with_tenant() {
        assert_eq!(
            composio_mcp_server_name(Some("acme"), "gmail"),
            "zcgw-acme-gmail"
        );
    }

    #[test]
    fn composio_user_id_roundtrip_without_tenant() {
        let instance_id = "my-bot";
        let user_id = composio_user_id(None, instance_id);
        let prefix = composio_prefix(None);
        let recovered = user_id.strip_prefix(&prefix).unwrap();
        assert_eq!(recovered, instance_id);
    }

    #[test]
    fn composio_user_id_roundtrip_with_tenant() {
        let instance_id = "my-bot";
        let tenant = Some("acme");
        let user_id = composio_user_id(tenant, instance_id);
        let prefix = composio_prefix(tenant);
        let recovered = user_id.strip_prefix(&prefix).unwrap();
        assert_eq!(recovered, instance_id);
    }
}
