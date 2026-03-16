use crate::app_state::AppState;
use crate::proto;
use crate::registry::InstanceHealth;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
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
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

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
