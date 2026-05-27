use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

use crate::schema::{IndexDefinition, IndexState, TableSchema};
use crate::trigger::{TriggerDeliveryCursor, TriggerWriteOrigin};
use crate::types::{DocumentId, IndexId, SequenceNumber, TableId, TableName, Timestamp};
use crate::{Document, Error, ResourcePathBinding, Result};

/// A mutation request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mutation {
    Insert {
        table: TableName,
        #[serde(default)]
        id: Option<DocumentId>,
        fields: serde_json::Map<String, Value>,
    },
    Update {
        table: TableName,
        id: DocumentId,
        patch: serde_json::Map<String, Value>,
    },
    Delete {
        table: TableName,
        id: DocumentId,
    },
}

/// The kind of write recorded in the commit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOpType {
    Insert,
    Update,
    Delete,
}

/// A write recorded in the commit log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteOp {
    pub table: TableName,
    pub table_id: TableId,
    pub op_type: WriteOpType,
    pub doc_id: DocumentId,
    #[serde(default)]
    pub resource_path_binding: Option<ResourcePathBinding>,
    #[serde(default)]
    pub trigger_write_origin: Option<TriggerWriteOrigin>,
    #[serde(default)]
    pub previous: Option<Document>,
    #[serde(default)]
    pub current: Option<Document>,
}

/// A committed mutation batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitEntry {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    pub writes: Vec<WriteOp>,
}

impl CommitEntry {
    /// Returns the distinct logical tables touched by the commit.
    pub fn affected_tables(&self) -> HashSet<TableName> {
        self.writes
            .iter()
            .map(|write| write.table.clone())
            .collect()
    }

    /// Returns the distinct stable logical table instances touched by the commit.
    pub fn affected_table_ids(&self) -> HashSet<TableId> {
        self.writes
            .iter()
            .map(|write| write.table_id.clone())
            .collect()
    }
}

const TENANT_EVENT_RECORD_VERSION: u16 = 3;

/// Lifecycle transitions for a stable logical table identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TableLifecycleEvent {
    StageHidden {
        table: TableName,
        table_id: TableId,
    },
    ActivateHidden {
        table: TableName,
        table_id: TableId,
        #[serde(default)]
        replaced_table_id: Option<TableId>,
    },
    MarkDeleting {
        table: TableName,
        table_id: TableId,
    },
    HardDelete {
        table: TableName,
        table_id: TableId,
    },
}

/// Schema/index lifecycle transition recorded in the tenant event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchemaChangeEvent {
    SetTable {
        table: TableName,
        table_id: TableId,
        #[serde(default)]
        previous: Option<TableSchema>,
        current: TableSchema,
    },
    DeleteTable {
        table: TableName,
        #[serde(default)]
        table_id: Option<TableId>,
        #[serde(default)]
        previous: Option<TableSchema>,
    },
}

/// Index lifecycle metadata extracted from schema changes for diagnostics and
/// replay consumers that care about index identity separately from schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexLifecycleEvent {
    pub table: TableName,
    pub table_id: TableId,
    pub index_id: IndexId,
    pub state: IndexState,
    pub definition: IndexDefinition,
}

/// Ordered durable event for every replay-affecting tenant state transition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TenantEventKind {
    DocumentWrite { writes: Vec<WriteOp> },
    SchemaChange { change: SchemaChangeEvent },
    TableLifecycle { lifecycle: TableLifecycleEvent },
    IndexLifecycle { index: IndexLifecycleEvent },
    ScheduledExecution { execution_id: String },
    TriggerDelivery { cursor: TriggerDeliveryCursor },
    Barrier { label: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct TenantEventRecordHashPayload<'a> {
    version: u16,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &'a [TenantEventKind],
    writes: &'a [WriteOp],
    #[serde(default)]
    scheduled_execution_id: Option<&'a str>,
}

/// A replayable event record stored in the durable tenant event journal.
///
/// `writes` and `scheduled_execution_id` remain as compatibility projections
/// while engine and adapter call sites migrate from document-mutation language
/// to tenant-event language. The authoritative durable payload is `events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenantEventRecord {
    pub version: u16,
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    #[serde(default)]
    pub events: Vec<TenantEventKind>,
    pub writes: Vec<WriteOp>,
    #[serde(default)]
    pub scheduled_execution_id: Option<String>,
    pub integrity_sha256: [u8; 32],
}

impl TenantEventRecord {
    pub fn new(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        writes: Vec<WriteOp>,
        scheduled_execution_id: Option<String>,
    ) -> Result<Self> {
        let mut events = Vec::new();
        if !writes.is_empty() {
            events.push(TenantEventKind::DocumentWrite {
                writes: writes.clone(),
            });
        }
        if let Some(execution_id) = scheduled_execution_id.clone() {
            events.push(TenantEventKind::ScheduledExecution { execution_id });
        }
        Self::from_events(sequence, timestamp, events)
    }

    pub fn from_events(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        events: Vec<TenantEventKind>,
    ) -> Result<Self> {
        let writes = document_writes_for_events(&events);
        let scheduled_execution_id = events.iter().find_map(|event| match event {
            TenantEventKind::ScheduledExecution { execution_id } => Some(execution_id.clone()),
            _ => None,
        });
        let mut record = Self {
            version: TENANT_EVENT_RECORD_VERSION,
            sequence,
            timestamp,
            events,
            writes,
            scheduled_execution_id,
            integrity_sha256: [0; 32],
        };
        record.integrity_sha256 = record.compute_integrity()?;
        Ok(record)
    }

    pub fn schema_change(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        change: SchemaChangeEvent,
    ) -> Result<Self> {
        Self::from_events(
            sequence,
            timestamp,
            vec![TenantEventKind::SchemaChange { change }],
        )
    }

    pub fn table_lifecycle(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        lifecycle: TableLifecycleEvent,
    ) -> Result<Self> {
        Self::from_events(
            sequence,
            timestamp,
            vec![TenantEventKind::TableLifecycle { lifecycle }],
        )
    }

    pub fn trigger_delivery(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        cursor: TriggerDeliveryCursor,
    ) -> Result<Self> {
        Self::from_events(
            sequence,
            timestamp,
            vec![TenantEventKind::TriggerDelivery { cursor }],
        )
    }

    pub fn barrier(sequence: SequenceNumber, timestamp: Timestamp, label: String) -> Result<Self> {
        Self::from_events(
            sequence,
            timestamp,
            vec![TenantEventKind::Barrier { label }],
        )
    }

    pub fn events(&self) -> &[TenantEventKind] {
        self.events.as_slice()
    }

    pub fn compatibility_document_record(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        writes: Vec<WriteOp>,
        scheduled_execution_id: Option<String>,
    ) -> Result<Self> {
        let mut record = Self {
            version: TENANT_EVENT_RECORD_VERSION,
            writes,
            scheduled_execution_id,
            events: Vec::new(),
            sequence,
            timestamp,
            integrity_sha256: [0; 32],
        };
        record.events = compatibility_events(&record.writes, &record.scheduled_execution_id);
        record.integrity_sha256 = record.compute_integrity()?;
        Ok(record)
    }

    pub fn validate_integrity(&self) -> Result<()> {
        let expected = self.compute_integrity()?;
        if self.integrity_sha256 == expected {
            Ok(())
        } else {
            Err(Error::Internal(format!(
                "durable mutation record {} failed integrity verification",
                self.sequence.0
            )))
        }
    }

    pub fn as_commit_entry(&self) -> CommitEntry {
        CommitEntry {
            sequence: self.sequence,
            timestamp: self.timestamp,
            writes: self.writes.clone(),
        }
    }

    pub fn into_commit_entry(self) -> CommitEntry {
        CommitEntry {
            sequence: self.sequence,
            timestamp: self.timestamp,
            writes: self.writes,
        }
    }

    fn compute_integrity(&self) -> Result<[u8; 32]> {
        let payload = TenantEventRecordHashPayload {
            version: self.version,
            sequence: self.sequence,
            timestamp: self.timestamp,
            events: &self.events,
            writes: &self.writes,
            scheduled_execution_id: self.scheduled_execution_id.as_deref(),
        };
        let encoded = rmp_serde::to_vec_named(&payload)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok(Sha256::digest(encoded).into())
    }
}

fn document_writes_for_events(events: &[TenantEventKind]) -> Vec<WriteOp> {
    events
        .iter()
        .flat_map(|event| match event {
            TenantEventKind::DocumentWrite { writes } => writes.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn compatibility_events(
    writes: &[WriteOp],
    scheduled_execution_id: &Option<String>,
) -> Vec<TenantEventKind> {
    let mut events = Vec::new();
    if !writes.is_empty() {
        events.push(TenantEventKind::DocumentWrite {
            writes: writes.to_vec(),
        });
    }
    if let Some(execution_id) = scheduled_execution_id.clone() {
        events.push(TenantEventKind::ScheduledExecution { execution_id });
    }
    events
}

/// Compatibility alias while call sites migrate to tenant-event terminology.
pub type DurableMutationRecord = TenantEventRecord;
