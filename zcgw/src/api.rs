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
use tonic::metadata::MetadataValue;
use tokio_stream::StreamExt;

/// Attach gRPC auth metadata to a request.
fn authed_request<T>(body: T, secret: &str) -> tonic::Request<T> {
    let mut req = tonic::Request::new(body);
    let val: MetadataValue<_> = format!("Bearer {}", secret).parse().unwrap();
    req.metadata_mut().insert("authorization", val);
    req
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
            health: health
                .get(id)
                .cloned()
                .unwrap_or(InstanceHealth::Unknown),
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
        .get_status(authed_request(proto::GetStatusRequest {}, &state.grpc_secret))
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
        .get_config(authed_request(proto::GetConfigRequest {}, &state.grpc_secret))
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
        .list_tools(authed_request(proto::ListToolsRequest {}, &state.grpc_secret))
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
    Ok(Json(serde_json::json!({ "filename": filename, "content": content })))
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
                ChannelField { name: "bot_token", label: "Bot Token", field_type: "string", required: true, sensitive: true, help: "Telegram bot token from @BotFather" },
                ChannelField { name: "allowed_users", label: "Allowed Users", field_type: "string_list", required: false, sensitive: false, help: "Telegram user IDs or usernames. Empty = deny all" },
                ChannelField { name: "stream_mode", label: "Stream Mode", field_type: "select:off,partial", required: false, sensitive: false, help: "off = single message, partial = progressive edits" },
                ChannelField { name: "draft_update_interval_ms", label: "Draft Update Interval (ms)", field_type: "u64", required: false, sensitive: false, help: "Min interval between draft edits (default: 1000)" },
                ChannelField { name: "interrupt_on_new_message", label: "Interrupt on New Message", field_type: "bool", required: false, sensitive: false, help: "Cancel in-flight request on new message from same sender" },
                ChannelField { name: "mention_only", label: "Mention Only", field_type: "bool", required: false, sensitive: false, help: "Only respond to @-mentions in groups (DMs always processed)" },
            ],
        },
        ChannelDescriptor {
            channel_type: "discord",
            label: "Discord",
            fields: vec![
                ChannelField { name: "bot_token", label: "Bot Token", field_type: "string", required: true, sensitive: true, help: "Discord bot token" },
                ChannelField { name: "guild_id", label: "Guild ID", field_type: "string", required: false, sensitive: false, help: "Restrict to a specific guild" },
                ChannelField { name: "allowed_users", label: "Allowed Users", field_type: "string_list", required: false, sensitive: false, help: "Allowed user IDs" },
                ChannelField { name: "listen_to_bots", label: "Listen to Bots", field_type: "bool", required: false, sensitive: false, help: "Process messages from other bots" },
                ChannelField { name: "mention_only", label: "Mention Only", field_type: "bool", required: false, sensitive: false, help: "Only respond when mentioned" },
            ],
        },
        ChannelDescriptor {
            channel_type: "slack",
            label: "Slack",
            fields: vec![
                ChannelField { name: "bot_token", label: "Bot Token", field_type: "string", required: true, sensitive: true, help: "Slack bot token (xoxb-...)" },
                ChannelField { name: "app_token", label: "App Token", field_type: "string", required: false, sensitive: true, help: "Socket mode app token (xapp-...)" },
                ChannelField { name: "channel_id", label: "Channel ID", field_type: "string", required: false, sensitive: false, help: "Default channel ID" },
                ChannelField { name: "allowed_users", label: "Allowed Users", field_type: "string_list", required: false, sensitive: false, help: "Allowed user IDs" },
            ],
        },
        ChannelDescriptor {
            channel_type: "whatsapp",
            label: "WhatsApp",
            fields: vec![
                ChannelField { name: "access_token", label: "Access Token", field_type: "string", required: false, sensitive: true, help: "Cloud API access token" },
                ChannelField { name: "phone_number_id", label: "Phone Number ID", field_type: "string", required: false, sensitive: false, help: "Cloud API phone number ID" },
                ChannelField { name: "session_path", label: "Session Path", field_type: "string", required: false, sensitive: false, help: "Web client session path (alternative to Cloud)" },
                ChannelField { name: "allowed_numbers", label: "Allowed Numbers", field_type: "string_list", required: false, sensitive: false, help: "Allowed phone numbers" },
            ],
        },
        ChannelDescriptor {
            channel_type: "email",
            label: "Email",
            fields: vec![
                ChannelField { name: "imap_host", label: "IMAP Host", field_type: "string", required: true, sensitive: false, help: "IMAP server hostname" },
                ChannelField { name: "smtp_host", label: "SMTP Host", field_type: "string", required: true, sensitive: false, help: "SMTP server hostname" },
                ChannelField { name: "username", label: "Username", field_type: "string", required: true, sensitive: false, help: "Email account username" },
                ChannelField { name: "password", label: "Password", field_type: "string", required: true, sensitive: true, help: "Email account password" },
                ChannelField { name: "from_address", label: "From Address", field_type: "string", required: true, sensitive: false, help: "Sender email address" },
                ChannelField { name: "allowed_senders", label: "Allowed Senders", field_type: "string_list", required: false, sensitive: false, help: "Allowed sender addresses" },
            ],
        },
        ChannelDescriptor {
            channel_type: "signal",
            label: "Signal",
            fields: vec![
                ChannelField { name: "http_url", label: "HTTP URL", field_type: "string", required: true, sensitive: false, help: "signal-cli REST API URL" },
                ChannelField { name: "account", label: "Account", field_type: "string", required: true, sensitive: false, help: "Signal account phone number" },
                ChannelField { name: "group_id", label: "Group ID", field_type: "string", required: false, sensitive: false, help: "Signal group ID" },
                ChannelField { name: "allowed_from", label: "Allowed From", field_type: "string_list", required: false, sensitive: false, help: "Allowed sender numbers" },
            ],
        },
        ChannelDescriptor {
            channel_type: "matrix",
            label: "Matrix",
            fields: vec![
                ChannelField { name: "homeserver", label: "Homeserver", field_type: "string", required: true, sensitive: false, help: "Matrix homeserver URL" },
                ChannelField { name: "access_token", label: "Access Token", field_type: "string", required: true, sensitive: true, help: "Matrix access token" },
                ChannelField { name: "room_id", label: "Room ID", field_type: "string", required: true, sensitive: false, help: "Room to join" },
                ChannelField { name: "allowed_users", label: "Allowed Users", field_type: "string_list", required: true, sensitive: false, help: "Allowed Matrix user IDs" },
            ],
        },
        ChannelDescriptor {
            channel_type: "irc",
            label: "IRC",
            fields: vec![
                ChannelField { name: "server", label: "Server", field_type: "string", required: true, sensitive: false, help: "IRC server address" },
                ChannelField { name: "nickname", label: "Nickname", field_type: "string", required: true, sensitive: false, help: "Bot nickname" },
                ChannelField { name: "channels", label: "Channels", field_type: "string_list", required: false, sensitive: false, help: "Channels to join" },
                ChannelField { name: "server_password", label: "Server Password", field_type: "string", required: false, sensitive: true, help: "Server password" },
            ],
        },
        ChannelDescriptor {
            channel_type: "mattermost",
            label: "Mattermost",
            fields: vec![
                ChannelField { name: "url", label: "URL", field_type: "string", required: true, sensitive: false, help: "Mattermost server URL" },
                ChannelField { name: "bot_token", label: "Bot Token", field_type: "string", required: true, sensitive: true, help: "Mattermost bot token" },
                ChannelField { name: "channel_id", label: "Channel ID", field_type: "string", required: false, sensitive: false, help: "Default channel ID" },
                ChannelField { name: "allowed_users", label: "Allowed Users", field_type: "string_list", required: false, sensitive: false, help: "Allowed user IDs" },
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
                ch.entry("enabled")
                    .or_insert(toml::Value::Boolean(true));
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
    let mut channels_toml: toml::Value = serde_json::from_value(body.channels_config)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    if let toml::Value::Table(ref mut channels) = channels_toml {
        let keys: Vec<String> = channels.keys().cloned().collect();
        for key in keys {
            let remove = if let Some(toml::Value::Table(ref mut ch)) = channels.get_mut(&key) {
                // Check if enabled is false — if so, remove the whole channel
                let enabled = ch
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
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
        channels
            .entry("cli")
            .or_insert(toml::Value::Boolean(true));
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
    // Mask api_key: only show if it exists
    let has_api_key = composio
        .and_then(|c| c.get("api_key"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "entity_id": entity_id,
        "has_api_key": has_api_key,
    })))
}

#[derive(Deserialize)]
pub struct UpdateComposioBody {
    enabled: Option<bool>,
    api_key: Option<String>,
    entity_id: Option<String>,
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
        }
    }

    write_agent_config(&state, &id, &config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
    let json = serde_json::to_string_pretty(&*store).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&accounts_path, json)
        .await
        .map_err(|e| {
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
            tokio::fs::copy(entry.path(), dest_file).await.map_err(|e| {
                tracing::error!(error = %e, "Failed to copy keyring file to agent");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }
    }

    Ok(())
}

/// Write/update the TOOLS.md file for an agent with Google Workspace instructions.
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
            // Already has the section
            return Ok(());
        }
        let updated = format!("{content}\n\n{google_section}\n");
        tokio::fs::write(&tools_path, updated)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        let content = format!(
            "# TOOLS.md\n\n## Built-in Tools\n\
             - **shell** — Execute terminal commands (subject to security policy)\n\
             - **file_read** — Read file contents\n\
             - **file_write** — Write/edit files\n\
             - **memory_store** — Save durable context to long-term memory\n\
             - **memory_recall** — Search long-term memory\n\
             - **memory_forget** — Remove a memory entry\n\n{google_section}\n"
        );
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
pub async fn list_google_accounts(
    State(state): State<AppState>,
) -> impl IntoResponse {
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
    let output = run_gog_command(&state.gog_home, &[
        "auth", "add", &body.email,
        "--services", "user", "--remote", "--step", "1",
        "--redirect-uri", &redirect_uri_arg,
    ])
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to run gog auth init");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    // Parse the auth URL from gog output
    let auth_url = combined
        .lines()
        .find_map(|line| {
            if line.starts_with("http") {
                Some(line.trim().to_string())
            } else {
                line.strip_prefix("auth_url\t").map(|url| url.trim().to_string())
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
    let output = match run_gog_command(&app_state.gog_home, &[
        "auth", "add", email,
        "--remote", "--step", "2",
        "--auth-url", &full_callback_url,
        "--redirect-uri", &pending.redirect_uri,
    ])
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
                    authenticated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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
    let output = run_gog_command(&app_state.gog_home, &[
        "auth", "add", &body.email,
        "--remote", "--step", "2",
        "--auth-url", &full_callback_url,
        "--redirect-uri", &redirect_uri,
    ])
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
                    authenticated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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
        copy_keyring_to_agent(
            &state.docker_config.agents_dir,
            &id,
            &state.gog_home,
        )
        .await?;

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
                if assign_accounts.contains(&account.email)
                    && !account.assigned_to.contains(&id)
                {
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
    if !name.ends_with(".md") {
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
    if !name.ends_with(".md") {
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
                        conn.execute(
                            &sql,
                            rusqlite::params![n.as_i64().unwrap_or(0), job_id],
                        )?;
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
        conn.execute("DELETE FROM cron_runs WHERE job_id = ?1", rusqlite::params![job_id])?;
        conn.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id])?;
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
