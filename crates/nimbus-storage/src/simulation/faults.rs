use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result, TenantEventRecord, TenantId};

use super::seeding::splitmix64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FaultPoint {
    StorageCommitBeforeVisibility = 1,
    StorageCommitAfterVisibilityBeforeReturn = 2,
    JournalAppendBeforeDurableFlush = 3,
    JournalFlushBeforeVisibility = 4,
    CheckpointPublishBeforeManifestUpdate = 5,
    CompactionStartBeforePublish = 6,
    JournalDurableAppendBeforeApply = 7,
    TriggerInvocationMaterializeBeforeCommit = 8,
    ScheduledJobRecordResultBeforeWrite = 9,
    ScheduledJobCompleteBeforeWrite = 10,
    TriggerExecutionBeforeSave = 11,
    TenantCreateBeforeRegistration = 12,
}

impl FaultPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StorageCommitBeforeVisibility => "storage_commit_before_visibility",
            Self::StorageCommitAfterVisibilityBeforeReturn => {
                "storage_commit_after_visibility_before_return"
            }
            Self::JournalAppendBeforeDurableFlush => "journal_append_before_durable_flush",
            Self::JournalFlushBeforeVisibility => "journal_flush_before_visibility",
            Self::CheckpointPublishBeforeManifestUpdate => {
                "checkpoint_publish_before_manifest_update"
            }
            Self::CompactionStartBeforePublish => "compaction_start_before_publish",
            Self::JournalDurableAppendBeforeApply => "journal_durable_append_before_apply",
            Self::TriggerInvocationMaterializeBeforeCommit => {
                "trigger_invocation_materialize_before_commit"
            }
            Self::ScheduledJobRecordResultBeforeWrite => "scheduled_job_record_result_before_write",
            Self::ScheduledJobCompleteBeforeWrite => "scheduled_job_complete_before_write",
            Self::TriggerExecutionBeforeSave => "trigger_execution_before_save",
            Self::TenantCreateBeforeRegistration => "tenant_create_before_registration",
        }
    }
}

pub trait FaultInjector: Send + Sync {
    fn check(&self, point: FaultPoint) -> Result<()>;

    /// Checks a fault at a tenant-owned storage boundary. Implementations that
    /// do not need tenant targeting retain the ordinary process-wide behavior.
    fn check_for_tenant(&self, point: FaultPoint, tenant_id: &TenantId) -> Result<()> {
        let _ = tenant_id;
        self.check(point)
    }

    /// Checks a fault for one durable-record transaction. This lets a fault
    /// adapter discriminate the operation it owns without relying on timing or
    /// visit counts while unrelated tenant work is concurrent.
    fn check_for_durable_records(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let _ = records;
        self.check_for_tenant(point, tenant_id)
    }
}

struct TenantScopedFaultInjector {
    inner: Arc<dyn FaultInjector>,
    tenant_id: TenantId,
}

impl FaultInjector for TenantScopedFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        self.inner.check_for_tenant(point, &self.tenant_id)
    }

    fn check_for_tenant(&self, point: FaultPoint, _tenant_id: &TenantId) -> Result<()> {
        self.inner.check_for_tenant(point, &self.tenant_id)
    }

    fn check_for_durable_records(
        &self,
        point: FaultPoint,
        _tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.inner
            .check_for_durable_records(point, &self.tenant_id, records)
    }
}

/// Binds legacy point-only storage checks to the tenant that owns the store.
///
/// Production no-op and process-wide injectors retain their existing behavior
/// through `FaultInjector`'s defaults. Tenant-aware deterministic injectors can
/// now target redb, SQLite, Memory, and replica-cache work without racing an
/// unrelated tenant that happens to reach the same fault point first.
pub(crate) fn tenant_scoped_fault_injector(
    inner: Arc<dyn FaultInjector>,
    tenant_id: TenantId,
) -> Arc<dyn FaultInjector> {
    Arc::new(TenantScopedFaultInjector { inner, tenant_id })
}

#[derive(Default)]
pub struct NoopFaultInjector;

impl FaultInjector for NoopFaultInjector {
    fn check(&self, _point: FaultPoint) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaultOccurrence {
    pub point: FaultPoint,
    pub visit: u64,
}

#[derive(Default)]
struct FaultState {
    visits: HashMap<FaultPoint, u64>,
}

pub struct ScriptedFaultInjector {
    scheduled: HashSet<FaultOccurrence>,
    state: Mutex<FaultState>,
}

impl ScriptedFaultInjector {
    pub fn new(scheduled: impl IntoIterator<Item = FaultOccurrence>) -> Self {
        Self {
            scheduled: scheduled.into_iter().collect(),
            state: Mutex::new(FaultState::default()),
        }
    }
}

impl FaultInjector for ScriptedFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("scripted fault injector lock should not be poisoned");
        let visit = state.visits.entry(point).or_insert(0);
        *visit = visit.saturating_add(1);
        if self.scheduled.contains(&FaultOccurrence {
            point,
            visit: *visit,
        }) {
            return Err(injected_fault(point, *visit));
        }
        Ok(())
    }
}

pub struct SeededFaultInjector {
    seed: u64,
    one_in: NonZeroU64,
    state: Mutex<FaultState>,
}

impl SeededFaultInjector {
    pub fn new(seed: u64, one_in: NonZeroU64) -> Self {
        Self {
            seed,
            one_in,
            state: Mutex::new(FaultState::default()),
        }
    }
}

impl FaultInjector for SeededFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("seeded fault injector lock should not be poisoned");
        let visit = state.visits.entry(point).or_insert(0);
        *visit = visit.saturating_add(1);
        let draw = splitmix64(self.seed ^ ((*visit).rotate_left(17)) ^ ((point as u64) << 48));
        if draw.is_multiple_of(self.one_in.get()) {
            return Err(injected_fault(point, *visit));
        }
        Ok(())
    }
}

fn injected_fault(point: FaultPoint, visit: u64) -> Error {
    Error::Internal(format!(
        "injected fault at {} on visit {}",
        point.as_str(),
        visit
    ))
}
