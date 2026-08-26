use super::contract_scenarios::{
    exercise_durable_recovery_replays_unapplied_records, exercise_journal_progress_round_trip,
    exercise_materialized_position_is_provider_independent,
    exercise_materialized_verification_root_is_provider_independent,
    reference_materialized_position, reference_materialized_verification_root,
};
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
fn redb_ppsc_identical_replay_is_idempotent_for_all_write_shapes() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_ppsc_identical_applied_sequence_replay(&store, "redb_duplicate_replay");
}

#[test]
fn redb_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_ppsc_different_content_applied_sequence_reuse_rejection(
        &store,
        "redb_duplicate_corruption",
    );
}

#[test]
fn redb_pending_prefix_blocks_generic_zero_write() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_pending_prefix_blocks_generic_zero_write(&store, "redb_pending_prefix", || {
        store.set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(1)))
    });
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
fn memory_durable_journal_page_serializes_concurrent_prune_until_rows_return() {
    let (fault, rows_read, resume) = pause_after_retention_read_page();
    let store = Arc::new(MemoryTenantStore::with_simulation(
        Arc::new(nimbus_core::SystemWallClock),
        fault,
    ));
    for title in ["first", "second", "third"] {
        store
            .insert_document(&sample_document("memory_retention_page", title))
            .expect("insert should commit");
    }

    let reader_store = Arc::clone(&store);
    let reader =
        std::thread::spawn(move || reader_store.stream_durable_journal(SequenceNumber(0), 1));
    rows_read
        .recv_timeout(Duration::from_secs(5))
        .expect("memory reader should reach the post-page boundary");

    let (compacted_tx, compacted_rx) = mpsc::sync_channel(1);
    let compactor_store = Arc::clone(&store);
    let compactor = std::thread::spawn(move || {
        let result = compactor_store.compact_retained_history(
            crate::RetentionGcConfig::new(1).expect("retention config should be valid"),
        );
        compacted_tx
            .send(result)
            .expect("compaction result receiver should remain open");
    });
    assert!(
        compacted_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "memory compaction must wait for the page's state read guard"
    );
    resume
        .send(())
        .expect("memory reader should still wait at the page boundary");

    let page = reader
        .join()
        .expect("memory reader should not panic")
        .expect("the page linearized before retention should succeed");
    assert_eq!(page.records.len(), 1);
    compacted_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("memory compaction should finish after the page releases its read guard")
        .expect("memory compaction should succeed");
    compactor.join().expect("memory compactor should not panic");
}

#[test]
fn redb_disabled_cron_job_still_reports_scheduled_work() {
    let store = TenantStore::create_in_memory().expect("redb store should open");
    exercise_disabled_cron_job_still_reports_scheduled_work(&store);
}

#[test]
fn memory_disabled_cron_job_still_reports_scheduled_work() {
    exercise_disabled_cron_job_still_reports_scheduled_work(&MemoryTenantStore::new());
}

#[test]
fn memory_pending_prefix_blocks_generic_zero_write() {
    let store = MemoryTenantStore::new();
    exercise_pending_prefix_blocks_generic_zero_write(&store, "memory_pending_prefix", || {
        store.set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(1)))
    });
}

#[test]
fn redb_durable_update_guard_reports_corruption() {
    let missing = TenantStore::create_in_memory().expect("redb store should open");
    exercise_durable_update_guard_is_corruption(&missing, "redb_missing_preimage", false);
    let mismatched = TenantStore::create_in_memory().expect("redb store should open");
    exercise_durable_update_guard_is_corruption(&mismatched, "redb_mismatched_preimage", true);
}

#[test]
fn memory_durable_update_guard_reports_corruption() {
    exercise_durable_update_guard_is_corruption(
        &MemoryTenantStore::new(),
        "memory_missing_preimage",
        false,
    );
    exercise_durable_update_guard_is_corruption(
        &MemoryTenantStore::new(),
        "memory_mismatched_preimage",
        true,
    );
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

#[test]
fn redb_journal_progress_round_trips_through_insert_update_delete() {
    let directory = tempdir().expect("temporary directory should create");
    let store =
        TenantStore::open(directory.path().join("progress.redb")).expect("redb store should open");
    exercise_journal_progress_round_trip(&store, "redb_progress_tasks");
}

#[test]
fn memory_journal_progress_round_trips_through_insert_update_delete() {
    let store = MemoryTenantStore::new();
    exercise_journal_progress_round_trip(&store, "memory_progress_tasks");
}

#[test]
fn redb_durable_recovery_replays_durable_but_unapplied_records() {
    let directory = tempdir().expect("temporary directory should create");
    let store =
        TenantStore::open(directory.path().join("recovery.redb")).expect("redb store should open");
    exercise_durable_recovery_replays_unapplied_records(&store, "redb_recovery_tasks");
}

#[test]
fn memory_durable_recovery_replays_durable_but_unapplied_records() {
    let store = MemoryTenantStore::new();
    exercise_durable_recovery_replays_unapplied_records(&store, "memory_recovery_tasks");
}

#[test]
fn redb_materialized_position_matches_the_provider_independent_reference() {
    let directory = tempdir().expect("temporary directory should create");
    let store =
        TenantStore::open(directory.path().join("position.redb")).expect("redb store should open");
    assert_eq!(
        exercise_materialized_position_is_provider_independent(&store),
        reference_materialized_position()
    );
}

#[test]
fn memory_materialized_position_matches_the_provider_independent_reference() {
    let store = MemoryTenantStore::new();
    assert_eq!(
        exercise_materialized_position_is_provider_independent(&store),
        reference_materialized_position()
    );
}

#[test]
fn redb_materialized_verification_root_is_provider_independent() {
    let directory = tempdir().expect("temporary directory should create");
    let store =
        TenantStore::open(directory.path().join("root.redb")).expect("redb store should open");
    assert_eq!(
        exercise_materialized_verification_root_is_provider_independent(&store),
        reference_materialized_verification_root()
    );
}

#[test]
fn memory_materialized_verification_root_is_provider_independent() {
    let store = MemoryTenantStore::new();
    assert_eq!(
        exercise_materialized_verification_root_is_provider_independent(&store),
        reference_materialized_verification_root()
    );
}

#[test]
fn memory_ppsc_identical_replay_is_idempotent_for_all_write_shapes() {
    let store = MemoryTenantStore::new();
    exercise_ppsc_identical_applied_sequence_replay(&store, "memory_duplicate_replay");
}

#[test]
fn memory_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes() {
    let store = MemoryTenantStore::new();
    exercise_ppsc_different_content_applied_sequence_reuse_rejection(
        &store,
        "memory_duplicate_corruption",
    );
}
