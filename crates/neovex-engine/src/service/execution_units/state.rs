use std::collections::HashMap;
use std::sync::MutexGuard;

use neovex_core::{DependencySet, Document, DocumentId, Error, IndexDefinition, Result, TableName};
use neovex_storage::{ResolvedScheduleOp, ResolvedWrite};

use super::MutationExecutionUnit;

#[derive(Debug, Clone)]
pub(super) struct StagedWriteEntry {
    pub(super) original: Option<Document>,
    pub(super) current: Option<Document>,
    pub(super) indexes: Vec<IndexDefinition>,
}

#[derive(Debug, Clone)]
pub(super) enum StagedSchedulerEntry {
    Insert(neovex_core::ScheduledJob),
    CancelExisting,
    NoOp,
}

#[derive(Debug, Default)]
pub(super) struct MutationExecutionUnitState {
    pub(super) lifecycle: ExecutionUnitLifecycle,
    pub(super) read_dependencies: DependencySet,
    pub(super) write_dependencies: DependencySet,
    pub(super) staged_writes: HashMap<(TableName, DocumentId), StagedWriteEntry>,
    pub(super) write_order: Vec<(TableName, DocumentId)>,
    pub(super) staged_scheduler_jobs: HashMap<neovex_core::JobId, StagedSchedulerEntry>,
    pub(super) scheduler_order: Vec<neovex_core::JobId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ExecutionUnitLifecycle {
    #[default]
    Active,
    Finalizing,
    Finalized,
}

impl MutationExecutionUnit {
    pub(super) fn active_state(&self) -> Result<MutexGuard<'_, MutationExecutionUnitState>> {
        let state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        Self::ensure_active_lifecycle(state.lifecycle)?;
        Ok(state)
    }

    pub(super) fn ensure_active(&self) -> Result<()> {
        let state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        Self::ensure_active_lifecycle(state.lifecycle)
    }

    fn ensure_active_lifecycle(lifecycle: ExecutionUnitLifecycle) -> Result<()> {
        match lifecycle {
            ExecutionUnitLifecycle::Active => Ok(()),
            ExecutionUnitLifecycle::Finalizing => Err(Error::InvalidInput(
                "mutation execution unit is finalizing; start a new execution unit".to_string(),
            )),
            ExecutionUnitLifecycle::Finalized => Err(Error::InvalidInput(
                "mutation execution unit is finalized; start a new execution unit".to_string(),
            )),
        }
    }

    pub(super) fn finish_finalization(&self) {
        let mut state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        // Execution units are single-use: successful, conflicting, and no-op
        // commit attempts all retire the staged state permanently.
        state.lifecycle = ExecutionUnitLifecycle::Finalized;
        state.staged_writes.clear();
        state.write_order.clear();
        state.staged_scheduler_jobs.clear();
        state.scheduler_order.clear();
        state.read_dependencies = DependencySet::default();
        state.write_dependencies = DependencySet::default();
    }

    pub(super) fn build_resolved_writes(
        &self,
        state: &MutationExecutionUnitState,
    ) -> Vec<ResolvedWrite> {
        state
            .write_order
            .iter()
            .filter_map(|key| state.staged_writes.get(key))
            .filter_map(|entry| match (&entry.original, &entry.current) {
                (None, Some(current)) => Some(ResolvedWrite::Insert {
                    document: current.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (Some(previous), Some(current)) => Some(ResolvedWrite::Update {
                    previous: previous.clone(),
                    current: current.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (Some(previous), None) => Some(ResolvedWrite::Delete {
                    previous: previous.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (None, None) => None,
            })
            .collect()
    }

    pub(super) fn build_resolved_schedule_ops(
        &self,
        state: &MutationExecutionUnitState,
    ) -> Vec<ResolvedScheduleOp> {
        state
            .scheduler_order
            .iter()
            .filter_map(|job_id| match state.staged_scheduler_jobs.get(job_id) {
                Some(StagedSchedulerEntry::Insert(job)) => {
                    Some(ResolvedScheduleOp::Insert { job: job.clone() })
                }
                Some(StagedSchedulerEntry::CancelExisting) => {
                    Some(ResolvedScheduleOp::Cancel { job_id: *job_id })
                }
                Some(StagedSchedulerEntry::NoOp) | None => None,
            })
            .collect()
    }
}
