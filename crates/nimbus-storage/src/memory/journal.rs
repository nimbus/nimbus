use nimbus_core::{
    CommitEntry, Error, Result, SchemaChangeEvent, SequenceNumber, TableLifecycleEvent, TableState,
    TenantEventKind, TenantEventRecord, TriggerDeliveryCursor, WriteOp,
};

use crate::MAX_DURABLE_JOURNAL_STREAM_LIMIT;
use crate::changefeed::{ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage};
use crate::retention::RetentionGcConfig;
use crate::simulation::{DurableApplyKind, FaultPoint};
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, MaterializedJournalSnapshot,
    PointInTimeRestoreArchive, PointInTimeRestoreTarget, describe_materialized_position,
};
use crate::{
    MaterializedRetentionCheckpoint, PreparedRetentionHistory, RetentionHistoryState,
    RetentionHistorySummary, RetentionReadFloors,
};

use super::MemoryTenantStore;
use super::state::{MemoryState, MemoryTableIdentity};

impl MemoryState {
    fn apply_write(&mut self, sequence: SequenceNumber, write: &WriteOp) -> Result<()> {
        self.ensure_active_table_id(&write.table, &write.table_id)?;
        let documents = self.documents.entry(write.table_id.clone()).or_default();
        match (&write.previous, &write.current) {
            (None, Some(current)) => match documents.get(&write.doc_id) {
                Some(existing) if existing == current => {}
                Some(_) => {
                    return Err(crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "insert",
                        write.doc_id.as_str(),
                        "found unexpected state",
                    ));
                }
                None => {
                    documents.insert(write.doc_id.clone(), current.clone());
                }
            },
            (Some(previous), Some(current)) => {
                let existing = documents.get(&write.doc_id).ok_or_else(|| {
                    crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "update",
                        write.doc_id.as_str(),
                        "is missing the expected pre-image",
                    )
                })?;
                if existing != current {
                    if existing != previous {
                        return Err(crate::commit_log::durable_replay_preimage_corruption(
                            sequence,
                            "update",
                            write.doc_id.as_str(),
                            "found a pre-image mismatch",
                        ));
                    }
                    documents.insert(write.doc_id.clone(), current.clone());
                }
            }
            (Some(previous), None) => {
                if let Some(existing) = documents.get(&write.doc_id)
                    && existing != previous
                {
                    return Err(crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "delete",
                        write.doc_id.as_str(),
                        "found a pre-image mismatch",
                    ));
                }
                documents.remove(&write.doc_id);
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }

        match (&write.current, &write.resource_path_binding) {
            (Some(_), Some(binding)) => self.upsert_resource_path_binding(binding)?,
            (None, _) => {
                self.remove_resource_path_binding(&nimbus_core::DocumentLocator::new(
                    write.table.clone(),
                    write.doc_id.clone(),
                ));
            }
            (Some(_), None) => {}
        }
        Ok(())
    }

    fn apply_schema_change(&mut self, change: &SchemaChangeEvent) -> Result<()> {
        match change {
            SchemaChangeEvent::SetTable {
                table,
                table_id,
                current,
                ..
            } => {
                self.ensure_active_table_id(table, table_id)?;
                self.schema.tables.insert(table.clone(), current.clone());
            }
            SchemaChangeEvent::DeleteTable { table, .. } => {
                self.schema.tables.remove(table);
            }
        }
        Ok(())
    }

    fn apply_table_lifecycle(&mut self, lifecycle: &TableLifecycleEvent) -> Result<()> {
        match lifecycle {
            TableLifecycleEvent::StageHidden { table, table_id } => {
                match self.table_identities.get(table_id) {
                    Some(identity)
                        if identity.table == *table && identity.state == TableState::Hidden => {}
                    Some(_) => {
                        return Err(Error::conflict(format!(
                            "cannot stage hidden table {table} with already assigned id {table_id}"
                        )));
                    }
                    None => {
                        self.table_identities.insert(
                            table_id.clone(),
                            MemoryTableIdentity {
                                table: table.clone(),
                                state: TableState::Hidden,
                            },
                        );
                    }
                }
            }
            TableLifecycleEvent::ActivateHidden {
                table,
                table_id,
                replaced_table_id,
            } => {
                if let Some(replaced) = replaced_table_id {
                    if self.active_tables.get(table) == Some(replaced) {
                        self.active_tables.remove(table);
                    }
                    if let Some(identity) = self.table_identities.get_mut(replaced) {
                        identity.state = TableState::Deleting;
                    }
                }
                let identity = self.table_identities.get_mut(table_id).ok_or_else(|| {
                    Error::conflict(format!(
                        "cannot activate missing hidden table {table} with id {table_id}"
                    ))
                })?;
                if identity.table != *table {
                    return Err(Error::conflict(format!(
                        "hidden table id {table_id} belongs to {}, not {table}",
                        identity.table
                    )));
                }
                identity.state = TableState::Active;
                self.active_tables.insert(table.clone(), table_id.clone());
            }
            TableLifecycleEvent::MarkDeleting { table, table_id } => {
                if self.active_tables.get(table) == Some(table_id) {
                    self.active_tables.remove(table);
                    if let Some(identity) = self.table_identities.get_mut(table_id) {
                        identity.state = TableState::Deleting;
                    }
                } else if let Some(identity) = self.table_identities.get(table_id)
                    && identity.state != TableState::Deleting
                {
                    return Err(Error::conflict(format!(
                        "cannot mark non-active table id {table_id} deleting"
                    )));
                }
            }
            TableLifecycleEvent::HardDelete { table, table_id } => {
                if self
                    .table_identities
                    .get(table_id)
                    .is_some_and(|identity| identity.state == TableState::Deleting)
                {
                    self.table_identities.remove(table_id);
                    self.documents.remove(table_id);
                    if !self.active_tables.contains_key(table) {
                        self.schema.tables.remove(table);
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_event(&mut self, sequence: SequenceNumber, event: &TenantEventKind) -> Result<()> {
        match event {
            TenantEventKind::DocumentWrite { writes } => {
                for write in writes {
                    self.apply_write(sequence, write)?;
                }
            }
            TenantEventKind::SchemaChange { change } => self.apply_schema_change(change)?,
            TenantEventKind::TableLifecycle { lifecycle } => {
                self.apply_table_lifecycle(lifecycle)?;
            }
            TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => {}
            TenantEventKind::ScheduledExecution { execution_id } => {
                self.scheduled_execution_ids.insert(execution_id.clone());
            }
            TenantEventKind::TriggerDelivery { cursor } => {
                self.trigger_delivery_cursor = *cursor;
            }
        }
        Ok(())
    }

    pub(super) fn apply_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        record.validate_integrity()?;
        if record.events.is_empty() {
            for write in &record.writes {
                self.apply_write(record.sequence, write)?;
            }
            if let Some(execution_id) = &record.scheduled_execution_id {
                self.scheduled_execution_ids.insert(execution_id.clone());
            }
            return Ok(());
        }
        for event in &record.events {
            self.apply_event(record.sequence, event)?;
        }
        Ok(())
    }

    fn restore_snapshot(&mut self, snapshot: &MaterializedJournalSnapshot) -> Result<()> {
        snapshot.validate()?;
        for identity in &snapshot.table_identities {
            self.table_identities.insert(
                identity.table_id.clone(),
                MemoryTableIdentity {
                    table: identity.table.clone(),
                    state: identity.state,
                },
            );
            if identity.state == TableState::Active {
                self.active_tables
                    .insert(identity.table.clone(), identity.table_id.clone());
            }
        }
        self.schema = snapshot.schema.clone();
        for document in &snapshot.documents {
            let table_id = snapshot.default_table_id(&document.table)?;
            self.documents
                .entry(table_id)
                .or_default()
                .insert(document.id.clone(), document.clone());
        }
        for binding in &snapshot.resource_path_bindings {
            self.upsert_resource_path_binding(binding)?;
        }
        self.scheduled_execution_ids = snapshot.scheduled_execution_ids.iter().cloned().collect();
        self.trigger_delivery_cursor = snapshot.trigger_delivery_cursor;
        self.durable_head = snapshot.applied_sequence;
        self.applied_head = snapshot.applied_sequence;
        Ok(())
    }
}

impl MemoryTenantStore {
    pub fn retention_gc_watermarks(
        &self,
        config: RetentionGcConfig,
    ) -> Result<crate::RetentionGcWatermarks> {
        Ok(self
            .retention_floor
            .gc_watermarks(self.journal_progress()?.applied_head, config))
    }

    pub fn retention_history_state(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistoryState> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let state = self.read_state()?;
        let checkpoint = state
            .retention_checkpoint
            .clone()
            .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
        let read_floors = RetentionReadFloors::new(
            state.retention_document_version_floor,
            state.retention_index_version_floor,
            state.retention_physical_floor,
        );
        self.retention_floor
            .observe_published_read_floors(read_floors);
        RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            crate::retention::desired_journal_floor(&watermarks).max(checkpoint.sequence()),
            state.retention_physical_floor,
            checkpoint,
        )
    }

    pub fn prepare_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<PreparedRetentionHistory> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let prepared = self.read_state()?.clone();
        let checkpoint = prepared
            .retention_checkpoint
            .clone()
            .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
        let desired_floor =
            crate::retention::desired_journal_floor(&watermarks).max(checkpoint.sequence());
        let before = RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            desired_floor,
            prepared.retention_physical_floor,
            checkpoint.clone(),
        )?;
        let expected_read_floors = RetentionReadFloors::new(
            prepared.retention_document_version_floor,
            prepared.retention_index_version_floor,
            prepared.retention_physical_floor,
        );
        self.retention_floor
            .observe_published_read_floors(expected_read_floors);
        let journal_tail = prepared
            .durable_journal
            .range(checkpoint.sequence().0.saturating_add(1)..)
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>();
        let candidate = checkpoint.advance(&journal_tail, desired_floor)?;
        self.check_fault(FaultPoint::RetentionCheckpointAfterPrepare)?;
        Ok(PreparedRetentionHistory {
            watermarks,
            before,
            candidate,
            expected_checkpoint_blob: None,
            expected_read_floors,
            expected_revision: Some(prepared.revision),
        })
    }

    pub fn finalize_retained_history(
        &self,
        prepared: PreparedRetentionHistory,
    ) -> Result<RetentionHistorySummary> {
        let _pin_barrier = self
            .retention_floor
            .guard_prepared_watermarks(&prepared.watermarks)?;
        let PreparedRetentionHistory {
            watermarks,
            before,
            candidate,
            expected_read_floors,
            expected_revision,
            ..
        } = prepared;

        let mut live = self.write_state()?;
        if Some(live.revision) != expected_revision {
            return Err(Error::conflict(
                "memory retention state changed while compaction was prepared".to_string(),
            ));
        }
        if candidate.sequence().0 > live.applied_head.0 {
            return Err(Error::conflict(format!(
                "retention checkpoint target {} exceeds current applied head {}",
                candidate.sequence().0,
                live.applied_head.0
            )));
        }
        let mut next = live.clone();
        let published_read_floors = expected_read_floors.max(RetentionReadFloors::new(
            watermarks.document_versions.safe_prune_before,
            watermarks.index_versions.safe_prune_before,
            candidate.sequence(),
        ));
        let keys = next
            .durable_journal
            .range(..=candidate.sequence().0)
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        let journal_records_pruned = keys.len() as u64;
        for sequence in keys {
            next.durable_journal.remove(&sequence);
        }
        next.retention_checkpoint = Some(candidate.clone());
        next.retention_physical_floor = published_read_floors.journal;
        next.retention_document_version_floor = published_read_floors.document_versions;
        next.retention_index_version_floor = published_read_floors.index_versions;
        self.check_fault(FaultPoint::RetentionCheckpointBeforeCommit)?;
        self.retention_floor.publish_read_floors_with_commit(
            published_read_floors,
            || -> Result<()> {
                next.revision = live.revision.saturating_add(1);
                *live = next;
                Ok(())
            },
        )?;
        drop(live);
        self.check_fault(FaultPoint::RetentionCheckpointAfterCommit)?;

        let after = RetentionHistoryState::new(
            before.latest_sequence,
            before.desired_floor,
            published_read_floors.journal,
            candidate,
        )?;
        Ok(RetentionHistorySummary {
            watermarks,
            before,
            after,
            journal_records_pruned,
            document_versions_pruned: 0,
            index_versions_pruned: 0,
        })
    }

    pub fn compact_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistorySummary> {
        self.finalize_retained_history(self.prepare_retained_history(config)?)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn prune_durable_journal_through_for_testing(&self, through: SequenceNumber) -> Result<()> {
        self.transact(|state| {
            state
                .durable_journal
                .retain(|sequence, _| *sequence > through.0);
            Ok(())
        })
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(self.read_state()?.progress())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.read_state()?.durable_head())
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.read_state()?.applied_head)
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let state = self.read_state()?;
        let suffix_after =
            SequenceNumber(sequence.0.saturating_sub(1)).max(state.retention_physical_floor);
        let records = state
            .durable_journal
            .range(suffix_after.0.saturating_add(1)..)
            .map(|(_, record)| {
                record.validate_integrity()?;
                Ok(record.clone())
            })
            .collect::<Result<Vec<_>>>()?;
        crate::retention::validate_contiguous_journal_page(
            suffix_after,
            records.as_slice(),
            state.durable_head(),
            false,
        )?;
        Ok(records)
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        if limit == 0 {
            return Err(Error::InvalidInput(
                "journal stream limit must be greater than zero".to_string(),
            ));
        }
        if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
            return Err(Error::InvalidInput(format!(
                "journal stream limit {limit} exceeds the maximum {MAX_DURABLE_JOURNAL_STREAM_LIMIT}"
            )));
        }
        let state = self.read_state()?;
        let latest_sequence = state.durable_head();
        let inferred_floor = state
            .durable_journal
            .first_key_value()
            .map_or(SequenceNumber(0), |(sequence, _)| {
                SequenceNumber(sequence.saturating_sub(1))
            });
        let cursor_floor = state.retention_physical_floor.max(inferred_floor);
        crate::retention::validate_retention_after_page(
            after,
            cursor_floor,
            "durable journal cursor",
        )?;
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }
        let mut records = state
            .durable_journal
            .range(after.0.saturating_add(1)..)
            .take(limit.saturating_add(1))
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>();
        let has_more = records.len() > limit;
        records.truncate(limit);
        for record in &records {
            record.validate_integrity()?;
        }
        let next_cursor = records.last().map_or(after, |record| record.sequence);
        self.fault_injector
            .check(crate::FaultPoint::RetentionReadAfterPage)?;
        crate::retention::validate_retention_after_page(
            after,
            cursor_floor,
            "durable journal page",
        )?;
        crate::retention::validate_contiguous_journal_page(
            after,
            records.as_slice(),
            latest_sequence,
            has_more,
        )?;
        Ok(DurableJournalPage {
            records,
            next_cursor,
            latest_sequence,
            cursor_floor,
            has_more,
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let state = self.read_state()?;
        let snapshot = state.materialized_snapshot();
        let inferred_floor = state
            .durable_journal
            .first_key_value()
            .map_or(SequenceNumber(0), |(sequence, _)| {
                SequenceNumber(sequence.saturating_sub(1))
            });
        let cursor_floor = state.retention_physical_floor.max(inferred_floor);
        crate::retention::validate_retention_after_page(
            snapshot.applied_sequence,
            cursor_floor,
            "durable journal bootstrap",
        )?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap> {
        ChangefeedBootstrap::from_durable_bootstrap(self.export_durable_journal_bootstrap()?)
    }

    pub fn stream_changefeed(
        &self,
        cursor: &ChangefeedCursor,
        limit: usize,
    ) -> Result<ChangefeedPage> {
        cursor.rotate_handle(cursor.handle.clone())?;
        let page = self
            .stream_durable_journal(cursor.after, limit)
            .map_err(crate::changefeed::map_changefeed_journal_error)?;
        ChangefeedPage::from_durable_page(cursor.handle.clone(), page)
    }

    pub fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut state = self.write_state()?;
        let mut next = state.clone();
        let mut expected = next.durable_head().0.saturating_add(1);
        for record in records {
            record.validate_integrity()?;
            if record.sequence.0 != expected {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {expected}, got {}",
                    record.sequence.0
                )));
            }
            next.durable_journal
                .insert(record.sequence.0, record.clone());
            next.durable_head = record.sequence;
            expected = expected.saturating_add(1);
        }
        self.check_durable_records_fault(FaultPoint::JournalAppendBeforeDurableFlush, records)?;
        next.revision = state.revision.saturating_add(1);
        *state = next;
        drop(state);
        self.check_durable_records_fault(FaultPoint::JournalFlushBeforeVisibility, records)?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_durable_records_batch_as(records, DurableApplyKind::ClientBatch)
    }

    /// See [`DurableApplyKind::JournalReplay`]: recovery re-applies records that
    /// are already durable, so this boundary names none.
    pub fn replay_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_durable_records_batch_as(records, DurableApplyKind::JournalReplay)
    }

    fn apply_durable_records_batch_as(
        &self,
        records: &[TenantEventRecord],
        kind: DurableApplyKind,
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.transact_durable_records(kind.newly_durable_records(records), |state| {
            let mut applied_head = state.applied_head.0;
            for record in records {
                if record.sequence.0 <= applied_head {
                    crate::commit_log::ensure_applied_record_matches(
                        record,
                        state.durable_journal.get(&record.sequence.0),
                    )?;
                    continue;
                }
                let expected = applied_head.saturating_add(1);
                if record.sequence.0 != expected {
                    return Err(Error::Internal(format!(
                        "durable journal apply expected sequence {expected}, got {}",
                        record.sequence.0
                    )));
                }
                state.apply_record(record)?;
                applied_head = record.sequence.0;
            }
            state.applied_head = SequenceNumber(applied_head);
            Ok(())
        })
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let pending = self
            .read_durable_journal_from(SequenceNumber(progress.applied_head.0.saturating_add(1)))?;
        self.replay_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        Ok(self.read_state()?.materialized_snapshot())
    }

    pub fn restore_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
    ) -> Result<()> {
        self.transact(|state| {
            if state.durable_head().0 != 0
                || state.applied_head.0 != 0
                || !state.documents.is_empty()
                || !state.schema.tables.is_empty()
                || !state.table_identities.is_empty()
                || !state.scheduled_execution_ids.is_empty()
                || !state.resource_bindings.is_empty()
                || state.trigger_delivery_cursor != TriggerDeliveryCursor::default()
            {
                return Err(Error::Internal(
                    "materialized journal snapshot restore requires an empty tenant store"
                        .to_string(),
                ));
            }
            let _verification_update = self.materialized_verification.begin_update()?;
            state.restore_snapshot(snapshot)
        })
    }

    pub fn rebuild_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
        journal_tail: &[TenantEventRecord],
        target_sequence: Option<SequenceNumber>,
    ) -> Result<JournalProgress> {
        snapshot.validate()?;
        let available_head = journal_tail
            .last()
            .map_or(snapshot.applied_sequence, |record| record.sequence);
        if let Some(target) = target_sequence {
            if target.0 < snapshot.applied_sequence.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is behind snapshot sequence {}",
                    target.0, snapshot.applied_sequence.0
                )));
            }
            if target.0 > available_head.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is beyond available journal head {}",
                    target.0, available_head.0
                )));
            }
        } else if available_head.0 < snapshot.durable_head.0 {
            return Err(Error::InvalidInput(format!(
                "journal tail is incomplete for snapshot boundary: available head {} is behind snapshot durable head {}",
                available_head.0, snapshot.durable_head.0
            )));
        }
        self.restore_materialized_journal_from_snapshot(snapshot)?;
        let replay_target = target_sequence.unwrap_or(available_head);
        let tail = journal_tail
            .iter()
            .filter(|record| {
                record.sequence.0 > snapshot.applied_sequence.0
                    && record.sequence.0 <= replay_target.0
            })
            .cloned()
            .collect::<Vec<_>>();
        self.append_durable_records_batch(&tail)?;
        self.recover_durable_journal()
    }

    pub fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        let (checkpoint, initial_floor) = {
            let state = self.read_state()?;
            (
                state
                    .retention_checkpoint
                    .clone()
                    .unwrap_or(MaterializedRetentionCheckpoint::genesis()?),
                state.retention_physical_floor,
            )
        };
        let base_sequence = checkpoint.sequence();
        crate::retention::validate_retention_after_page(
            base_sequence,
            initial_floor.max(self.retention_floor.published_read_floors().journal),
            "point-in-time archive base",
        )?;
        let records =
            self.read_durable_journal_from(SequenceNumber(base_sequence.0.saturating_add(1)))?;
        self.fault_injector
            .check(crate::FaultPoint::RetentionReadAfterPage)?;
        let authoritative_floor = self
            .read_state()?
            .retention_physical_floor
            .max(self.retention_floor.published_read_floors().journal);
        crate::retention::validate_retention_after_page(
            base_sequence,
            authoritative_floor,
            "point-in-time archive base",
        )?;
        let progress = self.journal_progress()?;
        let watermarks = self.retention_gc_watermarks(retention_config)?;
        crate::store::build_point_in_time_restore_archive_from_checkpoint(
            target,
            records,
            progress.durable_head,
            watermarks.pitr_exports.safe_prune_before,
            checkpoint,
        )
    }

    pub fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        archive.validate()?;
        let progress = self.rebuild_materialized_journal_from_snapshot(
            &archive.base_snapshot,
            &archive.journal_tail,
            Some(archive.target_sequence),
        )?;
        let restored_position = self
            .export_materialized_journal_snapshot()?
            .materialized_position()?;
        if restored_position != archive.target_position {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore position mismatch: restored {} expected {}",
                    describe_materialized_position(&restored_position),
                    describe_materialized_position(&archive.target_position)
                ),
            ));
        }
        let checkpoint = MaterializedRetentionCheckpoint::new(
            archive.base_snapshot.clone(),
            archive.base_checkpoint_timestamp,
        )?;
        self.transact(|state| {
            if checkpoint.sequence().0 > state.applied_head.0 {
                return Err(Error::InvalidInput(format!(
                    "imported retention checkpoint {} exceeds restored applied head {}",
                    checkpoint.sequence().0,
                    state.applied_head.0
                )));
            }
            state.retention_checkpoint = Some(checkpoint.clone());
            state.retention_physical_floor = checkpoint.sequence();
            state.retention_document_version_floor = checkpoint.sequence();
            state.retention_index_version_floor = checkpoint.sequence();
            Ok(())
        })?;
        Ok(progress)
    }
}
