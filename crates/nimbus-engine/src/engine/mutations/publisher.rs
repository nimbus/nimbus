use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{CommitEntry, Error, Retryability, SequenceNumber, TenantEventRecord};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::Engine;
use crate::engine::execution_units::{CommitFaultClient, labels};
use crate::tenant::{AssignedPublisherBatch, PublisherMessage, TenantRuntime};

const DEFAULT_PUBLISHER_RETRY_LIMIT: usize = 4;
const DEFAULT_PUBLISHER_RETRY_INITIAL_MS: u64 = 1;
const DEFAULT_PUBLISHER_RETRY_MAX_MS: u64 = 100;
#[cfg(test)]
const PUBLISHER_BATCH_BASE: usize = crate::config::COMMITTER_PUBLISHER_BATCH_BASE;
#[cfg(test)]
const DEFAULT_PUBLISHER_BATCH_MAX: usize = crate::config::COMMITTER_PUBLISHER_BATCH_MAX_DEFAULT;
#[cfg(test)]
const PUBLISHER_BATCH_MAX_ENV: &str = crate::config::COMMITTER_PUBLISHER_BATCH_MAX_ENV;
#[cfg(test)]
const PUBLISHER_COALESCE_ENV: &str = crate::config::COMMITTER_PUBLISHER_COALESCE_ENV;
// Once assignment has produced a base-sized burst, leave a short scheduling
// window for the next assigned suffix to arrive. Low-volume batches publish
// immediately, so this preserves singleton latency while recovering the
// adaptive actor's burst fsync amortization at the publisher boundary.
#[cfg(test)]
const DEFAULT_PUBLISHER_COALESCE_MICROS: u64 =
    crate::config::COMMITTER_PUBLISHER_COALESCE_DEFAULT_MICROS;

fn publisher_batch_policy() -> crate::config::BatchPolicy {
    crate::config::committer_publisher_batch_policy()
}

fn has_assignment_pressure(
    receiver_has_backlog: bool,
    assignment_backlog: usize,
    burst_threshold: usize,
) -> bool {
    receiver_has_backlog || assignment_backlog > burst_threshold
}

#[derive(Debug)]
pub(crate) enum PublishAttemptError {
    Definitive(Error),
    Ambiguous(Error),
}

pub(crate) struct PublishedBatch {
    pub(crate) applied: Vec<CommitEntry>,
    pub(crate) durable_append: Duration,
    pub(crate) apply: Duration,
    pub(crate) publish: Duration,
}

pub(crate) async fn run_ordered_publisher(
    runtime: Weak<TenantRuntime>,
    mut receiver: mpsc::Receiver<PublisherMessage>,
    engine_shutdown: CancellationToken,
    tenant_shutdown: CancellationToken,
) {
    // Publisher accumulation is independently tunable from actor admission:
    // NIMBUS_COMMITTER_PUBLISHER_BATCH_MAX defaults to 256 records and
    // NIMBUS_COMMITTER_PUBLISHER_COALESCE_MICROS defaults to 750 microseconds.
    let policy = publisher_batch_policy();
    let mut pending_message = None;
    loop {
        let message = if let Some(pending) = pending_message.take() {
            Some(pending)
        } else {
            tokio::select! {
                message = receiver.recv() => message,
                _ = engine_shutdown.cancelled() => {
                    receiver.close();
                    receiver.recv().await
                }
                _ = tenant_shutdown.cancelled() => {
                    receiver.close();
                    receiver.recv().await
                }
            }
        };
        let Some(message) = message else {
            break;
        };
        let batch = match message {
            PublisherMessage::Batch(batch) => batch,
            PublisherMessage::Barrier(completed) => {
                let _ = completed.send(());
                continue;
            }
            PublisherMessage::ResponseFence(responses) => {
                for response in responses {
                    response.complete();
                }
                continue;
            }
            PublisherMessage::SerialJob { job, drained } => {
                let Some(runtime) = runtime.upgrade() else {
                    job.fail(Error::Internal(
                        "tenant runtime stopped before serial publisher job".to_string(),
                    ));
                    let _ = drained.send(());
                    break;
                };
                run_serial_publisher_job(runtime, job).await;
                let _ = drained.send(());
                continue;
            }
        };
        let Some(runtime) = runtime.upgrade() else {
            let first_sequence = batch.first_sequence();
            let error = Error::Internal(
                "tenant runtime stopped before assigned batch publication".to_string(),
            );
            receiver.close();
            let mut completions = batch.defer_failure_after_recovery(first_sequence, &error);
            if let Some(pending) = pending_message.take() {
                completions.extend(defer_publisher_message_failure(
                    pending,
                    first_sequence,
                    &error,
                ));
            }
            while let Some(queued) = receiver.recv().await {
                completions.extend(defer_publisher_message_failure(
                    queued,
                    first_sequence,
                    &error,
                ));
            }
            for complete in completions {
                complete();
            }
            break;
        };
        let batch = accumulate_assigned_batches(
            batch,
            runtime.as_ref(),
            &mut receiver,
            &mut pending_message,
            &engine_shutdown,
            &tenant_shutdown,
            policy,
        )
        .await;

        // Opaque embedded commit jobs share this publisher and fence later
        // assignment until they drain. Re-anchor in case the preceding queue
        // item was such a job; provider jobs remain on the actor-owned serial
        // arm and never share this publisher.
        let expected_previous = runtime.durable_head();

        if let Err(invariant) = crate::tenant::validate_append_sequences(
            expected_previous,
            batch.records.iter().map(|record| record.sequence),
        ) {
            runtime.publisher_record_fatal_error();
            fail_definitive_batch_and_recover(
                runtime,
                batch,
                invariant,
                &mut receiver,
                &mut pending_message,
            )
            .await;
            continue;
        }

        runtime.set_mutation_worker_running(true);
        let result = publish_with_retry(runtime.clone(), &batch, expected_previous).await;
        runtime.set_mutation_worker_running(false);
        match result {
            Ok(published) => {
                complete_published_batch(runtime, batch, published);
            }
            Err(PublishAttemptError::Definitive(error)) => {
                fail_definitive_batch_and_recover(
                    runtime,
                    batch,
                    error,
                    &mut receiver,
                    &mut pending_message,
                )
                .await;
            }
            Err(PublishAttemptError::Ambiguous(error)) => {
                fail_and_restart(runtime, batch, error, &mut receiver, &mut pending_message).await;
                break;
            }
        }
    }
    if let Some(runtime) = runtime.upgrade() {
        runtime.close_committed_mutation_observers();
        runtime
            .wait_for_committed_mutation_observers_drained()
            .await;
    }
}

async fn run_serial_publisher_job(runtime: Arc<TenantRuntime>, job: crate::tenant::CommitterJob) {
    if runtime.eviction_started() {
        job.fail(runtime.durable_recovery_eviction_error());
        runtime.record_mutation_worker_failure();
        return;
    }
    runtime.set_mutation_worker_running(true);
    let tenant_id = runtime.tenant_id().clone();
    let (task, completed) = job.into_parts();
    let failed = tokio::task::spawn_blocking(move || crate::tenant::run_job(&tenant_id, task))
        .await
        .is_err();
    runtime.set_mutation_worker_running(false);
    let _ = completed.send(());
    if failed {
        runtime.record_mutation_worker_failure();
    }
}

async fn accumulate_assigned_batches(
    mut batch: AssignedPublisherBatch,
    runtime: &TenantRuntime,
    receiver: &mut mpsc::Receiver<PublisherMessage>,
    pending_message: &mut Option<PublisherMessage>,
    engine_shutdown: &CancellationToken,
    tenant_shutdown: &CancellationToken,
    policy: crate::config::BatchPolicy,
) -> AssignedPublisherBatch {
    while batch.records.len() < policy.max {
        match receiver.try_recv() {
            Ok(PublisherMessage::Batch(next))
                if batch.records.len().saturating_add(next.records.len()) <= policy.max =>
            {
                if let Err(next) = batch.try_merge(next) {
                    *pending_message = Some(PublisherMessage::Batch(*next));
                    return batch;
                }
            }
            Ok(message) => {
                *pending_message = Some(message);
                return batch;
            }
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                break;
            }
        }
    }

    // A sub-base batch with no queued mutation work represents low offered
    // concurrency and should not pay an extra scheduling delay. Under actor or
    // publisher pressure, leave assignment one short Tokio-time window to
    // produce a larger contiguous suffix before fsync.
    let assignment_pressure = has_assignment_pressure(
        !receiver.is_empty(),
        runtime.mutation_assignment_backlog_depth(),
        policy.base.saturating_mul(2),
    );
    if !assignment_pressure || batch.records.len() >= policy.max || policy.coalesce.is_zero() {
        return batch;
    }

    let deadline = tokio::time::Instant::now() + policy.coalesce;
    while batch.records.len() < policy.max {
        let message = tokio::select! {
            message = receiver.recv() => message,
            _ = tokio::time::sleep_until(deadline) => None,
            _ = engine_shutdown.cancelled() => {
                receiver.close();
                None
            }
            _ = tenant_shutdown.cancelled() => {
                receiver.close();
                None
            }
        };
        let Some(message) = message else {
            break;
        };
        match message {
            PublisherMessage::Batch(next)
                if batch.records.len().saturating_add(next.records.len()) <= policy.max =>
            {
                if let Err(next) = batch.try_merge(next) {
                    *pending_message = Some(PublisherMessage::Batch(*next));
                    break;
                }
            }
            message => {
                *pending_message = Some(message);
                break;
            }
        }
    }
    batch
}

async fn publish_with_retry(
    runtime: Arc<TenantRuntime>,
    batch: &AssignedPublisherBatch,
    expected_previous: SequenceNumber,
) -> std::result::Result<PublishedBatch, PublishAttemptError> {
    let retry_limit = crate::config::env_positive_usize(
        "NIMBUS_COMMITTER_PUBLISHER_RETRY_LIMIT",
        DEFAULT_PUBLISHER_RETRY_LIMIT,
    );
    let initial_ms = crate::config::env_nonnegative_u64(
        "NIMBUS_COMMITTER_PUBLISHER_RETRY_INITIAL_MS",
        DEFAULT_PUBLISHER_RETRY_INITIAL_MS,
    );
    let max_ms = crate::config::env_nonnegative_u64(
        "NIMBUS_COMMITTER_PUBLISHER_RETRY_MAX_MS",
        DEFAULT_PUBLISHER_RETRY_MAX_MS,
    )
    .max(initial_ms);

    for attempt in 1..=retry_limit {
        let runtime_for_attempt = runtime.clone();
        let records = batch.records.clone();
        let faults = batch.engine.commit_faults.clone();
        let attempt_result = tokio::task::spawn_blocking(move || {
            persist_assigned_batch_once(
                runtime_for_attempt.as_ref(),
                records.as_slice(),
                expected_previous,
                &faults,
            )
        })
        .await;
        let attempt_result = match attempt_result {
            Ok(result) => result,
            Err(error) => {
                runtime.publisher_record_ambiguous_error();
                return Err(PublishAttemptError::Ambiguous(Error::Internal(format!(
                    "publisher task panicked after persistence may have started; crash-and-replay required: {error}"
                ))));
            }
        };

        match attempt_result {
            Ok(published) => return Ok(published),
            Err(PublishAttemptError::Ambiguous(error)) => {
                runtime.publisher_record_ambiguous_error();
                error!(
                    tenant = %runtime.tenant_id(),
                    first_sequence = %batch.first_sequence(),
                    last_sequence = %batch.last_sequence(),
                    error = %error,
                    "publisher append outcome is ambiguous; restarting tenant runtime for replay"
                );
                return Err(PublishAttemptError::Ambiguous(Error::Internal(format!(
                    "ambiguous publisher append requires crash-and-replay: {error}"
                ))));
            }
            Err(PublishAttemptError::Definitive(error))
                if error.retryability() == Retryability::RetryableAfterBackoff =>
            {
                runtime.publisher_record_transient_error();
                if attempt == retry_limit {
                    return Err(PublishAttemptError::Definitive(error));
                }
                let shift = u32::try_from(attempt.saturating_sub(1))
                    .unwrap_or(u32::MAX)
                    .min(63);
                let delay =
                    Duration::from_millis(initial_ms.saturating_mul(1u64 << shift).min(max_ms));
                warn!(
                    tenant = %runtime.tenant_id(),
                    attempt,
                    retry_limit,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "transient publisher append failed; retrying the same ordered batch"
                );
                // Tokio time is the publisher scheduling clock so paused-time
                // tests can drive retry deterministically.
                tokio::time::sleep(delay).await;
            }
            Err(PublishAttemptError::Definitive(error)) => {
                runtime.publisher_record_fatal_error();
                return Err(PublishAttemptError::Definitive(error));
            }
        }
    }

    unreachable!("publisher retry loop always returns on its final attempt")
}

pub(crate) fn persist_assigned_batch_once(
    runtime: &TenantRuntime,
    records: &[TenantEventRecord],
    expected_previous: SequenceNumber,
    commit_faults: &CommitFaultClient,
) -> std::result::Result<PublishedBatch, PublishAttemptError> {
    crate::tenant::validate_append_sequences(
        expected_previous,
        records.iter().map(|record| record.sequence),
    )
    .map_err(PublishAttemptError::Definitive)?;
    debug_assert_eq!(
        runtime.durable_head(),
        expected_previous,
        "ordered publisher durable head must equal the prior completed batch"
    );

    let durable_append_started = Instant::now();
    let write_log_guard = runtime.arm_write_log_append();
    let provider_applied = match runtime.persist_fenced_provider_batch(expected_previous, records) {
        Ok(provider_applied) => provider_applied,
        Err(error) => {
            return Err(PublishAttemptError::Definitive(error));
        }
    };
    if !provider_applied && let Err(error) = runtime.store.append_durable_records_batch(records) {
        let progress = runtime.store.journal_progress();
        return match progress {
            Ok(progress) if progress.durable_head == expected_previous => {
                Err(PublishAttemptError::Definitive(error))
            }
            Ok(progress) => Err(PublishAttemptError::Ambiguous(Error::Internal(format!(
                "append returned {error}, but durable head advanced from {expected_previous} to {}",
                progress.durable_head
            )))),
            Err(progress_error) => Err(PublishAttemptError::Ambiguous(Error::Internal(format!(
                "append returned {error}, and durable progress could not be read: {progress_error}"
            )))),
        };
    }
    let last_sequence = records
        .last()
        .expect("publisher persistence batches must not be empty")
        .sequence;
    runtime.mark_durable_head(last_sequence);
    write_log_guard.disarm();
    let durable_append = durable_append_started.elapsed();

    let apply_started = Instant::now();
    if !provider_applied {
        runtime
            .store
            .check_fault(nimbus_storage::FaultPoint::JournalDurableAppendBeforeApply)
            .map_err(PublishAttemptError::Ambiguous)?;
        commit_faults
            .wait(labels::DURABLE_BEFORE_PUBLISH)
            .into_result()
            .map_err(PublishAttemptError::Ambiguous)?;
    }
    let applied_head = if provider_applied {
        last_sequence
    } else {
        match runtime.store.apply_durable_records_batch(records) {
            Ok(()) => runtime
                .store
                .applied_head_after_durable_apply(records)
                .map_err(PublishAttemptError::Ambiguous)?,
            Err(apply_error) => runtime
                .store
                .recover_durable_journal()
                .map(|progress| progress.applied_head)
                .map_err(|recovery_error| {
                    PublishAttemptError::Ambiguous(Error::Internal(format!(
                        "durable batch apply failed ({apply_error}) and recovery failed ({recovery_error})"
                    )))
                })?,
        }
    };
    let mut applied = records
        .iter()
        .map(TenantEventRecord::as_commit_entry)
        .filter(|commit| commit.sequence <= applied_head)
        .collect::<Vec<_>>();
    let published_frontier = runtime.publish_write_log_through(applied_head);
    applied.retain(|commit| commit.sequence <= published_frontier);
    runtime.invalidate_document_cache_for_commits(applied.iter());
    let apply = apply_started.elapsed();

    let publish_started = Instant::now();
    // Obligation #9c: the applied watermark is visible before this function
    // returns control to the task that performs subscription fan-out.
    runtime.mark_applied_head(published_frontier);
    commit_faults
        .wait(labels::POST_PUBLISH_PRE_FANOUT)
        .into_result()
        .map_err(PublishAttemptError::Ambiguous)?;
    let publish = publish_started.elapsed();

    Ok(PublishedBatch {
        applied,
        durable_append,
        apply,
        publish,
    })
}

#[cfg(test)]
impl Engine {
    pub(crate) fn persist_provider_publisher_barrier_for_testing(
        &self,
        tenant_id: &nimbus_core::TenantId,
        label: &str,
    ) -> nimbus_core::Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let commit_faults = self.commit_faults.clone();
        let label = label.to_string();
        runtime.submit_internal_committer(move || {
            runtime_for_commit.ensure_committer_lease_for_assignment()?;
            let previous = runtime_for_commit.durable_head();
            let sequence = crate::tenant::assign_and_validate(previous, 1)?[0];
            let record = TenantEventRecord::barrier(
                sequence,
                runtime_for_commit.assign_commit_timestamp(),
                label,
            )?;
            runtime_for_commit.stage_zero_write_record_in_write_log(&record);
            let result = persist_assigned_batch_once(
                &runtime_for_commit,
                std::slice::from_ref(&record),
                previous,
                &commit_faults,
            );
            if result.is_err() {
                runtime_for_commit.discard_unpersisted_write_log_suffix(sequence);
            }
            result.map(|_| ()).map_err(|error| match error {
                PublishAttemptError::Definitive(error) | PublishAttemptError::Ambiguous(error) => {
                    error
                }
            })
        })
    }
}

fn complete_published_batch(
    runtime: Arc<TenantRuntime>,
    mut batch: AssignedPublisherBatch,
    published: PublishedBatch,
) {
    batch.phases.durable_append = published.durable_append;
    batch.phases.apply = published.apply;
    batch.phases.publish = published.publish;
    let batch_size = u64::try_from(batch.records.len()).unwrap_or(u64::MAX);
    runtime
        .commit_phase_metrics()
        .record_journal_batch(batch_size);
    runtime.record_commit_phase_sample(
        "journal",
        batch_size,
        batch.phases,
        batch.sample_started_at.elapsed(),
    );

    let commit_identity = (published.applied.len() == 1).then(|| published.applied[0].clone());
    // `persist_assigned_batch_once` advanced applied_head before returning,
    // making this call the explicit #9c fan-out boundary.
    batch.engine.process_applied_commit_batch_fanout(
        runtime.clone(),
        &published.applied,
        commit_identity,
        true,
    );
    // Accept observer work before making commit success visible. Otherwise a
    // caller can race its observer fence ahead of this handoff and flush a
    // commit whose projection dispatch has not entered the queue yet. The send
    // is non-blocking; callbacks still run only on the separate dispatcher.
    batch
        .engine
        .enqueue_applied_commit_batch_observers(runtime, &published.applied);
    for deferred in batch.deferred {
        deferred.complete();
    }
    for pending in batch.responses {
        let _ = pending.response.send(Ok(pending.result));
    }
}

async fn fail_definitive_batch_and_recover(
    runtime: Arc<TenantRuntime>,
    batch: AssignedPublisherBatch,
    error: Error,
    receiver: &mut mpsc::Receiver<PublisherMessage>,
    pending_message: &mut Option<PublisherMessage>,
) {
    // Assignment and rollback share one gate. Once held, every batch already
    // assigned from the failed suffix is either in the local stash or the
    // channel, and the actor cannot assign a replacement suffix until durable
    // recovery has re-anchored the write log.
    let _recovery_guard = runtime.lock_publisher_assignment_recovery().await;
    let mut drained_messages = Vec::new();
    // Assigned-batch producers hold the same gate across reserve + send, so
    // no outstanding batch permit can deliver after this drain. Other message
    // kinds may reserve outside the gate, but they do not own an assigned
    // suffix and the live publisher loop will receive them after recovery.
    loop {
        let message = pending_message.take().or_else(|| receiver.try_recv().ok());
        let Some(message) = message else {
            break;
        };
        drained_messages.push(message);
    }

    // A response fence may sit between assigned batches. Drain through every
    // fence while assignment is excluded, then roll back from the earliest
    // assigned suffix observed anywhere in the drained queue.
    let first_sequence = drained_messages
        .iter()
        .filter_map(|message| match message {
            PublisherMessage::Batch(batch) => Some(batch.first_sequence()),
            PublisherMessage::Barrier(_)
            | PublisherMessage::ResponseFence(_)
            | PublisherMessage::SerialJob { .. } => None,
        })
        .fold(batch.first_sequence(), std::cmp::min);
    runtime.discard_unpersisted_write_log_suffix(first_sequence);
    runtime.record_mutation_worker_failure();
    match runtime
        .read_storage
        .execute(|store| store.recover_durable_journal())
        .await
    {
        Ok(progress) => runtime.publish_mutation_journal_progress_in_actor(progress),
        Err(recovery_error) => warn!(
            tenant = %runtime.tenant_id(),
            error = %error,
            recovery_error = %recovery_error,
            "definitive publisher batch failed before durable advance; journal recovery failed"
        ),
    }
    batch.fail_after_recovery(first_sequence, &error);
    for message in drained_messages {
        match message {
            PublisherMessage::Batch(batch) => batch.fail_after_recovery(first_sequence, &error),
            PublisherMessage::Barrier(completed) => {
                let _ = completed.send(());
            }
            PublisherMessage::ResponseFence(responses) => {
                for response in responses {
                    response.complete_after_recovery(first_sequence, &error);
                }
            }
            PublisherMessage::SerialJob { job, drained } => {
                run_serial_publisher_job(runtime.clone(), job).await;
                let _ = drained.send(());
            }
        }
    }
}

fn defer_publisher_message_failure(
    message: PublisherMessage,
    discarded_first_sequence: SequenceNumber,
    error: &Error,
) -> Vec<Box<dyn FnOnce() + Send + 'static>> {
    match message {
        PublisherMessage::Batch(batch) => {
            batch.defer_failure_after_recovery(discarded_first_sequence, error)
        }
        PublisherMessage::Barrier(completed) => {
            drop(completed);
            Vec::new()
        }
        PublisherMessage::ResponseFence(responses) => responses
            .into_iter()
            .map(|response| response.defer_failure(error))
            .collect(),
        PublisherMessage::SerialJob { job, drained } => {
            let complete_job = job.defer_failure(error.clone());
            vec![Box::new(move || {
                complete_job();
                let _ = drained.send(());
            })]
        }
    }
}

async fn fail_and_restart(
    runtime: Arc<TenantRuntime>,
    batch: AssignedPublisherBatch,
    error: Error,
    receiver: &mut mpsc::Receiver<PublisherMessage>,
    pending_message: &mut Option<PublisherMessage>,
) {
    let engine = batch.engine.clone();
    receiver.close();
    let mut queued_messages = Vec::new();
    if let Some(pending) = pending_message.take() {
        queued_messages.push(pending);
    }
    // `Receiver::close` prevents new reservations but permits reserved before
    // close remain valid. Awaiting `None` is the Tokio clean-shutdown pattern:
    // it drains messages sent by every outstanding permit before eviction can
    // wait on the operation guards carried by those messages.
    while let Some(queued) = receiver.recv().await {
        queued_messages.push(queued);
    }
    runtime.record_mutation_worker_failure();
    begin_durable_recovery_eviction(&runtime, &error);

    // Converting queued work to deferred completions drops every publisher
    // operation guard immediately. Run the sender-only completions before any
    // engine-wide gate acquisition, then explicitly drain queues whose actor
    // wake may have been consumed at shutdown.
    let first_sequence = batch.first_sequence();
    let mut failure_completions = batch.defer_failure_after_recovery(first_sequence, &error);
    for queued in queued_messages {
        failure_completions.extend(defer_publisher_message_failure(
            queued,
            first_sequence,
            &error,
        ));
    }
    for complete in failure_completions {
        complete();
    }
    runtime.fail_and_drain_mutation_queues(&error);
    runtime.close_committed_mutation_observers();
    runtime
        .wait_for_committed_mutation_observers_drained()
        .await;
    runtime.wait_for_operation_drain_for_eviction().await;

    finish_durable_recovery_eviction(engine, runtime).await;
}

pub(super) fn begin_durable_recovery_eviction(runtime: &TenantRuntime, error: &Error) {
    // Close admission before stopping the actor so any producer that already
    // entered the tenant is either rejected at the queue lock or included in
    // the explicit queue drain below.
    runtime.mark_deleting_for_eviction();
    runtime.shutdown_committer();
    runtime.shutdown_trigger_candidates();
    runtime.shutdown_trigger_execution();
    runtime.shutdown_subscription_delivery();
    runtime.subscriptions.shutdown_all(format!(
        "tenant committer stopped for durable recovery: {error}"
    ));
}

pub(super) async fn finish_durable_recovery_eviction(
    engine: Arc<Engine>,
    runtime: Arc<TenantRuntime>,
) {
    // No tenant operation guard crosses this engine-wide gate. Explicit
    // deletion takes the gate before waiting for operations, so this order is
    // what prevents the former AB-BA cycle.
    let _tenant_load_guard = engine.tenant_load_gate.lock().await;
    finish_durable_recovery_eviction_locked(engine.as_ref(), &runtime);
    drop(runtime);
    drop(_tenant_load_guard);
}

pub(super) fn finish_durable_recovery_eviction_blocking(
    engine: &Engine,
    runtime: Arc<TenantRuntime>,
) {
    let _tenant_load_guard = engine.lock_tenant_load_gate_blocking();
    finish_durable_recovery_eviction_locked(engine, &runtime);
    drop(runtime);
    drop(_tenant_load_guard);
}

pub(crate) fn finish_durable_recovery_eviction_locked(
    engine: &Engine,
    runtime: &Arc<TenantRuntime>,
) {
    let tenant_id = runtime.tenant_id().clone();
    engine
        .publisher_failure_diagnostics
        .write()
        .expect("publisher failure diagnostics lock should not be poisoned")
        .insert(tenant_id.clone(), runtime.publisher_error_counts());
    let removed = {
        let mut tenants = engine
            .tenants
            .write()
            .expect("tenant registry lock should not be poisoned");
        if tenants
            .get(&tenant_id)
            .is_some_and(|loaded| Arc::ptr_eq(loaded, runtime))
        {
            tenants.remove(&tenant_id)
        } else {
            None
        }
    };
    drop(removed);
    // Wake accessors only after the failed runtime is absent from the
    // registry. They will then serialize a fresh open behind the load gate.
    runtime.finish_eviction();
}

#[cfg(test)]
impl Engine {
    pub(crate) async fn evict_runtime_without_deleting_for_testing(
        self: &Arc<Self>,
        tenant_id: &nimbus_core::TenantId,
    ) -> nimbus_core::Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let error = Error::Internal(
            "test-only runtime eviction preserving durable tenant state".to_string(),
        );
        begin_durable_recovery_eviction(&runtime, &error);
        runtime.fail_and_drain_mutation_queues(&error);
        runtime.close_committed_mutation_observers();
        runtime
            .wait_for_committed_mutation_observers_drained()
            .await;
        runtime.wait_for_operation_drain_for_eviction().await;
        finish_durable_recovery_eviction(self.clone(), runtime).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure_test_batch(
        engine: Arc<crate::Engine>,
        runtime: &Arc<TenantRuntime>,
    ) -> (
        AssignedPublisherBatch,
        tokio::sync::oneshot::Receiver<nimbus_core::Result<crate::tenant::QueuedMutationResult>>,
    ) {
        let (response, result) = tokio::sync::oneshot::channel();
        (
            AssignedPublisherBatch {
                engine,
                records: Arc::new(vec![
                    nimbus_core::TenantEventRecord::barrier(
                        SequenceNumber(1),
                        nimbus_core::Timestamp(1),
                        "publisher failure-path test".to_string(),
                    )
                    .expect("failure-test record should build"),
                ]),
                responses: vec![crate::tenant::PendingPublisherResponse {
                    _operation: runtime
                        .enter_operation(runtime.tenant_id())
                        .expect("failure-test operation should enter"),
                    response: crate::tenant::MutationResponseSender::new(response),
                    result: crate::tenant::QueuedMutationResult::Scheduled(false),
                }],
                deferred: Vec::new(),
                phases: crate::engine::CommitPhaseDurations::default(),
                sample_started_at: Instant::now(),
            },
            result,
        )
    }

    #[test]
    fn publisher_current_batch_responses_do_not_create_assignment_pressure() {
        let threshold = PUBLISHER_BATCH_BASE * 2;
        assert!(!has_assignment_pressure(false, 0, threshold));
        assert!(!has_assignment_pressure(false, threshold, threshold));
        assert!(has_assignment_pressure(true, 0, threshold));
        assert!(has_assignment_pressure(false, threshold + 1, threshold));
    }

    #[test]
    fn publisher_batch_policy_uses_independent_env_keys_and_documented_defaults() {
        assert_ne!(PUBLISHER_BATCH_MAX_ENV, "NIMBUS_MUTATION_JOURNAL_BATCH_MAX");
        assert_ne!(
            PUBLISHER_COALESCE_ENV,
            "NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS"
        );
        let policy = crate::config::BatchPolicy::new(
            PUBLISHER_BATCH_BASE,
            DEFAULT_PUBLISHER_BATCH_MAX,
            DEFAULT_PUBLISHER_COALESCE_MICROS,
        );
        assert_eq!(policy.base, PUBLISHER_BATCH_BASE);
        assert_eq!(policy.max, DEFAULT_PUBLISHER_BATCH_MAX);
        assert_eq!(
            policy.coalesce,
            Duration::from_micros(DEFAULT_PUBLISHER_COALESCE_MICROS)
        );
    }

    #[tokio::test]
    async fn fail_and_restart_completes_stashed_messages_with_the_typed_error() {
        let data_dir = tempfile::tempdir().expect("stashed-message tempdir should build");
        let engine = Arc::new(crate::Engine::new(data_dir.path()).expect("engine should create"));
        let tenant_id = nimbus_core::TenantId::new("stashed-message").expect("tenant id");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let runtime = engine
            .get_existing_tenant(&tenant_id)
            .expect("runtime should load");
        let (batch_response, mut batch_result) = tokio::sync::oneshot::channel();
        let batch = AssignedPublisherBatch {
            engine: engine.clone(),
            records: Arc::new(vec![
                nimbus_core::TenantEventRecord::barrier(
                    SequenceNumber(1),
                    nimbus_core::Timestamp(1),
                    "stashed publisher failure-path test".to_string(),
                )
                .expect("stashed failure-test record should build"),
            ]),
            responses: vec![crate::tenant::PendingPublisherResponse {
                _operation: runtime
                    .enter_operation(&tenant_id)
                    .expect("operation should enter"),
                response: crate::tenant::MutationResponseSender::new(batch_response),
                result: crate::tenant::QueuedMutationResult::Scheduled(false),
            }],
            deferred: Vec::new(),
            phases: crate::engine::CommitPhaseDurations::default(),
            sample_started_at: Instant::now(),
        };
        let typed = Error::rejected_before_execution("typed stashed-message failure");
        let mut completions = defer_publisher_message_failure(
            PublisherMessage::Batch(batch),
            SequenceNumber(1),
            &typed,
        );
        assert!(matches!(
            batch_result.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        let response_slot = Arc::new(std::sync::Mutex::new(None));
        let response_slot_for_rejection = response_slot.clone();
        let (job, mut completed) = crate::tenant::CommitterJob::new(
            || panic!("a failed stashed serial job must not execute"),
            move |error| {
                *response_slot_for_rejection
                    .lock()
                    .expect("rejection slot should lock") = Some(error);
            },
        );
        let (drained, mut drain_completed) = tokio::sync::oneshot::channel();
        completions.extend(defer_publisher_message_failure(
            PublisherMessage::SerialJob { job, drained },
            SequenceNumber(1),
            &typed,
        ));
        assert!(matches!(
            completed.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            drain_completed.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        for complete in completions {
            complete();
        }

        assert!(matches!(
            batch_result.await.expect("batch response should send"),
            Err(Error::RejectedBeforeExecution { .. })
        ));
        completed.await.expect("job completion should send");
        drain_completed.await.expect("drain completion should send");
        assert!(matches!(
            response_slot
                .lock()
                .expect("rejection slot should lock")
                .as_ref(),
            Some(Error::RejectedBeforeExecution { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fail_and_restart_drains_batch_sent_by_permit_reserved_before_close() {
        let data_dir = tempfile::tempdir().expect("late-permit tempdir should build");
        let engine = Arc::new(crate::Engine::new(data_dir.path()).expect("engine should create"));
        let tenant_id = nimbus_core::TenantId::new("late-permit-eviction").expect("tenant id");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let runtime = engine
            .get_existing_tenant(&tenant_id)
            .expect("runtime should load");
        let runtime_before = Arc::as_ptr(&runtime) as usize;
        let (sender, receiver) = mpsc::channel(1);
        let permit = sender
            .reserve()
            .await
            .expect("late batch should reserve before receiver close");
        let (failed_batch, failed_result) = failure_test_batch(engine.clone(), &runtime);
        let (late_batch, late_result) = failure_test_batch(engine.clone(), &runtime);
        let typed = Error::Internal("typed late-permit replay failure".to_string());

        let eviction = tokio::spawn({
            let runtime = runtime.clone();
            let typed = typed.clone();
            async move {
                let mut receiver = receiver;
                let mut pending = None;
                fail_and_restart(runtime, failed_batch, typed, &mut receiver, &mut pending).await;
            }
        });
        sender.closed().await;
        assert!(
            !eviction.is_finished(),
            "eviction must wait for the already-reserved permit"
        );
        permit.send(PublisherMessage::Batch(late_batch));
        drop(sender);

        tokio::time::timeout(Duration::from_secs(5), eviction)
            .await
            .expect("eviction should drain the late permit batch")
            .expect("eviction task should join");
        for result in [failed_result, late_result] {
            let error = match result.await.expect("typed failure should be sent") {
                Err(error) => error,
                Ok(_) => panic!("evicted batch should fail"),
            };
            assert!(
                matches!(error, Error::Internal(ref message) if message == "typed late-permit replay failure")
            );
        }

        let reopened = tokio::time::timeout(
            Duration::from_secs(5),
            engine.get_existing_tenant_async(&tenant_id),
        )
        .await
        .expect("tenant reload should not hang behind a leaked operation guard")
        .expect("tenant should reopen after eviction");
        assert_ne!(Arc::as_ptr(&reopened) as usize, runtime_before);
    }
}
