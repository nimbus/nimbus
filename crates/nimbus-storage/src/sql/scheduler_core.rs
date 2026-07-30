//! Dialect-shared scheduler write-transaction orchestration.
//!
//! The claim loop and the running-job recovery rule are identical across the SQL
//! backends; only the statements they run differ. [`SqlSchedulerTransaction`]
//! captures those statement-level hooks, and the free functions below own the
//! orchestration — the batch guard, the per-job cancellation checks, the
//! move-to-running / move-to-pending ordering, and the recovery `min(now)` rule
//! that had drifted into three separate copies of the same twelve-line comment.
//!
//! What deliberately stays per-backend:
//!
//! - **Claim statement and lock mode.** PostgreSQL relies on its per-tenant
//!   advisory transaction lock and issues a plain `SELECT`; MySQL serializes
//!   claimers with `FOR UPDATE`. Dialect lock mode is load-bearing (CO6), so
//!   each backend keeps its own statement inside `select_due_jobs`.
//! - **Timestamp binding.** PostgreSQL converts to `i64` fallibly through
//!   `i64_from_timestamp`; MySQL binds the raw `u64` microseconds. That
//!   conversion stays at each backend's SQL edge.
//! - **The libsql replica.** Its scheduler methods have the same *shape* but a
//!   different borrow structure — it re-acquires the session inside each
//!   `block_on` future rather than holding one across the call — and its
//!   statements are inline literals with no qualified table names. Forcing it
//!   through this seam would mean rewriting its transaction plumbing, so it
//!   keeps its own copy.

use nimbus_core::{Result, ScheduledJob, Timestamp};

use crate::sql::write_core::SqlWriteBackend;

/// Statement-level seam for scheduler writes inside an open transaction.
///
/// As in [`crate::sql::write_core`], a method here may share a name with an
/// inherent method on the implementing transaction type; inside the trait `impl`
/// the body resolves to the inherent method, so the forwarding impls are not
/// recursive.
pub(crate) trait SqlSchedulerTransaction: SqlWriteBackend {
    /// Selects up to `max_jobs` jobs due at or before `now`, ordered by
    /// `(run_at, id)`. Callers guarantee `max_jobs > 0`.
    fn select_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>>;

    /// Moves one claimed job from `scheduled_jobs` to `running_scheduled_jobs`.
    /// Both backends delete before inserting.
    fn move_job_to_running(&mut self, job: &ScheduledJob) -> Result<()>;

    /// Loads every row currently in `running_scheduled_jobs`.
    fn load_running_jobs(&mut self) -> Result<Vec<ScheduledJob>>;

    /// Moves one job from `running_scheduled_jobs` back to `scheduled_jobs` at
    /// its (already adjusted) `run_at`. Both backends insert before deleting —
    /// the opposite order from [`Self::move_job_to_running`].
    fn move_job_to_pending(&mut self, job: &ScheduledJob) -> Result<()>;

    /// Flags that this transaction touched scheduler state. PostgreSQL turns
    /// this into a `LISTEN/NOTIFY` payload at commit; backends with no
    /// notification channel keep the default no-op.
    fn mark_scheduler_changed(&mut self) {}
}

/// Claims due jobs and moves them to the running table.
///
/// Returns the claimed jobs. An empty claim marks nothing changed and issues no
/// move statements.
pub(crate) fn sql_claim_due_jobs<B: SqlSchedulerTransaction>(
    backend: &mut B,
    now: Timestamp,
    max_jobs: usize,
) -> Result<Vec<ScheduledJob>> {
    backend.check_cancel()?;
    if max_jobs == 0 {
        return Ok(Vec::new());
    }

    let due = backend.select_due_jobs(now, max_jobs)?;
    if due.is_empty() {
        return Ok(Vec::new());
    }

    for job in &due {
        backend.check_cancel()?;
        backend.move_job_to_running(job)?;
    }
    backend.mark_scheduler_changed();
    Ok(due)
}

/// Returns every running job to the pending table so the next tick re-claims it.
pub(crate) fn sql_recover_running_jobs<B: SqlSchedulerTransaction>(
    backend: &mut B,
    now: Timestamp,
) -> Result<()> {
    backend.check_cancel()?;
    let running_jobs = backend.load_running_jobs()?;
    for mut job in running_jobs {
        backend.check_cancel()?;
        // A recovered running job was already DUE when it was claimed (claim
        // only takes run_at <= now), so keep its original due time instead of
        // re-stamping the recovery instant: stamping `now` artificially delays
        // the job and — under wall-clock regression (e.g. NTP slew) between
        // recovery and the next tick — can push it past that tick's `now`,
        // silently deferring recovery (flaked scheduler_recovery_campaign on
        // CI). min() keeps any older due time intact and never moves a job into
        // the future.
        job.run_at = job.run_at.min(now);
        backend.move_job_to_pending(&job)?;
    }
    backend.mark_scheduler_changed();
    Ok(())
}
