use super::history_store::{HistoryRow, HistoryStore};
use super::session::build_context_prefix;

#[test]
fn history_store_create_and_append() {
    let store = HistoryStore::open_in_memory().expect("open in-memory store");
    assert_eq!(store.count().unwrap(), 0);

    store.append(0, "user", "hello").unwrap();
    store.append(0, "assistant", "hi there").unwrap();
    assert_eq!(store.count().unwrap(), 2);
}

#[test]
fn history_store_load_paginated() {
    let store = HistoryStore::open_in_memory().unwrap();

    for i in 0..10 {
        store
            .append(i, "user", &format!("msg-{i}"))
            .unwrap();
    }

    // Load first 3
    let page1 = store.load(0, 3).unwrap();
    assert_eq!(page1.len(), 3);
    assert_eq!(page1[0].content, "msg-0");
    assert_eq!(page1[2].content, "msg-2");

    // Load next 3
    let page2 = store.load(3, 3).unwrap();
    assert_eq!(page2.len(), 3);
    assert_eq!(page2[0].content, "msg-3");

    // Load beyond end
    let page_end = store.load(9, 5).unwrap();
    assert_eq!(page_end.len(), 1);
    assert_eq!(page_end[0].content, "msg-9");
}

#[test]
fn history_store_load_recent() {
    let store = HistoryStore::open_in_memory().unwrap();

    for i in 0..5 {
        store
            .append(i, "user", &format!("msg-{i}"))
            .unwrap();
    }

    let recent = store.load_recent(3).unwrap();
    assert_eq!(recent.len(), 3);
    // Should be oldest-first within the recent window.
    assert_eq!(recent[0].content, "msg-2");
    assert_eq!(recent[1].content, "msg-3");
    assert_eq!(recent[2].content, "msg-4");
}

#[test]
fn history_store_clear() {
    let store = HistoryStore::open_in_memory().unwrap();

    store.append(0, "user", "hello").unwrap();
    store.append(0, "assistant", "hi").unwrap();
    assert_eq!(store.count().unwrap(), 2);

    let cleared = store.clear().unwrap();
    assert_eq!(cleared, 2);
    assert_eq!(store.count().unwrap(), 0);

    // Verify load returns empty.
    let rows = store.load(0, 100).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn history_store_empty_load() {
    let store = HistoryStore::open_in_memory().unwrap();
    let rows = store.load(0, 10).unwrap();
    assert!(rows.is_empty());
    let recent = store.load_recent(5).unwrap();
    assert!(recent.is_empty());
}

// ── Context injection tests ──────────────────────────────────────────

#[test]
fn context_prefix_empty_history() {
    let prefix = build_context_prefix(&[]);
    assert!(prefix.is_empty());
}

#[test]
fn context_prefix_includes_previous_messages() {
    let rows = vec![
        HistoryRow {
            turn_index: 0,
            role: "user".into(),
            content: "My name is Daniel".into(),
            created_at: "2026-03-13T00:00:00Z".into(),
        },
        HistoryRow {
            turn_index: 0,
            role: "assistant".into(),
            content: "Nice to meet you, Daniel!".into(),
            created_at: "2026-03-13T00:00:01Z".into(),
        },
    ];

    let prefix = build_context_prefix(&rows);
    assert!(prefix.contains("[Previous conversation context]"));
    assert!(prefix.contains("User: My name is Daniel"));
    assert!(prefix.contains("Assistant: Nice to meet you, Daniel!"));
    assert!(prefix.contains("[End of previous context]"));
}

/// This is the acceptance test mechanism: after storing "My name is Daniel"
/// in history, the context prefix for the next message must contain that info
/// so the LLM can recall it.
#[test]
fn context_injection_preserves_identity_across_turns() {
    let store = HistoryStore::open_in_memory().unwrap();

    // Simulate turn 0: user says their name, assistant acknowledges.
    store.append(0, "user", "My name is Daniel").unwrap();
    store
        .append(0, "assistant", "Nice to meet you, Daniel!")
        .unwrap();

    // Now simulate the second turn: load recent history and build context.
    let recent = store.load_recent(20).unwrap();
    let prefix = build_context_prefix(&recent);

    // The second message would be: "{prefix}\n\nCurrent message: What is my name?"
    let enriched = format!("{prefix}\n\nCurrent message: What is my name?");

    // Verify the enriched message contains the name from the first turn.
    assert!(
        enriched.contains("My name is Daniel"),
        "enriched message must contain the user's name from the first turn"
    );
    assert!(
        enriched.contains("What is my name?"),
        "enriched message must contain the current question"
    );
}

#[test]
fn context_prefix_skips_error_markers() {
    let rows = vec![
        HistoryRow {
            turn_index: 0,
            role: "user".into(),
            content: "hello".into(),
            created_at: "2026-03-13T00:00:00Z".into(),
        },
        HistoryRow {
            turn_index: 0,
            role: "assistant".into(),
            content: "[error] provider timeout".into(),
            created_at: "2026-03-13T00:00:01Z".into(),
        },
    ];

    let prefix = build_context_prefix(&rows);
    assert!(prefix.contains("User: hello"));
    assert!(
        !prefix.contains("[error] provider timeout"),
        "error markers should be skipped"
    );
}

#[test]
fn history_store_persistent_file() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test_history.db");

    // Write data and drop store.
    {
        let store = HistoryStore::open(&db_path).unwrap();
        store.append(0, "user", "My name is Daniel").unwrap();
        store
            .append(0, "assistant", "Hello Daniel!")
            .unwrap();
    }

    // Reopen and verify data persists.
    {
        let store = HistoryStore::open(&db_path).unwrap();
        assert_eq!(store.count().unwrap(), 2);
        let recent = store.load_recent(10).unwrap();
        assert_eq!(recent[0].content, "My name is Daniel");
        assert_eq!(recent[1].content, "Hello Daniel!");
    }
}

// ── gRPC integration tests ──────────────────────────────────────────

/// Full round-trip test: start a gRPC server with a mock handler, send two
/// messages via a gRPC client, and verify the second message's context
/// contains information from the first. This is the acceptance test.
#[tokio::test]
async fn grpc_roundtrip_memory_persistence() {
    use crate::config::Config;
    use crate::serve::grpc_server::ClawAgentService;
    use crate::serve::proto::claw_agent_client::ClawAgentClient;
    use crate::serve::proto::claw_agent_server::ClawAgentServer;
    use crate::serve::session::{MessageHandler, SessionManager};
    use std::sync::Mutex;

    // Capture the enriched messages sent to the handler.
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_clone = captured.clone();

    // Mock handler: echoes back a static response but captures the enriched message.
    let handler: MessageHandler = Box::new(move |enriched: String| {
        let cap = captured_clone.clone();
        Box::pin(async move {
            cap.lock().unwrap().push(enriched);
            Ok("Nice to meet you, Daniel!".to_string())
        })
    });

    let dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    let session = SessionManager::new_with_handler(config.clone(), dir.path().to_path_buf(), handler)
        .expect("create session");

    // Start gRPC server on a random port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = ClawAgentService::new(session);
    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(ClawAgentServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // Give server a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Connect gRPC client.
    let mut client = ClawAgentClient::connect(format!("http://{addr}"))
        .await
        .expect("connect to gRPC server");

    // ── Turn 1: user says their name ──
    let req1 = crate::serve::proto::SendMessageRequest {
        message: "My name is Daniel".into(),
    };
    let mut stream1 = client
        .send_message(tonic::Request::new(req1))
        .await
        .expect("send message 1")
        .into_inner();

    // Drain the stream to completion.
    let mut got_done_1 = false;
    while let Some(msg) = stream1.message().await.unwrap() {
        if let Some(crate::serve::proto::chat_output::Output::Done(done)) = msg.output {
            assert_eq!(done.content, "Nice to meet you, Daniel!");
            got_done_1 = true;
        }
    }
    assert!(got_done_1, "must receive Done for turn 1");

    // ── Turn 2: ask about the name ──
    let req2 = crate::serve::proto::SendMessageRequest {
        message: "What is my name?".into(),
    };
    let mut stream2 = client
        .send_message(tonic::Request::new(req2))
        .await
        .expect("send message 2")
        .into_inner();

    // Drain the stream.
    while let Some(_msg) = stream2.message().await.unwrap() {}

    // ── Verify: the second enriched message contains the first turn's content ──
    let messages = captured.lock().unwrap();
    assert_eq!(messages.len(), 2, "handler should have been called twice");

    // First message: no history context (fresh conversation).
    assert!(
        !messages[0].contains("[Previous conversation context]"),
        "first message should have no history prefix"
    );
    assert!(
        messages[0].contains("My name is Daniel"),
        "first enriched message should contain the original text"
    );

    // Second message: MUST contain history from first turn.
    assert!(
        messages[1].contains("[Previous conversation context]"),
        "second message should have history prefix"
    );
    assert!(
        messages[1].contains("My name is Daniel"),
        "second enriched message must contain 'My name is Daniel' from turn 1"
    );
    assert!(
        messages[1].contains("Nice to meet you, Daniel!"),
        "second enriched message must contain assistant response from turn 1"
    );
    assert!(
        messages[1].contains("What is my name?"),
        "second enriched message must contain the current question"
    );
}

#[tokio::test]
async fn grpc_health_check() {
    use crate::config::Config;
    use crate::serve::grpc_server::ClawAgentService;
    use crate::serve::proto::claw_agent_client::ClawAgentClient;
    use crate::serve::proto::claw_agent_server::ClawAgentServer;
    use crate::serve::session::{MessageHandler, SessionManager};

    let handler: MessageHandler = Box::new(|_| Box::pin(async { Ok("ok".into()) }));
    let dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    let session =
        SessionManager::new_with_handler(config, dir.path().to_path_buf(), handler).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = ClawAgentService::new(session);
    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(ClawAgentServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ClawAgentClient::connect(format!("http://{addr}"))
        .await
        .unwrap();

    let resp = client
        .health_check(tonic::Request::new(crate::serve::proto::HealthCheckRequest {}))
        .await
        .unwrap()
        .into_inner();

    assert!(resp.healthy);
    assert!(!resp.version.is_empty());
}

#[tokio::test]
async fn grpc_history_persists_across_turns() {
    use crate::config::Config;
    use crate::serve::grpc_server::ClawAgentService;
    use crate::serve::proto::claw_agent_client::ClawAgentClient;
    use crate::serve::proto::claw_agent_server::ClawAgentServer;
    use crate::serve::session::{MessageHandler, SessionManager};

    let handler: MessageHandler =
        Box::new(|_| Box::pin(async { Ok("acknowledged".into()) }));
    let dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    let session =
        SessionManager::new_with_handler(config, dir.path().to_path_buf(), handler).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = ClawAgentService::new(session);
    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        tonic::transport::Server::builder()
            .add_service(ClawAgentServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = ClawAgentClient::connect(format!("http://{addr}"))
        .await
        .unwrap();

    // Send 3 messages.
    for msg_text in &["hello", "world", "test"] {
        let req = crate::serve::proto::SendMessageRequest {
            message: msg_text.to_string(),
        };
        let mut stream = client
            .send_message(tonic::Request::new(req))
            .await
            .unwrap()
            .into_inner();
        while let Some(_) = stream.message().await.unwrap() {}
    }

    // Query history.
    let hist = client
        .get_history(tonic::Request::new(crate::serve::proto::HistoryRequest {
            offset: 0,
            limit: 100,
        }))
        .await
        .unwrap()
        .into_inner();

    // 3 turns × 2 entries (user + assistant) = 6 total.
    assert_eq!(hist.total, 6);
    assert_eq!(hist.messages.len(), 6);

    // Verify ordering and content.
    assert_eq!(hist.messages[0].role, "user");
    assert_eq!(hist.messages[0].content, "hello");
    assert_eq!(hist.messages[1].role, "assistant");
    assert_eq!(hist.messages[1].content, "acknowledged");
    assert_eq!(hist.messages[2].role, "user");
    assert_eq!(hist.messages[2].content, "world");
}

use std::sync::Arc;
