use crate::app_state::AppState;
use crate::proto;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tonic::metadata::MetadataValue;

#[derive(Deserialize)]
pub struct WsChatQuery {
    pub instance: String,
    #[allow(dead_code)]
    pub token: Option<String>,
}

pub async fn ws_chat(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<WsChatQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    // Verify instance exists
    if !state.registry.instances().await.contains_key(&q.instance) {
        return Err(StatusCode::NOT_FOUND);
    }

    let instance_id = q.instance.clone();
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, instance_id)))
}

async fn handle_ws(socket: WebSocket, state: AppState, instance_id: String) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    while let Some(Ok(msg)) = ws_rx.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                let _ = ws_tx
                    .send(Message::Text(
                        serde_json::json!({"type":"error","message":"invalid JSON"}).to_string().into(),
                    ))
                    .await;
                continue;
            }
        };

        let msg_type = parsed["type"].as_str().unwrap_or("");

        match msg_type {
            "message" => {
                let content = parsed["content"].as_str().unwrap_or("").to_string();
                if content.is_empty() {
                    continue;
                }

                let client = match state.registry.get_client(&instance_id).await {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ws_tx
                            .send(Message::Text(
                                serde_json::json!({"type":"error","message": e.to_string()})
                                    .to_string().into(),
                            ))
                            .await;
                        continue;
                    }
                };

                let mut client = client;
                let mut req = tonic::Request::new(proto::SendMessageRequest { message: content });
                let val: MetadataValue<_> =
                    format!("Bearer {}", state.grpc_secret).parse().unwrap();
                req.metadata_mut().insert("authorization", val);

                match client.send_message(req).await {
                    Ok(resp) => {
                        let mut stream = resp.into_inner();
                        while let Some(Ok(chat_out)) = stream.next().await {
                            let frame = chat_output_to_json(&chat_out);
                            if ws_tx
                                .send(Message::Text(frame.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = ws_tx
                            .send(Message::Text(
                                serde_json::json!({"type":"error","message": e.message().to_string()})
                                    .to_string().into(),
                            ))
                            .await;
                    }
                }
            }
            "cancel" => {
                let client = match state.registry.get_client(&instance_id).await {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let mut client = client;
                let mut req = tonic::Request::new(proto::CancelTurnRequest {});
                let val: MetadataValue<_> =
                    format!("Bearer {}", state.grpc_secret).parse().unwrap();
                req.metadata_mut().insert("authorization", val);
                let _ = client.cancel_turn(req).await;
            }
            _ => {
                let _ = ws_tx
                    .send(Message::Text(
                        serde_json::json!({"type":"error","message":"unknown message type"})
                            .to_string().into(),
                    ))
                    .await;
            }
        }
    }
}

fn chat_output_to_json(out: &proto::ChatOutput) -> serde_json::Value {
    use proto::chat_output::Output;
    let turn_id = &out.turn_id;

    match &out.output {
        Some(Output::Delta(d)) => serde_json::json!({
            "type": "delta",
            "turn_id": turn_id,
            "content": d,
        }),
        Some(Output::ToolStart(ts)) => serde_json::json!({
            "type": "tool_start",
            "turn_id": turn_id,
            "tool": ts.tool,
            "arguments": ts.arguments,
        }),
        Some(Output::ToolResult(tr)) => serde_json::json!({
            "type": "tool_result",
            "turn_id": turn_id,
            "tool": tr.tool,
            "success": tr.success,
            "output": tr.output,
        }),
        Some(Output::Done(d)) => serde_json::json!({
            "type": "done",
            "turn_id": turn_id,
            "content": d.content,
            "input_tokens": d.input_tokens,
            "output_tokens": d.output_tokens,
        }),
        Some(Output::Error(e)) => serde_json::json!({
            "type": "error",
            "turn_id": turn_id,
            "message": e.message,
        }),
        Some(Output::Queued(q)) => serde_json::json!({
            "type": "queued",
            "turn_id": turn_id,
            "position": q.position,
        }),
        Some(Output::TurnStart(ts)) => serde_json::json!({
            "type": "turn_start",
            "turn_id": turn_id,
            "turn_index": ts.turn_index,
        }),
        Some(Output::Status(s)) => serde_json::json!({
            "type": "status",
            "turn_id": turn_id,
            "busy": s.busy,
            "current_turn_index": s.current_turn_index,
            "history_length": s.history_length,
        }),
        None => serde_json::json!({
            "type": "unknown",
            "turn_id": turn_id,
        }),
    }
}
