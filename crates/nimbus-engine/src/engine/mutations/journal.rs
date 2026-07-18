use std::{
    collections::{HashSet, VecDeque},
    future,
    sync::Arc,
    time::{Duration, Instant},
};

use nimbus_core::{
    AccessAction, CommitEntry, DependencySet, Document, Error, IdSource, Mutation, Result,
    SequenceNumber, TenantId, Timestamp,
};
use tracing::warn;

use crate::Engine;
use crate::engine::execution_units::{CommitFaultClient, labels};
use crate::tenant::{
    AssignedPublisherBatch, DeferredPublisherResponse, MutationResponseSender,
    PendingPublisherResponse, PreparedPayloadAccounting, QueuedMutationRequest,
    QueuedMutationResult, TenantRuntime,
};

use super::caps::{MutationUsage, check_mutation_caps};
use super::direct::{MutationExecutionMode, MutationExecutionResult};
use super::enforce_mutation_authorization;
use super::inline_reprepare::{InlineReprepareOutcome, reprepare_single_document_from_window};
use super::phase_metrics::CommitPhaseDurations;
use super::prepared::PreparedCommit;
use super::publisher::{begin_durable_recovery_eviction, finish_durable_recovery_eviction};
use super::shadow_conflicts::{observe_shadow_conflicts, prepared_document_dependencies};
use super::window_prepare::{WindowPreparedWrite, prepare_single_document_write_from_window};

struct PendingMutationResponseGuard {
    runtime: Arc<TenantRuntime>,
}

impl Drop for PendingMutationResponseGuard {
    fn drop(&mut self) {
        self.runtime.finish_pending_mutation_response();
    }
}

struct PreparedQueuedParts {
    prepared_commit: PreparedCommit,
    conflict_dependencies: DependencySet,
    result: QueuedMutationResult,
    prepare_nanos: u64,
}

struct QueuedMutationBatchResult {
    applied: Vec<CommitEntry>,
    responses: Vec<PendingPublisherResponse>,
}

struct FailedSerialQueuedMutationBatch {
    error: Error,
    deferred: Vec<DeferredPublisherResponse>,
}

enum SerialBatchRecovery {
    Definitive {
        discarded_first_sequence: Option<SequenceNumber>,
    },
    CrashAndReplay(Error),
}

impl FailedSerialQueuedMutationBatch {
    fn without_deferred(error: Error) -> Self {
        Self {
            error,
            deferred: Vec::new(),
        }
    }
}

struct AssignedPublisherWork {
    batch: Option<AssignedPublisherBatch>,
    deferred: Vec<DeferredPublisherResponse>,
}

#[cfg(test)]
static STRIP_NEXT_INLINE_REPREPARE_FOR_TESTING: std::sync::OnceLock<
    std::sync::Mutex<HashSet<TenantId>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
static SERIAL_RECOVERY_READ_FAILURES_FOR_TESTING: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<TenantId, (bool, bool)>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
fn take_strip_next_inline_reprepare_for_testing(tenant_id: &TenantId) -> bool {
    STRIP_NEXT_INLINE_REPREPARE_FOR_TESTING
        .get_or_init(|| std::sync::Mutex::new(HashSet::new()))
        .lock()
        .expect("inline re-prepare test-hook lock should not be poisoned")
        .remove(tenant_id)
}

#[cfg(test)]
fn take_serial_recovery_read_failures_for_testing(tenant_id: &TenantId) -> (bool, bool) {
    SERIAL_RECOVERY_READ_FAILURES_FOR_TESTING
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .expect("serial recovery test-hook lock should not be poisoned")
        .remove(tenant_id)
        .unwrap_or_default()
}

#[cfg(not(test))]
fn take_serial_recovery_read_failures_for_testing(_tenant_id: &TenantId) -> (bool, bool) {
    (false, false)
}

impl Engine {
    #[cfg(test)]
    pub(crate) fn fail_serial_recovery_reads_for_testing(
        &self,
        tenant_id: TenantId,
        durable_recovery: bool,
        progress_fallback: bool,
    ) {
        SERIAL_RECOVERY_READ_FAILURES_FOR_TESTING
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
            .expect("serial recovery test-hook lock should not be poisoned")
            .insert(tenant_id, (durable_recovery, progress_fallback));
    }

    pub(crate) async fn run_one_committer_journal_batch(
        self: Arc<Self>,
        runtime: Arc<TenantRuntime>,
    ) {
        #[cfg(any(test, debug_assertions))]
        Engine::assert_running_on_background_task("mutation_committer");

        let batch_policy = crate::config::mutation_journal_batch_policy();
        runtime.drain_mutation_admission_queue();
        #[cfg(any(test, feature = "test-hooks"))]
        runtime.wait_before_mutation_drain().await;
        let batch = runtime
            .drain_mutation_batch_adaptive(
                batch_policy.base,
                batch_policy.max,
                batch_policy.coalesce,
            )
            .await;
        if batch.is_empty() {
            return;
        }

        let use_pipeline = if runtime.store.has_process_local_sequence_authority() {
            match runtime.reconcile_committer_pipeline_mode().await {
                Ok(enabled) => enabled,
                Err(error) => {
                    let current = runtime.committer_pipeline_mode();
                    warn!(
                        tenant = %runtime.tenant_id(),
                        error = %error,
                        ?current,
                        "committer pipeline mode transition failed; processing client batch in the current mode"
                    );
                    matches!(current, crate::tenant::CommitterPipelineMode::Pipeline)
                }
            }
        } else {
            false
        };

        if use_pipeline {
            let _assignment_guard = runtime.lock_publisher_assignment_recovery().await;
            let assignment_baseline = runtime.assigned_head();
            let assignment_responses = batch
                .iter()
                .map(|request| request.response.clone())
                .collect::<Vec<_>>();
            let runtime_for_task = runtime.clone();
            let engine = self.clone();
            let commit_faults = self.commit_faults.clone();
            let assigned = tokio::task::spawn_blocking(move || {
                assign_queued_mutation_batch(
                    runtime_for_task,
                    batch,
                    engine,
                    assignment_baseline,
                    &commit_faults,
                )
            })
            .await;
            match assigned {
                Ok(Ok(mut work)) => {
                    if let Err(error) = runtime
                        .send_publisher_response_fence(std::mem::take(&mut work.deferred))
                        .await
                    {
                        let (responses, error) = *error;
                        for response in responses {
                            response.fail(&error);
                        }
                        if let Some(assigned) = work.batch {
                            let first = assigned.first_sequence();
                            assigned.fail(&error);
                            runtime.discard_unpersisted_write_log_suffix(first);
                        }
                        runtime.record_mutation_worker_failure();
                        warn!(error = %error, "publisher queue rejected ordered response fence");
                        return;
                    }
                    let Some(assigned) = work.batch else {
                        return;
                    };
                    if let Err(error) = runtime.send_assigned_publisher_batch(assigned).await {
                        let (assigned, error) = *error;
                        let first = assigned.first_sequence();
                        assigned.fail(&error);
                        runtime.discard_unpersisted_write_log_suffix(first);
                        runtime.record_mutation_worker_failure();
                        warn!(error = %error, "publisher queue rejected assigned mutation batch");
                    }
                }
                Ok(Err(error)) => {
                    runtime.record_mutation_worker_failure();
                    warn!(error = %error, "mutation assignment batch failed");
                    recover_failed_assignment(
                        runtime.clone(),
                        assignment_baseline,
                        "mutation assignment batch failed",
                    )
                    .await;
                    fail_mutation_responses(&assignment_responses, &error);
                }
                Err(join_error) => {
                    let error = Error::Internal(format!(
                        "committer assignment batch panicked: {join_error}"
                    ));
                    runtime.record_mutation_worker_failure();
                    warn!(error = %error, "committer assignment batch panicked");
                    recover_failed_assignment(
                        runtime.clone(),
                        assignment_baseline,
                        "committer assignment batch panicked",
                    )
                    .await;
                    fail_mutation_responses(&assignment_responses, &error);
                }
            }
            return;
        }

        // This is both the kill-switch fallback and the provider arm. Provider
        // persistence stays here until slice C adds ordered network pipelining
        // and lease fencing.
        let assignment_baseline = runtime.durable_head();
        let assignment_responses = batch
            .iter()
            .map(|request| request.response.clone())
            .collect::<Vec<_>>();
        let runtime_for_task = runtime.clone();
        let engine = self.clone();
        let commit_faults = self.commit_faults.clone();
        let batch_result = tokio::task::spawn_blocking(move || {
            process_serial_queued_mutation_batch(runtime_for_task, batch, engine, &commit_faults)
        })
        .await;

        match batch_result {
            Ok(Ok(batch_result)) => {
                // Real document commits only: this batch is drained from
                // the mutation admission queue, never mixed with a
                // zero-write commit from another source (the
                // trigger-candidate feed's own cursor advance is
                // appended through a separate path that never reaches
                // here). So `len() == 1` alone is an exact identity
                // check -- no need for the kind-aware records check the
                // provider catch-up path requires.
                let commit_identity =
                    (batch_result.applied.len() == 1).then(|| batch_result.applied[0].clone());
                self.process_applied_commit_batch_fanout(
                    runtime.clone(),
                    &batch_result.applied,
                    commit_identity,
                    true,
                );
                for pending_response in batch_result.responses {
                    let _ = pending_response.response.send(Ok(pending_response.result));
                }
                self.enqueue_applied_commit_batch_observers(runtime, &batch_result.applied);
            }
            Ok(Err(mut failure)) => {
                runtime.record_mutation_worker_failure();
                warn!(error = %failure.error, "mutation journal batch failed");
                let recovery = recover_failed_serial_batch(
                    runtime.clone(),
                    assignment_baseline,
                    "serial mutation batch failed",
                )
                .await;
                match recovery {
                    SerialBatchRecovery::Definitive {
                        discarded_first_sequence,
                    } => {
                        for response in failure.deferred.drain(..) {
                            if let Some(first_sequence) = discarded_first_sequence {
                                response.complete_after_recovery(first_sequence, &failure.error);
                            } else {
                                response.complete();
                            }
                        }
                        fail_mutation_responses(&assignment_responses, &failure.error);
                    }
                    SerialBatchRecovery::CrashAndReplay(error) => {
                        runtime.publisher_record_ambiguous_error();
                        begin_durable_recovery_eviction(&runtime, &error);
                        for response in failure.deferred.drain(..) {
                            response.fail(&error);
                        }
                        fail_mutation_responses(&assignment_responses, &error);
                        runtime.fail_and_drain_mutation_queues(&error);
                        runtime.close_committed_mutation_observers();
                        let engine = self.clone();
                        self.spawn_background("serial_durable_recovery_eviction", async move {
                            runtime
                                .wait_for_committed_mutation_observers_drained()
                                .await;
                            runtime.wait_for_operation_drain_for_eviction().await;
                            finish_durable_recovery_eviction(engine, runtime).await;
                        });
                    }
                }
            }
            Err(join_error) => {
                let error = Error::Internal(format!(
                    "serial committer queued batch panicked: {join_error}"
                ));
                runtime.record_mutation_worker_failure();
                warn!(error = %error, "committer queued batch panicked");
                let recovery = recover_failed_serial_batch(
                    runtime.clone(),
                    assignment_baseline,
                    "serial mutation batch panicked",
                )
                .await;
                match recovery {
                    SerialBatchRecovery::Definitive { .. } => {
                        fail_mutation_responses(&assignment_responses, &error);
                    }
                    SerialBatchRecovery::CrashAndReplay(recovery_error) => {
                        runtime.publisher_record_ambiguous_error();
                        begin_durable_recovery_eviction(&runtime, &recovery_error);
                        fail_mutation_responses(&assignment_responses, &recovery_error);
                        runtime.fail_and_drain_mutation_queues(&recovery_error);
                        runtime.close_committed_mutation_observers();
                        let engine = self.clone();
                        self.spawn_background("serial_durable_recovery_eviction", async move {
                            runtime
                                .wait_for_committed_mutation_observers_drained()
                                .await;
                            runtime.wait_for_operation_drain_for_eviction().await;
                            finish_durable_recovery_eviction(engine, runtime).await;
                        });
                    }
                }
            }
        }
    }

    pub(super) async fn submit_journaled_async_mutation<Fut>(
        self: &Arc<Self>,
        runtime: Arc<TenantRuntime>,
        tenant_id: &TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: nimbus_core::PrincipalContext,
        cancel_wait: Fut,
    ) -> Result<MutationExecutionResult>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
    {
        let mutation = normalize_queued_insert_id(self.id_source.as_ref(), mutation);
        let usage = MutationUsage::for_journal_admission(
            &mutation,
            matches!(&mode, MutationExecutionMode::Scheduled { .. }),
        );
        check_mutation_caps(&runtime, usage)?;
        runtime.check_tenant_write_rate(self.now(), usage.total_write_bytes())?;
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        runtime.begin_pending_mutation_response();
        let _pending_response = PendingMutationResponseGuard {
            runtime: runtime.clone(),
        };
        tokio::pin!(cancel_wait);
        let scheduled_execution_id = match &mode {
            MutationExecutionMode::Immediate => None,
            MutationExecutionMode::Scheduled { execution_id } => Some(execution_id.clone()),
        };
        let max_attempts = mutation_occ_max_attempts();
        let mut attempt = 1;
        loop {
            let operation = runtime.enter_operation(tenant_id)?;
            let fast_prepare_started = Instant::now();
            let prepared = if scheduled_execution_id.is_none()
                && let Some(prepared) = prepare_single_document_write_from_window(
                    runtime.as_ref(),
                    &mutation,
                    &principal,
                )? {
                runtime.commit_phase_metrics().record_window_prepare();
                prepared_queued_from_window(
                    prepared,
                    principal.clone(),
                    fast_prepare_started.elapsed(),
                )?
            } else {
                runtime.commit_phase_metrics().record_storage_prepare();
                let runtime_for_prepare = runtime.clone();
                let mutation_for_prepare = mutation.clone();
                let principal_for_prepare = principal.clone();
                let scheduled_for_prepare = scheduled_execution_id.clone();
                let id_source = Arc::clone(&self.id_source);
                let prepare_permit = runtime.acquire_prepare_permit().await?;
                tokio::task::spawn_blocking(move || {
                    let _prepare_permit = prepare_permit;
                    prepare_queued_mutation(
                        runtime_for_prepare.as_ref(),
                        mutation_for_prepare,
                        principal_for_prepare,
                        scheduled_for_prepare,
                        id_source.as_ref(),
                    )
                })
                .await
                .map_err(|error| {
                    Error::Internal(format!("mutation prepare task failed: {error}"))
                })??
            };
            #[cfg(test)]
            let mut prepared = prepared;
            #[cfg(test)]
            if take_strip_next_inline_reprepare_for_testing(tenant_id) {
                prepared.prepared_commit.inline_reprepare = None;
            }
            runtime
                .commit_phase_metrics()
                .record_prepare_pool(Duration::from_nanos(prepared.prepare_nanos));
            let shadow_snapshot_sequence = prepared.prepared_commit.snapshot_sequence;
            let prepared_bytes = prepared.prepared_commit.accounted_bytes();
            let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
            let enqueued_at = Instant::now();
            runtime.enqueue_mutation_admission_request(QueuedMutationRequest {
                prepared_commit: Box::new(prepared.prepared_commit),
                conflict_dependencies: prepared.conflict_dependencies,
                result: prepared.result,
                prepared_payload_accounting: Some(PreparedPayloadAccounting::new(
                    runtime.clone(),
                    prepared_bytes,
                )),
                cancelled: cancelled.clone(),
                _operation: operation,
                response: MutationResponseSender::new(response_tx),
                enqueued_at,
                shadow_snapshot_sequence,
            })?;
            if let Err(error) = runtime.send_queued_committer_batch(self.clone()).await {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                return Err(error);
            }

            let response = tokio::select! {
                result = &mut response_rx => result,
                _ = &mut cancel_wait => {
                    cancelled.store(true, std::sync::atomic::Ordering::Release);
                    (&mut response_rx).await
                }
            }
            .map_err(|_| {
                Error::Internal("committer actor dropped mutation response".to_string())
            })?;
            match response {
                Ok(result) => {
                    return Ok(match result {
                        QueuedMutationResult::Immediate(document_id) => {
                            MutationExecutionResult::Immediate(document_id)
                        }
                        QueuedMutationResult::Scheduled(applied) => {
                            MutationExecutionResult::Scheduled(applied)
                        }
                    });
                }
                Err(error) if error.retryability() == nimbus_core::Retryability::Retryable => {
                    if attempt >= max_attempts {
                        runtime
                            .commit_phase_metrics()
                            .record_mutation_conflict_exhausted();
                        return Err(error.with_conflict_attempts(attempt));
                    }
                    if let Some(sequence) = error.conflicting_sequence() {
                        runtime
                            .wait_for_applied_sequence_cancellable(sequence, &mut cancel_wait)
                            .await?;
                    }
                    runtime
                        .commit_phase_metrics()
                        .record_mutation_conflict_retry();
                    tokio::select! {
                        _ = &mut cancel_wait => return Err(Error::Cancelled),
                        _ = tokio::time::sleep(mutation_occ_backoff(attempt)) => {}
                    }
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub(super) fn mutation_occ_max_attempts() -> usize {
    crate::config::env_positive_usize("NIMBUS_MUTATION_OCC_MAX_RETRIES", 4)
}

pub(super) fn mutation_occ_backoff(attempt: usize) -> Duration {
    let initial = crate::config::env_nonnegative_u64("NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS", 100);
    let maximum = crate::config::env_nonnegative_u64("NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS", 2_000)
        .max(initial);
    let shift = u32::try_from(attempt.saturating_sub(1))
        .unwrap_or(u32::MAX)
        .min(63);
    Duration::from_millis(initial.saturating_mul(1u64 << shift).min(maximum))
}

fn assign_queued_mutation_batch(
    runtime: Arc<TenantRuntime>,
    batch: Vec<QueuedMutationRequest>,
    engine: Arc<Engine>,
    mut previous_sequence: SequenceNumber,
    commit_faults: &CommitFaultClient,
) -> Result<AssignedPublisherWork> {
    let mut phases = CommitPhaseDurations::default();
    let mut scheduled_execution_overlay = HashSet::new();
    let mut active = Vec::new();
    let mut deferred = Vec::new();
    let mut records = Vec::new();
    let mut sample_started_at = None::<Instant>;
    let mut batch_shadow_dependencies = Vec::new();
    let mut batch_shadow_snapshot = None::<nimbus_core::SequenceNumber>;
    let mut requests = VecDeque::from(batch);
    while let Some(request) = requests.pop_front() {
        let QueuedMutationRequest {
            prepared_commit,
            conflict_dependencies,
            result,
            prepared_payload_accounting,
            cancelled,
            _operation,
            response,
            shadow_snapshot_sequence,
            enqueued_at,
        } = request;
        let mut prepared_commit = *prepared_commit;
        drop(prepared_payload_accounting);
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            deferred.push(DeferredPublisherResponse {
                _operation,
                response,
                result: Err(Error::Cancelled),
            });
            continue;
        }
        if prepared_commit.is_empty_journal() {
            deferred.push(DeferredPublisherResponse {
                _operation,
                response,
                result: Ok(result),
            });
            continue;
        }
        if let Some(execution_id) = prepared_commit.scheduled_execution_id()
            && !scheduled_execution_overlay.insert(execution_id.to_string())
        {
            deferred.push(DeferredPublisherResponse {
                _operation,
                response,
                result: Ok(QueuedMutationResult::Scheduled(false)),
            });
            continue;
        }
        let conflict_started = Instant::now();
        let validation = reprepare_single_document_from_window(
            runtime.as_ref(),
            &mut prepared_commit,
            &conflict_dependencies,
        );
        match validation {
            Ok(InlineReprepareOutcome::Fresh | InlineReprepareOutcome::Reprepared) => {}
            Ok(InlineReprepareOutcome::CallerWait(error)) | Err(error) => {
                phases.add_conflict_check(conflict_started.elapsed());
                deferred.push(DeferredPublisherResponse {
                    _operation,
                    response,
                    result: Err(error),
                });
                continue;
            }
        }
        phases.add_conflict_check(conflict_started.elapsed());
        let queue_wait = enqueued_at.elapsed();
        phases.add_queue_wait(queue_wait);
        sample_started_at = Some(
            sample_started_at
                .map(|started_at| started_at.min(enqueued_at))
                .unwrap_or(enqueued_at),
        );
        batch_shadow_dependencies.push(conflict_dependencies);
        batch_shadow_snapshot = Some(match batch_shadow_snapshot {
            Some(existing) => existing.min(shadow_snapshot_sequence),
            None => shadow_snapshot_sequence,
        });
        let serialize_started = Instant::now();
        let sequence = match crate::tenant::assign_and_validate(previous_sequence, 1) {
            Ok(sequences) => sequences[0],
            Err(error) => {
                return Err(error);
            }
        };
        let record = match prepared_commit.into_record(sequence, runtime.assign_commit_timestamp())
        {
            Ok(record) => record,
            Err(error) => {
                deferred.push(DeferredPublisherResponse {
                    _operation,
                    response,
                    result: Err(error),
                });
                continue;
            }
        };
        runtime.stage_pending_write_log_commits([record.as_commit_entry()], runtime.store.now());
        commit_faults
            .wait(labels::JOURNAL_ASSIGN_AFTER_STAGE)
            .into_result()?;
        previous_sequence = sequence;
        phases.add_prepare(serialize_started.elapsed());
        active.push(PendingPublisherResponse {
            _operation,
            response,
            result,
        });
        records.push(record);
    }

    if active.is_empty() {
        return Ok(AssignedPublisherWork {
            batch: None,
            deferred,
        });
    }

    // One sampled shadow observation per batch (pre-append, so the batch
    // never self-conflicts), against the batch's earliest planning snapshot.
    if let Some(batch_snapshot) = batch_shadow_snapshot {
        let conflict_started = Instant::now();
        observe_shadow_conflicts(runtime.as_ref(), batch_snapshot, &batch_shadow_dependencies);
        phases.add_conflict_check(conflict_started.elapsed());
    }

    Ok(AssignedPublisherWork {
        batch: Some(AssignedPublisherBatch {
            engine,
            records: Arc::new(records),
            responses: active,
            phases,
            sample_started_at: sample_started_at
                .expect("a non-empty active batch must retain an admitted request timestamp"),
        }),
        deferred,
    })
}

fn fail_mutation_responses(responses: &[MutationResponseSender], error: &Error) {
    for response in responses {
        let _ = response.send(Err(error.clone()));
    }
}

fn discard_failed_assignment_suffix(runtime: &TenantRuntime, assignment_baseline: SequenceNumber) {
    if let Some(first) = assignment_baseline.0.checked_add(1).map(SequenceNumber) {
        runtime.discard_unpersisted_write_log_suffix(first);
    }
}

async fn recover_failed_assignment(
    runtime: Arc<TenantRuntime>,
    assignment_baseline: SequenceNumber,
    context: &'static str,
) {
    discard_failed_assignment_suffix(runtime.as_ref(), assignment_baseline);
    match runtime
        .read_storage
        .execute(|store| store.recover_durable_journal())
        .await
    {
        Ok(progress) => runtime.publish_mutation_journal_progress_in_actor(progress),
        Err(error) => warn!(
            tenant = %runtime.tenant_id(),
            error = %error,
            "{context}; durable journal recovery failed"
        ),
    }
}

async fn recover_failed_serial_batch(
    runtime: Arc<TenantRuntime>,
    assignment_baseline: SequenceNumber,
    context: &'static str,
) -> SerialBatchRecovery {
    let forced_failures = take_serial_recovery_read_failures_for_testing(runtime.tenant_id());
    let durable_recovery = if forced_failures.0 {
        Err(Error::Internal(
            "injected serial durable recovery read failure".to_string(),
        ))
    } else {
        runtime
            .read_storage
            .execute(|store| store.recover_durable_journal())
            .await
    };
    match durable_recovery {
        Ok(progress) => {
            if progress.durable_head == assignment_baseline {
                discard_failed_assignment_suffix(runtime.as_ref(), assignment_baseline);
                let discarded_first_sequence =
                    assignment_baseline.0.checked_add(1).map(SequenceNumber);
                // Already on the tenant's committer task: sending a
                // JournalProgressSync message here would wait on our own inbox.
                runtime.publish_mutation_journal_progress_in_actor(progress);
                SerialBatchRecovery::Definitive {
                    discarded_first_sequence,
                }
            } else {
                SerialBatchRecovery::CrashAndReplay(Error::Internal(format!(
                    "serial append outcome requires crash-and-replay: durable head advanced from {assignment_baseline} to {}",
                    progress.durable_head
                )))
            }
        }
        Err(error) => {
            let progress_fallback = if forced_failures.1 {
                Err(Error::Internal(
                    "injected serial journal progress fallback failure".to_string(),
                ))
            } else {
                runtime.store.journal_progress()
            };
            let recovery = match progress_fallback {
                Ok(progress) => {
                    if progress.durable_head != assignment_baseline {
                        SerialBatchRecovery::CrashAndReplay(Error::Internal(format!(
                            "serial append outcome requires crash-and-replay: durable head advanced from {assignment_baseline} to {} after recovery failed: {error}",
                            progress.durable_head
                        )))
                    } else {
                        let first_unpersisted = progress
                            .durable_head
                            .0
                            .checked_add(1)
                            .map(SequenceNumber)
                            .filter(|first| *first <= runtime.assigned_head());
                        if let Some(first) = first_unpersisted {
                            runtime.discard_unpersisted_write_log_suffix(first);
                        }
                        // The fallback progress is authoritative for both heads
                        // only when it proves that the append did not land.
                        runtime.publish_mutation_journal_progress_in_actor(progress);
                        SerialBatchRecovery::Definitive {
                            discarded_first_sequence: first_unpersisted,
                        }
                    }
                }
                Err(progress_error) => {
                    warn!(
                        tenant = %runtime.tenant_id(),
                        error = %progress_error,
                        "{context}; journal progress fallback failed; evicting the runtime because the serial append outcome is ambiguous"
                    );
                    SerialBatchRecovery::CrashAndReplay(Error::Internal(format!(
                        "serial append outcome is ambiguous; crash-and-replay required: durable recovery failed ({error}) and journal progress failed ({progress_error})"
                    )))
                }
            };
            warn!(
                tenant = %runtime.tenant_id(),
                error = %error,
                "{context}; durable journal recovery failed"
            );
            recovery
        }
    }
}

fn process_serial_queued_mutation_batch(
    runtime: Arc<TenantRuntime>,
    batch: Vec<QueuedMutationRequest>,
    engine: Arc<Engine>,
    commit_faults: &CommitFaultClient,
) -> std::result::Result<QueuedMutationBatchResult, FailedSerialQueuedMutationBatch> {
    let previous_sequence = runtime.durable_head();
    let assignment_responses = batch
        .iter()
        .map(|request| request.response.clone())
        .collect::<Vec<_>>();
    let mut work = match assign_queued_mutation_batch(
        runtime.clone(),
        batch,
        engine,
        previous_sequence,
        commit_faults,
    ) {
        Ok(work) => work,
        Err(error) => {
            // The serial arm stages the same process-local write-log suffix as
            // the pipeline arm. Discard it before waking callers; the actor's
            // outer error path then refreshes durable progress from storage.
            discard_failed_assignment_suffix(runtime.as_ref(), previous_sequence);
            fail_mutation_responses(&assignment_responses, &error);
            return Err(FailedSerialQueuedMutationBatch::without_deferred(error));
        }
    };
    let Some(mut assigned) = work.batch else {
        for response in std::mem::take(&mut work.deferred) {
            response.complete();
        }
        return Ok(QueuedMutationBatchResult {
            applied: Vec::new(),
            responses: Vec::new(),
        });
    };
    let records = assigned.records.clone();
    let mut active = std::mem::take(&mut assigned.responses);
    let mut phases = assigned.phases;
    let sample_started_at = assigned.sample_started_at;
    let first_staged_sequence = Some(assigned.first_sequence());

    let durable_append_started = Instant::now();
    let append_baseline = runtime.durable_head();
    if let Err(error) = crate::tenant::validate_append_sequences(
        append_baseline,
        records.iter().map(|record| record.sequence),
    ) {
        if let Some(first) = first_staged_sequence {
            runtime.discard_unpersisted_write_log_suffix(first);
        }
        for active_request in active.drain(..) {
            let _ = active_request.response.send(Err(error.clone()));
        }
        return Err(FailedSerialQueuedMutationBatch {
            error,
            deferred: work.deferred,
        });
    }
    let write_log_guard = runtime.arm_write_log_append();
    if let Err(error) = runtime.store.append_durable_records_batch(&records) {
        let mapped_error = map_durable_journal_append_error(&error);
        drop(active);
        return Err(FailedSerialQueuedMutationBatch {
            error: mapped_error,
            deferred: work.deferred,
        });
    }

    if let Some(last_record) = records.last() {
        runtime.mark_durable_head(last_record.sequence);
    }
    write_log_guard.disarm();
    for response in std::mem::take(&mut work.deferred) {
        response.complete();
    }
    phases.durable_append = durable_append_started.elapsed();

    let mut applied = Vec::with_capacity(records.len());
    let mut responses = Vec::with_capacity(records.len());
    for (active_request, record) in active.into_iter().zip(records.iter()) {
        responses.push(PendingPublisherResponse {
            _operation: active_request._operation,
            response: active_request.response,
            result: active_request.result,
        });
        applied.push(record.as_commit_entry());
    }

    let apply_started = Instant::now();
    runtime
        .store
        .check_fault(nimbus_storage::FaultPoint::JournalDurableAppendBeforeApply)
        .map_err(FailedSerialQueuedMutationBatch::without_deferred)?;
    commit_faults
        .wait(labels::DURABLE_BEFORE_PUBLISH)
        .into_result()
        .map_err(FailedSerialQueuedMutationBatch::without_deferred)?;

    let applied_head = match runtime.store.apply_durable_records_batch(&records) {
        Ok(()) => runtime
            .store
            .applied_head_after_durable_apply(&records)
            .map_err(FailedSerialQueuedMutationBatch::without_deferred)?,
        Err(_) => {
            let progress = runtime
                .store
                .recover_durable_journal()
                .map_err(FailedSerialQueuedMutationBatch::without_deferred)?;
            progress.applied_head
        }
    };
    retain_commits_through_applied_head(&mut applied, applied_head);
    let published_frontier = runtime.publish_write_log_through(applied_head);
    runtime.invalidate_document_cache_for_commits(applied.iter());
    phases.apply = apply_started.elapsed();
    let publish_started = Instant::now();
    runtime.mark_applied_head(published_frontier);
    phases.publish = publish_started.elapsed();
    let committed_batch_size = u64::try_from(records.len()).unwrap_or(u64::MAX);
    runtime
        .commit_phase_metrics()
        .record_journal_batch(committed_batch_size);
    runtime.record_commit_phase_sample(
        "journal",
        committed_batch_size,
        phases,
        sample_started_at.elapsed(),
    );

    Ok(QueuedMutationBatchResult { applied, responses })
}

#[cfg(test)]
impl Engine {
    pub(crate) fn strip_next_inline_reprepare_for_testing(&self, tenant_id: &TenantId) {
        STRIP_NEXT_INLINE_REPREPARE_FOR_TESTING
            .get_or_init(|| std::sync::Mutex::new(HashSet::new()))
            .lock()
            .expect("inline re-prepare test-hook lock should not be poisoned")
            .insert(tenant_id.clone());
    }
}

fn normalize_queued_insert_id(id_source: &dyn IdSource, mutation: Mutation) -> Mutation {
    match mutation {
        Mutation::Insert {
            table,
            id: None,
            fields,
        } => Mutation::Insert {
            table,
            id: Some(id_source.next_document_id()),
            fields,
        },
        mutation => mutation,
    }
}

fn prepared_queued_from_window(
    prepared: WindowPreparedWrite,
    principal: nimbus_core::PrincipalContext,
    elapsed: Duration,
) -> Result<PreparedQueuedParts> {
    let result = match prepared.result_document_id {
        Some(id) => QueuedMutationResult::Immediate(Some(id)),
        None => QueuedMutationResult::Immediate(None),
    };
    let prepared_commit =
        PreparedCommit::for_journal(prepared.snapshot_sequence, vec![prepared.write], None)
            .with_inline_reprepare(prepared.normalized_mutation, principal, prepared.schema);
    Ok(PreparedQueuedParts {
        conflict_dependencies: prepared.dependencies,
        prepared_commit,
        result,
        prepare_nanos: duration_nanos(elapsed),
    })
}

fn retain_commits_through_applied_head(
    applied: &mut Vec<CommitEntry>,
    applied_head: SequenceNumber,
) {
    applied.retain(|commit| commit.sequence.0 <= applied_head.0);
}

fn prepare_queued_mutation(
    runtime: &TenantRuntime,
    mutation: Mutation,
    principal: nimbus_core::PrincipalContext,
    scheduled_execution_id: Option<String>,
    id_source: &dyn IdSource,
) -> Result<PreparedQueuedParts> {
    let started = Instant::now();
    if let Some(execution_id) = scheduled_execution_id.as_deref()
        && runtime.store.scheduled_execution_exists(execution_id)?
    {
        return Ok(PreparedQueuedParts {
            prepared_commit: PreparedCommit::for_journal(
                runtime.applied_head(),
                Vec::new(),
                scheduled_execution_id,
            ),
            conflict_dependencies: DependencySet::default(),
            result: QueuedMutationResult::Scheduled(false),
            prepare_nanos: duration_nanos(started.elapsed()),
        });
    }

    // The snapshot itself supplies the OCC pin. Sampling applied_head separately
    // could pair document images with the wrong sequence.
    let snapshot = runtime.store.read_snapshot()?;
    let snapshot_sequence = snapshot.applied_sequence()?;
    let schema = runtime.schema();
    let (write, result, inline_mutation) = match mutation {
        Mutation::Insert { table, id, fields } => {
            let table_id = runtime.prepared_table_id(&table, snapshot.table_id(&table)?);
            let table_schema = schema.get_table(&table).cloned();
            if let Some(table_schema) = table_schema.as_ref() {
                table_schema.validate(&fields)?;
            }
            let document = match id {
                Some(id) => Document::with_id_at(id, table.clone(), fields, Timestamp(0)),
                None => Document::with_id_at(
                    id_source.next_document_id(),
                    table.clone(),
                    fields,
                    Timestamp(0),
                ),
            };
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Create,
                &principal,
                Some(&document),
                None,
            )?;
            let id = document.id.clone();
            let inline_mutation = Mutation::Insert {
                table: table.clone(),
                id: Some(id.clone()),
                fields: document.fields.clone(),
            };
            (
                nimbus_core::WriteOp {
                    table,
                    table_id,
                    op_type: nimbus_core::WriteOpType::Insert,
                    doc_id: id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                },
                if scheduled_execution_id.is_some() {
                    QueuedMutationResult::Scheduled(true)
                } else {
                    QueuedMutationResult::Immediate(Some(id))
                },
                inline_mutation,
            )
        }
        Mutation::Update { table, id, patch } => {
            let inline_mutation = Mutation::Update {
                table: table.clone(),
                id: id.clone(),
                patch: patch.clone(),
            };
            let table_id = snapshot.table_id(&table)?.ok_or_else(|| {
                Error::Internal(format!("missing table identity for logical table {table}"))
            })?;
            let existing = snapshot
                .get(&table, &id)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let mut document = existing.clone();
            for (field, value) in patch {
                document.fields.insert(field, value);
            }
            let table_schema = schema.get_table(&table).cloned();
            if let Some(table_schema) = table_schema.as_ref() {
                table_schema.validate(&document.fields)?;
            }
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Update,
                &principal,
                Some(&document),
                Some(&existing),
            )?;
            (
                nimbus_core::WriteOp {
                    table,
                    table_id,
                    op_type: nimbus_core::WriteOpType::Update,
                    doc_id: id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(existing),
                    current: Some(document),
                },
                if scheduled_execution_id.is_some() {
                    QueuedMutationResult::Scheduled(true)
                } else {
                    QueuedMutationResult::Immediate(Some(id))
                },
                inline_mutation,
            )
        }
        Mutation::Delete { table, id } => {
            let table_id = snapshot.table_id(&table)?.ok_or_else(|| {
                Error::Internal(format!("missing table identity for logical table {table}"))
            })?;
            let existing = snapshot
                .get(&table, &id)?
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let table_schema = schema.get_table(&table).cloned();
            enforce_mutation_authorization(
                table_schema.as_ref(),
                AccessAction::Delete,
                &principal,
                None,
                Some(&existing),
            )?;
            let inline_mutation = Mutation::Delete {
                table: table.clone(),
                id: id.clone(),
            };
            (
                nimbus_core::WriteOp {
                    table,
                    table_id,
                    op_type: nimbus_core::WriteOpType::Delete,
                    doc_id: id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(existing),
                    current: None,
                },
                if scheduled_execution_id.is_some() {
                    QueuedMutationResult::Scheduled(true)
                } else {
                    QueuedMutationResult::Immediate(None)
                },
                inline_mutation,
            )
        }
    };
    let prepared_commit =
        PreparedCommit::for_journal(snapshot_sequence, vec![write], scheduled_execution_id)
            .with_inline_reprepare(inline_mutation, principal, schema);
    let conflict_dependencies = prepared_document_dependencies(&prepared_commit, |_| None);
    validate_prepared_for_provider(runtime, snapshot_sequence, &conflict_dependencies)?;
    Ok(PreparedQueuedParts {
        prepared_commit,
        conflict_dependencies,
        result,
        prepare_nanos: duration_nanos(started.elapsed()),
    })
}

pub(super) fn validate_prepared_for_provider(
    runtime: &TenantRuntime,
    snapshot_sequence: SequenceNumber,
    dependencies: &DependencySet,
) -> Result<()> {
    if dependencies.is_empty() || runtime.store.has_process_local_sequence_authority() {
        return Ok(());
    }
    runtime
        .store
        .stream_durable_journal(snapshot_sequence, 1)
        .map_err(|error| map_prepare_floor_error(error, snapshot_sequence))?;
    let commits = runtime
        .store
        .read_commit_log_from(SequenceNumber(snapshot_sequence.0.saturating_add(1)))?;
    if let Some(sequence) = commits.into_iter().find_map(|commit| {
        nimbus_core::commit_intersects_dependency_set(
            &commit,
            dependencies,
            &[],
            |table, document_id| runtime.store.get(table, &document_id),
        )
        .then_some(commit.sequence)
    }) {
        return Err(Error::retryable_conflict(
            "prepared mutation became stale before actor admission",
            Some(sequence),
        ));
    }
    Ok(())
}

/// Pure prepared-op validation: the view contains complete old/new images, so
/// evaluating document dependencies needs no storage access and cannot await.
#[cfg(test)]
fn validate_prepared_window_view(
    view: &super::write_log::WriteLogView,
    dependencies: &DependencySet,
) -> Result<()> {
    if let Some(sequence) = view.first_conflicting_sequence(dependencies, |_, _| {
        Err(Error::Internal(
            "full-image write-log validation unexpectedly requested storage".to_string(),
        ))
    }) {
        return Err(Error::retryable_conflict(
            "prepared mutation became stale before sequence assignment",
            Some(sequence),
        ));
    }
    Ok(())
}

fn map_prepare_floor_error(error: Error, snapshot_sequence: SequenceNumber) -> Error {
    match error {
        Error::InvalidInput(message) if message.contains("retention floor") => {
            Error::out_of_retention(
                format!(
                    "mutation snapshot {snapshot_sequence} is older than the durable commit-log retention horizon"
                ),
                None,
            )
        }
        other => other,
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn map_durable_journal_append_error(error: &Error) -> Error {
    match error {
        Error::InvalidInput(_) | Error::Storage { .. } => error.clone(),
        _ => Error::Internal(format!("durable journal append failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sequence: u64) -> CommitEntry {
        CommitEntry {
            sequence: SequenceNumber(sequence),
            timestamp: nimbus_core::Timestamp(sequence),
            writes: Vec::new(),
        }
    }

    #[test]
    fn retain_commits_through_applied_head_clips_recovered_batches() {
        let mut applied = vec![commit(10), commit(11), commit(12)];
        retain_commits_through_applied_head(&mut applied, SequenceNumber(11));
        assert_eq!(
            applied
                .iter()
                .map(|commit| commit.sequence)
                .collect::<Vec<_>>(),
            vec![SequenceNumber(10), SequenceNumber(11)]
        );

        retain_commits_through_applied_head(&mut applied, SequenceNumber(9));
        assert!(
            applied.is_empty(),
            "no downstream commit should remain when recovery reports an applied head before the batch"
        );

        let mut fully_visible = vec![commit(20), commit(21)];
        retain_commits_through_applied_head(&mut fully_visible, SequenceNumber(25));
        assert_eq!(
            fully_visible
                .iter()
                .map(|commit| commit.sequence)
                .collect::<Vec<_>>(),
            vec![SequenceNumber(20), SequenceNumber(21)]
        );
    }

    #[test]
    fn prepared_window_validation_includes_assigned_unpublished_writes() {
        let table = nimbus_core::TableName::new("tasks").expect("table should build");
        let table_id = nimbus_core::TableId::new();
        let document_id = nimbus_core::DocumentId::from_key("same").expect("id should build");
        let document = Document::with_id_at(
            document_id.clone(),
            table.clone(),
            serde_json::Map::new(),
            Timestamp(1),
        );
        let log = super::super::write_log::WriteLog::new(
            super::super::write_log::WriteLogConfig::from_env(),
            SequenceNumber(0),
            SequenceNumber(0),
        );
        log.stage_pending(
            [CommitEntry {
                sequence: SequenceNumber(1),
                timestamp: Timestamp(1),
                writes: vec![nimbus_core::WriteOp {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    op_type: nimbus_core::WriteOpType::Insert,
                    doc_id: document_id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                }],
            }],
            Timestamp(1),
        );
        let mut dependencies = DependencySet::default();
        dependencies.record_document(&table, &table_id, document_id);
        let super::super::write_log::ValidationSource::InMemory(view) = log
            .validation_source(SequenceNumber(0), SequenceNumber(0))
            .expect("pending window should cover the snapshot")
        else {
            panic!("pending window should validate in memory")
        };
        let error = validate_prepared_window_view(&view, &dependencies)
            .expect_err("the later prepare must see the pending write");
        assert_eq!(error.conflicting_sequence(), Some(SequenceNumber(1)));
    }
}
