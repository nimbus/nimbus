pub(crate) use std::collections::BTreeMap;
pub(crate) use std::num::NonZeroU64;
pub(crate) use std::sync::Arc;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Condvar, Mutex};

pub(crate) use nimbus_core::{
    DependencySet, Document, DocumentId, Error, FieldSchema, FieldType, IndexDefinition,
    IndexLifecycleEvent, IndexRangeDependency, Schema, SchemaChangeEvent, SequenceNumber, TableId,
    TableName, TableSchema, TenantEventKind, TenantEventRecord, Timestamp, TriggerDeliveryCursor,
    WriteOp, WriteOpType, durable_record_intersects_dependency_set,
};
pub(crate) use serde_json::json;
pub(crate) use tempfile::tempdir;
pub(crate) use time::{Date, Month, PrimitiveDateTime, Time};
pub(crate) use tokio::sync::Notify;
pub(crate) use tokio::time::{Duration, timeout};

pub(crate) use crate::keys::{document_key, prefix_end, table_prefix};
pub(crate) use crate::{
    DeterministicHarness, FaultInjector, FaultOccurrence, FaultPoint, GeneratedTaskHistory,
    GeneratedTaskHistorySeedCase, GeneratedTaskRecord, HardDeleteDecision, LibsqlReplicaProvider,
    LibsqlReplicaProviderConfig, ManualClock, MemoryTenantStore, MySqlProvider,
    MySqlProviderConfig, PostgresProvider, PostgresProviderConfig, RedbTenantStorage,
    RestartBoundary, RetentionFloor, RetentionParticipant, ScriptedRestartSchedule,
    SeededFaultInjector, ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest,
    SqliteTenantStorage, SqliteTenantStore, TenantReadStorage, TenantStore, TenantWriteOutcome,
    TenantWriteStorage, UsageStore, VerificationHarnessMode, replay_generated_task_history,
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
mod provider_fixtures;
mod recovery;
mod sqlite_foundation;
mod store_basics;
mod usage_store;

const BLOCKING_TEST_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) use provider_fixtures::{
    implicit_external_provider_fixtures_disabled, require_explicit_external_provider_fixture_envs,
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

    store
        .apply_durable_records_batch(std::slice::from_ref(pending))
        .expect("pending durable record should still apply after rejection");
    assert_eq!(
        store
            .journal_progress()
            .expect("applied journal progress should load"),
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
