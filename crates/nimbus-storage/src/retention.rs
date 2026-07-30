use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result, SequenceNumber, TableId};
use redb::ReadableTable;
use serde::{Deserialize, Serialize};

#[cfg(feature = "libsql")]
use crate::LibsqlReplicaTenantStore;
#[cfg(feature = "mysql")]
use crate::MySqlTenantStore;
#[cfg(feature = "postgres")]
use crate::PostgresTenantStore;
use crate::{SqliteTenantStore, TenantStore};

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
    pub history_window_sequences: u64,
}

impl RetentionGcConfig {
    pub const fn retain_all() -> Self {
        Self {
            history_window_sequences: u64::MAX,
        }
    }

    pub fn new(history_window_sequences: u64) -> Result<Self> {
        if history_window_sequences == 0 {
            return Err(Error::InvalidInput(
                "retention history window must retain at least one sequence".to_string(),
            ));
        }
        Ok(Self {
            history_window_sequences,
        })
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
}

pub struct RetentionPinGuard {
    floor: Arc<RetentionFloor>,
    pin_id: u64,
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

    pub fn gc_watermarks(
        &self,
        latest_sequence: SequenceNumber,
        config: RetentionGcConfig,
    ) -> RetentionGcWatermarks {
        let pins = self.snapshot();
        let window_floor = SequenceNumber(
            latest_sequence
                .0
                .saturating_sub(config.history_window_sequences),
        );
        let watermark = |resource| {
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
