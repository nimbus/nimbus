use super::*;
use crate::{DurableJournal, TenantPointRead, TenantPointWrite, TenantRangeScan};

fn exercise_crud_and_range<S>(store: &S)
where
    S: TenantPointRead + TenantPointWrite + TenantRangeScan,
{
    let document = sample_document("memory_conformance_tasks", "v1");
    let insert = store
        .insert_document(&document)
        .expect("insert should commit");
    assert_eq!(insert.sequence, SequenceNumber(1));
    assert_eq!(
        store
            .get(&document.table, &document.id)
            .expect("point read should succeed"),
        Some(document.clone())
    );

    let patch = serde_json::Map::from_iter([("title".to_string(), json!("v2"))]);
    let update = store
        .update_document_validated(
            &document.table,
            &document.id,
            &patch,
            |previous, current| {
                assert_eq!(previous.fields.get("title"), Some(&json!("v1")));
                assert_eq!(current.fields.get("title"), Some(&json!("v2")));
                Ok(())
            },
        )
        .expect("validated update should commit");
    assert_eq!(update.sequence, SequenceNumber(2));

    let mut check_cancel = || Ok(());
    let scanned = store
        .scan_table_id_prefix_cancellable(&document.table, document.id.as_str(), &mut check_cancel)
        .expect("range scan should succeed");
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].fields.get("title"), Some(&json!("v2")));

    let (delete, deleted) = store
        .delete_document_validated(&document.table, &document.id, |current| {
            assert_eq!(current.fields.get("title"), Some(&json!("v2")));
            Ok(())
        })
        .expect("validated delete should commit");
    assert_eq!(delete.sequence, SequenceNumber(3));
    assert_eq!(deleted.fields.get("title"), Some(&json!("v2")));
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("point read after delete should succeed")
            .is_none()
    );
}

fn exercise_durable_journal_lifecycle<S>(store: S, restart: impl FnOnce(S) -> S)
where
    S: DurableJournal + TenantPointRead,
{
    let table = TableName::new("memory_journal_tasks").expect("table should be valid");
    let table_id =
        TableId::try_from("memory-journal-table".to_string()).expect("table id should be valid");
    let inserted = sample_document("memory_journal_tasks", "v1");
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.update_time = Timestamp(inserted.update_time.0.saturating_add(1));

    let records = [
        TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(inserted.clone()),
            }],
            None,
        )
        .expect("insert record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: table.clone(),
                table_id,
                op_type: WriteOpType::Update,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(inserted.clone()),
                current: Some(updated.clone()),
            }],
            None,
        )
        .expect("update record should build"),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert_eq!(
        store.journal_progress().expect("progress should load"),
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let gap = store
        .apply_durable_records_batch(&records[1..])
        .expect_err("sequence gap must be a hard error");
    assert!(matches!(gap, Error::Internal(_)));
    assert_eq!(
        store
            .applied_sequence()
            .expect("applied head should load after gap"),
        SequenceNumber(0),
        "a failed gap apply must be atomic"
    );

    store
        .apply_durable_records_batch(&records[..1])
        .expect("first record should apply");
    store
        .apply_durable_records_batch(&records[..1])
        .expect("already-applied replay should be skipped");
    assert_eq!(
        store
            .get(&table, &inserted.id)
            .expect("materialized insert should load"),
        Some(inserted.clone())
    );

    let store = restart(store);
    assert_eq!(
        store
            .journal_progress()
            .expect("restart progress should load"),
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(1),
        }
    );
    assert_eq!(
        store
            .recover_durable_journal()
            .expect("restart recovery should apply pending tail"),
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(2),
        }
    );
    assert_eq!(
        store
            .get(&table, &inserted.id)
            .expect("recovered document should load"),
        Some(updated)
    );
}

#[test]
fn redb_tenant_store_crud_and_range_conformance() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_crud_and_range(&store);
}

#[test]
fn memory_tenant_store_crud_and_range_conformance() {
    exercise_crud_and_range(&MemoryTenantStore::new());
}

#[test]
fn redb_tenant_store_durable_journal_conformance() {
    let directory = tempdir().expect("temporary directory should create");
    let path = directory.path().join("journal-conformance.redb");
    let store = TenantStore::open(&path).expect("redb store should open");
    exercise_durable_journal_lifecycle(store, |store| {
        drop(store);
        TenantStore::open(&path).expect("redb store should reopen")
    });
}

#[test]
fn redb_applied_sequence_recovery_replay_is_idempotent_for_all_write_shapes() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_applied_sequence_recovery_replay(&store, "redb_duplicate_replay");
}

#[test]
fn redb_applied_sequence_rejects_divergent_content_for_all_write_shapes() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_applied_sequence_corruption_rejection(&store, "redb_duplicate_corruption");
}

#[test]
fn memory_tenant_store_durable_journal_conformance() {
    let store = MemoryTenantStore::new();
    exercise_durable_journal_lifecycle(store, |store| {
        store
            .restart_from_durable_state()
            .expect("volatile restart image should clone")
    });
}

#[test]
fn memory_tenant_store_rebuilds_from_nonzero_snapshot_boundary() {
    let source = MemoryTenantStore::new();
    let inserted = sample_document("memory_snapshot_tasks", "v1");
    let insert = source
        .insert(&inserted)
        .expect("source insert should commit");
    let snapshot = source
        .export_materialized_journal_snapshot()
        .expect("materialized snapshot should export");
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("v2"));
    updated.update_time = Timestamp(inserted.update_time.0.saturating_add(1));
    let record = TenantEventRecord::new(
        SequenceNumber(insert.sequence.0.saturating_add(1)),
        Timestamp(200),
        vec![WriteOp {
            table: inserted.table.clone(),
            table_id: insert.writes[0].table_id.clone(),
            op_type: WriteOpType::Update,
            doc_id: inserted.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(inserted.clone()),
            current: Some(updated.clone()),
        }],
        None,
    )
    .expect("snapshot tail record should build");

    let restored = MemoryTenantStore::new();
    let progress = restored
        .rebuild_materialized_journal_from_snapshot(
            &snapshot,
            std::slice::from_ref(&record),
            Some(record.sequence),
        )
        .expect("snapshot plus tail should rebuild");

    assert_eq!(progress.durable_head, record.sequence);
    assert_eq!(progress.applied_head, record.sequence);
    assert_eq!(
        restored
            .get(&inserted.table, &inserted.id)
            .expect("restored document should read"),
        Some(updated)
    );
}
