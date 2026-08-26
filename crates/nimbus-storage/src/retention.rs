use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::{Error, Result, SequenceNumber, TableId, TenantEventRecord, Timestamp};
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "libsql")]
use crate::LibsqlReplicaTenantStore;
#[cfg(feature = "mysql")]
use crate::MySqlTenantStore;
#[cfg(feature = "postgres")]
use crate::PostgresTenantStore;
use crate::{MaterializedJournalSnapshot, MaterializedPosition, SqliteTenantStore, TenantStore};

mod read_safety;

pub use read_safety::RetentionReadFloors;
pub(crate) use read_safety::{validate_contiguous_journal_page, validate_retention_after_page};

pub const MATERIALIZED_RETENTION_CHECKPOINT_VERSION: u16 = 1;
pub(crate) const RETENTION_CHECKPOINT_METADATA_KEY: &str = "retention_materialized_checkpoint";
pub(crate) const RETENTION_PHYSICAL_FLOOR_METADATA_KEY: &str = "retention_physical_floor";
pub(crate) const RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY: &str =
    "retention_document_version_floor";
pub(crate) const RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY: &str = "retention_index_version_floor";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionGcResource {
    DocumentVersions,
    IndexVersions,
    RegistryMetadata,
    ReadPolicyMetadata,
    CdcJournal,
    PitrExport,
    ShadowMaterializer,
    EmbeddedReplica,
    TransactionSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionGcConfig {
    pub document_version_window_sequences: u64,
    pub index_version_window_sequences: u64,
    pub cdc_window_sequences: u64,
    pub pitr_window_sequences: u64,
}

impl RetentionGcConfig {
    pub const fn retain_all() -> Self {
        Self {
            document_version_window_sequences: u64::MAX,
            index_version_window_sequences: u64::MAX,
            cdc_window_sequences: u64::MAX,
            pitr_window_sequences: u64::MAX,
        }
    }

    pub fn new(history_window_sequences: u64) -> Result<Self> {
        Self::with_windows(
            history_window_sequences,
            history_window_sequences,
            history_window_sequences,
            history_window_sequences,
        )
    }

    pub fn with_windows(
        document_version_window_sequences: u64,
        index_version_window_sequences: u64,
        cdc_window_sequences: u64,
        pitr_window_sequences: u64,
    ) -> Result<Self> {
        if [
            document_version_window_sequences,
            index_version_window_sequences,
            cdc_window_sequences,
            pitr_window_sequences,
        ]
        .contains(&0)
        {
            return Err(Error::InvalidInput(
                "retention windows must retain at least one sequence".to_string(),
            ));
        }
        Ok(Self {
            document_version_window_sequences,
            index_version_window_sequences,
            cdc_window_sequences,
            pitr_window_sequences,
        })
    }

    fn window_for(self, resource: RetentionGcResource) -> u64 {
        match resource {
            RetentionGcResource::DocumentVersions => self.document_version_window_sequences,
            RetentionGcResource::IndexVersions => self.index_version_window_sequences,
            RetentionGcResource::CdcJournal => self.cdc_window_sequences,
            RetentionGcResource::PitrExport => self.pitr_window_sequences,
            RetentionGcResource::RegistryMetadata
            | RetentionGcResource::ReadPolicyMetadata
            | RetentionGcResource::ShadowMaterializer
            | RetentionGcResource::EmbeddedReplica
            | RetentionGcResource::TransactionSession => self
                .document_version_window_sequences
                .max(self.index_version_window_sequences)
                .max(self.cdc_window_sequences)
                .max(self.pitr_window_sequences),
        }
    }
}

impl Default for RetentionGcConfig {
    fn default() -> Self {
        Self::retain_all()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionGcWatermark {
    pub resource: RetentionGcResource,
    pub latest_sequence: SequenceNumber,
    pub window_floor: SequenceNumber,
    pub pinned_floor: Option<SequenceNumber>,
    pub safe_prune_before: SequenceNumber,
    pub active_pin_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionGcWatermarks {
    pub document_versions: RetentionGcWatermark,
    pub index_versions: RetentionGcWatermark,
    pub registry_metadata: RetentionGcWatermark,
    pub read_policy_metadata: RetentionGcWatermark,
    pub cdc_journal: RetentionGcWatermark,
    pub pitr_exports: RetentionGcWatermark,
    pub shadow_materializers: RetentionGcWatermark,
    pub embedded_replicas: RetentionGcWatermark,
    pub transaction_sessions: RetentionGcWatermark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionGcSummary {
    pub watermarks: RetentionGcWatermarks,
    pub document_versions_pruned: u64,
    pub index_versions_pruned: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterializedRetentionCheckpoint {
    pub version: u16,
    pub checkpoint_timestamp: Timestamp,
    pub snapshot: MaterializedJournalSnapshot,
    pub position: MaterializedPosition,
    pub snapshot_sha256: [u8; 32],
}

impl MaterializedRetentionCheckpoint {
    pub fn genesis() -> Result<Self> {
        Self::new(
            MaterializedJournalSnapshot::empty_for_point_in_time_base(),
            Timestamp(0),
        )
    }

    pub fn new(
        snapshot: MaterializedJournalSnapshot,
        checkpoint_timestamp: Timestamp,
    ) -> Result<Self> {
        let position = snapshot.materialized_position()?;
        let snapshot_sha256 =
            retention_checkpoint_snapshot_digest(&snapshot, checkpoint_timestamp, &position)?;
        let checkpoint = Self {
            version: MATERIALIZED_RETENTION_CHECKPOINT_VERSION,
            checkpoint_timestamp,
            snapshot,
            position,
            snapshot_sha256,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn sequence(&self) -> SequenceNumber {
        self.snapshot.applied_sequence
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != MATERIALIZED_RETENTION_CHECKPOINT_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported materialized retention checkpoint version {}",
                self.version
            )));
        }
        self.snapshot.validate()?;
        if self.snapshot.applied_sequence != self.snapshot.durable_head {
            return Err(Error::InvalidInput(format!(
                "materialized retention checkpoint applied sequence {} does not equal durable head {}",
                self.snapshot.applied_sequence.0, self.snapshot.durable_head.0
            )));
        }
        if self.position != self.snapshot.materialized_position()? {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                "materialized retention checkpoint position does not match its snapshot",
            ));
        }
        let expected_snapshot_sha256 = retention_checkpoint_snapshot_digest(
            &self.snapshot,
            self.checkpoint_timestamp,
            &self.position,
        )?;
        if self.snapshot_sha256 != expected_snapshot_sha256 {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                "materialized retention checkpoint snapshot digest does not match its contents",
            ));
        }
        Ok(())
    }

    pub(crate) fn advance(
        &self,
        journal_tail: &[TenantEventRecord],
        target: SequenceNumber,
    ) -> Result<Self> {
        self.validate()?;
        if target.0 < self.sequence().0 {
            return Err(Error::InvalidInput(format!(
                "retention checkpoint target {} is behind confirmed checkpoint {}",
                target.0,
                self.sequence().0
            )));
        }
        if target == self.sequence() {
            return Ok(self.clone());
        }
        validate_retained_journal_range(self.sequence(), journal_tail, target)?;
        let checkpoint_timestamp = journal_tail
            .iter()
            .find(|record| record.sequence == target)
            .map(|record| record.timestamp)
            .ok_or_else(|| {
                Error::storage(
                    nimbus_core::StorageErrorKind::Corruption,
                    format!(
                        "retention checkpoint journal is missing target sequence {}",
                        target.0
                    ),
                )
            })?;
        let snapshot = crate::store::materialized_snapshot_after_rebuild(
            &self.snapshot,
            journal_tail,
            target,
        )?;
        Self::new(snapshot, checkpoint_timestamp)
    }
}

fn retention_checkpoint_snapshot_digest(
    snapshot: &MaterializedJournalSnapshot,
    checkpoint_timestamp: Timestamp,
    position: &MaterializedPosition,
) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    digest.update(b"nimbus.materialized-retention-checkpoint.v1");
    digest.update(snapshot.version.to_be_bytes());
    digest.update(snapshot.applied_sequence.0.to_be_bytes());
    digest.update(snapshot.durable_head.0.to_be_bytes());
    digest.update(checkpoint_timestamp.0.to_be_bytes());
    digest.update(position.version().to_be_bytes());
    digest.update(position.applied_sequence().0.to_be_bytes());
    digest.update((position.state_digest().len() as u64).to_be_bytes());
    digest.update(position.state_digest().as_bytes());
    digest.update(
        snapshot
            .trigger_delivery_cursor
            .materialized_through
            .0
            .to_be_bytes(),
    );

    let mut bindings = snapshot.resource_path_bindings.clone();
    bindings.sort_by(|left, right| {
        left.document_path
            .to_string()
            .cmp(&right.document_path.to_string())
            .then_with(|| left.locator.table.cmp(&right.locator.table))
            .then_with(|| left.locator.id.cmp(&right.locator.id))
    });
    digest.update((bindings.len() as u64).to_be_bytes());
    for binding in bindings {
        let encoded = rmp_serde::to_vec_named(&binding)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        digest.update((encoded.len() as u64).to_be_bytes());
        digest.update(encoded);
    }

    Ok(digest.finalize().into())
}

fn validate_retained_journal_range(
    base: SequenceNumber,
    journal_tail: &[TenantEventRecord],
    target: SequenceNumber,
) -> Result<()> {
    let mut expected = base.0.saturating_add(1);
    for record in journal_tail
        .iter()
        .take_while(|record| record.sequence.0 <= target.0)
    {
        record.validate_integrity()?;
        if record.sequence.0 != expected {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "retained journal expected sequence {expected}, got {}",
                    record.sequence.0
                ),
            ));
        }
        expected = expected.saturating_add(1);
    }
    if expected != target.0.saturating_add(1) {
        return Err(Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            format!("retained journal is missing target sequence {}", target.0),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionHistoryState {
    pub latest_sequence: SequenceNumber,
    pub desired_floor: SequenceNumber,
    pub confirmed_floor: SequenceNumber,
    pub physical_floor: SequenceNumber,
    pub checkpoint: MaterializedRetentionCheckpoint,
}

/// Immutable checkpoint work prepared before entering a store's serial write
/// boundary. Finalization validates the captured checkpoint identity again,
/// so a concurrent writer produces a conflict instead of publishing stale
/// materialized state.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRetentionHistory {
    pub watermarks: RetentionGcWatermarks,
    pub before: RetentionHistoryState,
    pub(crate) candidate: MaterializedRetentionCheckpoint,
    pub(crate) expected_checkpoint_blob: Option<Vec<u8>>,
    pub(crate) expected_read_floors: RetentionReadFloors,
    pub(crate) expected_revision: Option<u64>,
}

impl RetentionHistoryState {
    pub(crate) fn new(
        latest_sequence: SequenceNumber,
        desired_floor: SequenceNumber,
        physical_floor: SequenceNumber,
        checkpoint: MaterializedRetentionCheckpoint,
    ) -> Result<Self> {
        checkpoint.validate()?;
        let confirmed_floor = checkpoint.sequence();
        if desired_floor.0 > latest_sequence.0 || confirmed_floor.0 > latest_sequence.0 {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "retention floors desired {} and confirmed {} exceed latest sequence {}",
                    desired_floor.0, confirmed_floor.0, latest_sequence.0
                ),
            ));
        }
        if physical_floor.0 > confirmed_floor.0 {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "physical retention floor {} exceeds confirmed checkpoint {}",
                    physical_floor.0, confirmed_floor.0
                ),
            ));
        }
        Ok(Self {
            latest_sequence,
            desired_floor,
            confirmed_floor,
            physical_floor,
            checkpoint,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionHistorySummary {
    pub watermarks: RetentionGcWatermarks,
    pub before: RetentionHistoryState,
    pub after: RetentionHistoryState,
    pub journal_records_pruned: u64,
    pub document_versions_pruned: u64,
    pub index_versions_pruned: u64,
}

impl RetentionGcSummary {
    pub fn total_pruned(&self) -> u64 {
        self.document_versions_pruned
            .saturating_add(self.index_versions_pruned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionParticipant {
    TransactionSession,
    ExportedSnapshot,
    JournalConsumer,
    EmbeddedReplica,
    ShadowMaterializer,
    CdcSubscription,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPin {
    pub id: u64,
    pub participant: RetentionParticipant,
    pub sequence: SequenceNumber,
    pub table_id: Option<TableId>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardDeleteDecision {
    Allowed,
    Denied { pin: RetentionPin },
}

#[derive(Debug, Default)]
pub struct RetentionFloor {
    next_pin_id: AtomicU64,
    pins: Mutex<BTreeMap<u64, RetentionPin>>,
    published_read_floors: Mutex<RetentionReadFloors>,
}

pub struct RetentionPinGuard {
    floor: Arc<RetentionFloor>,
    pin_id: u64,
}

/// Holds the participant-pin set stable while one prepared retention cut is
/// finalized. New pins wait until the storage transaction commits or aborts.
pub(crate) struct RetentionFinalizationGuard<'a> {
    _pins: MutexGuard<'a, BTreeMap<u64, RetentionPin>>,
}

impl RetentionFloor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn pin(
        self: &Arc<Self>,
        participant: RetentionParticipant,
        sequence: SequenceNumber,
        table_id: Option<TableId>,
        reason: impl Into<String>,
    ) -> RetentionPinGuard {
        let pin_id = self.next_pin_id.fetch_add(1, Ordering::AcqRel) + 1;
        let pin = RetentionPin {
            id: pin_id,
            participant,
            sequence,
            table_id,
            reason: reason.into(),
        };
        self.pins
            .lock()
            .expect("retention floor mutex should not be poisoned")
            .insert(pin_id, pin);
        RetentionPinGuard {
            floor: self.clone(),
            pin_id,
        }
    }

    pub fn snapshot(&self) -> Vec<RetentionPin> {
        self.pins
            .lock()
            .expect("retention floor mutex should not be poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn restore_from_snapshot(pins: Vec<RetentionPin>) -> Arc<Self> {
        let floor = Self::new();
        let mut max_pin_id = 0;
        {
            let mut stored = floor
                .pins
                .lock()
                .expect("retention floor mutex should not be poisoned");
            for pin in pins {
                max_pin_id = max_pin_id.max(pin.id);
                stored.insert(pin.id, pin);
            }
        }
        floor.next_pin_id.store(max_pin_id, Ordering::Release);
        floor
    }

    pub fn lowest_pinned_sequence(&self) -> Option<SequenceNumber> {
        self.pins
            .lock()
            .expect("retention floor mutex should not be poisoned")
            .values()
            .map(|pin| pin.sequence)
            .min_by_key(|sequence| sequence.0)
    }

    pub fn published_read_floors(&self) -> RetentionReadFloors {
        *self
            .published_read_floors
            .lock()
            .expect("retention read-floor mutex should not be poisoned")
    }

    pub(crate) fn observe_published_read_floors(&self, floors: RetentionReadFloors) {
        let mut published = self
            .published_read_floors
            .lock()
            .expect("retention read-floor mutex should not be poisoned");
        *published = published.max(floors);
    }

    pub(crate) fn publish_read_floors_with_commit<T, E>(
        &self,
        floors: RetentionReadFloors,
        commit: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        // Readers acquire this mutex after they load a page. Holding it across
        // the durable commit makes the page linearize on one side of pruning:
        // it either returns before the commit or observes the new floors.
        // A failed commit leaves the process-local floors unchanged.
        let mut published = self
            .published_read_floors
            .lock()
            .expect("retention read-floor mutex should not be poisoned");
        let value = commit()?;
        *published = published.max(floors);
        Ok(value)
    }

    pub fn gc_watermarks(
        &self,
        latest_sequence: SequenceNumber,
        config: RetentionGcConfig,
    ) -> RetentionGcWatermarks {
        let pins = self.snapshot();
        let watermark = |resource| {
            let window_floor = SequenceNumber(
                latest_sequence
                    .0
                    .saturating_sub(config.window_for(resource)),
            );
            let relevant = pins
                .iter()
                .filter(|pin| pin_protects_resource(pin, resource))
                .collect::<Vec<_>>();
            let pinned_floor = relevant
                .iter()
                .map(|pin| pin.sequence)
                .min_by_key(|sequence| sequence.0);
            let safe_prune_before = pinned_floor
                .map(|pinned| pinned.min(window_floor))
                .unwrap_or(window_floor);
            RetentionGcWatermark {
                resource,
                latest_sequence,
                window_floor,
                pinned_floor,
                safe_prune_before,
                active_pin_count: relevant.len(),
            }
        };

        RetentionGcWatermarks {
            document_versions: watermark(RetentionGcResource::DocumentVersions),
            index_versions: watermark(RetentionGcResource::IndexVersions),
            registry_metadata: watermark(RetentionGcResource::RegistryMetadata),
            read_policy_metadata: watermark(RetentionGcResource::ReadPolicyMetadata),
            cdc_journal: watermark(RetentionGcResource::CdcJournal),
            pitr_exports: watermark(RetentionGcResource::PitrExport),
            shadow_materializers: watermark(RetentionGcResource::ShadowMaterializer),
            embedded_replicas: watermark(RetentionGcResource::EmbeddedReplica),
            transaction_sessions: watermark(RetentionGcResource::TransactionSession),
        }
    }

    pub(crate) fn guard_prepared_watermarks(
        &self,
        watermarks: &RetentionGcWatermarks,
    ) -> Result<RetentionFinalizationGuard<'_>> {
        let pins = self
            .pins
            .lock()
            .expect("retention floor mutex should not be poisoned");
        for watermark in [
            &watermarks.document_versions,
            &watermarks.index_versions,
            &watermarks.cdc_journal,
            &watermarks.pitr_exports,
        ] {
            if let Some(pin) = pins.values().find(|pin| {
                pin_protects_resource(pin, watermark.resource)
                    && pin.sequence < watermark.safe_prune_before
            }) {
                return Err(Error::conflict(format!(
                    "retention participant {:?} at sequence {} invalidated the prepared {:?} floor {}",
                    pin.participant,
                    pin.sequence.0,
                    watermark.resource,
                    watermark.safe_prune_before.0,
                )));
            }
        }
        Ok(RetentionFinalizationGuard { _pins: pins })
    }

    pub fn hard_delete_decision(
        &self,
        table_id: &TableId,
        current_head: SequenceNumber,
    ) -> HardDeleteDecision {
        let pins = self
            .pins
            .lock()
            .expect("retention floor mutex should not be poisoned");
        for pin in pins.values() {
            let pins_table = pin
                .table_id
                .as_ref()
                .map(|pinned| pinned == table_id)
                .unwrap_or(true);
            if pins_table && pin.sequence.0 <= current_head.0 {
                return HardDeleteDecision::Denied { pin: pin.clone() };
            }
        }
        HardDeleteDecision::Allowed
    }

    pub fn ensure_hard_delete_allowed(
        &self,
        table_id: &TableId,
        current_head: SequenceNumber,
    ) -> Result<()> {
        match self.hard_delete_decision(table_id, current_head) {
            HardDeleteDecision::Allowed => Ok(()),
            HardDeleteDecision::Denied { pin } => Err(Error::conflict(format!(
                "hard delete for table id {} is blocked by retention participant {:?} at sequence {} ({})",
                table_id, pin.participant, pin.sequence.0, pin.reason
            ))),
        }
    }
}

fn pin_protects_resource(pin: &RetentionPin, resource: RetentionGcResource) -> bool {
    match resource {
        RetentionGcResource::DocumentVersions | RetentionGcResource::IndexVersions => matches!(
            pin.participant,
            RetentionParticipant::TransactionSession
                | RetentionParticipant::ExportedSnapshot
                | RetentionParticipant::EmbeddedReplica
                | RetentionParticipant::ShadowMaterializer
        ),
        RetentionGcResource::RegistryMetadata | RetentionGcResource::ReadPolicyMetadata => true,
        RetentionGcResource::CdcJournal => matches!(
            pin.participant,
            RetentionParticipant::JournalConsumer
                | RetentionParticipant::CdcSubscription
                | RetentionParticipant::ExportedSnapshot
                | RetentionParticipant::EmbeddedReplica
                | RetentionParticipant::ShadowMaterializer
        ),
        RetentionGcResource::PitrExport => {
            pin.participant == RetentionParticipant::ExportedSnapshot
        }
        RetentionGcResource::ShadowMaterializer => {
            pin.participant == RetentionParticipant::ShadowMaterializer
        }
        RetentionGcResource::EmbeddedReplica => {
            pin.participant == RetentionParticipant::EmbeddedReplica
        }
        RetentionGcResource::TransactionSession => {
            pin.participant == RetentionParticipant::TransactionSession
        }
    }
}

impl Drop for RetentionPinGuard {
    fn drop(&mut self) {
        if let Ok(mut pins) = self.floor.pins.lock() {
            pins.remove(&self.pin_id);
        }
    }
}

impl TenantStore {
    pub fn retention_floor(&self) -> Arc<RetentionFloor> {
        self.retention_floor.clone()
    }

    pub fn pin_retention_participant(
        &self,
        participant: RetentionParticipant,
        sequence: SequenceNumber,
        table_id: Option<TableId>,
        reason: impl Into<String>,
    ) -> RetentionPinGuard {
        self.retention_floor
            .pin(participant, sequence, table_id, reason)
    }

    pub fn retention_gc_watermarks(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionGcWatermarks> {
        Ok(self
            .retention_floor
            .gc_watermarks(self.journal_progress()?.applied_head, config))
    }

    pub fn compact_retained_versions(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionGcSummary> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let _pin_barrier = self
            .retention_floor
            .guard_prepared_watermarks(&watermarks)?;
        let document_prune_before = watermarks.document_versions.safe_prune_before;
        let index_prune_before = watermarks.index_versions.safe_prune_before;
        if document_prune_before.0 == 0 && index_prune_before.0 == 0 {
            return Ok(RetentionGcSummary {
                watermarks,
                document_versions_pruned: 0,
                index_versions_pruned: 0,
            });
        }

        let write_txn = self
            .db
            .begin_write()
            .map_err(crate::store::map_redb_error)?;
        let document_versions_pruned =
            prune_redb_document_versions_before(&write_txn, document_prune_before)?;
        let index_versions_pruned =
            prune_redb_index_versions_before(&write_txn, index_prune_before)?;
        write_txn.commit().map_err(crate::store::map_redb_error)?;
        Ok(RetentionGcSummary {
            watermarks,
            document_versions_pruned,
            index_versions_pruned,
        })
    }

    pub fn retention_history_state(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistoryState> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, read_floors, _) = self.load_retention_checkpoint()?;
        self.retention_floor
            .observe_published_read_floors(read_floors);
        RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            desired_journal_floor(&watermarks).max(checkpoint.sequence()),
            read_floors.journal,
            checkpoint,
        )
    }

    pub fn prepare_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<PreparedRetentionHistory> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let (checkpoint, expected_read_floors, expected_checkpoint_blob) =
            self.load_retention_checkpoint()?;
        self.retention_floor
            .observe_published_read_floors(expected_read_floors);
        let desired_floor = desired_journal_floor(&watermarks).max(checkpoint.sequence());
        let before = RetentionHistoryState::new(
            watermarks.document_versions.latest_sequence,
            desired_floor,
            expected_read_floors.journal,
            checkpoint.clone(),
        )?;
        let journal_tail = self
            .read_durable_journal_from(SequenceNumber(checkpoint.sequence().0.saturating_add(1)))?;
        let candidate = checkpoint.advance(&journal_tail, desired_floor)?;
        self.fault_injector
            .check(crate::FaultPoint::RetentionCheckpointAfterPrepare)?;
        Ok(PreparedRetentionHistory {
            watermarks,
            before,
            candidate,
            expected_checkpoint_blob,
            expected_read_floors,
            expected_revision: None,
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
            expected_checkpoint_blob,
            expected_read_floors,
            ..
        } = prepared;
        let candidate_blob = serialize_retention_checkpoint(&candidate)?;
        let published_read_floors = expected_read_floors.max(RetentionReadFloors::new(
            watermarks.document_versions.safe_prune_before,
            watermarks.index_versions.safe_prune_before,
            candidate.sequence(),
        ));

        let write_txn = self
            .db
            .begin_write()
            .map_err(crate::store::map_redb_error)?;
        {
            let metadata = write_txn
                .open_table(crate::store::METADATA)
                .map_err(crate::store::map_redb_error)?;
            let current = metadata
                .get(RETENTION_CHECKPOINT_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| value.value().to_vec());
            if current != expected_checkpoint_blob {
                return Err(Error::conflict(
                    "retention checkpoint changed while compaction was prepared".to_string(),
                ));
            }
            let current_read_floors = RetentionReadFloors::new(
                metadata
                    .get(RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY)
                    .map_err(crate::store::map_redb_error)?
                    .map(|value| decode_retention_floor(value.value()))
                    .transpose()?
                    .unwrap_or_default(),
                metadata
                    .get(RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY)
                    .map_err(crate::store::map_redb_error)?
                    .map(|value| decode_retention_floor(value.value()))
                    .transpose()?
                    .unwrap_or_default(),
                metadata
                    .get(RETENTION_PHYSICAL_FLOOR_METADATA_KEY)
                    .map_err(crate::store::map_redb_error)?
                    .map(|value| decode_retention_floor(value.value()))
                    .transpose()?
                    .unwrap_or_default(),
            );
            if current_read_floors != expected_read_floors {
                return Err(Error::conflict(
                    "retention read floors changed while compaction was prepared".to_string(),
                ));
            }
        }
        let applied_head =
            redb_metadata_u64(&write_txn, crate::store::APPLIED_SEQUENCE_KEY)?.unwrap_or(0);
        if candidate.sequence().0 > applied_head {
            return Err(Error::conflict(format!(
                "retention checkpoint target {} exceeds current applied head {}",
                candidate.sequence().0,
                applied_head
            )));
        }

        let document_versions_pruned = prune_redb_document_versions_before(
            &write_txn,
            published_read_floors.document_versions,
        )?;
        let index_versions_pruned =
            prune_redb_index_versions_before(&write_txn, published_read_floors.index_versions)?;
        let journal_records_pruned = prune_redb_journal_through(&write_txn, candidate.sequence())?;
        {
            let mut metadata = write_txn
                .open_table(crate::store::METADATA)
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(RETENTION_CHECKPOINT_METADATA_KEY, candidate_blob.as_slice())
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_PHYSICAL_FLOOR_METADATA_KEY,
                    published_read_floors.journal.0.to_be_bytes().as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY,
                    published_read_floors
                        .document_versions
                        .0
                        .to_be_bytes()
                        .as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY,
                    published_read_floors
                        .index_versions
                        .0
                        .to_be_bytes()
                        .as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
        }
        self.fault_injector
            .check(crate::FaultPoint::RetentionCheckpointBeforeCommit)?;
        self.retention_floor
            .publish_read_floors_with_commit(published_read_floors, || {
                write_txn.commit().map_err(crate::store::map_redb_error)
            })?;
        self.fault_injector
            .check(crate::FaultPoint::RetentionCheckpointAfterCommit)?;

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
            document_versions_pruned,
            index_versions_pruned,
        })
    }

    pub fn compact_retained_history(
        &self,
        config: RetentionGcConfig,
    ) -> Result<RetentionHistorySummary> {
        self.finalize_retained_history(self.prepare_retained_history(config)?)
    }

    pub(crate) fn load_retention_checkpoint(
        &self,
    ) -> Result<(
        MaterializedRetentionCheckpoint,
        RetentionReadFloors,
        Option<Vec<u8>>,
    )> {
        let read_txn = self.db.begin_read().map_err(crate::store::map_redb_error)?;
        let metadata = match read_txn.open_table(crate::store::METADATA) {
            Ok(metadata) => metadata,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok((
                    MaterializedRetentionCheckpoint::genesis()?,
                    RetentionReadFloors::default(),
                    None,
                ));
            }
            Err(error) => return Err(crate::store::map_redb_error(error)),
        };
        let checkpoint_blob = metadata
            .get(RETENTION_CHECKPOINT_METADATA_KEY)
            .map_err(crate::store::map_redb_error)?
            .map(|value| value.value().to_vec());
        let checkpoint = checkpoint_blob
            .as_deref()
            .map(deserialize_retention_checkpoint)
            .transpose()?
            .unwrap_or(MaterializedRetentionCheckpoint::genesis()?);
        let read_floors = RetentionReadFloors::new(
            metadata
                .get(RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
            metadata
                .get(RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
            metadata
                .get(RETENTION_PHYSICAL_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
        );
        RetentionHistoryState::new(
            checkpoint.sequence(),
            checkpoint.sequence(),
            read_floors.journal,
            checkpoint.clone(),
        )?;
        Ok((checkpoint, read_floors, checkpoint_blob))
    }

    pub(crate) fn install_imported_retention_checkpoint(
        &self,
        checkpoint: &MaterializedRetentionCheckpoint,
    ) -> Result<()> {
        checkpoint.validate()?;
        let applied_head = self.journal_progress()?.applied_head;
        if checkpoint.sequence().0 > applied_head.0 {
            return Err(Error::InvalidInput(format!(
                "imported retention checkpoint {} exceeds restored applied head {}",
                checkpoint.sequence().0,
                applied_head.0
            )));
        }
        let checkpoint_blob = serialize_retention_checkpoint(checkpoint)?;
        let write_txn = self
            .db
            .begin_write()
            .map_err(crate::store::map_redb_error)?;
        {
            let mut metadata = write_txn
                .open_table(crate::store::METADATA)
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_CHECKPOINT_METADATA_KEY,
                    checkpoint_blob.as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_PHYSICAL_FLOOR_METADATA_KEY,
                    checkpoint.sequence().0.to_be_bytes().as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY,
                    checkpoint.sequence().0.to_be_bytes().as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
            metadata
                .insert(
                    RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY,
                    checkpoint.sequence().0.to_be_bytes().as_slice(),
                )
                .map_err(crate::store::map_redb_error)?;
        }
        write_txn.commit().map_err(crate::store::map_redb_error)?;
        self.retention_floor
            .observe_published_read_floors(RetentionReadFloors::new(
                checkpoint.sequence(),
                checkpoint.sequence(),
                checkpoint.sequence(),
            ));
        Ok(())
    }
}

impl crate::TenantReadSnapshot {
    pub(crate) fn retained_history_read_floors(&self) -> Result<RetentionReadFloors> {
        let metadata = match self.read_txn.open_table(crate::store::METADATA) {
            Ok(metadata) => metadata,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(RetentionReadFloors::default());
            }
            Err(error) => return Err(crate::store::map_redb_error(error)),
        };
        Ok(RetentionReadFloors::new(
            metadata
                .get(RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
            metadata
                .get(RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
            metadata
                .get(RETENTION_PHYSICAL_FLOOR_METADATA_KEY)
                .map_err(crate::store::map_redb_error)?
                .map(|value| decode_retention_floor(value.value()))
                .transpose()?
                .unwrap_or_default(),
        ))
    }

    pub(crate) fn retained_journal_physical_floor(&self) -> Result<SequenceNumber> {
        Ok(self.retained_history_read_floors()?.journal)
    }
}

pub(crate) fn desired_journal_floor(watermarks: &RetentionGcWatermarks) -> SequenceNumber {
    watermarks
        .cdc_journal
        .safe_prune_before
        .min(watermarks.pitr_exports.safe_prune_before)
}

pub(crate) fn serialize_retention_checkpoint(
    checkpoint: &MaterializedRetentionCheckpoint,
) -> Result<Vec<u8>> {
    checkpoint.validate()?;
    rmp_serde::to_vec_named(checkpoint).map_err(|error| Error::Serialization(error.to_string()))
}

pub(crate) fn deserialize_retention_checkpoint(
    bytes: &[u8],
) -> Result<MaterializedRetentionCheckpoint> {
    let checkpoint: MaterializedRetentionCheckpoint =
        rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))?;
    checkpoint.validate()?;
    Ok(checkpoint)
}

pub(crate) fn decode_retention_floor(bytes: &[u8]) -> Result<SequenceNumber> {
    let value: [u8; 8] = bytes.try_into().map_err(|_| {
        Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "retention physical floor metadata must contain eight bytes",
        )
    })?;
    Ok(SequenceNumber(u64::from_be_bytes(value)))
}

fn redb_metadata_u64(write_txn: &redb::WriteTransaction, key: &str) -> Result<Option<u64>> {
    let metadata = write_txn
        .open_table(crate::store::METADATA)
        .map_err(crate::store::map_redb_error)?;
    metadata
        .get(key)
        .map_err(crate::store::map_redb_error)?
        .map(|value| decode_retention_floor(value.value()).map(|sequence| sequence.0))
        .transpose()
}

fn prune_redb_journal_through(
    write_txn: &redb::WriteTransaction,
    floor: SequenceNumber,
) -> Result<u64> {
    if floor.0 == 0 {
        return Ok(0);
    }
    let mut journal = match write_txn.open_table(crate::store::COMMIT_LOG) {
        Ok(journal) => journal,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
        Err(error) => return Err(crate::store::map_redb_error(error)),
    };
    let keys = journal
        .range(..=floor.0)
        .map_err(crate::store::map_redb_error)?
        .map(|item| {
            item.map(|(key, _)| key.value())
                .map_err(crate::store::map_redb_error)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut pruned = 0_u64;
    for key in keys {
        if journal
            .remove(key)
            .map_err(crate::store::map_redb_error)?
            .is_some()
        {
            pruned = pruned.saturating_add(1);
        }
    }
    Ok(pruned)
}

macro_rules! impl_retention_floor_accessors {
    ($store:ty) => {
        impl $store {
            pub fn retention_floor(&self) -> Arc<RetentionFloor> {
                self.retention_floor.clone()
            }

            pub fn pin_retention_participant(
                &self,
                participant: RetentionParticipant,
                sequence: SequenceNumber,
                table_id: Option<TableId>,
                reason: impl Into<String>,
            ) -> RetentionPinGuard {
                self.retention_floor
                    .pin(participant, sequence, table_id, reason)
            }
        }
    };
}

impl_retention_floor_accessors!(SqliteTenantStore);
#[cfg(feature = "postgres")]
impl_retention_floor_accessors!(PostgresTenantStore);
#[cfg(feature = "mysql")]
impl_retention_floor_accessors!(MySqlTenantStore);
#[cfg(feature = "libsql")]
impl_retention_floor_accessors!(LibsqlReplicaTenantStore);

fn prune_redb_document_versions_before(
    write_txn: &redb::WriteTransaction,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    let mut versions = match write_txn.open_table(crate::store::DOCUMENT_VERSIONS) {
        Ok(versions) => versions,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
        Err(error) => return Err(crate::store::map_redb_error(error)),
    };
    let mut latest_anchor_by_document = BTreeMap::<Vec<u8>, Vec<u8>>::new();
    let mut candidates = Vec::<Vec<u8>>::new();
    for item in versions.iter().map_err(crate::store::map_redb_error)? {
        let (key, _) = item.map_err(crate::store::map_redb_error)?;
        let key = key.value().to_vec();
        let sequence = redb_version_sequence_from_key(key.as_slice())?;
        if sequence.0 <= prune_before.0 {
            let document_key = redb_version_document_key(key.as_slice())?.to_vec();
            latest_anchor_by_document.insert(document_key, key.clone());
        }
        if sequence.0 < prune_before.0 {
            candidates.push(key);
        }
    }
    let mut pruned = 0_u64;
    for key in candidates {
        if latest_anchor_by_document
            .get(redb_version_document_key(key.as_slice())?)
            .is_some_and(|anchor| anchor == &key)
        {
            continue;
        }
        if versions
            .remove(key.as_slice())
            .map_err(crate::store::map_redb_error)?
            .is_some()
        {
            pruned = pruned.saturating_add(1);
        }
    }
    Ok(pruned)
}

fn prune_redb_index_versions_before(
    write_txn: &redb::WriteTransaction,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    let mut versions = match write_txn.open_table(crate::store::INDEX_VERSIONS) {
        Ok(versions) => versions,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
        Err(error) => return Err(crate::store::map_redb_error(error)),
    };
    let mut keys_to_prune = Vec::<Vec<u8>>::new();
    for item in versions.iter().map_err(crate::store::map_redb_error)? {
        let (key, value) = item.map_err(crate::store::map_redb_error)?;
        let value: RedbRetentionIndexVersionValue = rmp_serde::from_slice(value.value())
            .map_err(|error| Error::Serialization(error.to_string()))?;
        if value
            .visible_until
            .is_some_and(|until| until <= prune_before.0)
        {
            keys_to_prune.push(key.value().to_vec());
        }
    }
    let mut pruned = 0_u64;
    for key in keys_to_prune {
        if versions
            .remove(key.as_slice())
            .map_err(crate::store::map_redb_error)?
            .is_some()
        {
            pruned = pruned.saturating_add(1);
        }
    }
    Ok(pruned)
}

#[derive(Debug, Deserialize)]
struct RedbRetentionIndexVersionValue {
    visible_until: Option<u64>,
}

fn redb_version_sequence_from_key(key: &[u8]) -> Result<SequenceNumber> {
    let sequence_bytes = key.get(key.len().saturating_sub(8)..).ok_or_else(|| {
        Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "version key is too short",
        )
    })?;
    let array: [u8; 8] = sequence_bytes.try_into().map_err(|_| {
        Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "version key has invalid sequence suffix",
        )
    })?;
    Ok(SequenceNumber(u64::from_be_bytes(array)))
}

fn redb_version_document_key(key: &[u8]) -> Result<&[u8]> {
    key.get(..key.len().saturating_sub(8)).ok_or_else(|| {
        Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "version key is too short",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_delete_denied_while_retention_floor_pins_table_identity() {
        let floor = RetentionFloor::new();
        let table_id = TableId::new();
        let _pin = floor.pin(
            RetentionParticipant::ExportedSnapshot,
            SequenceNumber(4),
            Some(table_id.clone()),
            "snapshot export",
        );

        assert!(matches!(
            floor.hard_delete_decision(&table_id, SequenceNumber(7)),
            HardDeleteDecision::Denied { .. }
        ));
    }

    #[test]
    fn retention_floor_survives_crash_recovery() {
        let floor = RetentionFloor::new();
        let table_id = TableId::new();
        let _pin = floor.pin(
            RetentionParticipant::JournalConsumer,
            SequenceNumber(9),
            Some(table_id.clone()),
            "stream cursor",
        );

        let recovered = RetentionFloor::restore_from_snapshot(floor.snapshot());
        assert!(matches!(
            recovered.hard_delete_decision(&table_id, SequenceNumber(10)),
            HardDeleteDecision::Denied { .. }
        ));
        assert_eq!(recovered.lowest_pinned_sequence(), Some(SequenceNumber(9)));
    }

    #[test]
    fn retention_gc_watermarks_are_resource_specific() {
        let floor = RetentionFloor::new();
        let _cdc_pin = floor.pin(
            RetentionParticipant::CdcSubscription,
            SequenceNumber(4),
            None,
            "cdc cursor",
        );
        let _transaction_pin = floor.pin(
            RetentionParticipant::TransactionSession,
            SequenceNumber(6),
            None,
            "read transaction",
        );

        let watermarks = floor.gc_watermarks(
            SequenceNumber(10),
            RetentionGcConfig::new(2).expect("config should build"),
        );

        assert_eq!(
            watermarks.document_versions.pinned_floor,
            Some(SequenceNumber(6))
        );
        assert_eq!(watermarks.document_versions.active_pin_count, 1);
        assert_eq!(watermarks.cdc_journal.pinned_floor, Some(SequenceNumber(4)));
        assert_eq!(watermarks.cdc_journal.active_pin_count, 1);
        assert_eq!(
            watermarks.registry_metadata.pinned_floor,
            Some(SequenceNumber(4))
        );
        assert_eq!(watermarks.registry_metadata.active_pin_count, 2);
    }
}
