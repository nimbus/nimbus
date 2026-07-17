use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{CommitEntry, Error, Result, Retryability, SequenceNumber, TenantEventRecord};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::Engine;
use crate::engine::execution_units::{CommitFaultClient, labels};
use crate::tenant::{AssignedPublisherBatch, PublisherMessage, TenantRuntime};

const DEFAULT_PUBLISHER_RETRY_LIMIT: usize = 4;
const DEFAULT_PUBLISHER_RETRY_INITIAL_MS: u64 = 1;
const DEFAULT_PUBLISHER_RETRY_MAX_MS: u64 = 100;
const PUBLISHER_BATCH_BASE: usize = 32;
const DEFAULT_PUBLISHER_BATCH_MAX: usize = 256;
// Once assignment has produced a base-sized burst, leave a short scheduling
// window for the next assigned suffix to arrive. Low-volume batches publish
// immediately, so this preserves singleton latency while recovering the
// adaptive actor's burst fsync amortization at the publisher boundary.
const DEFAULT_PUBLISHER_COALESCE_MICROS: u64 = 750;

#[derive(Clone, Copy)]
struct PublisherBatchPolicy {
    base: usize,
    max: usize,
    coalesce: Duration,
}

impl PublisherBatchPolicy {
    fn from_env() -> Self {
        Self {
            base: PUBLISHER_BATCH_BASE,
            max: env_positive_usize(
                "NIMBUS_MUTATION_JOURNAL_BATCH_MAX",
                DEFAULT_PUBLISHER_BATCH_MAX,
            )
            .max(PUBLISHER_BATCH_BASE),
            coalesce: Duration::from_micros(env_nonnegative_u64(
                "NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS",
                DEFAULT_PUBLISHER_COALESCE_MICROS,
            )),
        }
    }
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
                    let (task, completed) = job.into_parts();
                    drop(task);
                    let _ = completed.send(());
                    let _ = drained.send(());
                    break;
                };
                run_serial_publisher_job(runtime, job).await;
                let _ = drained.send(());
                continue;
            }
        };
        let Some(runtime) = runtime.upgrade() else {
            batch.fail(&Error::Internal(
                "tenant runtime stopped before assigned batch publication".to_string(),
            ));
            break;
        };
        let batch = accumulate_assigned_batches(
            batch,
            runtime.as_ref(),
            &mut receiver,
            &mut pending_message,
            &engine_shutdown,
            &tenant_shutdown,
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
            fail_and_restart(runtime, batch, invariant, &mut receiver);
            break;
        }

        runtime.set_mutation_worker_running(true);
        let result = publish_with_retry(runtime.clone(), &batch, expected_previous).await;
        runtime.set_mutation_worker_running(false);
        match result {
            Ok(published) => {
                complete_published_batch(runtime, batch, published);
            }
            Err(error) => {
                fail_and_restart(runtime, batch, error, &mut receiver);
                break;
            }
        }
    }
}

async fn run_serial_publisher_job(runtime: Arc<TenantRuntime>, job: crate::tenant::CommitterJob) {
    runtime.set_mutation_worker_running(true);
    let (task, completed) = job.into_parts();
    let failed = tokio::task::spawn_blocking(move || crate::tenant::run_job(task))
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
) -> AssignedPublisherBatch {
    let policy = PublisherBatchPolicy::from_env();

    while batch.records.len() < policy.max {
        match receiver.try_recv() {
            Ok(PublisherMessage::Batch(next))
                if batch.records.len().saturating_add(next.records.len()) <= policy.max =>
            {
                batch.merge(next);
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
    let burst_threshold = policy.base.saturating_mul(2);
    let assignment_pressure = !receiver.is_empty()
        || runtime.mutation_assignment_backlog_depth() > burst_threshold
        || runtime.mutation_journal_stats().pending_response_count
            > u64::try_from(burst_threshold).unwrap_or(u64::MAX);
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
                batch.merge(next);
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
) -> Result<PublishedBatch> {
    let retry_limit = env_positive_usize(
        "NIMBUS_COMMITTER_PUBLISHER_RETRY_LIMIT",
        DEFAULT_PUBLISHER_RETRY_LIMIT,
    );
    let initial_ms = env_nonnegative_u64(
        "NIMBUS_COMMITTER_PUBLISHER_RETRY_INITIAL_MS",
        DEFAULT_PUBLISHER_RETRY_INITIAL_MS,
    );
    let max_ms = env_nonnegative_u64(
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
        .await
        .map_err(|error| Error::Internal(format!("publisher task panicked: {error}")))?;

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
                return Err(Error::Internal(format!(
                    "ambiguous publisher append requires crash-and-replay: {error}"
                )));
            }
            Err(PublishAttemptError::Definitive(error))
                if error.retryability() == Retryability::RetryableAfterBackoff =>
            {
                runtime.publisher_record_transient_error();
                if attempt == retry_limit {
                    runtime.publisher_record_ambiguous_error();
                    return Err(Error::Internal(format!(
                        "publisher transient append failure exhausted {retry_limit} attempts; crash-and-replay required: {error}"
                    )));
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
                return Err(error);
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
    if let Err(error) = runtime.store.append_durable_records_batch(records) {
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
    runtime
        .store
        .check_fault(nimbus_storage::FaultPoint::JournalDurableAppendBeforeApply)
        .map_err(PublishAttemptError::Ambiguous)?;
    commit_faults
        .wait(labels::DURABLE_BEFORE_PUBLISH)
        .into_result()
        .map_err(PublishAttemptError::Ambiguous)?;
    let applied_head = match runtime.store.apply_durable_records_batch(records) {
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
    for pending in batch.responses {
        let _ = pending.response.send(Ok(pending.result));
    }
    // Observers are synchronous hooks and may enqueue another mutation. Do
    // not run them on the publisher task: a nested actor job must be able to
    // enqueue and complete its publisher barrier without waiting on the task
    // that invoked the observer. Responses already precede observers on the
    // serial path, so this continuation preserves that contract.
    let observer_engine = batch.engine.clone();
    let observer_runtime = runtime;
    let observer_applied = published.applied;
    batch
        .engine
        .spawn_background("committed_mutation_observers", async move {
            observer_engine
                .notify_applied_commit_batch_observers(observer_runtime, &observer_applied);
        });
}

fn fail_and_restart(
    runtime: Arc<TenantRuntime>,
    batch: AssignedPublisherBatch,
    error: Error,
    receiver: &mut mpsc::Receiver<PublisherMessage>,
) {
    let engine = batch.engine.clone();
    receiver.close();
    let mut queued_messages = Vec::new();
    while let Ok(queued) = receiver.try_recv() {
        queued_messages.push(queued);
    }
    runtime.record_mutation_worker_failure();
    runtime.shutdown_committer();
    engine.evict_failed_tenant_runtime(&runtime, &error);

    // Complete callers only after eviction, so a caller's next access cannot
    // race back onto the failed runtime instead of reopening and replaying its
    // durable tail.
    batch.fail(&error);
    for queued in queued_messages {
        match queued {
            PublisherMessage::Batch(queued) => queued.fail(&error),
            PublisherMessage::Barrier(completed) => drop(completed),
            PublisherMessage::ResponseFence(responses) => {
                for response in responses {
                    response.fail(&error);
                }
            }
            PublisherMessage::SerialJob { job, drained } => {
                let (task, completed) = job.into_parts();
                drop(task);
                let _ = completed.send(());
                let _ = drained.send(());
            }
        }
    }
}

impl Engine {
    fn evict_failed_tenant_runtime(&self, runtime: &Arc<TenantRuntime>, error: &Error) {
        let tenant_id = runtime.tenant_id().clone();
        self.publisher_failure_diagnostics
            .write()
            .expect("publisher failure diagnostics lock should not be poisoned")
            .insert(tenant_id.clone(), runtime.publisher_error_counts());
        let removed = {
            let mut tenants = self
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
        if let Some(runtime) = removed {
            runtime.shutdown_trigger_candidates();
            runtime.shutdown_trigger_execution();
            runtime.shutdown_subscription_delivery();
            runtime.subscriptions.shutdown_all(format!(
                "tenant committer stopped for durable recovery: {error}"
            ));
        }
    }
}

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_nonnegative_u64(key: &str, default: u64) -> u64 {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}
