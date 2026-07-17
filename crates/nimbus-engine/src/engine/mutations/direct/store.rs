use std::sync::Arc;
use std::time::Instant;

use nimbus_core::{CommitEntry, Result};

use crate::{Engine, tenant::TenantRuntime};

use super::super::inline_reprepare::{
    InlineReprepareOutcome, reprepare_single_document_from_window,
};
use super::super::phase_metrics::CommitPhaseDurations;
use super::super::prepared::PreparedCommit;

#[derive(Clone, Copy)]
pub(super) struct DirectMutationProfile {
    started_at: Instant,
    phases: CommitPhaseDurations,
}

impl DirectMutationProfile {
    pub(super) fn after_prepare(started_at: Instant) -> Self {
        Self {
            started_at,
            phases: CommitPhaseDurations {
                prepare: started_at.elapsed(),
                ..CommitPhaseDurations::default()
            },
        }
    }
}

impl Engine {
    pub(super) fn run_prepared_direct_mutation(
        &self,
        runtime: Arc<TenantRuntime>,
        mut prepared_commit: PreparedCommit,
        mut profile: DirectMutationProfile,
    ) -> Result<Option<CommitEntry>> {
        let runtime_for_commit = runtime.clone();
        let runtime_for_fanout = runtime.clone();
        let queued_at = Instant::now();
        let (commit, profile) = runtime.submit_direct_committer_then(
            move || {
                let runtime = runtime_for_commit;
                profile.phases.queue_wait = queued_at.elapsed();

                // DirectCommit's serial boundary is intentionally limited to
                // full-image window validation, assignment, assignment-only
                // stamping, and the unchanged atomic storage append/apply.
                let conflict_started = Instant::now();
                let dependencies = prepared_commit.read_set.clone();
                match reprepare_single_document_from_window(
                    runtime.as_ref(),
                    &mut prepared_commit,
                    &dependencies,
                )? {
                    InlineReprepareOutcome::Fresh | InlineReprepareOutcome::Reprepared => {}
                    InlineReprepareOutcome::CallerWait(error) => return Err(error),
                }
                profile.phases.conflict_check = conflict_started.elapsed();
                let previous_sequence = runtime.durable_head();
                let sequence = crate::tenant::assign_and_validate(previous_sequence, 1)?[0];
                let timestamp = runtime.assign_commit_timestamp();
                prepared_commit.stamp_for_assignment(sequence, timestamp)?;
                let (record, schedule_ops, scheduled_execution_id) = {
                    let (record, _, scheduled_execution_id) = prepared_commit.direct_effects()?;
                    (record, &[][..], scheduled_execution_id)
                };

                runtime.stage_pending_write_log_commits(
                    [record.as_commit_entry()],
                    runtime.store.now(),
                );
                let durable_append_started = Instant::now();
                let write_log_guard = runtime.arm_write_log_append();
                let commit = match runtime.store.apply_prepared_write_batch(
                    record,
                    schedule_ops,
                    scheduled_execution_id,
                ) {
                    Ok(commit) => commit,
                    Err(error) => {
                        if runtime
                            .store
                            .journal_progress()
                            .is_ok_and(|progress| progress.durable_head == previous_sequence)
                        {
                            runtime.discard_unpersisted_write_log_suffix(sequence);
                        }
                        return Err(error);
                    }
                };
                let Some(commit) = commit else {
                    runtime.discard_unpersisted_write_log_suffix(sequence);
                    write_log_guard.disarm();
                    return Ok((None, profile));
                };
                crate::tenant::validate_append_sequences(previous_sequence, [commit.sequence])?;
                debug_assert_eq!(commit.sequence, sequence);
                runtime.mark_durable_head(commit.sequence);
                profile.phases.durable_append = durable_append_started.elapsed();

                let publish_started = Instant::now();
                let published_frontier = runtime.publish_write_log_through(commit.sequence);
                runtime.invalidate_document_cache_for_commit(&commit);
                runtime.mark_applied_head(published_frontier);
                profile.phases.publish = publish_started.elapsed();
                write_log_guard.disarm();
                Ok((Some(commit), profile))
            },
            |(commit, _)| {
                if let Some(commit) = commit {
                    self.process_commit_fanout(runtime_for_fanout.clone(), commit);
                    self.enqueue_applied_commit_batch_observers(
                        runtime_for_fanout,
                        std::slice::from_ref(commit),
                    );
                }
            },
        )?;
        let Some(commit) = commit else {
            return Ok(None);
        };
        runtime.record_commit_phase_sample(
            "direct",
            1,
            profile.phases,
            profile.started_at.elapsed(),
        );
        Ok(Some(commit))
    }
}
