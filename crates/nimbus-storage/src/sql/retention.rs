//! Provider-neutral retention orchestration for SQL-family stores.

use nimbus_core::{Error, Result, SequenceNumber};

use crate::FaultPoint;
use crate::retention::{
    MaterializedRetentionCheckpoint, PreparedRetentionHistory, RetentionGcConfig,
    RetentionGcSummary, RetentionGcWatermarks, RetentionHistoryState, RetentionHistorySummary,
    RetentionReadFloors, deserialize_retention_checkpoint, desired_journal_floor,
    serialize_retention_checkpoint,
};
use crate::sql::store_core::{
    FENCED_COMMITTER_LEASE_MARKER, SqlStoreCore, SqlWriteTransactionCore, map_fenced_write_result,
};
use crate::sql::write_core::SqlWriteBackend;
use crate::store::{PointInTimeRestoreArchive, PointInTimeRestoreTarget};
use crate::traits::CommitterLeaseResult;

pub(crate) fn retention_gc_watermarks<S: SqlStoreCore>(
    store: &S,
    config: RetentionGcConfig,
) -> Result<RetentionGcWatermarks> {
    Ok(store
        .retention_floor()
        .gc_watermarks(store.journal_progress()?.applied_head, config))
}

pub(crate) fn compact_retained_versions<S: SqlStoreCore>(
    store: &S,
    config: RetentionGcConfig,
) -> Result<RetentionGcSummary> {
    let watermarks = store.retention_gc_watermarks(config)?;
    let _pin_barrier = store
        .retention_floor()
        .guard_prepared_watermarks(&watermarks)?;
    let (checkpoint, expected_read_floors, expected_checkpoint_blob) =
        store.load_retention_checkpoint()?;
    store
        .retention_floor()
        .observe_published_read_floors(expected_read_floors);
    let published_read_floors = expected_read_floors.max(RetentionReadFloors::new(
        watermarks.document_versions.safe_prune_before,
        watermarks.index_versions.safe_prune_before,
        expected_read_floors.journal,
    ));
    if published_read_floors == expected_read_floors {
        return Ok(RetentionGcSummary {
            watermarks,
            document_versions_pruned: 0,
            index_versions_pruned: 0,
        });
    }
    let checkpoint_blob = serialize_retention_checkpoint(&checkpoint)?;
    let committed =
        store
            .retention_floor()
            .publish_read_floors_with_commit(published_read_floors, || {
                store.execute_write(move |transaction| {
                    let (current_checkpoint_blob, current_read_floors) =
                        transaction.load_retention_metadata()?;
                    if current_checkpoint_blob != expected_checkpoint_blob
                        || current_read_floors != expected_read_floors
                    {
                        return Err(Error::conflict(
                            "retention metadata changed while version compaction was prepared"
                                .to_string(),
                        ));
                    }
                    let pruned = transaction.prune_retained_versions(
                        published_read_floors.document_versions,
                        published_read_floors.index_versions,
                    )?;
                    transaction
                        .store_retention_metadata(&checkpoint_blob, published_read_floors)?;
                    Ok(pruned)
                })
            })?;
    debug_assert!(committed.commit.is_none());
    Ok(RetentionGcSummary {
        watermarks,
        document_versions_pruned: committed.value.0,
        index_versions_pruned: committed.value.1,
    })
}

pub(crate) fn load_retention_checkpoint<S: SqlStoreCore>(
    store: &S,
) -> Result<(
    MaterializedRetentionCheckpoint,
    RetentionReadFloors,
    Option<Vec<u8>>,
)> {
    let (checkpoint_blob, read_floors) = store.load_retention_metadata_snapshot()?;
    let checkpoint = checkpoint_blob
        .as_deref()
        .map(deserialize_retention_checkpoint)
        .transpose()?
        .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
    RetentionHistoryState::new(
        checkpoint.sequence(),
        checkpoint.sequence(),
        read_floors.journal,
        checkpoint.clone(),
    )?;
    Ok((checkpoint, read_floors, checkpoint_blob))
}

pub(crate) fn retention_history_state<S: SqlStoreCore>(
    store: &S,
    config: RetentionGcConfig,
) -> Result<RetentionHistoryState> {
    let watermarks = store.retention_gc_watermarks(config)?;
    let (checkpoint, read_floors, _) = store.load_retention_checkpoint()?;
    store
        .retention_floor()
        .observe_published_read_floors(read_floors);
    RetentionHistoryState::new(
        watermarks.document_versions.latest_sequence,
        desired_journal_floor(&watermarks).max(checkpoint.sequence()),
        read_floors.journal,
        checkpoint,
    )
}

pub(crate) fn prepare_retained_history<S: SqlStoreCore>(
    store: &S,
    config: RetentionGcConfig,
) -> Result<PreparedRetentionHistory> {
    let watermarks = store.retention_gc_watermarks(config)?;
    let (checkpoint, expected_read_floors, expected_checkpoint_blob) =
        store.load_retention_checkpoint()?;
    store
        .retention_floor()
        .observe_published_read_floors(expected_read_floors);
    let desired_floor = desired_journal_floor(&watermarks).max(checkpoint.sequence());
    let before = RetentionHistoryState::new(
        watermarks.document_versions.latest_sequence,
        desired_floor,
        expected_read_floors.journal,
        checkpoint.clone(),
    )?;
    let journal_tail = store
        .read_durable_journal_from(SequenceNumber(checkpoint.sequence().0.saturating_add(1)))?;
    let candidate = checkpoint.advance(&journal_tail, desired_floor)?;
    Ok(PreparedRetentionHistory {
        watermarks,
        before,
        candidate,
        expected_checkpoint_blob,
        expected_read_floors,
        expected_revision: None,
    })
}

pub(crate) fn fenced_finalize_retained_history<S: SqlStoreCore>(
    store: &S,
    owner_id: &str,
    epoch: u64,
    durable_sequence: SequenceNumber,
    prepared: PreparedRetentionHistory,
) -> CommitterLeaseResult<RetentionHistorySummary> {
    let _pin_barrier = store
        .retention_floor()
        .guard_prepared_watermarks(&prepared.watermarks)?;
    let PreparedRetentionHistory {
        watermarks,
        before,
        candidate,
        expected_checkpoint_blob,
        expected_read_floors,
        ..
    } = prepared;
    let candidate_blob = serialize_retention_checkpoint(&candidate)?;
    let candidate_sequence = candidate.sequence();
    let published_read_floors = expected_read_floors.max(RetentionReadFloors::new(
        watermarks.document_versions.safe_prune_before,
        watermarks.index_versions.safe_prune_before,
        candidate_sequence,
    ));
    let fenced_owner_id = owner_id.to_string();
    let owner_id = owner_id.to_string();
    let result =
        store
            .retention_floor()
            .publish_read_floors_with_commit(published_read_floors, || {
                store.execute_write(move |transaction| {
                    if transaction.validate_fenced_committer_lease(
                        owner_id.as_str(),
                        epoch,
                        durable_sequence,
                    )? != 1
                    {
                        return Err(Error::PreconditionFailed(
                            FENCED_COMMITTER_LEASE_MARKER.to_string(),
                        ));
                    }
                    let (current_checkpoint_blob, current_read_floors) =
                        transaction.load_retention_metadata()?;
                    if current_checkpoint_blob != expected_checkpoint_blob
                        || current_read_floors != expected_read_floors
                    {
                        return Err(Error::conflict(
                            "retention checkpoint changed while compaction was prepared"
                                .to_string(),
                        ));
                    }
                    let applied_head = transaction.applied_sequence_for_retention()?;
                    if candidate_sequence.0 > applied_head.0 {
                        return Err(Error::conflict(format!(
                            "retention checkpoint target {} exceeds current applied head {}",
                            candidate_sequence.0, applied_head.0
                        )));
                    }
                    let (document_versions_pruned, index_versions_pruned) = transaction
                        .prune_retained_versions(
                            published_read_floors.document_versions,
                            published_read_floors.index_versions,
                        )?;
                    let journal_records_pruned =
                        transaction.prune_durable_journal_through(candidate_sequence)?;
                    transaction.store_retention_metadata(&candidate_blob, published_read_floors)?;
                    transaction.check_fault(FaultPoint::RetentionCheckpointBeforeCommit)?;
                    Ok((
                        journal_records_pruned,
                        document_versions_pruned,
                        index_versions_pruned,
                        published_read_floors,
                    ))
                })
            });
    let committed = map_fenced_write_result(result, fenced_owner_id, epoch)?;
    debug_assert!(committed.commit.is_none());
    let after = RetentionHistoryState::new(
        before.latest_sequence,
        before.desired_floor,
        committed.value.3.journal,
        candidate,
    )?;
    Ok(RetentionHistorySummary {
        watermarks,
        before,
        after,
        journal_records_pruned: committed.value.0,
        document_versions_pruned: committed.value.1,
        index_versions_pruned: committed.value.2,
    })
}

pub(crate) fn fenced_compact_retained_history<S: SqlStoreCore>(
    store: &S,
    owner_id: &str,
    epoch: u64,
    durable_sequence: SequenceNumber,
    config: RetentionGcConfig,
) -> CommitterLeaseResult<RetentionHistorySummary> {
    let prepared = store.prepare_retained_history(config)?;
    store.fenced_finalize_retained_history(owner_id, epoch, durable_sequence, prepared)
}

pub(crate) fn export_point_in_time_restore_archive<S: SqlStoreCore>(
    store: &S,
    target: PointInTimeRestoreTarget,
    retention_config: RetentionGcConfig,
) -> Result<PointInTimeRestoreArchive> {
    let (checkpoint, initial_read_floors, _) = store.load_retention_checkpoint()?;
    let base_sequence = checkpoint.sequence();
    crate::retention::validate_retention_after_page(
        base_sequence,
        initial_read_floors
            .journal
            .max(store.retention_floor().published_read_floors().journal),
        "point-in-time archive base",
    )?;
    let records =
        store.read_durable_journal_from(SequenceNumber(base_sequence.0.saturating_add(1)))?;
    store.check_retention_read_page()?;
    let (authoritative_checkpoint, authoritative_read_floors, _) =
        store.load_retention_checkpoint()?;
    crate::retention::validate_retention_after_page(
        base_sequence,
        authoritative_checkpoint
            .sequence()
            .max(authoritative_read_floors.journal)
            .max(store.retention_floor().published_read_floors().journal),
        "point-in-time archive base",
    )?;
    let progress = store.journal_progress()?;
    let watermarks = store.retention_gc_watermarks(retention_config)?;
    crate::store::build_point_in_time_restore_archive_from_checkpoint(
        target,
        records,
        progress.durable_head,
        watermarks.pitr_exports.safe_prune_before,
        checkpoint,
    )
}
