use crate::serve::proto::claw_agent_server::ClawAgent;
use crate::serve::proto::{self as pb};
use crate::serve::session::{AgentCommand, AgentResponse, SessionManager};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// The tonic gRPC service implementation backed by a `SessionManager`.
pub struct ClawAgentService {
    session: Arc<SessionManager>,
}

impl ClawAgentService {
    pub fn new(session: Arc<SessionManager>) -> Self {
        Self { session }
    }
}

#[tonic::async_trait]
impl ClawAgent for ClawAgentService {
    type SendMessageStream = ReceiverStream<Result<pb::ChatOutput, Status>>;
    type SubscribeEventsStream = ReceiverStream<Result<pb::AgentEvent, Status>>;

    async fn send_message(
        &self,
        request: Request<pb::SendMessageRequest>,
    ) -> Result<Response<Self::SendMessageStream>, Status> {
        let msg = request.into_inner().message;
        if msg.trim().is_empty() {
            return Err(Status::invalid_argument("message must not be empty"));
        }

        let (stream_tx, stream_rx) = mpsc::channel(32);
        let session = self.session.clone();

        tokio::spawn(async move {
            let turn_index = session.next_turn();
            let turn_id = format!("turn-{turn_index}");

            // Check if agent is busy.
            if session
                .busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                let _ = stream_tx
                    .send(Ok(pb::ChatOutput {
                        turn_id: turn_id.clone(),
                        output: Some(pb::chat_output::Output::Queued(pb::QueuePosition {
                            position: 1,
                        })),
                    }))
                    .await;
            }

            // Send command to actor.
            let (reply_tx, mut reply_rx) = mpsc::channel::<AgentResponse>(16);
            let send_result = session
                .cmd_tx
                .send(AgentCommand::SendMessage {
                    message: msg,
                    reply: reply_tx,
                })
                .await;

            if send_result.is_err() {
                let _ = stream_tx
                    .send(Ok(pb::ChatOutput {
                        turn_id: turn_id.clone(),
                        output: Some(pb::chat_output::Output::Error(pb::TurnError {
                            message: "agent actor unavailable".into(),
                        })),
                    }))
                    .await;
                session.busy.store(false, Ordering::SeqCst);
                return;
            }

            // Relay responses from the actor to the gRPC stream.
            while let Some(resp) = reply_rx.recv().await {
                let output = match resp {
                    AgentResponse::TurnStarted { turn_index: ti } => {
                        pb::chat_output::Output::TurnStart(pb::TurnStarted { turn_index: ti })
                    }
                    AgentResponse::Done {
                        content,
                        turn_index: ti,
                        input_tokens,
                        output_tokens,
                    } => pb::chat_output::Output::Done(pb::TurnComplete {
                        content,
                        turn_index: ti,
                        input_tokens,
                        output_tokens,
                    }),
                    AgentResponse::Error(e) => {
                        pb::chat_output::Output::Error(pb::TurnError { message: e })
                    }
                    AgentResponse::Queued { position } => {
                        pb::chat_output::Output::Queued(pb::QueuePosition { position })
                    }
                };

                let _ = stream_tx
                    .send(Ok(pb::ChatOutput {
                        turn_id: turn_id.clone(),
                        output: Some(output),
                    }))
                    .await;
            }

            session.busy.store(false, Ordering::SeqCst);
        });

        Ok(Response::new(ReceiverStream::new(stream_rx)))
    }

    async fn cancel_turn(
        &self,
        _request: Request<pb::CancelTurnRequest>,
    ) -> Result<Response<pb::CancelTurnResponse>, Status> {
        let _ = self.session.cmd_tx.send(AgentCommand::Cancel).await;
        Ok(Response::new(pb::CancelTurnResponse {
            was_running: self.session.busy.load(Ordering::SeqCst),
            turn_id: String::new(),
        }))
    }

    async fn get_history(
        &self,
        request: Request<pb::HistoryRequest>,
    ) -> Result<Response<pb::HistoryResponse>, Status> {
        let req = request.into_inner();
        let offset = req.offset;
        let limit = if req.limit == 0 { 50 } else { req.limit };

        let total = self
            .session
            .history
            .count()
            .map_err(|e| Status::internal(format!("history count failed: {e}")))?;

        let rows = self
            .session
            .history
            .load(offset, limit)
            .map_err(|e| Status::internal(format!("history load failed: {e}")))?;

        let messages = rows
            .into_iter()
            .map(|r| pb::HistoryEntry {
                turn_index: r.turn_index,
                role: r.role,
                content: r.content,
                created_at: r.created_at,
            })
            .collect();

        Ok(Response::new(pb::HistoryResponse {
            total,
            offset,
            limit,
            messages,
        }))
    }

    async fn clear_history(
        &self,
        request: Request<pb::ClearHistoryRequest>,
    ) -> Result<Response<pb::ClearHistoryResponse>, Status> {
        let req = request.into_inner();
        if !req.confirm {
            return Err(Status::invalid_argument(
                "confirm must be true to clear history",
            ));
        }

        let cleared = self
            .session
            .history
            .clear()
            .map_err(|e| Status::internal(format!("clear history failed: {e}")))?;

        Ok(Response::new(pb::ClearHistoryResponse {
            messages_cleared: cleared,
        }))
    }

    async fn get_status(
        &self,
        _request: Request<pb::GetStatusRequest>,
    ) -> Result<Response<pb::StatusResponse>, Status> {
        let state = if self.session.busy.load(Ordering::SeqCst) {
            "busy"
        } else {
            "idle"
        };

        let history_length = self.session.history.count().unwrap_or(0);

        let cfg = self.session.config.read().await;
        let model = cfg
            .default_model
            .clone()
            .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
        let provider = cfg
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".into());
        drop(cfg);

        Ok(Response::new(pb::StatusResponse {
            state: state.into(),
            uptime_secs: self.session.uptime_secs(),
            total_turns: self.session.total_turns(),
            history_length,
            model,
            provider,
        }))
    }

    async fn health_check(
        &self,
        _request: Request<pb::HealthCheckRequest>,
    ) -> Result<Response<pb::HealthResponse>, Status> {
        Ok(Response::new(pb::HealthResponse {
            healthy: true,
            version: env!("CARGO_PKG_VERSION").into(),
        }))
    }

    async fn get_config(
        &self,
        _request: Request<pb::GetConfigRequest>,
    ) -> Result<Response<pb::ConfigResponse>, Status> {
        // Return a sanitized version of the config (no secrets).
        let mut safe_config = self.session.config.read().await.clone();
        safe_config.api_key = safe_config.api_key.as_ref().map(|_| "[REDACTED]".into());
        let json = serde_json::to_string_pretty(&safe_config)
            .map_err(|e| Status::internal(format!("config serialization failed: {e}")))?;
        Ok(Response::new(pb::ConfigResponse { config_json: json }))
    }

    async fn update_config(
        &self,
        request: Request<pb::UpdateConfigRequest>,
    ) -> Result<Response<pb::UpdateConfigResponse>, Status> {
        let partial_json = request.into_inner().partial_json;

        // Parse the partial JSON patch.
        let mut patch: serde_json::Value = serde_json::from_str(&partial_json)
            .map_err(|e| Status::invalid_argument(format!("invalid JSON: {e}")))?;

        let patch_obj = patch
            .as_object()
            .ok_or_else(|| Status::invalid_argument("expected a JSON object"))?;

        let updated_fields: Vec<String> = patch_obj.keys().cloned().collect();

        // Strip redacted sentinel values so the UI can't overwrite real secrets
        // by sending back the "[REDACTED]" placeholder from get_config.
        strip_redacted(&mut patch);

        // Merge into current config: serialize current → merge patch → deserialize back.
        // Preserve #[serde(skip)] fields (workspace_dir, config_path) since they are
        // not included in serialization and would be lost during the round-trip.
        let mut config = self.session.config.write().await;
        let workspace_dir = config.workspace_dir.clone();
        let config_path = config.config_path.clone();

        let mut current_json: serde_json::Value = serde_json::to_value(&*config)
            .map_err(|e| Status::internal(format!("config serialization failed: {e}")))?;

        json_merge_patch(&mut current_json, &patch);

        let mut new_config: crate::config::Config = serde_json::from_value(current_json)
            .map_err(|e| Status::invalid_argument(format!("merged config is invalid: {e}")))?;
        new_config.workspace_dir = workspace_dir;
        new_config.config_path = config_path.clone();

        // Persist to disk so changes survive restarts.
        // Clear env-sourced secrets before writing so they don't leak into the file.
        if !config_path.as_os_str().is_empty() {
            let mut disk_config = new_config.clone();
            disk_config.api_key = None;
            let toml_str = toml::to_string_pretty(&disk_config)
                .map_err(|e| Status::internal(format!("config TOML serialization failed: {e}")))?;
            std::fs::write(&config_path, toml_str)
                .map_err(|e| Status::internal(format!("failed to write config to disk: {e}")))?;
        }

        *config = new_config;

        Ok(Response::new(pb::UpdateConfigResponse {
            updated_fields,
            requires_restart: false,
        }))
    }

    async fn list_memory(
        &self,
        _request: Request<pb::ListMemoryRequest>,
    ) -> Result<Response<pb::MemoryEntryList>, Status> {
        // Stub: memory listing via gRPC is not yet wired.
        Ok(Response::new(pb::MemoryEntryList {
            entries: vec![],
            total: 0,
        }))
    }

    async fn search_memory(
        &self,
        _request: Request<pb::SearchMemoryRequest>,
    ) -> Result<Response<pb::MemoryEntryList>, Status> {
        Ok(Response::new(pb::MemoryEntryList {
            entries: vec![],
            total: 0,
        }))
    }

    async fn store_memory(
        &self,
        _request: Request<pb::StoreMemoryRequest>,
    ) -> Result<Response<pb::StoreMemoryResponse>, Status> {
        Err(Status::unimplemented(
            "memory store via gRPC is not yet supported",
        ))
    }

    async fn forget_memory(
        &self,
        _request: Request<pb::ForgetMemoryRequest>,
    ) -> Result<Response<pb::ForgetMemoryResponse>, Status> {
        Err(Status::unimplemented(
            "memory forget via gRPC is not yet supported",
        ))
    }

    async fn list_tools(
        &self,
        _request: Request<pb::ListToolsRequest>,
    ) -> Result<Response<pb::ToolList>, Status> {
        // Stub: we'd need to build the tool registry to list specs.
        Ok(Response::new(pb::ToolList { tools: vec![] }))
    }

    async fn subscribe_events(
        &self,
        _request: Request<pb::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let (tx, rx) = mpsc::channel(64);
        let mut event_rx = self.session.event_tx.subscribe();

        tokio::spawn(async move {
            while let Ok(evt) = event_rx.recv().await {
                let proto_event = pb::AgentEvent {
                    event_type: evt.event_type,
                    data_json: evt.data_json,
                    timestamp: evt.timestamp,
                };
                if tx.send(Ok(proto_event)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// RFC 7396 JSON Merge Patch: recursively merge `patch` into `target`.
fn json_merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    if let serde_json::Value::Object(patch_obj) = patch {
        if !target.is_object() {
            *target = serde_json::Value::Object(serde_json::Map::new());
        }
        let target_obj = target.as_object_mut().unwrap();
        for (key, value) in patch_obj {
            if value.is_null() {
                target_obj.remove(key);
            } else if value.is_object() {
                let entry = target_obj
                    .entry(key.clone())
                    .or_insert(serde_json::Value::Object(serde_json::Map::new()));
                json_merge_patch(entry, value);
            } else {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Recursively remove any string values equal to "[REDACTED]" from a JSON value,
/// so that redacted placeholders from get_config don't overwrite real secrets.
fn strip_redacted(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        map.retain(|_, v| {
            if let serde_json::Value::String(s) = v {
                s != "[REDACTED]"
            } else {
                true
            }
        });
        for v in map.values_mut() {
            strip_redacted(v);
        }
    }
}
