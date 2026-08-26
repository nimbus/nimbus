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
    TriggerTransitionAfterHeadObservation = 13,
    RetentionCheckpointBeforeCommit = 14,
    RetentionCheckpointAfterCommit = 15,
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
            Self::TriggerTransitionAfterHeadObservation => {
                "trigger_transition_after_head_observation"
            }
            Self::RetentionCheckpointBeforeCommit => "retention_checkpoint_before_commit",
            Self::RetentionCheckpointAfterCommit => "retention_checkpoint_after_commit",
        }
    }
}

pub trait FaultInjector: Send + Sync {
    fn check(&self, point: FaultPoint) -> Result<()>;

    /// Checks a fault at a tenant-owned storage boundary, naming the durable
    /// journal records that boundary is about to make visible.
    ///
    /// `records` is empty whenever the boundary materializes no journal record:
    /// an ordinary document commit, a schedule-only execution unit, a trigger
    /// outcome. That emptiness is load-bearing, and it is the only signal that
    /// separates those transactions from a durable batch — every one of them
    /// reaches the same commit-sequence fault points, on the same tenant, at the
    /// same time. A fault adapter that arms a one-shot fault at a specific
    /// durable batch discriminates on `records` so an unrelated concurrent
    /// transaction cannot consume the arm; see
    /// `nimbus-testing`'s `PpscStorageFaultInjector`.
    ///
    /// Implementations that do not need tenant or record targeting retain the
    /// ordinary process-wide behavior.
    fn check_for_tenant(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let _ = (tenant_id, records);
        self.check(point)
    }

    /// The same check for a store that has already bound its tenant through
    /// [`tenant_scoped_fault_injector`] and therefore holds no [`TenantId`] at
    /// the call site — redb, SQLite and Memory reach their commit boundaries
    /// this way.
    fn check_durable_records(
        &self,
        point: FaultPoint,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let _ = records;
        self.check(point)
    }
}

/// Why a durable batch is being applied, and therefore what the apply boundary
/// is entitled to name as the records it made durable.
///
/// Every backend's `recover_durable_journal` has the same shape — read the
/// pending records back out of the durable journal, then apply them — and so
/// reaches the same commit-sequence fault points as a client batch. Without
/// this distinction a replay looks identical to the batch a scenario armed a
/// fault for, and consumes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableApplyKind {
    /// A caller is waiting on this batch. The apply is what makes its records
    /// visible, so they are what this boundary made durable.
    ClientBatch,
    /// Recovery re-applying records read back out of the durable journal.
    /// Whatever appended them already made them durable, and no caller is
    /// waiting to acknowledge them, so this boundary makes nothing durable.
    JournalReplay,
}

impl DurableApplyKind {
    /// The records this boundary makes durable, which is all the fault
    /// interface may see. A replay makes none, so a fault armed for a client
    /// batch cannot be consumed by a replay of an older one.
    pub fn newly_durable_records(self, records: &[TenantEventRecord]) -> &[TenantEventRecord] {
        match self {
            Self::ClientBatch => records,
            Self::JournalReplay => &[],
        }
    }
}

struct TenantScopedFaultInjector {
    inner: Arc<dyn FaultInjector>,
    tenant_id: TenantId,
}

impl FaultInjector for TenantScopedFaultInjector {
    fn check(&self, point: FaultPoint) -> Result<()> {
        self.inner.check_for_tenant(point, &self.tenant_id, &[])
    }

    fn check_for_tenant(
        &self,
        point: FaultPoint,
        _tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.inner.check_for_tenant(point, &self.tenant_id, records)
    }

    fn check_durable_records(
        &self,
        point: FaultPoint,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.inner.check_for_tenant(point, &self.tenant_id, records)
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
