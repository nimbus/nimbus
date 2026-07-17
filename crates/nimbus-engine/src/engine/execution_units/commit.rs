use std::time::Instant;

use nimbus_core::{
    CommitEntry, DependencySet, DocumentLocator, Error, Result, SequenceNumber, TenantEventRecord,
    Timestamp, TriggerWriteOrigin, WriteOp, WriteOpType,
};
use nimbus_storage::ResolvedWrite;

use super::super::mutations::caps::check_mutation_caps;
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
        let total_started = Instant::now();
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let finalization_guard = FinalizationGuard { unit: self };
        let prepare_started = Instant::now();
        let _prepare_permit = self.runtime.acquire_prepare_permit_blocking()?;
        let mut prepared_commit = {
            let mut state = self.active_state()?;
            state.lifecycle = ExecutionUnitLifecycle::Finalizing;
            let writes = self.build_resolved_writes(&state);
            let record = prepare_execution_unit_record(
                &self.runtime,
                &self.snapshot,
                &writes,
                state.trigger_write_origin.as_ref(),
            )?;
            let schedule_ops = self.build_resolved_schedule_ops(&state);
            let mut conflict_dependencies = state.read_dependencies.clone();
            conflict_dependencies.extend(&state.write_dependencies);
            PreparedCommit::for_execution_unit(
                self.snapshot_sequence,
                conflict_dependencies,
                writes,
                record,
                schedule_ops,
                state.deferred_server_timestamp_fields.clone(),
                state.usage,
            )?
        };
        drop(_prepare_permit);
        self.runtime
            .commit_phase_metrics()
            .record_prepare_pool(prepare_started.elapsed());
        let mut phases = CommitPhaseDurations {
            prepare: prepare_started.elapsed(),
            ..CommitPhaseDurations::default()
        };
        check_mutation_caps(&self.runtime, prepared_commit.usage())?;
        self.runtime.check_tenant_write_rate(
            self.engine.now(),
            prepared_commit.usage().total_write_bytes(),
        )?;
        if prepared_commit.is_empty_execution_unit() {
            return Ok(None);
        }
        let _prepared_payload = crate::tenant::PreparedPayloadAccounting::new(
            self.runtime.clone(),
            prepared_commit.accounted_bytes(),
        );
        maybe_warn_wide_read_set(&self.tenant_id, &prepared_commit.read_set);
        self.engine
            .wait_for_commit_fault(labels::PREPARE_COMPLETE)?;
        let has_scheduled_insert = prepared_commit.has_scheduled_insert();

        let result = (|| -> Result<Option<CommitEntry>> {
            self.engine.wait_for_commit_fault(labels::PRE_ASSIGN)?;
            #[cfg(any(test, feature = "test-hooks"))]
            if self
                .engine
                .commit_faults
                .is_armed(labels::SCHEMA_ASSIGNED_BEFORE_VISIBLE)
            {
                ensure_schema_unchanged(
                    &self.runtime,
                    &self.schema_epoch_snapshot,
                    &self.schema_snapshot,
                    &prepared_commit.read_set,
                )?;
            }
            // Provider scans and any document resolution happen on the caller
            // before actor admission. The actor repeats only the process-local
            // full-image check so pending assignments cannot be missed.
            if !self.runtime.store.has_process_local_sequence_authority() {
                ensure_no_conflicts(
                    &self.runtime,
                    prepared_commit.snapshot_sequence,
                    &prepared_commit.read_set,
                )?;
            }
            let runtime_for_commit = self.runtime.clone();
            let engine_for_commit = self.engine.clone();
            let engine_for_fanout = self.engine.clone();
            let runtime_for_fanout = self.runtime.clone();
            let schema_epoch_snapshot = self.schema_epoch_snapshot.clone();
            let schema_snapshot = self.schema_snapshot.clone();
            let queued_at = Instant::now();
            let (commit, phases) = self.runtime.submit_execution_unit_committer_then(
                move || {
                    let runtime = runtime_for_commit;
                    let engine = engine_for_commit;
                    phases.queue_wait = queued_at.elapsed();
                    let conflict_check_started = Instant::now();
                    ensure_schema_unchanged(
                        &runtime,
                        &schema_epoch_snapshot,
                        &schema_snapshot,
                        &prepared_commit.read_set,
                    )?;
                    ensure_no_conflicts_in_window(
                        &runtime,
                        prepared_commit.snapshot_sequence,
                        &prepared_commit.read_set,
                    )?;
                    phases.conflict_check = conflict_check_started.elapsed();
                    engine.wait_for_commit_fault(labels::POST_VALIDATE_PRE_STAGE)?;
                    // This retained fault seam sits immediately before assignment;
                    // pending-window staging follows the assignment stamp below.
                    engine.wait_for_commit_fault(labels::PRE_PERSIST)?;
                    let previous_sequence = runtime.durable_head();
                    let expected_sequence =
                        crate::tenant::assign_and_validate(previous_sequence, 1)?[0];
                    let commit_timestamp = runtime.assign_commit_timestamp();
                    prepared_commit.stamp_for_assignment(expected_sequence, commit_timestamp)?;
                    let (record, schedule_ops) = prepared_commit.execution_unit_effects()?;
                    if let Some(record) = record {
                        runtime.stage_pending_write_log_commits(
                            [record.as_commit_entry()],
                            runtime.store.now(),
                        );
                    }
                    let durable_append_started = Instant::now();
                    // The current storage contract atomically combines persistence
                    // and storage-layer application. Until that seam is split, the
                    // full call is attributed to durable append; `apply` below is
                    // engine cache/bookkeeping work. Direct commits have the same
                    // intentional collapse.
                    let write_log_guard = runtime.arm_write_log_append();
                    let commit = match runtime
                        .store
                        .apply_prepared_execution_unit_batch(record, schedule_ops)
                    {
                        Ok(commit) => commit,
                        Err(error) => {
                            if record.is_some() {
                                match runtime.store.journal_progress() {
                                    Ok(progress) if progress.durable_head == previous_sequence => {
                                        runtime.discard_unpersisted_write_log_suffix(
                                            expected_sequence,
                                        );
                                    }
                                    Ok(_) => {
                                        if let Ok(progress) =
                                            runtime.store.recover_durable_journal()
                                        {
                                            runtime.publish_mutation_journal_progress_in_actor(
                                                progress,
                                            );
                                            if progress.applied_head >= expected_sequence {
                                                write_log_guard.disarm();
                                            }
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                            return Err(error);
                        }
                    };
                    if let Some(commit) = &commit {
                        crate::tenant::validate_append_sequences(
                            previous_sequence,
                            [commit.sequence],
                        )?;
                        debug_assert_eq!(commit.sequence, expected_sequence);
                    }
                    phases.durable_append = durable_append_started.elapsed();
                    engine.wait_for_commit_fault(labels::DURABLE_BEFORE_PUBLISH)?;
                    if let Some(commit) = &commit {
                        let publish_started = Instant::now();
                        let published_frontier = runtime.publish_write_log_through(commit.sequence);
                        runtime.mark_durable_head(commit.sequence);
                        phases.add_publish(publish_started.elapsed());
                        let apply_started = Instant::now();
                        runtime.invalidate_document_cache_for_commit(commit);
                        phases.apply = apply_started.elapsed();
                        let publish_started = Instant::now();
                        runtime.mark_applied_head(published_frontier);
                        phases.add_publish(publish_started.elapsed());
                    }
                    write_log_guard.disarm();
                    Ok((commit, phases))
                },
                |(commit, _)| {
                    if let Some(commit) = commit {
                        engine_for_fanout.process_commit_fanout(runtime_for_fanout, commit);
                    }
                },
            )?;
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
            self.engine
                .notify_committed_mutation_observers(self.runtime.as_ref(), commit);
        }
        if has_scheduled_insert {
            self.engine.wake_scheduler();
        }
        Ok(commit)
    }
}

fn prepare_execution_unit_record(
    runtime: &crate::tenant::TenantRuntime,
    snapshot: &crate::persistence::TenantPersistenceSnapshot,
    writes: &[ResolvedWrite],
    trigger_write_origin: Option<&TriggerWriteOrigin>,
) -> Result<Option<TenantEventRecord>> {
    if writes.is_empty() {
        // Schedule-only execution units intentionally have no journal record
        // under the existing storage contract, so there is nothing to serialize.
        return Ok(None);
    }
    let mut prepared_writes = Vec::with_capacity(writes.len());
    for write in writes {
        let (table, document_id) = match write {
            ResolvedWrite::Insert { document, .. } => (&document.table, &document.id),
            ResolvedWrite::Update { current, .. } => (&current.table, &current.id),
            ResolvedWrite::Delete { previous, .. } => (&previous.table, &previous.id),
        };
        let table_id = match write {
            ResolvedWrite::Insert { .. } => {
                runtime.prepared_table_id(table, snapshot.table_id(table)?)
            }
            ResolvedWrite::Update { .. } | ResolvedWrite::Delete { .. } => snapshot
                .table_id(table)?
                .ok_or_else(|| Error::DocumentNotFound(document_id.clone()))?,
        };
        let prepared = match write {
            ResolvedWrite::Insert {
                document,
                resource_path_binding,
                ..
            } => WriteOp {
                table: table.clone(),
                table_id,
                op_type: WriteOpType::Insert,
                doc_id: document_id.clone(),
                resource_path_binding: resource_path_binding.clone(),
                trigger_write_origin: trigger_write_origin.cloned(),
                previous: None,
                current: Some(document.clone()),
            },
            ResolvedWrite::Update {
                previous,
                current,
                resource_path_binding,
                ..
            } => WriteOp {
                table: table.clone(),
                table_id,
                op_type: WriteOpType::Update,
                doc_id: document_id.clone(),
                resource_path_binding: resource_path_binding.clone(),
                trigger_write_origin: trigger_write_origin.cloned(),
                previous: Some(previous.clone()),
                current: Some(current.clone()),
            },
            ResolvedWrite::Delete { previous, .. } => WriteOp {
                table: table.clone(),
                table_id,
                op_type: WriteOpType::Delete,
                doc_id: document_id.clone(),
                resource_path_binding: snapshot.resource_path_binding(&DocumentLocator::new(
                    table.clone(),
                    document_id.clone(),
                ))?,
                trigger_write_origin: trigger_write_origin.cloned(),
                previous: Some(previous.clone()),
                current: None,
            },
        };
        prepared_writes.push(prepared);
    }
    Ok(Some(TenantEventRecord::new(
        SequenceNumber(0),
        Timestamp(0),
        prepared_writes,
        None,
    )?))
}

fn ensure_schema_unchanged(
    runtime: &crate::tenant::TenantRuntime,
    schema_epoch_snapshot: &std::collections::HashMap<nimbus_core::TableName, SequenceNumber>,
    schema_snapshot: &nimbus_core::Schema,
    dependencies: &DependencySet,
) -> Result<()> {
    let current_schema = runtime.schema();
    for table in dependencies.touched_tables() {
        let observed_epoch = schema_epoch_snapshot
            .get(&table)
            .copied()
            .unwrap_or(SequenceNumber(0));
        let current_epoch = runtime.current_schema_epoch(&table);
        if observed_epoch != current_epoch
            || current_schema.get_table(&table) != schema_snapshot.get_table(&table)
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
    runtime: &crate::tenant::TenantRuntime,
    snapshot_sequence: SequenceNumber,
    dependencies: &DependencySet,
) -> Result<()> {
    if dependencies.is_empty() {
        return Ok(());
    }

    let head = runtime.durable_head();
    let validation_source = if runtime.store.has_process_local_sequence_authority() {
        runtime
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
                runtime.store.get(table, &document_id)
            }),
        ValidationSource::StorageFallback => {
            // `read_commit_log_from` intentionally returns the available
            // suffix and does not enforce journal retention itself. Probe
            // the cursor contract first so a truncated prefix can never
            // turn into a silent validation pass.
            runtime
                .store
                .stream_durable_journal(snapshot_sequence, 1)
                .map_err(|error| map_fallback_floor_error(error, snapshot_sequence))?;
            let commits = runtime
                .store
                .read_commit_log_from(SequenceNumber(snapshot_sequence.0.saturating_add(1)))?;
            commits.into_iter().find_map(|commit| {
                nimbus_core::commit_intersects_dependency_set(
                    &commit,
                    dependencies,
                    &[],
                    |table, document_id| runtime.store.get(table, &document_id),
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

/// Assign-time conflict validation over plain, full-image window data. This
/// boundary performs no storage I/O and cannot await.
fn ensure_no_conflicts_in_window(
    runtime: &crate::tenant::TenantRuntime,
    snapshot_sequence: SequenceNumber,
    dependencies: &DependencySet,
) -> Result<()> {
    if dependencies.is_empty() || !runtime.store.has_process_local_sequence_authority() {
        return Ok(());
    }
    let head = runtime.durable_head();
    let ValidationSource::InMemory(view) = runtime
        .write_log
        .validation_source(snapshot_sequence, head)?
    else {
        return Err(Error::retryable_conflict(
            "execution-unit prepare fell outside the process-local conflict window",
            Some(runtime.applied_head()),
        ));
    };
    if let Some(sequence) = view.first_conflicting_sequence(dependencies, |_, _| {
        Err(Error::Internal(
            "full-image execution-unit validation unexpectedly requested storage".to_string(),
        ))
    }) {
        return Err(Error::retryable_conflict(
            "transaction conflict detected; retry the mutation",
            Some(sequence),
        ));
    }
    Ok(())
}

fn map_fallback_floor_error(error: Error, snapshot_sequence: SequenceNumber) -> Error {
    match error {
        Error::InvalidInput(message) if message.contains("retention floor") => {
            Error::out_of_retention(
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
            Error::OutOfRetention {
                ref message,
                minimum_sequence: None,
            } if message.contains("durable commit-log retention horizon")
        ));
    }
}
