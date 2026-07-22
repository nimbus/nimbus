use nimbus_core::{CronJob, DocumentId, Result, ScheduledJob, ScheduledJobResult, Timestamp};

use super::{SchedulerWrite, SchedulerWriteResult};
use crate::{
    LibsqlReplicaTenantStore, MemoryTenantStore, MySqlTenantStore, PostgresTenantStore,
    SqliteTenantStore, TenantStore,
};

/// Durable scheduler state needed to decide whether a failed write committed.
///
/// The snapshot is deliberately scheduler-specific. Scheduler writes do not
/// advance the mutation journal, so the journal head cannot classify a lost
/// provider acknowledgement. Capturing the affected scheduler state before
/// the transaction gives every backend the same exact post-error proof rule.
#[derive(Clone, Debug, PartialEq)]
struct SchedulerWriteSnapshot {
    pending: Vec<ScheduledJob>,
    running: Vec<ScheduledJob>,
    result: Option<ScheduledJobResult>,
    cron: Option<CronJob>,
}

/// A scheduler write plus its exact pre-state and intended transactional
/// post-state. Callers keep this value until the provider acknowledges commit.
/// Normal operations read only indexed rows named by the operation; claims
/// peek at most `max_jobs`, and only one-shot orphan recovery scales with the
/// number of running jobs. The tenant committer or held provider lease excludes
/// a second legal writer while these operation-scoped observations are taken.
#[derive(Clone, Debug)]
pub struct PreparedSchedulerWrite {
    operation: SchedulerWrite,
    before: SchedulerWriteSnapshot,
    intended: SchedulerWriteSnapshot,
    result: SchedulerWriteResult,
}

impl PreparedSchedulerWrite {
    pub fn operation(&self) -> SchedulerWrite {
        self.operation.clone()
    }

    pub fn result(&self) -> SchedulerWriteResult {
        self.result.clone()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SchedulerWriteReconciliation {
    Committed(SchedulerWriteResult),
    RolledBack,
    Ambiguous,
}

/// Read surface used by the scheduler outcome policy. Every production store
/// implements this seam, so provider acknowledgement handling cannot drift by
/// adapter.
pub trait SchedulerWriteOutcomeStore {
    fn prepare_scheduler_write(&self, operation: SchedulerWrite) -> Result<PreparedSchedulerWrite>;

    fn reconcile_scheduler_write(
        &self,
        prepared: &PreparedSchedulerWrite,
    ) -> Result<SchedulerWriteReconciliation>;
}

trait SchedulerStateReader {
    fn pending_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>>;
    fn running_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>>;
    fn due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>>;
    fn all_running_jobs(&self) -> Result<Vec<ScheduledJob>>;
    fn scheduled_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>>;
    fn cron_job(&self, name: &str) -> Result<Option<CronJob>>;
}

fn prepare(
    store: &impl SchedulerStateReader,
    operation: SchedulerWrite,
) -> Result<PreparedSchedulerWrite> {
    let before = prepare_snapshot(store, &operation)?;
    let (intended, result) = apply_intended(before.clone(), &operation);
    Ok(PreparedSchedulerWrite {
        operation,
        before,
        intended,
        result,
    })
}

fn reconcile(
    store: &impl SchedulerStateReader,
    prepared: &PreparedSchedulerWrite,
) -> Result<SchedulerWriteReconciliation> {
    let observed = observe_snapshot(store, prepared)?;
    if observed == prepared.intended {
        return Ok(SchedulerWriteReconciliation::Committed(prepared.result()));
    }
    if observed == prepared.before {
        return Ok(SchedulerWriteReconciliation::RolledBack);
    }
    Ok(SchedulerWriteReconciliation::Ambiguous)
}

fn prepare_snapshot(
    store: &impl SchedulerStateReader,
    operation: &SchedulerWrite,
) -> Result<SchedulerWriteSnapshot> {
    let (mut pending, mut running) = match operation {
        SchedulerWrite::Insert(job) => {
            jobs_by_id(store, std::slice::from_ref(&job.id), true, true)?
        }
        SchedulerWrite::ClaimDue { now, max_jobs } => {
            let pending = store.due_jobs(*now, *max_jobs)?;
            let ids = pending.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
            let (_, running) = jobs_by_id(store, &ids, false, true)?;
            (pending, running)
        }
        SchedulerWrite::Complete(job_id) => {
            jobs_by_id(store, std::slice::from_ref(job_id), true, true)?
        }
        SchedulerWrite::Cancel(job_id) => {
            jobs_by_id(store, std::slice::from_ref(job_id), true, true)?
        }
        SchedulerWrite::RecordResult(_)
        | SchedulerWrite::SaveCron(_)
        | SchedulerWrite::DeleteCron(_) => (Vec::new(), Vec::new()),
        SchedulerWrite::RecoverRunning { .. } => {
            let running = store.all_running_jobs()?;
            let ids = running.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
            let (pending, _) = jobs_by_id(store, &ids, true, false)?;
            (pending, running)
        }
    };
    sort_jobs(&mut pending);
    sort_jobs(&mut running);
    let result = match operation {
        SchedulerWrite::RecordResult(result) => store.scheduled_result(&result.id)?,
        _ => None,
    };
    let cron = match operation {
        SchedulerWrite::SaveCron(cron) => store.cron_job(&cron.name)?,
        SchedulerWrite::DeleteCron(name) => store.cron_job(name)?,
        _ => None,
    };
    Ok(SchedulerWriteSnapshot {
        pending,
        running,
        result,
        cron,
    })
}

fn observe_snapshot(
    store: &impl SchedulerStateReader,
    prepared: &PreparedSchedulerWrite,
) -> Result<SchedulerWriteSnapshot> {
    let ids = match &prepared.operation {
        SchedulerWrite::Insert(job) => vec![job.id.clone()],
        SchedulerWrite::ClaimDue { .. } => match &prepared.result {
            SchedulerWriteResult::Claimed(jobs) => {
                jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>()
            }
            _ => unreachable!("prepared claim must retain its claimed result"),
        },
        SchedulerWrite::Complete(job_id) | SchedulerWrite::Cancel(job_id) => {
            vec![job_id.clone()]
        }
        SchedulerWrite::RecoverRunning { .. } => prepared
            .before
            .running
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        SchedulerWrite::RecordResult(_)
        | SchedulerWrite::SaveCron(_)
        | SchedulerWrite::DeleteCron(_) => Vec::new(),
    };
    let (mut pending, mut running) = jobs_by_id(store, &ids, true, true)?;
    sort_jobs(&mut pending);
    sort_jobs(&mut running);
    let result = match &prepared.operation {
        SchedulerWrite::RecordResult(result) => store.scheduled_result(&result.id)?,
        _ => None,
    };
    let cron = match &prepared.operation {
        SchedulerWrite::SaveCron(cron) => store.cron_job(&cron.name)?,
        SchedulerWrite::DeleteCron(name) => store.cron_job(name)?,
        _ => None,
    };
    Ok(SchedulerWriteSnapshot {
        pending,
        running,
        result,
        cron,
    })
}

fn jobs_by_id(
    store: &impl SchedulerStateReader,
    job_ids: &[DocumentId],
    include_pending: bool,
    include_running: bool,
) -> Result<(Vec<ScheduledJob>, Vec<ScheduledJob>)> {
    let mut pending = Vec::new();
    let mut running = Vec::new();
    for job_id in job_ids {
        if include_pending && let Some(job) = store.pending_job(job_id)? {
            pending.push(job);
        }
        if include_running && let Some(job) = store.running_job(job_id)? {
            running.push(job);
        }
    }
    Ok((pending, running))
}

fn apply_intended(
    mut state: SchedulerWriteSnapshot,
    operation: &SchedulerWrite,
) -> (SchedulerWriteSnapshot, SchedulerWriteResult) {
    let result = match operation {
        SchedulerWrite::Insert(job) => {
            state.pending.push(job.clone());
            SchedulerWriteResult::Unit
        }
        SchedulerWrite::ClaimDue { now, max_jobs } => {
            let claimed = state
                .pending
                .iter()
                .filter(|job| job.run_at <= *now)
                .take(*max_jobs)
                .cloned()
                .collect::<Vec<_>>();
            let claimed_ids = claimed
                .iter()
                .map(|job| job.id.clone())
                .collect::<std::collections::HashSet<_>>();
            state.pending.retain(|job| !claimed_ids.contains(&job.id));
            state.running.extend(claimed.iter().cloned());
            SchedulerWriteResult::Claimed(claimed)
        }
        SchedulerWrite::Complete(job_id) => {
            state.running.retain(|job| &job.id != job_id);
            SchedulerWriteResult::Unit
        }
        SchedulerWrite::Cancel(job_id) => {
            let before = state.pending.len();
            state.pending.retain(|job| &job.id != job_id);
            SchedulerWriteResult::Removed(state.pending.len() != before)
        }
        SchedulerWrite::RecordResult(result) => {
            state.result = Some(result.clone());
            SchedulerWriteResult::Unit
        }
        SchedulerWrite::SaveCron(cron) => {
            state.cron = Some(cron.clone());
            SchedulerWriteResult::Unit
        }
        SchedulerWrite::DeleteCron(_) => {
            state.cron = None;
            SchedulerWriteResult::Unit
        }
        SchedulerWrite::RecoverRunning { now } => {
            for mut job in state.running.drain(..) {
                job.run_at = job.run_at.min(*now);
                state.pending.push(job);
            }
            SchedulerWriteResult::Unit
        }
    };
    sort_jobs(&mut state.pending);
    sort_jobs(&mut state.running);
    (state, result)
}

fn sort_jobs(jobs: &mut [ScheduledJob]) {
    jobs.sort_by(|left, right| {
        left.run_at
            .cmp(&right.run_at)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
}

macro_rules! impl_scheduler_state_reader {
    ($store:ty) => {
        impl SchedulerStateReader for $store {
            fn pending_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
                self.get_pending_scheduled_job(job_id)
            }

            fn running_job(&self, job_id: &DocumentId) -> Result<Option<ScheduledJob>> {
                self.get_running_scheduled_job(job_id)
            }

            fn due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
                self.peek_due_scheduled_jobs(now, max_jobs)
            }

            fn all_running_jobs(&self) -> Result<Vec<ScheduledJob>> {
                self.list_running_scheduled_jobs()
            }

            fn scheduled_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>> {
                self.get_scheduled_job_result(job_id)
            }

            fn cron_job(&self, name: &str) -> Result<Option<CronJob>> {
                self.get_cron_job(name)
            }
        }

        impl SchedulerWriteOutcomeStore for $store {
            fn prepare_scheduler_write(
                &self,
                operation: SchedulerWrite,
            ) -> Result<PreparedSchedulerWrite> {
                prepare(self, operation)
            }

            fn reconcile_scheduler_write(
                &self,
                prepared: &PreparedSchedulerWrite,
            ) -> Result<SchedulerWriteReconciliation> {
                reconcile(self, prepared)
            }
        }
    };
}

impl_scheduler_state_reader!(TenantStore);
impl_scheduler_state_reader!(SqliteTenantStore);
impl_scheduler_state_reader!(MemoryTenantStore);
impl_scheduler_state_reader!(PostgresTenantStore);
impl_scheduler_state_reader!(MySqlTenantStore);
impl_scheduler_state_reader!(LibsqlReplicaTenantStore);

#[cfg(test)]
mod tests {
    use nimbus_core::{CronSchedule, Mutation, ScheduledJobOutcome, TableName, Timestamp};
    use serde_json::json;

    use super::*;
    use crate::SchedulerWriteStore;

    fn job(id: &str, run_at: u64) -> ScheduledJob {
        ScheduledJob {
            id: DocumentId::from_key(id).expect("test job id should build"),
            run_at: Timestamp(run_at),
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("test table should build"),
                id: None,
                fields: serde_json::Map::from_iter([("title".to_string(), json!(id))]),
            },
            created_at: Timestamp(1),
        }
    }

    fn apply_and_prove(store: &TenantStore, operation: SchedulerWrite) -> SchedulerWriteResult {
        let prepared = store
            .prepare_scheduler_write(operation.clone())
            .expect("scheduler pre-state should read");
        assert_eq!(
            store
                .reconcile_scheduler_write(&prepared)
                .expect("unchanged scheduler state should reconcile"),
            SchedulerWriteReconciliation::RolledBack
        );
        let actual = store
            .scheduler_write_cancellable(operation, || Ok(()))
            .expect("scheduler write should commit");
        assert_eq!(actual, prepared.result());
        assert_eq!(
            store
                .reconcile_scheduler_write(&prepared)
                .expect("committed scheduler state should reconcile"),
            SchedulerWriteReconciliation::Committed(actual.clone())
        );
        actual
    }

    fn apply_and_prove_equivalent_noop(
        store: &TenantStore,
        operation: SchedulerWrite,
    ) -> SchedulerWriteResult {
        let prepared = store
            .prepare_scheduler_write(operation.clone())
            .expect("scheduler pre-state should read");
        let actual = store
            .scheduler_write_cancellable(operation, || Ok(()))
            .expect("scheduler no-op should commit");
        assert_eq!(actual, prepared.result());
        assert_eq!(
            store
                .reconcile_scheduler_write(&prepared)
                .expect("equivalent scheduler state should reconcile"),
            SchedulerWriteReconciliation::Committed(actual.clone())
        );
        actual
    }

    #[test]
    fn scheduler_outcome_contract_covers_every_transition() {
        let store = TenantStore::create_in_memory().expect("store should open");

        let completed = job("completed", 10);
        let queued_behind = job("queued-behind", 11);
        assert_eq!(
            apply_and_prove(&store, SchedulerWrite::Insert(completed.clone())),
            SchedulerWriteResult::Unit
        );
        apply_and_prove(&store, SchedulerWrite::Insert(queued_behind.clone()));
        assert_eq!(
            apply_and_prove_equivalent_noop(
                &store,
                SchedulerWrite::Complete(queued_behind.id.clone()),
            ),
            SchedulerWriteResult::Unit
        );
        assert_eq!(
            apply_and_prove(
                &store,
                SchedulerWrite::ClaimDue {
                    now: Timestamp(11),
                    max_jobs: 1,
                },
            ),
            SchedulerWriteResult::Claimed(vec![completed.clone()])
        );
        assert_eq!(
            apply_and_prove_equivalent_noop(&store, SchedulerWrite::Cancel(completed.id.clone()),),
            SchedulerWriteResult::Removed(false)
        );
        assert_eq!(
            apply_and_prove(&store, SchedulerWrite::Complete(completed.id.clone())),
            SchedulerWriteResult::Unit
        );
        assert_eq!(
            apply_and_prove(&store, SchedulerWrite::Cancel(queued_behind.id.clone()),),
            SchedulerWriteResult::Removed(true)
        );

        let cancelled = job("cancelled", 20);
        apply_and_prove(&store, SchedulerWrite::Insert(cancelled.clone()));
        assert_eq!(
            apply_and_prove(&store, SchedulerWrite::Cancel(cancelled.id.clone())),
            SchedulerWriteResult::Removed(true)
        );

        let result = ScheduledJobResult {
            id: completed.id.clone(),
            run_at: completed.run_at,
            finished_at: Timestamp(30),
            mutation: completed.mutation.clone(),
            outcome: ScheduledJobOutcome::Completed,
            error: None,
        };
        assert_eq!(
            apply_and_prove(&store, SchedulerWrite::RecordResult(result)),
            SchedulerWriteResult::Unit
        );

        let cron = CronJob {
            name: "hourly".to_string(),
            schedule: CronSchedule::Interval { seconds: 3_600 },
            mutation: completed.mutation.clone(),
            enabled: true,
            last_run: None,
            next_run: Timestamp(3_600_000),
            created_at: Timestamp(1),
        };
        apply_and_prove(&store, SchedulerWrite::SaveCron(cron));
        apply_and_prove(&store, SchedulerWrite::DeleteCron("hourly".to_string()));

        let orphan = job("orphan", 40);
        apply_and_prove(&store, SchedulerWrite::Insert(orphan.clone()));
        apply_and_prove(
            &store,
            SchedulerWrite::ClaimDue {
                now: Timestamp(40),
                max_jobs: 1,
            },
        );
        assert_eq!(
            apply_and_prove(
                &store,
                SchedulerWrite::RecoverRunning { now: Timestamp(50) },
            ),
            SchedulerWriteResult::Unit
        );
        assert_eq!(
            store
                .list_scheduled_jobs()
                .expect("recovered job should list"),
            vec![orphan]
        );
    }

    #[test]
    fn scheduler_outcome_contract_rejects_a_different_observed_transition() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let intended = job("collision", 10);
        let prepared = store
            .prepare_scheduler_write(SchedulerWrite::Insert(intended.clone()))
            .expect("scheduler pre-state should read");
        let mut conflicting = intended;
        conflicting.run_at = Timestamp(11);
        store
            .scheduler_write_cancellable(SchedulerWrite::Insert(conflicting), || Ok(()))
            .expect("conflicting state should commit for reconciliation test");
        assert_eq!(
            store
                .reconcile_scheduler_write(&prepared)
                .expect("conflicting state should remain readable"),
            SchedulerWriteReconciliation::Ambiguous
        );
    }
}
