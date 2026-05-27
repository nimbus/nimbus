use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result, SequenceNumber, TableId};
use serde::{Deserialize, Serialize};

use crate::TenantStore;

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
            HardDeleteDecision::Denied { pin } => Err(Error::Conflict(format!(
                "hard delete for table id {} is blocked by retention participant {:?} at sequence {} ({})",
                table_id, pin.participant, pin.sequence.0, pin.reason
            ))),
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
}
