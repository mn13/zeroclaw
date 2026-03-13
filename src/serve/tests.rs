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
