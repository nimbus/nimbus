//! U5 commit witness: one entry point shared by the three composite SQL commit
//! paths, and one type that names every effect such a commit can have.
//!
//! Before this seam existed, each commit path open-coded its own sequence of
//! effects inside its `execute_write` closure. Nothing tied those sequences
//! together, so they drifted: the fenced and unfenced prepared-batch paths are
//! the same commit apart from one lease statement, yet each spelled out the
//! whole ordering independently, and a path that simply never mentioned an
//! effect was indistinguishable from one that had considered it and decided it
//! did not apply.
//!
//! [`SqlCommitEffects`] closes that gap by construction. It has no `Default`
//! and no field is an `Option`, so a witnessed commit path must name every
//! effect — including the ones it does not perform, each of which has an
//! explicit variant that says so.
//!
//! # What is witnessed
//!
//! Exactly three paths, all in [`super::store_core`]:
//!
//! - `apply_prepared_write_batch`
//! - `fenced_apply_prepared_write_batch`
//! - `apply_execution_unit_batch_with_origin`
//!
//! Adding a field to [`SqlCommitEffects`] breaks all three construction sites,
//! which is the point: a new commit effect cannot be introduced without each of
//! those paths stating its position on it.
//!
//! # What is not witnessed, and what that costs
//!
//! The **direct path** — the `execute_write` call sites performing a single
//! insert, update, delete, or schema operation — is deliberately excluded
//! (decision U8). Both available encodings destroy the property the witness
//! exists for. Giving [`SqlCommitEffects`] a `Default` so a direct call can
//! name only the fields it cares about reintroduces exactly the silence the
//! witness removes: an unstated effect becomes indistinguishable from a
//! considered one. Erasing the direct path's caller validators and
//! per-operation return payloads behind boxed closures reaches the witness only
//! by replacing reviewer-visible variants with opaque callbacks. The direct
//! path therefore keeps its own per-operation validators.
//!
//! The accepted trade, stated plainly so it is not rediscovered as a surprise:
//! **adding a field here does not force the direct path to declare a position
//! on it.** Three of the four commit-log paths are compiler-linked to this
//! type; the fourth is not, and a new effect that also applies to single
//! operations must be carried there by hand.
//!
//! # Effects that are not fields
//!
//! Version rows and index effects are not fields because they are not
//! independently selectable at this seam — they are executed inside the
//! document-write statements themselves (`apply_durable_record` and
//! `apply_resolved_write` in [`super::write_core`]), which is also what keeps
//! them inside the one storage transaction. Each [`DocumentWrites`] variant
//! documents which of them it carries.

use nimbus_core::{
    Error, Result, SequenceNumber, TenantEventRecord, Timestamp, TriggerWriteOrigin,
};

use super::store_core::{SqlWriteTransactionCore, apply_schedule_ops_in_transaction};
use crate::store::{ResolvedScheduleOp, ResolvedWrite};

/// Whether this commit is admitted, or was already performed by an earlier
/// attempt carrying the same scheduled-execution id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlCommitAdmission {
    Committed,
    /// The scheduled-execution gate rejected the commit as a duplicate. Nothing
    /// after the gate ran, so the transaction has no effects to roll back.
    SkippedDuplicateExecution,
}

/// How this commit is deduplicated against a replayed scheduled execution.
pub(crate) enum ExecutionDedup {
    /// Admit only if this execution id has not already committed.
    ScheduledExecution(String),
    /// Run the gate with no execution id: the caller is a scheduled execution
    /// that carries no id, so the gate always admits. Distinct from
    /// [`ExecutionDedup::NotDeduplicated`] so the two callers stay visibly
    /// different.
    NoExecutionId,
    /// This path does not consult the gate at all.
    NotDeduplicated,
}

/// Whether this commit advances a fenced committer lease in the same
/// transaction as its document writes.
pub(crate) enum LeaseEffect {
    /// Advance the lease, and fence the commit if the owner or epoch no longer
    /// matches. Fencing must happen before any document write so a fenced
    /// committer cannot publish.
    Fenced {
        owner_id: String,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    },
    /// This commit is not fenced; no lease row is touched.
    NotFenced,
}

/// The trigger-write origin attributed to documents this commit writes.
pub(crate) enum TriggerOriginEffect {
    /// Leave the transaction's origin as opened. Equivalent to setting `None`,
    /// which is the initial state of every dialect's write transaction.
    TransactionDefault,
    Explicit(TriggerWriteOrigin),
}

/// The commit timestamp stamped onto documents this commit writes.
pub(crate) enum CommitTimestampEffect {
    /// Let the provider assign the timestamp at commit. Equivalent to setting
    /// `None`, the initial state of every dialect's write transaction.
    ProviderAssigned,
    Explicit(Timestamp),
}

/// The document rows this commit writes, and with them the version rows and
/// index effects each strategy carries.
pub(crate) enum DocumentWrites {
    /// Replay one prepared durable record. `apply_durable_record` writes the
    /// document rows, their version rows, and the index effects for each write
    /// in the record.
    PreparedDurableRecord(TenantEventRecord),
    /// Apply writes already resolved by an execution unit.
    /// `apply_resolved_write` writes each document row with its version rows
    /// and index effects.
    ResolvedExecutionUnit(Vec<ResolvedWrite>),
}

/// The scheduler rows this commit writes.
pub(crate) enum ScheduleOps {
    Apply(Vec<ResolvedScheduleOp>),
    /// This commit touches no scheduled jobs.
    NoScheduleOps,
}

/// How this commit's entry reaches the commit log.
///
/// Not independent of [`DocumentWrites`]: a prepared record is both the
/// document source and the journal payload. [`sql_apply_commit`] rejects the
/// mismatched pairings rather than leaving them to produce a wrong commit
/// entry.
pub(crate) enum JournalEffect {
    /// The prepared durable record is handed to the transaction, which appends
    /// it verbatim as the commit entry.
    PreparedRecord,
    /// The commit entry is built from the writes buffered during this
    /// transaction.
    CommitEntryFromBufferedWrites,
}

/// Whether this commit advances the tenant's applied-sequence watermark.
///
/// Also tied to [`DocumentWrites`]: replaying a durable record advances the
/// watermark as part of the apply, and resolved execution-unit writes never do.
pub(crate) enum WatermarkEffect {
    /// Advanced inside `apply_durable_record`, as part of applying the record.
    AdvancedByRecordApply,
    /// This commit does not move the applied-sequence watermark.
    NotAdvanced,
}

/// Every effect one composite SQL commit can have. No `Default`, no `Option`
/// field: a path that omits an effect does not compile.
pub(crate) struct SqlCommitEffects {
    pub(crate) dedup: ExecutionDedup,
    pub(crate) lease: LeaseEffect,
    pub(crate) trigger_origin: TriggerOriginEffect,
    pub(crate) commit_timestamp: CommitTimestampEffect,
    pub(crate) documents: DocumentWrites,
    pub(crate) schedule_ops: ScheduleOps,
    pub(crate) journal: JournalEffect,
    pub(crate) watermark: WatermarkEffect,
}

/// Run one composite commit's effects, in the one order all of them share.
///
/// The order is load-bearing and is fixed here so no path can reorder it: the
/// duplicate-execution gate and the lease fence both decide admission, so they
/// precede every write; documents and their schedule rows follow; the prepared
/// record is handed over last, once the writes it describes have landed.
///
/// This runs inside the caller's `execute_write` closure. Reaching the
/// visibility boundary and everything after it stays with `sql_commit` in
/// [`super::write_core`].
pub(crate) fn sql_apply_commit<T: SqlWriteTransactionCore>(
    transaction: &mut T,
    effects: SqlCommitEffects,
) -> Result<SqlCommitAdmission> {
    let SqlCommitEffects {
        dedup,
        lease,
        trigger_origin,
        commit_timestamp,
        documents,
        schedule_ops,
        journal,
        watermark,
    } = effects;

    check_effect_coherence(&documents, &journal, &watermark)?;

    match trigger_origin {
        TriggerOriginEffect::TransactionDefault => {}
        TriggerOriginEffect::Explicit(origin) => transaction.set_trigger_write_origin(Some(origin)),
    }
    match commit_timestamp {
        CommitTimestampEffect::ProviderAssigned => {}
        CommitTimestampEffect::Explicit(timestamp) => {
            transaction.set_commit_timestamp(Some(timestamp));
        }
    }

    match dedup {
        ExecutionDedup::ScheduledExecution(execution_id) => {
            if !transaction.begin_scheduled_execution(Some(execution_id.as_str()))? {
                return Ok(SqlCommitAdmission::SkippedDuplicateExecution);
            }
        }
        ExecutionDedup::NoExecutionId => {
            if !transaction.begin_scheduled_execution(None)? {
                return Ok(SqlCommitAdmission::SkippedDuplicateExecution);
            }
        }
        ExecutionDedup::NotDeduplicated => {}
    }

    if let LeaseEffect::Fenced {
        owner_id,
        epoch,
        expected_previous,
        durable_sequence,
    } = lease
        && transaction.advance_fenced_committer_lease(
            &owner_id,
            epoch,
            expected_previous,
            durable_sequence,
        )? != 1
    {
        return Err(Error::PreconditionFailed(
            super::store_core::FENCED_COMMITTER_LEASE_MARKER.to_string(),
        ));
    }

    // The prepared record is applied here and handed over below, so it has to
    // outlive the apply.
    let prepared_record = match documents {
        DocumentWrites::PreparedDurableRecord(record) => {
            transaction.apply_durable_record(&record)?;
            Some(record)
        }
        DocumentWrites::ResolvedExecutionUnit(writes) => {
            for write in &writes {
                transaction.apply_resolved_write(write)?;
            }
            None
        }
    };

    match schedule_ops {
        ScheduleOps::Apply(ops) => apply_schedule_ops_in_transaction(transaction, &ops)?,
        ScheduleOps::NoScheduleOps => {}
    }

    match journal {
        JournalEffect::PreparedRecord => {
            let record = prepared_record.ok_or_else(|| {
                Error::Internal(
                    "JournalEffect::PreparedRecord requires DocumentWrites::PreparedDurableRecord"
                        .to_string(),
                )
            })?;
            transaction.set_prepared_record(record);
        }
        JournalEffect::CommitEntryFromBufferedWrites => {}
    }

    Ok(SqlCommitAdmission::Committed)
}

/// Reject the [`DocumentWrites`]/[`JournalEffect`]/[`WatermarkEffect`] pairings
/// that cannot both be true, before any statement runs.
///
/// These three are declared separately so each is visible at the construction
/// site, but they are not independent: the document-write strategy determines
/// the other two. Checking up front keeps a mismatch from producing a wrong
/// commit entry or a silently unmoved watermark.
fn check_effect_coherence(
    documents: &DocumentWrites,
    journal: &JournalEffect,
    watermark: &WatermarkEffect,
) -> Result<()> {
    let coherent = match documents {
        DocumentWrites::PreparedDurableRecord(_) => matches!(
            (journal, watermark),
            (
                JournalEffect::PreparedRecord,
                WatermarkEffect::AdvancedByRecordApply
            )
        ),
        DocumentWrites::ResolvedExecutionUnit(_) => matches!(
            (journal, watermark),
            (
                JournalEffect::CommitEntryFromBufferedWrites,
                WatermarkEffect::NotAdvanced
            )
        ),
    };
    if coherent {
        return Ok(());
    }
    Err(Error::Internal(
        "commit effects are incoherent: the journal and watermark effects do not match the \
         document-write strategy"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coherent pairings are the only ones the three composite paths
    /// construct, and the mismatched ones are rejected before any statement
    /// runs rather than producing a wrong commit entry or a stalled watermark.
    #[test]
    fn effect_coherence_accepts_only_the_pairings_the_document_strategy_implies() {
        let record = DocumentWrites::PreparedDurableRecord(
            TenantEventRecord::barrier(SequenceNumber(1), Timestamp(1), "coherence".to_string())
                .expect("barrier record should build"),
        );
        let resolved = DocumentWrites::ResolvedExecutionUnit(Vec::new());

        check_effect_coherence(
            &record,
            &JournalEffect::PreparedRecord,
            &WatermarkEffect::AdvancedByRecordApply,
        )
        .expect("a prepared record journals itself and advances the watermark");
        check_effect_coherence(
            &resolved,
            &JournalEffect::CommitEntryFromBufferedWrites,
            &WatermarkEffect::NotAdvanced,
        )
        .expect("resolved writes journal from the buffer and leave the watermark alone");

        for (documents, journal, watermark, mismatch) in [
            (
                &record,
                JournalEffect::CommitEntryFromBufferedWrites,
                WatermarkEffect::AdvancedByRecordApply,
                "a prepared record cannot journal from the write buffer",
            ),
            (
                &record,
                JournalEffect::PreparedRecord,
                WatermarkEffect::NotAdvanced,
                "applying a prepared record always advances the watermark",
            ),
            (
                &resolved,
                JournalEffect::PreparedRecord,
                WatermarkEffect::NotAdvanced,
                "resolved writes have no prepared record to journal",
            ),
            (
                &resolved,
                JournalEffect::CommitEntryFromBufferedWrites,
                WatermarkEffect::AdvancedByRecordApply,
                "resolved writes never advance the applied-sequence watermark",
            ),
        ] {
            let error =
                check_effect_coherence(documents, &journal, &watermark).expect_err(mismatch);
            assert!(
                matches!(&error, Error::Internal(message)
                    if message.contains("commit effects are incoherent")),
                "{mismatch}: expected an incoherence error, got {error:?}"
            );
        }
    }
}
