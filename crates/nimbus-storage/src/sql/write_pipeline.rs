//! Provider SQL write-pipeline policy.
//!
//! This module owns the pieces that PostgreSQL and MySQL must not reimplement:
//! contiguous bounded journal-batch preparation, exact per-tenant counters,
//! and ordered bounded future admission. The dialect adapters still own SQL
//! syntax. PostgreSQL uses the ordered runner with two shared-connection
//! operations; MySQL deliberately stays at one mutable-connection operation.
//!
//! The ordering contract is: the caller awaits its lease CAS and applied-prefix
//! check, then passes the journal insert first and apply second. Futures are
//! admitted in input order, results are observed in input order, and no further
//! future is polled after the first ordered error. Dropping the runner drops all
//! remaining futures; the owning write transaction must then roll back.

// The ordered-runner imports follow `run_ordered_bounded`'s gate.
#[cfg(any(test, feature = "postgres"))]
use std::collections::VecDeque;
#[cfg(any(test, feature = "postgres"))]
use std::future::Future;
#[cfg(any(test, feature = "postgres"))]
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[cfg(any(test, feature = "postgres"))]
use futures::stream::{FuturesOrdered, StreamExt};
use nimbus_core::{Error, Result, SequenceNumber, TenantEventRecord};

use crate::diagnostics::ProviderWritePipelineDiagnostic;

/// Matches the largest supported ordered-publisher batch while leaving ample
/// room below MySQL's prepared-statement parameter limit (two parameters per
/// journal row). Oversize callers fail before issuing SQL.
pub(crate) const MAX_JOURNAL_RECORDS_PER_STATEMENT: usize = 4_096;
/// Per-dialect in-flight ceilings. Each is gated to its own provider: they are
/// tuning constants for one adapter, not shared policy.
#[cfg(feature = "postgres")]
pub(crate) const POSTGRES_MAX_IN_FLIGHT_OPERATIONS: usize = 2;
#[cfg(feature = "mysql")]
pub(crate) const MYSQL_MAX_IN_FLIGHT_OPERATIONS: usize = 1;

/// The ordered runner and its future type are PostgreSQL's admission policy --
/// MySQL stays at one operation and issues its statements directly. `test` is
/// in the gate because this module's own unit tests cover the runner, so they
/// still build in a MySQL-only configuration.
#[cfg(any(test, feature = "postgres"))]
pub(crate) type OrderedSqlFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

/// Serialized, contiguous durable-journal input shared by the PostgreSQL and
/// MySQL dialect adapters.
#[derive(Debug)]
pub(crate) struct PreparedJournalBatch {
    sequences: Vec<u64>,
    payloads: Vec<Vec<u8>>,
}

impl PreparedJournalBatch {
    pub(crate) fn prepare(latest: SequenceNumber, records: &[TenantEventRecord]) -> Result<Self> {
        if records.is_empty() {
            return Err(Error::InvalidInput(
                "SQL journal batch must contain at least one record".to_string(),
            ));
        }
        if records.len() > MAX_JOURNAL_RECORDS_PER_STATEMENT {
            return Err(Error::InvalidInput(format!(
                "SQL journal batch contains {} records; maximum is {}",
                records.len(),
                MAX_JOURNAL_RECORDS_PER_STATEMENT
            )));
        }

        let mut expected = latest.0.checked_add(1).ok_or_else(|| {
            Error::Internal("durable journal sequence space exhausted".to_string())
        })?;
        let mut sequences = Vec::with_capacity(records.len());
        let mut payloads = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            if record.sequence.0 != expected {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {expected}, got {}",
                    record.sequence.0
                )));
            }
            sequences.push(record.sequence.0);
            payloads.push(crate::commit_log::serialize_tenant_event_record(record)?);
            if index + 1 < records.len() {
                expected = expected.checked_add(1).ok_or_else(|| {
                    Error::Internal("durable journal sequence space exhausted".to_string())
                })?;
            }
        }
        Ok(Self {
            sequences,
            payloads,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.sequences.len()
    }

    pub(crate) fn sequences(&self) -> &[u64] {
        &self.sequences
    }

    pub(crate) fn payloads(&self) -> &[Vec<u8>] {
        &self.payloads
    }
}

pub(crate) struct SqlWritePipelineMetrics {
    adapter: &'static str,
    configured_max_in_flight: u64,
    batch_attempt_count: AtomicU64,
    journal_record_count: AtomicU64,
    journal_statement_count: AtomicU64,
    provider_operation_count: AtomicU64,
    current_in_flight: AtomicU64,
    max_observed_in_flight: AtomicU64,
    cancellation_count: AtomicU64,
    error_count: AtomicU64,
    elapsed_nanos: AtomicU64,
}

impl SqlWritePipelineMetrics {
    pub(crate) fn new(adapter: &'static str, configured_max_in_flight: usize) -> Self {
        Self {
            adapter,
            configured_max_in_flight: u64::try_from(configured_max_in_flight).unwrap_or(u64::MAX),
            batch_attempt_count: AtomicU64::new(0),
            journal_record_count: AtomicU64::new(0),
            journal_statement_count: AtomicU64::new(0),
            provider_operation_count: AtomicU64::new(0),
            current_in_flight: AtomicU64::new(0),
            max_observed_in_flight: AtomicU64::new(0),
            cancellation_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            elapsed_nanos: AtomicU64::new(0),
        }
    }

    pub(crate) fn record_batch_attempt(&self, records: usize) {
        self.batch_attempt_count.fetch_add(1, Ordering::Relaxed);
        self.journal_record_count.fetch_add(
            u64::try_from(records).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub(crate) fn record_journal_statement(&self) {
        self.journal_statement_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_error(&self, error: &Error) {
        if matches!(error, Error::Cancelled) {
            self.cancellation_count.fetch_add(1, Ordering::Relaxed);
        }
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_elapsed(&self, started: Instant) {
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.elapsed_nanos.fetch_add(elapsed, Ordering::Relaxed);
    }

    pub(crate) fn operation_started(&self) -> InFlightOperation<'_> {
        self.provider_operation_count
            .fetch_add(1, Ordering::Relaxed);
        let current = self.current_in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        self.max_observed_in_flight
            .fetch_max(current, Ordering::Relaxed);
        InFlightOperation { metrics: self }
    }

    pub(crate) fn snapshot(&self) -> ProviderWritePipelineDiagnostic {
        ProviderWritePipelineDiagnostic {
            adapter: self.adapter.to_string(),
            configured_max_in_flight: self.configured_max_in_flight,
            batch_attempt_count: self.batch_attempt_count.load(Ordering::Relaxed),
            journal_record_count: self.journal_record_count.load(Ordering::Relaxed),
            journal_statement_count: self.journal_statement_count.load(Ordering::Relaxed),
            provider_operation_count: self.provider_operation_count.load(Ordering::Relaxed),
            max_observed_in_flight: self.max_observed_in_flight.load(Ordering::Relaxed),
            cancellation_count: self.cancellation_count.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            elapsed_nanos: self.elapsed_nanos.load(Ordering::Relaxed),
        }
    }
}

pub(crate) struct InFlightOperation<'a> {
    metrics: &'a SqlWritePipelineMetrics,
}

impl Drop for InFlightOperation<'_> {
    fn drop(&mut self) {
        self.metrics
            .current_in_flight
            .fetch_sub(1, Ordering::AcqRel);
    }
}

/// Polls provider operations concurrently up to `max_in_flight`, while
/// yielding their results in input order. `FuturesOrdered` is essential here:
/// an error from a later statement may complete early, but cannot replace an
/// earlier statement's result.
#[cfg(any(test, feature = "postgres"))]
pub(crate) async fn run_ordered_bounded<'a>(
    metrics: &'a SqlWritePipelineMetrics,
    max_in_flight: usize,
    check_cancel: &'a (dyn Fn() -> Result<()> + Send),
    operations: impl IntoIterator<Item = OrderedSqlFuture<'a>>,
) -> Result<()> {
    if max_in_flight == 0
        || u64::try_from(max_in_flight).unwrap_or(u64::MAX) > metrics.configured_max_in_flight
    {
        return Err(Error::Internal(format!(
            "SQL write pipeline in-flight bound {max_in_flight} is outside configured range 1..={}",
            metrics.configured_max_in_flight
        )));
    }

    let started = Instant::now();
    let mut pending = operations.into_iter().collect::<VecDeque<_>>();
    let mut running = FuturesOrdered::new();

    loop {
        while running.len() < max_in_flight {
            let Some(operation) = pending.pop_front() else {
                break;
            };
            running.push_back(Box::pin(async move {
                check_cancel()?;
                let _in_flight = metrics.operation_started();
                operation.await
            }) as OrderedSqlFuture<'a>);
        }

        let Some(result) = running.next().await else {
            metrics.record_elapsed(started);
            return Ok(());
        };
        if let Err(error) = result {
            metrics.record_error(&error);
            metrics.record_elapsed(started);
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::sync::{Notify, Semaphore, mpsc};

    use super::*;

    fn operation<'a>(future: impl Future<Output = Result<()>> + 'a) -> OrderedSqlFuture<'a> {
        Box::pin(future)
    }

    fn barriers(start: u64, count: usize) -> Vec<TenantEventRecord> {
        (0..count)
            .map(|offset| {
                let sequence = start
                    .checked_add(u64::try_from(offset).expect("test offset fits u64"))
                    .expect("test sequence should not overflow");
                TenantEventRecord::barrier(
                    SequenceNumber(sequence),
                    nimbus_core::Timestamp(sequence),
                    format!("pipeline-unit-{sequence}"),
                )
                .expect("barrier record should build")
            })
            .collect()
    }

    #[test]
    fn sql_journal_batch_rejects_empty_oversize_noncontiguous_and_exhausted_inputs() {
        let empty = PreparedJournalBatch::prepare(SequenceNumber(0), &[])
            .expect_err("empty batch must be rejected");
        assert!(matches!(empty, Error::InvalidInput(_)));

        let oversized = barriers(1, MAX_JOURNAL_RECORDS_PER_STATEMENT + 1);
        let oversized_error = PreparedJournalBatch::prepare(SequenceNumber(0), &oversized)
            .expect_err("oversize batch must be rejected before serialization");
        assert!(matches!(oversized_error, Error::InvalidInput(_)));

        let mut noncontiguous = barriers(1, 2);
        noncontiguous[1] = TenantEventRecord::barrier(
            SequenceNumber(3),
            nimbus_core::Timestamp(3),
            "pipeline-unit-gap".to_string(),
        )
        .expect("barrier record should build");
        let gap = PreparedJournalBatch::prepare(SequenceNumber(0), &noncontiguous)
            .expect_err("noncontiguous batch must be rejected");
        assert!(gap.to_string().contains("expected sequence 2, got 3"));

        let exhausted = PreparedJournalBatch::prepare(SequenceNumber(u64::MAX), &barriers(1, 1))
            .expect_err("exhausted sequence space must be rejected");
        assert!(exhausted.to_string().contains("sequence space exhausted"));
    }

    #[tokio::test]
    async fn sql_pipeline_preserves_statement_order() {
        let metrics = SqlWritePipelineMetrics::new("fake", 2);
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let first_order = order.clone();
        let second_order = order.clone();
        run_ordered_bounded(
            &metrics,
            2,
            &|| Ok(()),
            [
                operation(async move {
                    first_order.lock().expect("order lock").push("first");
                    Ok(())
                }),
                operation(async move {
                    second_order.lock().expect("order lock").push("second");
                    Ok(())
                }),
            ],
        )
        .await
        .expect("ordered operations should pass");
        assert_eq!(*order.lock().expect("order lock"), ["first", "second"]);
    }

    #[tokio::test]
    async fn sql_pipeline_reports_first_error() {
        let metrics = SqlWritePipelineMetrics::new("fake", 2);
        let second_started = Arc::new(Notify::new());
        let first_wait = second_started.clone();
        let second_signal = second_started.clone();
        let error = run_ordered_bounded(
            &metrics,
            2,
            &|| Ok(()),
            [
                operation(async move {
                    first_wait.notified().await;
                    Err(Error::Internal("first statement failed".to_string()))
                }),
                operation(async move {
                    second_signal.notify_one();
                    Err(Error::Internal("second statement failed".to_string()))
                }),
            ],
        )
        .await
        .expect_err("pipeline should return an error");
        assert!(error.to_string().contains("first statement failed"));
    }

    #[tokio::test]
    async fn sql_pipeline_bounds_in_flight_statements() {
        let metrics = Arc::new(SqlWritePipelineMetrics::new("fake", 2));
        let permits = Arc::new(Semaphore::new(0));
        let (admitted_tx, mut admitted_rx) = mpsc::unbounded_channel();
        let operations = (0..5)
            .map(|_| {
                let permits = permits.clone();
                let admitted_tx = admitted_tx.clone();
                operation(async move {
                    admitted_tx
                        .send(())
                        .expect("admission observer should remain open");
                    let permit = permits.acquire().await.expect("semaphore should stay open");
                    permit.forget();
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        drop(admitted_tx);
        let run = run_ordered_bounded(metrics.as_ref(), 2, &|| Ok(()), operations);
        let inspect = async {
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                for admitted in 1..=2 {
                    admitted_rx.recv().await.unwrap_or_else(|| {
                        panic!(
                            "pipeline closed after {admitted_minus_one} admissions; expected two",
                            admitted_minus_one = admitted - 1
                        )
                    });
                }
            })
            .await
            .expect("pipeline did not admit exactly two provider operations within one second");
            let blocked = metrics.snapshot();
            assert_eq!(blocked.provider_operation_count, 2);
            assert_eq!(blocked.max_observed_in_flight, 2);
            permits.add_permits(5);
        };
        let (result, ()) = tokio::join!(run, inspect);
        result.expect("pipeline should complete");
        let complete = metrics.snapshot();
        assert_eq!(complete.provider_operation_count, 5);
        assert_eq!(complete.max_observed_in_flight, 2);
    }

    #[tokio::test]
    async fn sql_pipeline_cancellation_stops_new_admission() {
        let metrics = SqlWritePipelineMetrics::new("fake", 1);
        let cancelled = AtomicBool::new(false);
        let polled = AtomicUsize::new(0);
        let first_cancelled = &cancelled;
        let first_polled = &polled;
        let second_polled = &polled;
        let error = run_ordered_bounded(
            &metrics,
            1,
            &|| {
                if cancelled.load(Ordering::Acquire) {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
            [
                operation(async move {
                    first_polled.fetch_add(1, Ordering::Relaxed);
                    first_cancelled.store(true, Ordering::Release);
                    Ok(())
                }),
                operation(async move {
                    second_polled.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }),
            ],
        )
        .await
        .expect_err("cancellation should stop later admission");
        assert!(matches!(error, Error::Cancelled));
        assert_eq!(polled.load(Ordering::Relaxed), 1);
        let diagnostic = metrics.snapshot();
        assert_eq!(diagnostic.cancellation_count, 1);
        assert_eq!(diagnostic.error_count, 1);
    }
}
