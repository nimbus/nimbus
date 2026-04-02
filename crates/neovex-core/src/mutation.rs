use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

use crate::types::{DocumentId, SequenceNumber, TableName, Timestamp};
use crate::{Document, Error, Result};

/// A mutation request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mutation {
    Insert {
        table: TableName,
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
    pub op_type: WriteOpType,
    pub doc_id: DocumentId,
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
}

const DURABLE_MUTATION_RECORD_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
struct DurableMutationRecordHashPayload<'a> {
    version: u16,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &'a [WriteOp],
    #[serde(default)]
    scheduled_execution_id: Option<&'a str>,
}

/// A replayable mutation record stored in the durable journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableMutationRecord {
    pub version: u16,
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    pub writes: Vec<WriteOp>,
    #[serde(default)]
    pub scheduled_execution_id: Option<String>,
    pub integrity_sha256: [u8; 32],
}

impl DurableMutationRecord {
    pub fn new(
        sequence: SequenceNumber,
        timestamp: Timestamp,
        writes: Vec<WriteOp>,
        scheduled_execution_id: Option<String>,
    ) -> Result<Self> {
        let mut record = Self {
            version: DURABLE_MUTATION_RECORD_VERSION,
            sequence,
            timestamp,
            writes,
            scheduled_execution_id,
            integrity_sha256: [0; 32],
        };
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
        let payload = DurableMutationRecordHashPayload {
            version: self.version,
            sequence: self.sequence,
            timestamp: self.timestamp,
            writes: &self.writes,
            scheduled_execution_id: self.scheduled_execution_id.as_deref(),
        };
        let encoded = rmp_serde::to_vec_named(&payload)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok(Sha256::digest(encoded).into())
    }
}
