use std::time::Instant;

use nimbus_core::{CommitEntry, DependencySet, Error, Result, SequenceNumber, Timestamp};

use super::super::mutations::phase_metrics::{CommitPhaseDurations, maybe_warn_wide_read_set};
use super::super::mutations::prepared::PreparedCommit;
use super::super::mutations::write_log::ValidationSource;
use super::state::ExecutionUnitLifecycle;
use super::{MutationExecutionUnit, labels};

struct FinalizationGuard<'a> {
    unit: &'a MutationExecutionUnit,
}

impl Drop for FinalizationGuard<'_> {
    fn drop(&mut self) {
        self.unit.finish_finalization();
    }
}

impl MutationExecutionUnit {
    pub fn commit(&self) -> Result<Option<CommitEntry>> {
        self.commit_with_timestamp(None)
    }

    pub(super) fn commit_at(&self, commit_timestamp: Timestamp) -> Result<Option<CommitEntry>> {
        self.commit_with_timestamp(Some(commit_timestamp))
    }

    fn commit_with_timestamp(
        &self,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        let total_started = Instant::now();
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let finalization_guard = FinalizationGuard { unit: self };
        let prepare_started = Instant::now();
        let prepared_commit = {
            let mut state = self.active_state()?;
            state.lifecycle = ExecutionUnitLifecycle::Finalizing;
            let writes = self.build_resolved_writes(&state);
            let schedule_ops = self.build_resolved_schedule_ops(&state);
            let mut conflict_dependencies = state.read_dependencies.clone();
            conflict_dependencies.extend(&state.write_dependencies);
            PreparedCommit::for_execution_unit(
                self.snapshot_sequence,
                conflict_dependencies,
                writes,
                schedule_ops,
                state.trigger_write_origin.clone(),
            )
        };
        let mut phases = CommitPhaseDurations {
            prepare: prepare_started.elapsed(),
            ..CommitPhaseDurations::default()
        };
        if prepared_commit.is_empty_execution_unit() {
            return Ok(None);
        }
        maybe_warn_wide_read_set(&self.tenant_id, &prepared_commit.read_set);
        self.engine
            .wait_for_commit_fault(labels::PREPARE_COMPLETE)?;

        let result = (|| -> Result<Option<CommitEntry>> {
            self.engine.wait_for_commit_fault(labels::PRE_ASSIGN)?;
            let commit = {
                let queue_wait_started = Instant::now();
                let _sequence_guard = self.runtime.lock_mutation_sequence();
                phases.queue_wait = queue_wait_started.elapsed();
                let conflict_check_started = Instant::now();
                self.ensure_schema_unchanged(&prepared_commit.read_set)?;
                self.ensure_no_conflicts(
                    prepared_commit.snapshot_sequence,
                    &prepared_commit.read_set,
                )?;
                phases.conflict_check = conflict_check_started.elapsed();
                self.engine
                    .wait_for_commit_fault(labels::POST_VALIDATE_PRE_STAGE)?;
                // Staging and persistence are one storage call today, so this
                // remains the closest pre-persist boundary. PreparedCommit now
                // carries the exact storage payload without changing that call.
                self.engine.wait_for_commit_fault(labels::PRE_PERSIST)?;
                let (writes, schedule_ops, trigger_write_origin) =
                    prepared_commit.execution_unit_effects()?;
                let durable_append_started = Instant::now();
                // The current storage contract atomically combines persistence
                // and storage-layer application. Until that seam is split, the
                // full call is attributed to durable append; `apply` below is
                // engine cache/bookkeeping work. Direct commits have the same
                // intentional collapse.
                let write_log_guard = self.runtime.arm_write_log_append();
                let commit = self.runtime.store.apply_execution_unit_batch_with_origin(
                    writes,
                    schedule_ops,
                    trigger_write_origin,
                    commit_timestamp,
                )?;
                phases.durable_append = durable_append_started.elapsed();
                self.engine
                    .wait_for_commit_fault(labels::DURABLE_BEFORE_PUBLISH)?;
                if let Some(commit) = &commit {
                    let publish_started = Instant::now();
                    self.runtime.stage_pending_write_log_commits(
                        [commit.clone()],
                        self.runtime.store.now(),
                    );
                    self.runtime.publish_write_log_through(commit.sequence);
                    self.runtime.mark_durable_head(commit.sequence);
                    phases.add_publish(publish_started.elapsed());
                    let apply_started = Instant::now();
                    self.runtime.invalidate_document_cache_for_commit(commit);
                    phases.apply = apply_started.elapsed();
                    let publish_started = Instant::now();
                    self.runtime.mark_applied_head(commit.sequence);
                    phases.add_publish(publish_started.elapsed());
                }
                write_log_guard.disarm();
                commit
            };
            self.engine
                .wait_for_commit_fault(labels::POST_PUBLISH_PRE_FANOUT)?;
            if commit.is_some() {
                self.runtime.record_commit_phase_sample(
                    "execution-unit",
                    1,
                    phases,
                    total_started.elapsed(),
                );
            }
            Ok(commit)
        })();
        drop(finalization_guard);
        let commit = result?;

        if let Some(commit) = &commit {
            self.engine.process_commit(self.runtime.clone(), commit);
        }
        if prepared_commit.has_scheduled_insert() {
            self.engine.wake_scheduler();
        }
        Ok(commit)
    }

    fn ensure_schema_unchanged(&self, dependencies: &DependencySet) -> Result<()> {
        let current_schema = self.runtime.schema();
        for table in dependencies.touched_tables() {
            let observed_epoch = self
                .schema_epoch_snapshot
                .get(&table)
                .copied()
                .unwrap_or(SequenceNumber(0));
            let current_epoch = self.runtime.current_schema_epoch(&table);
            if observed_epoch != current_epoch
                || current_schema.get_table(&table) != self.schema_snapshot.get_table(&table)
            {
                return Err(Error::retryable_conflict(
                    format!(
                        "table schema changed during transaction: {table} (epoch {observed_epoch} -> {current_epoch})"
                    ),
                    (current_epoch != SequenceNumber(0)).then_some(current_epoch),
                ));
            }
        }
        Ok(())
    }

    fn ensure_no_conflicts(
        &self,
        snapshot_sequence: SequenceNumber,
        dependencies: &DependencySet,
    ) -> Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }

        let head = self.runtime.durable_head();
        let validation_source = if self.runtime.store.has_process_local_sequence_authority() {
            self.runtime
                .write_log
                .validation_source(snapshot_sequence, head)?
        } else {
            // Postgres/MySQL/remote-libSQL can accept a foreign process's
            // commit before its notification advances this runtime's head.
            // Until PPSC5 supplies a storage-coordinated publish watermark,
            // their process-local window is not an authoritative upper bound.
            ValidationSource::StorageFallback
        };
        let conflicting_sequence = match validation_source {
            ValidationSource::InMemory(view) => view
                .first_conflicting_sequence(dependencies, |table, document_id| {
                    self.runtime.store.get(table, &document_id)
                }),
            ValidationSource::StorageFallback => {
                // `read_commit_log_from` intentionally returns the available
                // suffix and does not enforce journal retention itself. Probe
                // the cursor contract first so a truncated prefix can never
                // turn into a silent validation pass.
                self.runtime
                    .store
                    .stream_durable_journal(snapshot_sequence, 1)
                    .map_err(|error| map_fallback_floor_error(error, snapshot_sequence))?;
                let commits = self
                    .runtime
                    .store
                    .read_commit_log_from(SequenceNumber(snapshot_sequence.0.saturating_add(1)))?;
                commits.into_iter().find_map(|commit| {
                    nimbus_core::commit_intersects_dependency_set(
                        &commit,
                        dependencies,
                        &[],
                        |table, document_id| self.runtime.store.get(table, &document_id),
                    )
                    .then_some(commit.sequence)
                })
            }
        };
        if let Some(conflicting_sequence) = conflicting_sequence {
            return Err(Error::retryable_conflict(
                "transaction conflict detected; retry the mutation",
                Some(conflicting_sequence),
            ));
        }
        Ok(())
    }
}

fn map_fallback_floor_error(error: Error, snapshot_sequence: SequenceNumber) -> Error {
    match error {
        Error::InvalidInput(message) if message.contains("retention floor") => {
            Error::retryable_conflict(
                format!(
                    "transaction snapshot {snapshot_sequence} is older than the durable commit-log retention horizon; retry from a fresh snapshot"
                ),
                None,
            )
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_fallback_retention_floor_fails_closed() {
        let error = map_fallback_floor_error(
            Error::InvalidInput("journal cursor 1 is behind the retention floor 2".to_string()),
            SequenceNumber(1),
        );

        assert!(matches!(
            error,
            Error::Conflict {
                ref message,
                retryable: true,
                ..
            } if message.contains("durable commit-log retention horizon")
        ));
    }
}
