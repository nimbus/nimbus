pub(crate) use std::collections::BTreeMap;
pub(crate) use std::num::NonZeroU64;
pub(crate) use std::sync::Arc;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Condvar, Mutex};

pub(crate) use nimbus_core::{
    DependencySet, Document, DocumentId, DurableMutationRecord, Error, FieldSchema, FieldType,
    IndexDefinition, IndexRangeDependency, SequenceNumber, TableName, TableSchema, Timestamp,
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
    GeneratedTaskHistorySeedCase, GeneratedTaskRecord, LibsqlReplicaProvider,
    LibsqlReplicaProviderConfig, ManualClock, MySqlProvider, MySqlProviderConfig, PostgresProvider,
    PostgresProviderConfig, RedbTenantStorage, RestartBoundary, ScriptedRestartSchedule,
    SeededFaultInjector, ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest,
    SqliteTenantStorage, SqliteTenantStore, TenantReadStorage, TenantStore, TenantWriteOutcome,
    TenantWriteStorage, UsageStore, VerificationHarnessMode, replay_generated_task_history,
    selected_generated_task_history_seed_corpus,
};

mod async_faults;
mod crud_and_journal;
mod generated_history;
mod libsql_provider;
mod mysql_provider;
mod postgres_provider;
mod provider_fixtures;
mod recovery;
mod sqlite_foundation;
mod store_basics;
mod usage_store;

pub(crate) use provider_fixtures::{
    implicit_external_provider_fixtures_disabled, require_explicit_external_provider_fixture_envs,
};

pub(crate) fn sample_document(table: &str, title: &str) -> Document {
    Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
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
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
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
        let mut released = lock
            .lock()
            .expect("blocking read gate should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking read gate should wait for release");
        }
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
