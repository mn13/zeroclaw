use crate::config::Config;
use crate::serve::history_store::{HistoryRow, HistoryStore};
use anyhow::{Context, Result};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc};

/// A command sent to the agent actor task.
pub enum AgentCommand {
    /// Process a user message and send deltas / final result back.
    SendMessage {
        message: String,
        reply: mpsc::Sender<AgentResponse>,
    },
    /// Cancel the current turn (if any).
    Cancel,
}

/// Responses produced by the agent actor.
#[derive(Debug, Clone)]
pub enum AgentResponse {
    /// The full assistant response text.
    Done {
        content: String,
        turn_index: u64,
    },
    /// An error during the turn.
    Error(String),
    /// Queued position (sent when agent is busy).
    Queued {
        position: u32,
    },
    TurnStarted {
        turn_index: u64,
    },
}

/// An event broadcast to all subscribers (for the SubscribeEvents RPC).
#[derive(Debug, Clone)]
pub struct AgentBroadcastEvent {
    pub event_type: String,
    pub data_json: String,
    pub timestamp: String,
}

/// Type alias for the async message handler function.
pub type MessageHandler = Box<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> + Send + Sync,
>;

/// Manages a single agent session: owns the history store, coordinates the
/// agent actor task, and tracks state.
pub struct SessionManager {
    pub config: Config,
    pub history: Arc<HistoryStore>,
    pub turn_counter: AtomicU64,
    pub busy: AtomicBool,
    pub start_time: Instant,
    pub cmd_tx: mpsc::Sender<AgentCommand>,
    pub event_tx: broadcast::Sender<AgentBroadcastEvent>,
    _actor_handle: tokio::task::JoinHandle<()>,
}

impl SessionManager {
    /// Create a new session manager. Spawns the agent actor task.
    pub fn new(config: Config, data_dir: PathBuf) -> Result<Arc<Self>> {
        let handler: MessageHandler = {
            let cfg = config.clone();
            Box::new(move |enriched_message: String| {
                let c = cfg.clone();
                Box::pin(async move { crate::agent::process_message(c, &enriched_message).await })
            })
        };
        Self::new_with_handler(config, data_dir, handler)
    }

    /// Create a session manager with a custom message handler (for testing).
    pub fn new_with_handler(
        config: Config,
        data_dir: PathBuf,
        handler: MessageHandler,
    ) -> Result<Arc<Self>> {
        let db_path = data_dir.join("history.db");
        let history =
            Arc::new(HistoryStore::open(&db_path).context("failed to open history store")?);

        let (cmd_tx, cmd_rx) = mpsc::channel::<AgentCommand>(64);
        let (event_tx, _event_rx) = broadcast::channel::<AgentBroadcastEvent>(256);

        let actor_history = history.clone();
        let actor_event_tx = event_tx.clone();

        let handle = tokio::spawn(async move {
            agent_actor_loop(handler, actor_history, cmd_rx, actor_event_tx).await;
        });

        // Recover turn counter from existing history.
        let existing_count = history.count().unwrap_or(0);
        let initial_turn = existing_count / 2; // rough: each turn = user + assistant

        Ok(Arc::new(Self {
            config,
            history,
            turn_counter: AtomicU64::new(initial_turn),
            busy: AtomicBool::new(false),
            start_time: Instant::now(),
            cmd_tx,
            event_tx,
            _actor_handle: handle,
        }))
    }

    /// Return the next turn index (atomic increment).
    pub fn next_turn(&self) -> u64 {
        self.turn_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// Total turns processed.
    pub fn total_turns(&self) -> u64 {
        self.turn_counter.load(Ordering::SeqCst)
    }

    /// Uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

/// The actor loop: sequentially processes agent commands.
async fn agent_actor_loop(
    handler: MessageHandler,
    history: Arc<HistoryStore>,
    mut rx: mpsc::Receiver<AgentCommand>,
    event_tx: broadcast::Sender<AgentBroadcastEvent>,
) {
    // Track turn index locally inside the actor.
    let mut local_turn: u64 = history.count().unwrap_or(0) / 2;

    while let Some(cmd) = rx.recv().await {
        match cmd {
            AgentCommand::SendMessage { message, reply } => {
                let turn_index = local_turn;
                local_turn += 1;

                // Notify: turn started
                let _ = reply
                    .send(AgentResponse::TurnStarted { turn_index })
                    .await;
                let _ = event_tx.send(AgentBroadcastEvent {
                    event_type: "turn_started".into(),
                    data_json: format!(r#"{{"turn_index":{turn_index}}}"#),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                });

                // Build context from recent history to provide continuity.
                let context_messages = history.load_recent(20).unwrap_or_default();
                let context_prefix = build_context_prefix(&context_messages);
                let enriched_message = if context_prefix.is_empty() {
                    message.clone()
                } else {
                    format!("{context_prefix}\n\nCurrent message: {message}")
                };

                // Call the message handler.
                let result = handler(enriched_message).await;

                match result {
                    Ok(response) => {
                        // Save to history store.
                        let _ = history.append(turn_index, "user", &message);
                        let _ = history.append(turn_index, "assistant", &response);

                        let _ = reply
                            .send(AgentResponse::Done {
                                content: response.clone(),
                                turn_index,
                            })
                            .await;
                        let _ = event_tx.send(AgentBroadcastEvent {
                            event_type: "turn_complete".into(),
                            data_json: format!(
                                r#"{{"turn_index":{turn_index},"content_length":{}}}"#,
                                response.len()
                            ),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("{e:#}");
                        let _ = history.append(turn_index, "user", &message);
                        let _ = history.append(turn_index, "assistant", &format!("[error] {e}"));

                        let _ = reply.send(AgentResponse::Error(error_msg.clone())).await;
                        let _ = event_tx.send(AgentBroadcastEvent {
                            event_type: "turn_error".into(),
                            data_json: format!(
                                r#"{{"turn_index":{turn_index},"error":{}}}"#,
                                serde_json::json!(error_msg)
                            ),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });
                    }
                }
            }
            AgentCommand::Cancel => {
                // Cancellation is a no-op for now since process_message doesn't
                // support cooperative cancellation. We'll add CancellationToken
                // support when we move to a persistent agent actor.
            }
        }
    }
}

/// Build a context prefix from recent history rows so the agent has continuity.
pub(crate) fn build_context_prefix(rows: &[HistoryRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let mut parts = Vec::with_capacity(rows.len() + 1);
    parts.push("[Previous conversation context]".to_string());
    for row in rows {
        // Skip error markers.
        if row.content.starts_with("[error]") {
            continue;
        }
        let role_label = match row.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            _ => &row.role,
        };
        parts.push(format!("{role_label}: {}", row.content));
    }
    parts.push("[End of previous context]".to_string());
    parts.join("\n")
}
