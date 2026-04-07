use neovex_core::SequenceNumber;
use serde_json::json;

use crate::TenantStore;

fn sample_document(table: &str, title: &str) -> neovex_core::Document {
    neovex_core::Document::new(
        neovex_core::TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

#[test]
fn durable_journal_stream_uses_cursor_floor_after_retention_cut() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let write_txn = store.db.begin_write().expect("write txn should open");
    {
        let mut journal = write_txn
            .open_table(super::super::COMMIT_LOG)
            .expect("commit log table should open");
        journal
            .remove(1)
            .expect("first durable journal entry should be removable");
    }
    store
        .commit_write_txn(write_txn)
        .expect("retention-cut transaction should commit");

    let error = store
        .stream_durable_journal(SequenceNumber(0), 10)
        .expect_err("cursor behind the retained floor should fail");
    assert!(matches!(
        error,
        neovex_core::Error::InvalidInput(message)
            if message.contains("behind the retention floor 1")
    ));

    let page = store
        .stream_durable_journal(SequenceNumber(1), 10)
        .expect("cursor at the retained floor should succeed");
    assert_eq!(page.cursor_floor, SequenceNumber(1));
    assert_eq!(page.latest_sequence, SequenceNumber(2));
    assert_eq!(page.next_cursor, SequenceNumber(2));
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(2));
}
