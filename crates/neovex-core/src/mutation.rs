use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::Document;
use crate::types::{DocumentId, SequenceNumber, TableName, Timestamp};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Document>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
