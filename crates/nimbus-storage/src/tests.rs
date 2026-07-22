pub(crate) use std::collections::BTreeMap;
pub(crate) use std::num::NonZeroU64;
pub(crate) use std::sync::Arc;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Condvar, Mutex};

pub(crate) use nimbus_core::{
    DependencySet, Document, DocumentId, Error, FieldSchema, FieldType, IndexDefinition,
    IndexLifecycleEvent, IndexRangeDependency, ManualWallClock, Schema, SchemaChangeEvent,
    SequenceNumber, TableId, TableName, TableSchema, TenantEventKind, TenantEventRecord, Timestamp,
    TriggerDeliveryCursor, WriteOp, WriteOpType, durable_record_intersects_dependency_set,
};
pub(crate) use serde_json::json;
pub(crate) use tempfile::tempdir;
pub(crate) use time::{Date, Month, PrimitiveDateTime, Time};
pub(crate) use tokio::sync::Notify;
pub(crate) use tokio::time::{Duration, timeout};

pub(crate) use crate::keys::{document_key, prefix_end, table_prefix};
pub(crate) use crate::{
    CommitterLeaseError, CommitterLeaseStore, DeterministicHarness, DurableJournal, FaultInjector,
    FaultOccurrence, FaultPoint, GeneratedTaskHistory, GeneratedTaskHistorySeedCase,
    GeneratedTaskRecord, HardDeleteDecision, LibsqlReplicaProvider, LibsqlReplicaProviderConfig,
    MemoryTenantStore, MySqlProvider, MySqlProviderConfig, PostgresProvider,
    PostgresProviderConfig, RedbTenantStorage, RestartBoundary, RetentionFloor,
    RetentionParticipant, ScriptedRestartSchedule, SeededFaultInjector, ShadowMaterializer,
    ShadowMaterializerConfig, ShadowMaterializerManifest, SqliteTenantStorage, SqliteTenantStore,
    TenantPointRead, TenantReadStorage, TenantStore, TenantWriteOutcome, TenantWriteStorage,
    UsageStore, VerificationHarnessMode, replay_generated_task_history,
    selected_generated_task_history_seed_corpus,
};

mod async_faults;
mod crud_and_journal;
mod generated_history;
mod libsql_provider;
mod memory_conformance;
mod mysql_provider;
mod object_meta;
mod postgres_provider;
mod recovery;
mod sqlite_foundation;
mod store_basics;
mod usage_store;

const BLOCKING_TEST_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn exercise_committer_lease_transitions<S>(store: &S)
where
    S: CommitterLeaseStore,
{
    assert_eq!(
        store
            .read_committer_lease()
            .expect("absent lease should read"),
        None
    );

    let first = store
        .acquire_committer_lease("owner-a", Duration::ZERO)
        .expect("absent lease should be acquired");
    assert_eq!(first.owner_id, "owner-a");
    assert_eq!(first.epoch, 1);
    assert_eq!(first.durable_sequence, SequenceNumber(0));

    let takeover = store
        .acquire_committer_lease("owner-b", Duration::from_secs(60))
        .expect("provider-expired lease should be acquired");
    assert_eq!(takeover.owner_id, "owner-b");
    assert_eq!(takeover.epoch, 2);
    assert_eq!(takeover.durable_sequence, SequenceNumber(0));

    let held = store.acquire_committer_lease("owner-c", Duration::from_secs(60));
    assert!(matches!(held, Err(CommitterLeaseError::Held)));

    let reacquired = store
        .acquire_committer_lease("owner-b", Duration::from_secs(60))
        .expect("current owner should reacquire its lease");
    assert_eq!(reacquired.epoch, takeover.epoch);
    assert!(reacquired.expires_at >= takeover.expires_at);

    let renewed = store
        .renew_committer_lease("owner-b", reacquired.epoch, Duration::from_secs(60))
        .expect("current owner and epoch should renew");
    assert_eq!(renewed.owner_id, "owner-b");
    assert_eq!(renewed.epoch, 2);
    assert!(renewed.expires_at >= reacquired.expires_at);

    let fenced = store.renew_committer_lease("owner-a", 1, Duration::from_secs(60));
    assert!(matches!(
        fenced,
        Err(CommitterLeaseError::Fenced {
            ref owner_id,
            epoch: 1,
        }) if owner_id == "owner-a"
    ));

    assert_eq!(
        store
            .read_committer_lease()
            .expect("acquired lease should read"),
        Some(renewed)
    );
}

pub(crate) fn exercise_concurrent_committer_lease_acquire<S>(store: S)
where
    S: CommitterLeaseStore + Clone + Send + Sync + 'static,
{
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for owner_id in ["concurrent-a", "concurrent-b"] {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.acquire_committer_lease(owner_id, Duration::from_secs(60))
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("acquirer thread should not panic"))
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(CommitterLeaseError::Held)))
            .count(),
        1
    );
    let winner = results
        .into_iter()
        .find_map(Result::ok)
        .expect("one concurrent acquirer should win");
    assert_eq!(winner.epoch, 1);
    assert_eq!(
        store
            .read_committer_lease()
            .expect("winning lease should read"),
        Some(winner)
    );
}

pub(crate) fn exercise_committer_lease_takeover_after_expiry_under_concurrency<S>(store: S)
where
    S: CommitterLeaseStore + Clone + Send + Sync + 'static,
{
    let original = store
        .acquire_committer_lease("expired-owner", Duration::ZERO)
        .expect("original lease should be acquired");

    let barrier = Arc::new(std::sync::Barrier::new(3));
    let renew = {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.renew_committer_lease("expired-owner", original.epoch, Duration::from_secs(60))
        })
    };
    let takeover = {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.acquire_committer_lease("successor", Duration::from_secs(60))
        })
    };
    barrier.wait();

    let renewed = renew.join().expect("renewal contender should join");
    let successor = takeover
        .join()
        .expect("takeover contender should join")
        .expect("successor should acquire the expired lease");
    assert!(matches!(
        renewed,
        Err(CommitterLeaseError::Fenced {
            ref owner_id,
            epoch,
        }) if owner_id == "expired-owner" && epoch == original.epoch
    ));
    assert_eq!(successor.owner_id, "successor");
    assert_eq!(successor.epoch, original.epoch.saturating_add(1));
    assert_eq!(
        store
            .read_committer_lease()
            .expect("winning lease should read"),
        Some(successor)
    );
}

fn fenced_insert_record(sequence: u64, document: &Document) -> TenantEventRecord {
    TenantEventRecord::new(
        SequenceNumber(sequence),
        Timestamp(sequence.saturating_mul(100)),
        vec![WriteOp {
            table: document.table.clone(),
            table_id: TableId::new(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("fenced insert record should build")
}

fn assert_fenced(result: crate::CommitterLeaseResult<()>, owner_id: &str, epoch: u64) {
    assert!(matches!(
        result,
        Err(CommitterLeaseError::Fenced {
            owner_id: ref fenced_owner,
            epoch: fenced_epoch,
        }) if fenced_owner == owner_id && fenced_epoch == epoch
    ));
}

pub(crate) fn exercise_fenced_durable_apply_happy_path<S>(store: &S, table_name: &str)
where
    S: CommitterLeaseStore + DurableJournal + TenantPointRead,
{
    let lease = store
        .acquire_committer_lease("holder", Duration::from_secs(60))
        .expect("lease should be acquired");
    let document = sample_document(table_name, "committed");
    let record = fenced_insert_record(1, &document);
    store
        .fenced_append_and_apply_durable_records_batch(
            "holder",
            lease.epoch,
            SequenceNumber(0),
            &[record],
        )
        .expect("current lease holder should publish");

    assert_eq!(
        store
            .get(&document.table, &document.id)
            .expect("document should read"),
        Some(document)
    );
    assert_eq!(store.latest_sequence().unwrap(), SequenceNumber(1));
    assert_eq!(store.applied_sequence().unwrap(), SequenceNumber(1));
    assert_eq!(
        store
            .read_committer_lease()
            .unwrap()
            .unwrap()
            .durable_sequence,
        SequenceNumber(1)
    );
}

pub(crate) fn exercise_fenced_durable_apply_total_rollback<S>(store: &S, table_name: &str)
where
    S: CommitterLeaseStore + DurableJournal + TenantPointRead,
{
    let lease = store
        .acquire_committer_lease("holder", Duration::from_secs(60))
        .expect("lease should be acquired");
    let document = sample_document(table_name, "must-not-land");
    let stale_epoch = lease.epoch.saturating_sub(1);
    assert_fenced(
        store.fenced_append_and_apply_durable_records_batch(
            "holder",
            stale_epoch,
            SequenceNumber(0),
            &[fenced_insert_record(1, &document)],
        ),
        "holder",
        stale_epoch,
    );

    assert!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .is_empty(),
        "fenced transaction must not leave an appended record"
    );
    assert_eq!(store.get(&document.table, &document.id).unwrap(), None);
    assert_eq!(store.latest_sequence().unwrap(), SequenceNumber(0));
    assert_eq!(store.applied_sequence().unwrap(), SequenceNumber(0));
    let persisted = store.read_committer_lease().unwrap().unwrap();
    assert_eq!(persisted.epoch, lease.epoch);
    assert_eq!(persisted.durable_sequence, SequenceNumber(0));
}

pub(crate) fn exercise_fenced_durable_apply_expired<S>(store: &S, table_name: &str)
where
    S: CommitterLeaseStore + DurableJournal + TenantPointRead,
{
    let lease = store
        .acquire_committer_lease("expired-holder", Duration::ZERO)
        .expect("zero-duration lease should be acquired");
    let document = sample_document(table_name, "expired");
    assert_fenced(
        store.fenced_append_and_apply_durable_records_batch(
            "expired-holder",
            lease.epoch,
            SequenceNumber(0),
            &[fenced_insert_record(1, &document)],
        ),
        "expired-holder",
        lease.epoch,
    );
    assert!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.get(&document.table, &document.id).unwrap(), None);
}

pub(crate) fn exercise_fenced_durable_apply_sequence_gap<S>(store: &S, table_name: &str)
where
    S: CommitterLeaseStore + DurableJournal + TenantPointRead,
{
    let lease = store
        .acquire_committer_lease("holder", Duration::from_secs(60))
        .expect("lease should be acquired");
    let document = sample_document(table_name, "gap");
    assert_fenced(
        store.fenced_append_and_apply_durable_records_batch(
            "holder",
            lease.epoch,
            SequenceNumber(1),
            &[fenced_insert_record(1, &document)],
        ),
        "holder",
        lease.epoch,
    );
    assert!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.get(&document.table, &document.id).unwrap(), None);
}

pub(crate) fn exercise_fenced_durable_apply_prefix_guard<S>(store: &S, table_name: &str)
where
    S: CommitterLeaseStore + DurableJournal + TenantPointRead,
{
    let lease = store
        .acquire_committer_lease("holder", Duration::from_secs(60))
        .expect("lease should be acquired");
    let predecessor = sample_document(table_name, "unapplied-predecessor");
    store
        .append_durable_records_batch(&[fenced_insert_record(1, &predecessor)])
        .expect("predecessor should append without applying");
    let refused = sample_document(table_name, "must-not-pass-prefix");
    let result = store.fenced_append_and_apply_durable_records_batch(
        "holder",
        lease.epoch,
        SequenceNumber(0),
        &[fenced_insert_record(2, &refused)],
    );
    assert!(matches!(
        result,
        Err(CommitterLeaseError::Storage(Error::Internal(ref message)))
            if message.contains("required contiguous predecessor 1")
    ));

    let records = store.read_durable_journal_from(SequenceNumber(1)).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].sequence, SequenceNumber(1));
    assert_eq!(store.applied_sequence().unwrap(), SequenceNumber(0));
    assert_eq!(
        store.get(&predecessor.table, &predecessor.id).unwrap(),
        None
    );
    assert_eq!(store.get(&refused.table, &refused.id).unwrap(), None);
    assert_eq!(
        store
            .read_committer_lease()
            .unwrap()
            .unwrap()
            .durable_sequence,
        SequenceNumber(0),
        "failed prefix check must roll the earlier lease update back"
    );
}

pub(crate) use crate::provider_test_fixtures::{
    ExternalProviderFixtureMode, external_provider_fixture_mode,
};

pub(crate) fn sample_document(table: &str, title: &str) -> Document {
    Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

fn duplicate_write_replay_records(
    table_name: &str,
) -> (Document, Document, [TenantEventRecord; 3]) {
    let table = TableName::new(table_name).expect("table name should be valid");
    let table_id = TableId::new();
    let inserted = sample_document(table_name, "inserted");
    let mut updated = inserted.clone();
    updated.fields.insert("title".to_string(), json!("updated"));
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
        .expect("insert replay record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(200),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Update,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(inserted.clone()),
                current: Some(updated.clone()),
            }],
            None,
        )
        .expect("update replay record should build"),
        TenantEventRecord::new(
            SequenceNumber(3),
            Timestamp(300),
            vec![WriteOp {
                table,
                table_id,
                op_type: WriteOpType::Delete,
                doc_id: inserted.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(updated.clone()),
                current: None,
            }],
            None,
        )
        .expect("delete replay record should build"),
    ];
    (inserted, updated, records)
}

pub(crate) fn exercise_applied_sequence_recovery_replay<S>(store: &S, table_name: &str)
where
    S: crate::DurableJournal + crate::TenantPointRead,
{
    let (inserted, updated, records) = duplicate_write_replay_records(table_name);
    let expected_states = [Some(inserted.clone()), Some(updated), None];

    for (index, (record, expected_state)) in records.iter().zip(expected_states).enumerate() {
        store
            .append_durable_records_batch(std::slice::from_ref(record))
            .expect("durable record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(record))
            .expect("durable record should apply");
        store
            .apply_durable_records_batch(std::slice::from_ref(record))
            .expect("identical already-applied record should be an idempotent replay");

        assert_eq!(
            store
                .get(&inserted.table, &inserted.id)
                .expect("materialized state should load after replay"),
            expected_state,
            "identical {:?} replay must not duplicate its materialized effect",
            record.writes[0].op_type
        );
        assert_eq!(
            store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("durable journal should remain readable")
                .len(),
            index + 1,
            "replay must not append a duplicate durable record"
        );
    }
}

pub(crate) fn exercise_applied_sequence_corruption_rejection<S>(store: &S, table_name: &str)
where
    S: crate::DurableJournal + crate::TenantPointRead,
{
    let (inserted, updated, records) = duplicate_write_replay_records(table_name);
    let expected_states = [Some(inserted.clone()), Some(updated), None];

    for (record, expected_state) in records.iter().zip(expected_states) {
        store
            .append_durable_records_batch(std::slice::from_ref(record))
            .expect("durable record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(record))
            .expect("durable record should apply");

        let mut divergent_write = record.writes[0].clone();
        let divergent_document = divergent_write
            .current
            .as_mut()
            .or(divergent_write.previous.as_mut())
            .expect("document write should carry a current or previous image");
        divergent_document
            .fields
            .insert("corruption".to_string(), json!(record.sequence.0));
        let divergent = TenantEventRecord::new(
            record.sequence,
            record.timestamp,
            vec![divergent_write],
            record.scheduled_execution_id.clone(),
        )
        .expect("divergent replay record should build with valid integrity");

        let error = store
            .apply_durable_records_batch(&[divergent])
            .expect_err("divergent already-applied record must be rejected as corruption");
        match &error {
            Error::Storage {
                kind: nimbus_core::StorageErrorKind::Corruption,
                message,
            } => {
                assert!(
                    message.contains(&record.sequence.0.to_string()),
                    "corruption error must name sequence {}: {message}",
                    record.sequence.0
                );
                assert!(
                    message.contains(inserted.id.as_str()),
                    "corruption error must name document {}: {message}",
                    inserted.id
                );
            }
            other => panic!("expected typed storage corruption, got {other:?}"),
        }
        assert_eq!(
            error.retryability(),
            nimbus_core::Retryability::Terminal,
            "corruption must never be advertised as retryable"
        );
        assert_eq!(
            store
                .get(&inserted.table, &inserted.id)
                .expect("materialized state should load after corruption rejection"),
            expected_state,
            "rejected {:?} replay must leave stored state unchanged",
            record.writes[0].op_type
        );
    }
}

pub(crate) fn exercise_pending_prefix_blocks_generic_zero_write<S>(
    store: &S,
    table_name: &str,
    generic_zero_write: impl FnOnce() -> nimbus_core::Result<()>,
) where
    S: crate::DurableJournal + crate::TenantPointRead,
{
    let (pending_document, _, records) = duplicate_write_replay_records(table_name);
    let pending = &records[0];
    store
        .append_durable_records_batch(std::slice::from_ref(pending))
        .expect("pending durable record should append without applying");
    assert_eq!(
        store
            .journal_progress()
            .expect("pending journal progress should load"),
        crate::JournalProgress {
            durable_head: pending.sequence,
            applied_head: SequenceNumber(0),
        }
    );

    let error = generic_zero_write()
        .expect_err("generic zero-write transaction must not advance across a pending record");
    assert!(
        error
            .to_string()
            .contains("required contiguous predecessor"),
        "prefix rejection should explain the unapplied predecessor: {error:?}"
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should survive rejected generic transaction"),
        crate::JournalProgress {
            durable_head: pending.sequence,
            applied_head: SequenceNumber(0),
        },
        "rejected generic transaction must leave both journal heads unchanged"
    );
    assert_eq!(
        store
            .get(&pending_document.table, &pending_document.id)
            .expect("pending document lookup should succeed"),
        None,
        "pending document must remain physically unapplied"
    );

    assert_eq!(
        store
            .recover_durable_journal()
            .expect("pending durable record should still recover after rejection"),
        crate::JournalProgress {
            durable_head: pending.sequence,
            applied_head: pending.sequence,
        }
    );
    assert_eq!(
        store
            .get(&pending_document.table, &pending_document.id)
            .expect("applied document lookup should succeed"),
        Some(pending_document),
        "pending durable document must remain recoverable"
    );
}

pub(crate) fn exercise_durable_update_guard_is_corruption<S>(
    store: &S,
    table_name: &str,
    materialize_unexpected_state: bool,
) where
    S: crate::DurableJournal + crate::TenantPointRead,
{
    let table = TableName::new(table_name).expect("table name should be valid");
    let table_id = TableId::new();
    let unexpected = sample_document(table_name, "unexpected");
    let mut expected_previous = unexpected.clone();
    expected_previous
        .fields
        .insert("title".to_string(), json!("expected previous"));
    let mut current = expected_previous.clone();
    current.fields.insert("title".to_string(), json!("current"));
    current.update_time = Timestamp(expected_previous.update_time.0.saturating_add(1));

    let sequence = if materialize_unexpected_state {
        let insert = TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: unexpected.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(unexpected.clone()),
            }],
            None,
        )
        .expect("setup insert record should build");
        store
            .append_durable_records_batch(std::slice::from_ref(&insert))
            .expect("setup insert should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&insert))
            .expect("setup insert should apply");
        SequenceNumber(2)
    } else {
        SequenceNumber(1)
    };

    let update = TenantEventRecord::new(
        sequence,
        Timestamp(200),
        vec![WriteOp {
            table,
            table_id,
            op_type: WriteOpType::Update,
            doc_id: unexpected.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(expected_previous),
            current: Some(current),
        }],
        None,
    )
    .expect("guard update record should build");
    store
        .append_durable_records_batch(std::slice::from_ref(&update))
        .expect("guard update should append");
    let error = store
        .apply_durable_records_batch(std::slice::from_ref(&update))
        .expect_err("inconsistent materialized pre-image must be rejected");
    match &error {
        Error::Storage {
            kind: nimbus_core::StorageErrorKind::Corruption,
            message,
        } => {
            assert!(
                message.contains(&sequence.0.to_string()),
                "corruption must name durable sequence {}: {message}",
                sequence.0
            );
            assert!(
                message.contains(unexpected.id.as_str()),
                "corruption must name document {}: {message}",
                unexpected.id
            );
            let expected_reason = if materialize_unexpected_state {
                "pre-image mismatch"
            } else {
                "missing the expected pre-image"
            };
            assert!(
                message.contains(expected_reason),
                "corruption must identify {expected_reason}: {message}"
            );
        }
        other => panic!("expected typed storage corruption, got {other:?}"),
    }
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert_eq!(
        error.conflicting_sequence(),
        None,
        "storage corruption must not carry conflict-specific sequence metadata"
    );
}

pub(crate) struct BlockingReadGate {
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

pub(crate) struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl BlockingFaultInjector {
    pub(crate) fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    pub(crate) fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        let (released, _) = cvar
            .wait_timeout_while(released, BLOCKING_TEST_RELEASE_TIMEOUT, |released| {
                !*released
            })
            .expect("blocking fault injector should wait for release");
        assert!(
            *released,
            "blocking storage fault injector was not released within \
             {BLOCKING_TEST_RELEASE_TIMEOUT:?}; the test likely exited before calling release()"
        );
        Ok(())
    }
}

impl BlockingReadGate {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    pub(crate) fn block(&self) {
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let released = lock
            .lock()
            .expect("blocking read gate should acquire release lock");
        let (released, _) = cvar
            .wait_timeout_while(released, BLOCKING_TEST_RELEASE_TIMEOUT, |released| {
                !*released
            })
            .expect("blocking read gate should wait for release");
        assert!(
            *released,
            "blocking storage read gate was not released within \
             {BLOCKING_TEST_RELEASE_TIMEOUT:?}; the test likely exited before calling release()"
        );
    }

    pub(crate) fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking read gate should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}
