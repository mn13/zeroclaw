use super::history_store::HistoryStore;

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
