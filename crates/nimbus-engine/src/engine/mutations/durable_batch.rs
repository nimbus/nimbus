//! The single transcription of the queued durable-commit sequence.
//!
//! Exactly two engine owners persist an assigned queued batch: the ordered
//! publisher and the serial kill-switch arm. Before this module existed each
//! carried its own copy of the append → durable-head → fault-window →
//! apply/recover → publish-frontier ordering, and the copies had already
//! drifted (the serial arm skipped the `DURABLE_BEFORE_PUBLISH` window when a
//! fenced provider applied, and discarded apply-error context on recovery).
//! Both owners now call [`persist_and_apply_assigned_batch`], so the sequence
//! is compiler-linked and cannot drift again. Canonical semantics follow the
//! ordered publisher: the durable-before-publish fault window applies on
//! every route, and a failed apply preserves its error context alongside any
//! recovery failure.
//!
//! Sequence-number validation stays with the callers (it is already one
//! shared function), as do route-specific error classification, response
//! plumbing, fan-out, and metrics.

use std::time::{Duration, Instant};

use nimbus_core::{CommitEntry, Error, TenantEventRecord};

use crate::engine::execution_units::{CommitFaultClient, labels};
use crate::tenant::TenantRuntime;

pub(crate) struct DurableBatchOutcome {
    /// Commit entries at or below the published frontier, in sequence order.
    pub(crate) applied: Vec<CommitEntry>,
    pub(crate) durable_append: Duration,
    pub(crate) apply: Duration,
}

pub(crate) enum DurableBatchFailure {
    /// The fenced or fallback persistence call failed before the durable head
    /// advanced; the caller owns route-specific classification. `fenced`
    /// distinguishes the fenced provider call from the fallback append.
    Persistence { fenced: bool, error: Error },
    /// Failure after the batch became durable; always ambiguous.
    Ambiguous(Error),
}

/// Persists one validated, sequence-checked batch and applies it, advancing
/// the durable head, applied head, and publish frontier in the canonical
/// order. `on_durable` runs exactly once, immediately after the durable head
/// advances and the write-log guard disarms — the serial arm completes its
/// deferred durability acknowledgements there.
pub(crate) fn persist_and_apply_assigned_batch(
    runtime: &TenantRuntime,
    records: &[TenantEventRecord],
    commit_faults: &CommitFaultClient,
    on_durable: impl FnOnce(),
) -> std::result::Result<DurableBatchOutcome, DurableBatchFailure> {
    let durable_append_started = Instant::now();
    let write_log_guard = runtime.arm_write_log_append();
    let expected_previous = runtime.durable_head();
    let provider_applied = match runtime.persist_fenced_provider_batch(expected_previous, records) {
        Ok(provider_applied) => provider_applied,
        Err(error) => {
            return Err(DurableBatchFailure::Persistence {
                fenced: true,
                error,
            });
        }
    };
    if !provider_applied && let Err(error) = runtime.store.append_durable_records_batch(records) {
        return Err(DurableBatchFailure::Persistence {
            fenced: false,
            error,
        });
    }
    let last_sequence = records
        .last()
        .expect("assigned durable batches must not be empty")
        .sequence;
    runtime.mark_durable_head(last_sequence);
    write_log_guard.disarm();
    on_durable();
    let durable_append = durable_append_started.elapsed();

    let apply_started = Instant::now();
    if !provider_applied {
        runtime
            .store
            .check_fault(nimbus_storage::FaultPoint::JournalDurableAppendBeforeApply)
            .map_err(DurableBatchFailure::Ambiguous)?;
    }
    commit_faults
        .wait(labels::DURABLE_BEFORE_PUBLISH)
        .into_result()
        .map_err(DurableBatchFailure::Ambiguous)?;
    let applied_head = if provider_applied {
        last_sequence
    } else {
        match runtime.store.apply_durable_records_batch(records) {
            Ok(()) => runtime
                .store
                .applied_head_after_durable_apply(records)
                .map_err(DurableBatchFailure::Ambiguous)?,
            Err(apply_error) => runtime
                .store
                .recover_durable_journal()
                .map(|progress| progress.applied_head)
                .map_err(|recovery_error| {
                    DurableBatchFailure::Ambiguous(Error::Internal(format!(
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
    retain_commits_through_applied_head(&mut applied, published_frontier);
    runtime.invalidate_document_cache_for_commits(applied.iter());
    let apply = apply_started.elapsed();

    runtime.mark_applied_head(published_frontier);
    Ok(DurableBatchOutcome {
        applied,
        durable_append,
        apply,
    })
}

/// Clip a recovered or freshly applied batch to the commits at or below the
/// given applied head so downstream visibility never runs ahead of it.
fn retain_commits_through_applied_head(
    applied: &mut Vec<CommitEntry>,
    applied_head: nimbus_core::SequenceNumber,
) {
    applied.retain(|commit| commit.sequence <= applied_head);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::SequenceNumber;

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
}
