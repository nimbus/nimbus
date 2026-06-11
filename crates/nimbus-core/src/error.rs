use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

use crate::types::{DocumentId, TableName, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageErrorKind {
    Busy,
    Corruption,
    Io,
    Other,
    Transient,
    Unavailable,
}

impl StorageErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Corruption => "corruption",
            Self::Io => "io",
            Self::Other => "other",
            Self::Transient => "transient",
            Self::Unavailable => "unavailable",
        }
    }
}

impl std::fmt::Display for StorageErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StorageErrorKind {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "busy" => Ok(Self::Busy),
            "corruption" => Ok(Self::Corruption),
            "io" => Ok(Self::Io),
            "other" => Ok(Self::Other),
            "transient" => Ok(Self::Transient),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(format!("unknown storage error kind '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalReadErrorKind {
    CursorMismatch,
    FormatMismatch,
    PolicySnapshotMissing,
    RetentionExpired,
    SnapshotUnavailable,
    TimestampOutOfRange,
    UnsupportedAdapter,
    UnsupportedBackend,
}

impl HistoricalReadErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CursorMismatch => "cursor_mismatch",
            Self::FormatMismatch => "format_mismatch",
            Self::PolicySnapshotMissing => "policy_snapshot_missing",
            Self::RetentionExpired => "retention_expired",
            Self::SnapshotUnavailable => "snapshot_unavailable",
            Self::TimestampOutOfRange => "timestamp_out_of_range",
            Self::UnsupportedAdapter => "unsupported_adapter",
            Self::UnsupportedBackend => "unsupported_backend",
        }
    }
}

impl std::fmt::Display for HistoricalReadErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Core Nimbus error type.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("operation canceled")]
    Cancelled,

    #[error("tenant not found: {0}")]
    TenantNotFound(TenantId),

    #[error("document not found: {0}")]
    DocumentNotFound(DocumentId),

    #[error("scheduled job not found: {0}")]
    ScheduledJobNotFound(DocumentId),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("resource already exists: {0}")]
    AlreadyExists(String),

    #[error("resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("structured query requires an index covering fields: {}", fields.join(", "))]
    MissingIndex { fields: Vec<String> },

    #[error("schema validation error: {0}")]
    SchemaValidation(String),

    #[error("schema not found for table: {0}")]
    SchemaNotFound(TableName),

    #[error("storage error [{kind}]: {message}")]
    Storage {
        kind: StorageErrorKind,
        message: String,
    },

    #[error("historical read error [{kind}]: {message}")]
    HistoricalRead {
        kind: HistoricalReadErrorKind,
        message: String,
    },

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Shared result alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn storage(kind: StorageErrorKind, message: impl Into<String>) -> Self {
        Self::Storage {
            kind,
            message: message.into(),
        }
    }

    pub fn storage_kind(&self) -> Option<StorageErrorKind> {
        match self {
            Self::Storage { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    pub fn storage_message(&self) -> Option<&str> {
        match self {
            Self::Storage { message, .. } => Some(message.as_str()),
            _ => None,
        }
    }

    pub fn historical_read(kind: HistoricalReadErrorKind, message: impl Into<String>) -> Self {
        Self::HistoricalRead {
            kind,
            message: message.into(),
        }
    }

    pub fn historical_read_kind(&self) -> Option<HistoricalReadErrorKind> {
        match self {
            Self::HistoricalRead { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_error_helper_preserves_kind_and_message() {
        let error = Error::storage(StorageErrorKind::Unavailable, "database unavailable");

        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Unavailable));
        assert_eq!(error.storage_message(), Some("database unavailable"));
        assert_eq!(
            error.to_string(),
            "storage error [unavailable]: database unavailable"
        );
    }

    #[test]
    fn storage_error_kind_round_trips_from_string() {
        assert_eq!(
            StorageErrorKind::from_str("corruption").expect("kind should parse"),
            StorageErrorKind::Corruption
        );
    }

    #[test]
    fn historical_read_error_helper_preserves_kind_and_message() {
        let error = Error::historical_read(
            HistoricalReadErrorKind::RetentionExpired,
            "read timestamp is older than the retention floor",
        );

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::RetentionExpired)
        );
        assert_eq!(
            error.to_string(),
            "historical read error [retention_expired]: read timestamp is older than the retention floor"
        );
    }
}
