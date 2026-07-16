use std::sync::Arc;
use std::time::Instant;

use nimbus_core::{CommitEntry, Document, Result};

use crate::persistence::TenantPersistence;
use crate::{Engine, tenant::TenantRuntime};

use super::super::phase_metrics::CommitPhaseDurations;
use super::super::prepared::PreparedCommit;
use super::super::shadow_conflicts::{observe_shadow_conflicts, prepared_document_dependencies};

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
    pub(super) fn run_store_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mut prepared_commit: PreparedCommit,
        mut profile: DirectMutationProfile,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(
                &TenantPersistence,
                &PreparedCommit,
                nimbus_core::Timestamp,
            ) -> Result<CommitEntry>
            + Send
            + 'static,
    {
        let runtime_for_commit = runtime.clone();
        let commit = runtime.submit_direct_committer(move || {
            let runtime = runtime_for_commit;
            let queue_wait_started = Instant::now();
            profile.phases.queue_wait = queue_wait_started.elapsed();
            observe_direct_shadow(&runtime, &prepared_commit, &mut profile.phases);
            let expected_sequence =
                crate::tenant::assign_and_validate(runtime.durable_head(), 1)?[0];
            let assignment_timestamp = runtime.assign_commit_timestamp();
            prepared_commit.stamp_for_assignment(assignment_timestamp)?;
            let durable_append_started = Instant::now();
            let write_log_guard = runtime.arm_write_log_append();
            let commit = mutate(runtime.store(), &prepared_commit, assignment_timestamp)?;
            debug_assert_eq!(commit.sequence, expected_sequence);
            profile.phases.durable_append = durable_append_started.elapsed();
            let publish_started = Instant::now();
            publish_direct_commit_to_write_log(&runtime, &commit);
            runtime.mark_durable_head(commit.sequence);
            profile.phases.add_publish(publish_started.elapsed());
            let apply_started = Instant::now();
            runtime.invalidate_document_cache_for_commit(&commit);
            profile.phases.apply = apply_started.elapsed();
            let publish_started = Instant::now();
            runtime.mark_applied_head(commit.sequence);
            profile.phases.add_publish(publish_started.elapsed());
            write_log_guard.disarm();
            Ok((commit, prepared_commit, profile))
        })?;
        let (commit, _prepared_commit, profile) = commit;
        runtime.record_commit_phase_sample(
            "direct",
            1,
            profile.phases,
            profile.started_at.elapsed(),
        );
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mut prepared_commit: PreparedCommit,
        mut profile: DirectMutationProfile,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(
                &TenantPersistence,
                &PreparedCommit,
                nimbus_core::Timestamp,
            ) -> Result<Option<CommitEntry>>
            + Send
            + 'static,
    {
        let runtime_for_commit = runtime.clone();
        let (commit, profile) = runtime.submit_direct_committer(move || {
            let runtime = runtime_for_commit;
            let queue_wait_started = Instant::now();
            profile.phases.queue_wait = queue_wait_started.elapsed();
            observe_direct_shadow(&runtime, &prepared_commit, &mut profile.phases);
            let expected_sequence =
                crate::tenant::assign_and_validate(runtime.durable_head(), 1)?[0];
            let assignment_timestamp = runtime.assign_commit_timestamp();
            prepared_commit.stamp_for_assignment(assignment_timestamp)?;
            let durable_append_started = Instant::now();
            let write_log_guard = runtime.arm_write_log_append();
            let commit = mutate(runtime.store(), &prepared_commit, assignment_timestamp)?;
            if let Some(commit) = &commit {
                debug_assert_eq!(commit.sequence, expected_sequence);
            }
            profile.phases.durable_append = durable_append_started.elapsed();
            if let Some(commit) = &commit {
                let publish_started = Instant::now();
                publish_direct_commit_to_write_log(&runtime, commit);
                runtime.mark_durable_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
                let apply_started = Instant::now();
                runtime.invalidate_document_cache_for_commit(commit);
                profile.phases.apply = apply_started.elapsed();
                let publish_started = Instant::now();
                runtime.mark_applied_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
            }
            write_log_guard.disarm();
            Ok((commit, profile))
        })?;
        let Some(commit) = commit else {
            return Ok(false);
        };
        runtime.record_commit_phase_sample(
            "direct",
            1,
            profile.phases,
            profile.started_at.elapsed(),
        );
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    pub(super) fn run_store_delete_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mut prepared_commit: PreparedCommit,
        mut profile: DirectMutationProfile,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(
                &TenantPersistence,
                &PreparedCommit,
                nimbus_core::Timestamp,
            ) -> Result<(CommitEntry, Document)>
            + Send
            + 'static,
    {
        let runtime_for_commit = runtime.clone();
        let ((commit, _deleted_document), profile) =
            runtime.submit_direct_committer(move || {
                let runtime = runtime_for_commit;
                let queue_wait_started = Instant::now();
                profile.phases.queue_wait = queue_wait_started.elapsed();
                observe_direct_shadow(&runtime, &prepared_commit, &mut profile.phases);
                let expected_sequence =
                    crate::tenant::assign_and_validate(runtime.durable_head(), 1)?[0];
                let assignment_timestamp = runtime.assign_commit_timestamp();
                prepared_commit.stamp_for_assignment(assignment_timestamp)?;
                let durable_append_started = Instant::now();
                let write_log_guard = runtime.arm_write_log_append();
                let (commit, deleted_document) =
                    mutate(runtime.store(), &prepared_commit, assignment_timestamp)?;
                debug_assert_eq!(commit.sequence, expected_sequence);
                profile.phases.durable_append = durable_append_started.elapsed();
                let publish_started = Instant::now();
                publish_direct_commit_to_write_log(&runtime, &commit);
                runtime.mark_durable_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
                let apply_started = Instant::now();
                runtime.invalidate_document_cache_for_commit(&commit);
                profile.phases.apply = apply_started.elapsed();
                let publish_started = Instant::now();
                runtime.mark_applied_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
                write_log_guard.disarm();
                Ok(((commit, deleted_document), profile))
            })?;
        runtime.record_commit_phase_sample(
            "direct",
            1,
            profile.phases,
            profile.started_at.elapsed(),
        );
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    pub(super) fn run_store_delete_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mut prepared_commit: PreparedCommit,
        mut profile: DirectMutationProfile,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(
                &TenantPersistence,
                &PreparedCommit,
                nimbus_core::Timestamp,
            ) -> Result<Option<(CommitEntry, Document)>>
            + Send
            + 'static,
    {
        let runtime_for_commit = runtime.clone();
        let (commit, profile) = runtime.submit_direct_committer(move || {
            let runtime = runtime_for_commit;
            let queue_wait_started = Instant::now();
            profile.phases.queue_wait = queue_wait_started.elapsed();
            observe_direct_shadow(&runtime, &prepared_commit, &mut profile.phases);
            let expected_sequence =
                crate::tenant::assign_and_validate(runtime.durable_head(), 1)?[0];
            let assignment_timestamp = runtime.assign_commit_timestamp();
            prepared_commit.stamp_for_assignment(assignment_timestamp)?;
            let durable_append_started = Instant::now();
            let write_log_guard = runtime.arm_write_log_append();
            let commit = mutate(runtime.store(), &prepared_commit, assignment_timestamp)?;
            if let Some((commit, _)) = &commit {
                debug_assert_eq!(commit.sequence, expected_sequence);
            }
            profile.phases.durable_append = durable_append_started.elapsed();
            if let Some((commit, _deleted_document)) = &commit {
                let publish_started = Instant::now();
                publish_direct_commit_to_write_log(&runtime, commit);
                runtime.mark_durable_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
                let apply_started = Instant::now();
                runtime.invalidate_document_cache_for_commit(commit);
                profile.phases.apply = apply_started.elapsed();
                let publish_started = Instant::now();
                runtime.mark_applied_head(commit.sequence);
                profile.phases.add_publish(publish_started.elapsed());
            }
            write_log_guard.disarm();
            Ok((commit, profile))
        })?;
        let Some((commit, _deleted_document)) = commit else {
            return Ok(false);
        };
        runtime.record_commit_phase_sample(
            "direct",
            1,
            profile.phases,
            profile.started_at.elapsed(),
        );
        self.process_commit(runtime, &commit);
        Ok(true)
    }
}

fn publish_direct_commit_to_write_log(runtime: &TenantRuntime, commit: &CommitEntry) {
    runtime.stage_pending_write_log_commits([commit.clone()], runtime.store.now());
    runtime.publish_write_log_through(commit.sequence);
}

fn observe_direct_shadow(
    runtime: &TenantRuntime,
    prepared_commit: &PreparedCommit,
    phases: &mut CommitPhaseDurations,
) {
    let conflict_check_started = Instant::now();
    let dependencies = prepared_document_dependencies(prepared_commit, |table| {
        runtime.store.table_id(table).ok().flatten()
    });
    observe_shadow_conflicts(
        runtime,
        prepared_commit.snapshot_sequence,
        std::slice::from_ref(&dependencies),
    );
    phases.add_conflict_check(conflict_check_started.elapsed());
}
