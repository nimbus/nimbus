use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

use crate::types::{DocumentId, SequenceNumber, TableName, TenantId};

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

/// Machine-readable guidance for retrying an operation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retryability {
    /// The operation may be retried immediately by a bounded transparent loop.
    Retryable,
    /// Retry only at the caller boundary after applying backoff.
    RetryableAfterBackoff,
    /// Discard the transaction snapshot and execute again from a fresh snapshot.
    RestartTransaction,
    /// Repeating the same operation cannot resolve the failure.
    Terminal,
}

/// Closed taxonomy of failures produced by the mutation commit path.
///
/// This enum is intentionally exhaustive. Protocol adapters match it without
/// a wildcard so adding a commit class forces every wire mapping to be updated,
/// while adding an unrelated [`Error`] variant does not affect those mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitErrorClass {
    Conflict,
    Overloaded,
    CommitterFull,
    RejectedBeforeExecution,
    RateLimited,
    OutOfRetention,
    CapExceeded,
}

impl CommitErrorClass {
    /// Returns the retry policy for this commit class.
    ///
    /// `conflict_retryable` preserves the per-instance policy carried by
    /// [`Error::Conflict`] and is ignored for every other class. Keeping that
    /// exceptional datum as an argument lets the class remain a simple closed
    /// taxonomy while making this the single source of commit retry policy.
    pub fn retryability(&self, conflict_retryable: bool) -> Retryability {
        match self {
            Self::Conflict if conflict_retryable => Retryability::Retryable,
            Self::Conflict | Self::CapExceeded => Retryability::Terminal,
            Self::Overloaded | Self::CommitterFull | Self::RateLimited => {
                Retryability::RetryableAfterBackoff
            }
            Self::RejectedBeforeExecution => Retryability::Retryable,
            Self::OutOfRetention => Retryability::RestartTransaction,
        }
    }
}

/// The prepare-time resource dimension that exceeded its transaction cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationCap {
    ReadBytes,
    WriteBytes,
    DocumentsScanned,
    DocumentsWritten,
    IndexRangeCalls,
}

impl MutationCap {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadBytes => "read_bytes",
            Self::WriteBytes => "write_bytes",
            Self::DocumentsScanned => "documents_scanned",
            Self::DocumentsWritten => "documents_written",
            Self::IndexRangeCalls => "index_range_calls",
        }
    }
}

impl std::fmt::Display for MutationCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Core Nimbus error type.
#[derive(Debug, Clone, ThisError)]
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

    #[error("conflict: {message}")]
    Conflict {
        message: String,
        conflicting_sequence: Option<SequenceNumber>,
        retryable: bool,
        attempts: Option<usize>,
    },

    #[error("overloaded: {message}")]
    Overloaded { message: String },

    #[error("committer full: {message}")]
    CommitterFull { message: String, capacity: usize },

    #[error("committer lease fenced: owner {owner_id} at epoch {epoch}")]
    CommitterFenced { owner_id: String, epoch: u64 },

    #[error("rejected before execution: {message}")]
    RejectedBeforeExecution { message: String },

    #[error("rate limited: {message} (retry after {retry_after:?})")]
    RateLimited {
        message: String,
        retry_after: Duration,
    },

    #[error("out of retention: {message}")]
    OutOfRetention {
        message: String,
        minimum_sequence: Option<SequenceNumber>,
    },

    #[error("mutation cap exceeded [{cap}]: observed {observed}, limit {limit}")]
    CapExceeded {
        cap: MutationCap,
        observed: u64,
        limit: u64,
    },

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

/// Validates that `value` is non-blank, returning it as a `String`.
///
/// Shared by the many newtype constructors across crates that reject
/// empty-or-whitespace-only strings with a uniform `Error::InvalidInput`.
pub fn non_empty(value: impl Into<String>, field: &str) -> Result<String> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(Error::InvalidInput(format!("{field} must not be empty")));
    }
    Ok(value)
}

impl Error {
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
            conflicting_sequence: None,
            retryable: false,
            attempts: None,
        }
    }

    pub fn retryable_conflict(
        message: impl Into<String>,
        conflicting_sequence: Option<SequenceNumber>,
    ) -> Self {
        Self::Conflict {
            message: message.into(),
            conflicting_sequence,
            retryable: true,
            attempts: None,
        }
    }

    pub fn overloaded(message: impl Into<String>) -> Self {
        Self::Overloaded {
            message: message.into(),
        }
    }

    pub fn committer_full(message: impl Into<String>, capacity: usize) -> Self {
        Self::CommitterFull {
            message: message.into(),
            capacity,
        }
    }

    pub fn rejected_before_execution(message: impl Into<String>) -> Self {
        Self::RejectedBeforeExecution {
            message: message.into(),
        }
    }

    pub fn rate_limited(message: impl Into<String>, retry_after: Duration) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after,
        }
    }

    pub fn out_of_retention(
        message: impl Into<String>,
        minimum_sequence: Option<SequenceNumber>,
    ) -> Self {
        Self::OutOfRetention {
            message: message.into(),
            minimum_sequence,
        }
    }

    pub fn cap_exceeded(cap: MutationCap, observed: u64, limit: u64) -> Self {
        Self::CapExceeded {
            cap,
            observed,
            limit,
        }
    }

    /// Returns the closed commit-path class, or `None` for non-commit errors.
    pub fn commit_class(&self) -> Option<CommitErrorClass> {
        match self {
            Self::Conflict { .. } => Some(CommitErrorClass::Conflict),
            Self::Overloaded { .. } => Some(CommitErrorClass::Overloaded),
            Self::CommitterFull { .. } => Some(CommitErrorClass::CommitterFull),
            Self::RejectedBeforeExecution { .. } => Some(CommitErrorClass::RejectedBeforeExecution),
            Self::RateLimited { .. } => Some(CommitErrorClass::RateLimited),
            Self::OutOfRetention { .. } => Some(CommitErrorClass::OutOfRetention),
            Self::CapExceeded { .. } => Some(CommitErrorClass::CapExceeded),
            _ => None,
        }
    }

    pub fn retryability(&self) -> Retryability {
        if let Some(class) = self.commit_class() {
            let conflict_retryable = matches!(
                self,
                Self::Conflict {
                    retryable: true,
                    ..
                }
            );
            return class.retryability(conflict_retryable);
        }

        match self {
            Self::Storage {
                kind:
                    StorageErrorKind::Busy
                    | StorageErrorKind::Io
                    | StorageErrorKind::Transient
                    | StorageErrorKind::Unavailable,
                ..
            }
            | Self::Transport(_) => Retryability::RetryableAfterBackoff,
            Self::HistoricalRead {
                kind: HistoricalReadErrorKind::SnapshotUnavailable,
                ..
            } => Retryability::RetryableAfterBackoff,
            Self::Cancelled
            | Self::TenantNotFound(_)
            | Self::DocumentNotFound(_)
            | Self::ScheduledJobNotFound(_)
            | Self::NotFound(_)
            | Self::AlreadyExists(_)
            | Self::ResourceExhausted(_)
            | Self::PermissionDenied(_)
            | Self::Conflict { .. }
            | Self::Overloaded { .. }
            | Self::CommitterFull { .. }
            | Self::CommitterFenced { .. }
            | Self::RejectedBeforeExecution { .. }
            | Self::RateLimited { .. }
            | Self::OutOfRetention { .. }
            | Self::CapExceeded { .. }
            | Self::PreconditionFailed(_)
            | Self::InvalidInput(_)
            | Self::MissingIndex { .. }
            | Self::SchemaValidation(_)
            | Self::SchemaNotFound(_)
            | Self::Storage { .. }
            | Self::HistoricalRead { .. }
            | Self::Serialization(_)
            | Self::Internal(_) => Retryability::Terminal,
        }
    }

    /// Whether this error is stable for the same user-supplied operation.
    ///
    /// Environmental failures are deliberately excluded so callers never
    /// cache transient infrastructure conditions as user bugs.
    pub fn is_deterministic_user_error(&self) -> bool {
        matches!(
            self,
            Self::TenantNotFound(_)
                | Self::DocumentNotFound(_)
                | Self::ScheduledJobNotFound(_)
                | Self::NotFound(_)
                | Self::AlreadyExists(_)
                | Self::PermissionDenied(_)
                | Self::PreconditionFailed(_)
                | Self::InvalidInput(_)
                | Self::MissingIndex { .. }
                | Self::SchemaValidation(_)
                | Self::SchemaNotFound(_)
                | Self::CapExceeded { .. }
        )
    }

    pub fn is_environmental(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::ResourceExhausted(_)
                | Self::Overloaded { .. }
                | Self::CommitterFull { .. }
                | Self::CommitterFenced { .. }
                | Self::RejectedBeforeExecution { .. }
                | Self::RateLimited { .. }
                | Self::OutOfRetention { .. }
                | Self::Storage { .. }
                | Self::HistoricalRead { .. }
                | Self::Serialization(_)
                | Self::Transport(_)
                | Self::Internal(_)
        )
    }

    pub fn conflicting_sequence(&self) -> Option<SequenceNumber> {
        match self {
            Self::Conflict {
                conflicting_sequence,
                ..
            } => *conflicting_sequence,
            _ => None,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => Some(*retry_after),
            _ => None,
        }
    }

    pub fn is_overload_class(&self) -> bool {
        matches!(
            self,
            Self::Overloaded { .. }
                | Self::CommitterFull { .. }
                | Self::RejectedBeforeExecution { .. }
                | Self::RateLimited { .. }
        )
    }

    pub fn with_conflict_attempts(self, attempts: usize) -> Self {
        match self {
            Self::Conflict {
                message,
                conflicting_sequence,
                retryable,
                ..
            } => Self::Conflict {
                message,
                conflicting_sequence,
                retryable,
                attempts: Some(attempts),
            },
            other => other,
        }
    }

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
    fn retryable_conflict_preserves_occ_metadata_and_attempts() {
        let error = Error::retryable_conflict("write raced", Some(SequenceNumber(42)))
            .with_conflict_attempts(4);

        assert!(matches!(
            error,
            Error::Conflict {
                ref message,
                conflicting_sequence: Some(SequenceNumber(42)),
                retryable: true,
                attempts: Some(4),
            } if message == "write raced"
        ));
    }

    #[test]
    fn retryability_is_explicit_for_each_commit_error_class() {
        let cases = [
            (
                Error::retryable_conflict("race", Some(SequenceNumber(1))),
                Retryability::Retryable,
            ),
            (Error::conflict("terminal conflict"), Retryability::Terminal),
            (
                Error::overloaded("node pressure"),
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::committer_full("bounded inbox", 64),
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::rejected_before_execution("admission shed"),
                Retryability::Retryable,
            ),
            (
                Error::rate_limited("tenant write rate", Duration::from_millis(250)),
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::out_of_retention("snapshot expired", Some(SequenceNumber(9))),
                Retryability::RestartTransaction,
            ),
            (
                Error::cap_exceeded(MutationCap::WriteBytes, 11, 10),
                Retryability::Terminal,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(
                error.retryability(),
                expected,
                "unexpected class for {error}"
            );
        }
    }

    #[test]
    fn commit_class_covers_every_commit_error_variant() {
        let cases = [
            (
                Error::retryable_conflict("race", Some(SequenceNumber(1))),
                CommitErrorClass::Conflict,
            ),
            (
                Error::overloaded("node pressure"),
                CommitErrorClass::Overloaded,
            ),
            (
                Error::committer_full("bounded inbox", 64),
                CommitErrorClass::CommitterFull,
            ),
            (
                Error::rejected_before_execution("admission shed"),
                CommitErrorClass::RejectedBeforeExecution,
            ),
            (
                Error::rate_limited("tenant write rate", Duration::from_millis(250)),
                CommitErrorClass::RateLimited,
            ),
            (
                Error::out_of_retention("snapshot expired", Some(SequenceNumber(9))),
                CommitErrorClass::OutOfRetention,
            ),
            (
                Error::cap_exceeded(MutationCap::WriteBytes, 11, 10),
                CommitErrorClass::CapExceeded,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.commit_class(), Some(expected), "{error}");
        }

        for error in [
            Error::storage(StorageErrorKind::Unavailable, "provider unavailable"),
            Error::Transport("connection timed out".to_string()),
            Error::InvalidInput("bad request".to_string()),
            Error::SchemaValidation("wrong shape".to_string()),
        ] {
            assert_eq!(error.commit_class(), None, "{error}");
        }
    }

    #[test]
    fn environmental_errors_are_never_deterministic_user_errors() {
        let errors = [
            Error::storage(StorageErrorKind::Io, "disk read failed"),
            Error::storage(StorageErrorKind::Unavailable, "provider unavailable"),
            Error::Transport("connection timed out".to_string()),
            Error::Internal("provider timeout".to_string()),
            Error::overloaded("node pressure"),
            Error::rate_limited("tenant write rate", Duration::from_secs(1)),
            Error::out_of_retention("snapshot expired", None),
        ];

        for error in errors {
            assert!(error.is_environmental(), "expected environmental: {error}");
            assert!(
                !error.is_deterministic_user_error(),
                "environmental error was classified as deterministic: {error}"
            );
        }
        assert!(Error::cap_exceeded(MutationCap::ReadBytes, 2, 1).is_deterministic_user_error());
    }

    #[test]
    fn storage_error_kind_round_trips_from_string() {
        assert_eq!(
            StorageErrorKind::from_str("corruption").expect("kind should parse"),
            StorageErrorKind::Corruption
        );
    }

    #[test]
    fn non_empty_accepts_trimmed_value() {
        let value = non_empty("  workload-a  ".to_string(), "field").expect("value should pass");
        assert_eq!(value, "  workload-a  ");
    }

    #[test]
    fn non_empty_rejects_empty_and_whitespace_only() {
        let empty = non_empty(String::new(), "widget name").unwrap_err();
        let blank = non_empty("   ".to_string(), "widget name").unwrap_err();

        assert!(
            matches!(empty, Error::InvalidInput(ref message) if message == "widget name must not be empty")
        );
        assert!(
            matches!(blank, Error::InvalidInput(ref message) if message == "widget name must not be empty")
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
