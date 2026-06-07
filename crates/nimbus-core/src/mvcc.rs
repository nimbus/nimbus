use serde::{Deserialize, Serialize};

use crate::{
    Error, HistoricalReadErrorKind, IndexId, Result, SequenceNumber, TableId, Timestamp,
    types::validate_logical_name,
};

/// Commit log position that made a version durable and visible to readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct CommitSequence(SequenceNumber);

impl CommitSequence {
    pub fn new(sequence: SequenceNumber) -> Self {
        Self(sequence)
    }

    pub fn sequence(self) -> SequenceNumber {
        self.0
    }
}

impl From<SequenceNumber> for CommitSequence {
    fn from(sequence: SequenceNumber) -> Self {
        Self::new(sequence)
    }
}

impl From<CommitSequence> for SequenceNumber {
    fn from(sequence: CommitSequence) -> Self {
        sequence.0
    }
}

/// Wall-clock commit timestamp attached to a durable commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct CommitTimestamp(Timestamp);

impl CommitTimestamp {
    pub fn new(timestamp: Timestamp) -> Self {
        Self(timestamp)
    }

    pub fn timestamp(self) -> Timestamp {
        self.0
    }
}

impl From<Timestamp> for CommitTimestamp {
    fn from(timestamp: Timestamp) -> Self {
        Self::new(timestamp)
    }
}

impl From<CommitTimestamp> for Timestamp {
    fn from(timestamp: CommitTimestamp) -> Self {
        timestamp.0
    }
}

/// Caller-requested historical read timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct ReadTimestamp(Timestamp);

impl ReadTimestamp {
    pub fn new(timestamp: Timestamp) -> Self {
        Self(timestamp)
    }

    pub fn timestamp(self) -> Timestamp {
        self.0
    }
}

impl From<Timestamp> for ReadTimestamp {
    fn from(timestamp: Timestamp) -> Self {
        Self::new(timestamp)
    }
}

impl From<ReadTimestamp> for Timestamp {
    fn from(timestamp: ReadTimestamp) -> Self {
        timestamp.0
    }
}

/// Resolved read point selected from the commit timeline for a product timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalReadSnapshot {
    read_timestamp: ReadTimestamp,
    sequence: CommitSequence,
    commit_timestamp: CommitTimestamp,
}

impl HistoricalReadSnapshot {
    pub fn new(
        read_timestamp: ReadTimestamp,
        sequence: CommitSequence,
        commit_timestamp: CommitTimestamp,
    ) -> Self {
        Self {
            read_timestamp,
            sequence,
            commit_timestamp,
        }
    }

    pub fn resolve_at_or_before(
        read_timestamp: ReadTimestamp,
        commits: impl IntoIterator<Item = (CommitTimestamp, CommitSequence)>,
    ) -> Result<Self> {
        commits
            .into_iter()
            .filter(|(commit_timestamp, _)| {
                commit_timestamp.timestamp() <= read_timestamp.timestamp()
            })
            .max_by_key(|(commit_timestamp, sequence)| (*commit_timestamp, *sequence))
            .map(|(commit_timestamp, sequence)| {
                Self::new(read_timestamp, sequence, commit_timestamp)
            })
            .ok_or_else(|| {
                Error::historical_read(
                    HistoricalReadErrorKind::TimestampOutOfRange,
                    format!("no commit exists at or before read timestamp {read_timestamp:?}"),
                )
            })
    }

    pub fn read_timestamp(self) -> ReadTimestamp {
        self.read_timestamp
    }

    pub fn sequence(self) -> CommitSequence {
        self.sequence
    }

    pub fn commit_timestamp(self) -> CommitTimestamp {
        self.commit_timestamp
    }
}

/// Oldest MVCC point that every document, index, policy, and stream reader can still observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionFloor {
    sequence: CommitSequence,
    timestamp: Option<CommitTimestamp>,
}

impl RetentionFloor {
    pub fn new(sequence: CommitSequence) -> Self {
        Self {
            sequence,
            timestamp: None,
        }
    }

    pub fn with_timestamp(sequence: CommitSequence, timestamp: CommitTimestamp) -> Self {
        Self {
            sequence,
            timestamp: Some(timestamp),
        }
    }

    pub fn sequence(self) -> CommitSequence {
        self.sequence
    }

    pub fn timestamp(self) -> Option<CommitTimestamp> {
        self.timestamp
    }

    pub fn permits_sequence(self, sequence: CommitSequence) -> bool {
        sequence >= self.sequence
    }

    pub fn permits_timestamp(self, timestamp: ReadTimestamp) -> bool {
        self.timestamp
            .is_none_or(|floor| timestamp.timestamp() >= floor.timestamp())
    }
}

/// Retained MVCC interval available for historical reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryWindow {
    retention_floor: RetentionFloor,
    latest_sequence: CommitSequence,
    latest_timestamp: Option<CommitTimestamp>,
}

impl HistoryWindow {
    pub fn new(retention_floor: RetentionFloor, latest_sequence: CommitSequence) -> Result<Self> {
        if latest_sequence < retention_floor.sequence() {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::RetentionExpired,
                format!(
                    "latest sequence {} is older than retention floor {}",
                    latest_sequence.sequence(),
                    retention_floor.sequence().sequence()
                ),
            ));
        }

        Ok(Self {
            retention_floor,
            latest_sequence,
            latest_timestamp: None,
        })
    }

    pub fn with_latest_timestamp(
        retention_floor: RetentionFloor,
        latest_sequence: CommitSequence,
        latest_timestamp: CommitTimestamp,
    ) -> Result<Self> {
        let window = Self::new(retention_floor, latest_sequence)?;
        if let Some(floor_timestamp) = retention_floor.timestamp()
            && latest_timestamp < floor_timestamp
        {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::TimestampOutOfRange,
                format!(
                    "latest timestamp {} is older than retention floor timestamp {}",
                    latest_timestamp.timestamp(),
                    floor_timestamp.timestamp()
                ),
            ));
        }

        Ok(Self {
            latest_timestamp: Some(latest_timestamp),
            ..window
        })
    }

    pub fn retention_floor(self) -> RetentionFloor {
        self.retention_floor
    }

    pub fn latest_sequence(self) -> CommitSequence {
        self.latest_sequence
    }

    pub fn latest_timestamp(self) -> Option<CommitTimestamp> {
        self.latest_timestamp
    }

    pub fn ensure_sequence_retained(self, sequence: CommitSequence) -> Result<()> {
        if !self.retention_floor.permits_sequence(sequence) {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::RetentionExpired,
                format!(
                    "read sequence {} is older than retention floor {}",
                    sequence.sequence(),
                    self.retention_floor.sequence().sequence()
                ),
            ));
        }
        if sequence > self.latest_sequence {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::TimestampOutOfRange,
                format!(
                    "read sequence {} is newer than latest durable sequence {}",
                    sequence.sequence(),
                    self.latest_sequence.sequence()
                ),
            ));
        }

        Ok(())
    }

    pub fn ensure_timestamp_retained(self, timestamp: ReadTimestamp) -> Result<()> {
        if !self.retention_floor.permits_timestamp(timestamp) {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::RetentionExpired,
                format!(
                    "read timestamp {} is older than retention floor timestamp {}",
                    timestamp.timestamp(),
                    self.retention_floor
                        .timestamp()
                        .expect("timestamp floor exists")
                        .timestamp()
                ),
            ));
        }
        if let Some(latest_timestamp) = self.latest_timestamp
            && timestamp.timestamp() > latest_timestamp.timestamp()
        {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::TimestampOutOfRange,
                format!(
                    "read timestamp {} is newer than latest durable timestamp {}",
                    timestamp.timestamp(),
                    latest_timestamp.timestamp()
                ),
            ));
        }

        Ok(())
    }
}

/// Stable identity of the access-policy snapshot used for a historical read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PolicySnapshotId(String);

impl PolicySnapshotId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PolicySnapshotId {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_logical_name(&value, "policy snapshot id")?;
        Ok(Self(value))
    }
}

impl From<PolicySnapshotId> for String {
    fn from(value: PolicySnapshotId) -> Self {
        value.0
    }
}

/// Query shape bound into historical cursors so pagination cannot drift.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum HistoricalQueryShape {
    FullScan {
        query_signature: String,
    },
    Index {
        index_id: IndexId,
        query_signature: String,
    },
}

impl HistoricalQueryShape {
    pub fn full_scan(query_signature: impl Into<String>) -> Result<Self> {
        let query_signature = query_signature.into();
        validate_query_signature(&query_signature)?;
        Ok(Self::FullScan { query_signature })
    }

    pub fn index(index_id: IndexId, query_signature: impl Into<String>) -> Result<Self> {
        let query_signature = query_signature.into();
        validate_query_signature(&query_signature)?;
        Ok(Self::Index {
            index_id,
            query_signature,
        })
    }

    pub fn query_signature(&self) -> &str {
        match self {
            Self::FullScan { query_signature }
            | Self::Index {
                query_signature, ..
            } => query_signature.as_str(),
        }
    }
}

/// Backend and adapter support state bound into a historical cursor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HistoricalReadSupport {
    backend: String,
    adapter: String,
}

impl HistoricalReadSupport {
    pub fn supported(backend: impl Into<String>, adapter: impl Into<String>) -> Result<Self> {
        let backend = backend.into();
        let adapter = adapter.into();
        validate_logical_name(&backend, "historical read backend")?;
        validate_logical_name(&adapter, "historical read adapter")?;
        Ok(Self { backend, adapter })
    }

    pub fn backend(&self) -> &str {
        self.backend.as_str()
    }

    pub fn adapter(&self) -> &str {
        self.adapter.as_str()
    }

    pub fn fail_unsupported_backend(backend: impl Into<String>) -> Error {
        Error::historical_read(
            HistoricalReadErrorKind::UnsupportedBackend,
            format!(
                "historical reads are not supported by backend {}",
                backend.into()
            ),
        )
    }

    pub fn fail_unsupported_adapter(adapter: impl Into<String>) -> Error {
        Error::historical_read(
            HistoricalReadErrorKind::UnsupportedAdapter,
            format!(
                "historical reads are not exposed through adapter {}",
                adapter.into()
            ),
        )
    }
}

/// Complete resume identity for a historical pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalCursorIdentity {
    read_snapshot: HistoricalReadSnapshot,
    table_id: TableId,
    query_shape: HistoricalQueryShape,
    policy_snapshot: PolicySnapshotId,
    retention_floor: RetentionFloor,
    support: HistoricalReadSupport,
    storage_format_generation: u16,
}

impl HistoricalCursorIdentity {
    pub fn new(
        read_snapshot: HistoricalReadSnapshot,
        table_id: TableId,
        query_shape: HistoricalQueryShape,
        policy_snapshot: PolicySnapshotId,
        retention_floor: RetentionFloor,
        support: HistoricalReadSupport,
        storage_format_generation: u16,
    ) -> Result<Self> {
        if storage_format_generation == 0 {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                "historical cursor storage format generation cannot be zero",
            ));
        }

        Ok(Self {
            read_snapshot,
            table_id,
            query_shape,
            policy_snapshot,
            retention_floor,
            support,
            storage_format_generation,
        })
    }

    pub fn ensure_resume_identity(&self, expected: &Self) -> Result<()> {
        if self == expected {
            return Ok(());
        }

        Err(Error::historical_read(
            HistoricalReadErrorKind::CursorMismatch,
            "historical cursor resume identity does not match the active read context",
        ))
    }

    pub fn read_snapshot(&self) -> HistoricalReadSnapshot {
        self.read_snapshot
    }

    pub fn table_id(&self) -> &TableId {
        &self.table_id
    }

    pub fn query_shape(&self) -> &HistoricalQueryShape {
        &self.query_shape
    }

    pub fn policy_snapshot(&self) -> &PolicySnapshotId {
        &self.policy_snapshot
    }

    pub fn retention_floor(&self) -> RetentionFloor {
        self.retention_floor
    }

    pub fn support(&self) -> &HistoricalReadSupport {
        &self.support
    }

    pub fn storage_format_generation(&self) -> u16 {
        self.storage_format_generation
    }
}

/// Authorization policy chosen for a read at a historical timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalAuthorization {
    read_timestamp: ReadTimestamp,
    policy_snapshot: PolicySnapshotId,
}

impl HistoricalAuthorization {
    pub fn new(
        read_timestamp: ReadTimestamp,
        policy_snapshot: Option<PolicySnapshotId>,
    ) -> Result<Self> {
        let policy_snapshot = policy_snapshot.ok_or_else(|| {
            Error::historical_read(
                HistoricalReadErrorKind::PolicySnapshotMissing,
                format!("no policy snapshot is available for read timestamp {read_timestamp:?}"),
            )
        })?;

        Ok(Self {
            read_timestamp,
            policy_snapshot,
        })
    }

    pub fn read_timestamp(&self) -> ReadTimestamp {
        self.read_timestamp
    }

    pub fn policy_snapshot(&self) -> &PolicySnapshotId {
        &self.policy_snapshot
    }
}

/// Visibility state for a version encountered by a historical reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalVersionVisibility {
    Pending,
    Committed {
        sequence: CommitSequence,
        timestamp: CommitTimestamp,
    },
}

impl HistoricalVersionVisibility {
    pub fn committed(sequence: CommitSequence, timestamp: CommitTimestamp) -> Self {
        Self::Committed {
            sequence,
            timestamp,
        }
    }

    pub fn is_visible_at(self, read_sequence: CommitSequence) -> bool {
        match self {
            Self::Pending => false,
            Self::Committed { sequence, .. } => sequence <= read_sequence,
        }
    }
}

fn validate_query_signature(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(
            "historical query signature cannot be empty".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(value: u64) -> CommitSequence {
        CommitSequence::new(SequenceNumber(value))
    }

    fn commit_timestamp(value: u64) -> CommitTimestamp {
        CommitTimestamp::new(Timestamp(value))
    }

    fn read_timestamp(value: u64) -> ReadTimestamp {
        ReadTimestamp::new(Timestamp(value))
    }

    fn read_snapshot(value: u64) -> HistoricalReadSnapshot {
        HistoricalReadSnapshot::new(
            read_timestamp(value),
            sequence(value / 100),
            commit_timestamp(value),
        )
    }

    #[test]
    fn history_window_rejects_sequences_before_retention_floor() {
        let window = HistoryWindow::new(RetentionFloor::new(sequence(10)), sequence(20)).unwrap();

        let error = window.ensure_sequence_retained(sequence(9)).unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::RetentionExpired)
        );
    }

    #[test]
    fn history_window_rejects_sequences_after_latest_commit() {
        let window = HistoryWindow::new(RetentionFloor::new(sequence(10)), sequence(20)).unwrap();

        let error = window.ensure_sequence_retained(sequence(21)).unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::TimestampOutOfRange)
        );
    }

    #[test]
    fn history_window_rejects_timestamps_before_retention_floor() {
        let window = HistoryWindow::with_latest_timestamp(
            RetentionFloor::with_timestamp(sequence(10), commit_timestamp(1_000)),
            sequence(20),
            commit_timestamp(2_000),
        )
        .unwrap();

        let error = window
            .ensure_timestamp_retained(read_timestamp(999))
            .unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::RetentionExpired)
        );
    }

    #[test]
    fn timestamp_resolution_uses_latest_sequence_at_or_before_read_timestamp() {
        let snapshot = HistoricalReadSnapshot::resolve_at_or_before(
            read_timestamp(1_000),
            [
                (commit_timestamp(900), sequence(9)),
                (commit_timestamp(1_000), sequence(10)),
                (commit_timestamp(1_000), sequence(11)),
                (commit_timestamp(1_001), sequence(12)),
            ],
        )
        .unwrap();

        assert_eq!(snapshot.sequence(), sequence(11));
        assert_eq!(snapshot.commit_timestamp(), commit_timestamp(1_000));
    }

    #[test]
    fn timestamp_resolution_rejects_reads_before_first_commit() {
        let error = HistoricalReadSnapshot::resolve_at_or_before(
            read_timestamp(899),
            [(commit_timestamp(900), sequence(9))],
        )
        .unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::TimestampOutOfRange)
        );
    }

    #[test]
    fn historical_authorization_requires_policy_snapshot() {
        let error = HistoricalAuthorization::new(read_timestamp(1_000), None).unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::PolicySnapshotMissing)
        );
    }

    #[test]
    fn historical_cursor_identity_rejects_context_drift() {
        let table_id = TableId::new();
        let support = HistoricalReadSupport::supported("sqlite", "convex").unwrap();
        let base = HistoricalCursorIdentity::new(
            read_snapshot(1_000),
            table_id.clone(),
            HistoricalQueryShape::full_scan("messages.by_created_at:asc:limit=32").unwrap(),
            PolicySnapshotId::new("policy-revision-a").unwrap(),
            RetentionFloor::with_timestamp(sequence(10), commit_timestamp(900)),
            support.clone(),
            1,
        )
        .unwrap();
        let changed_policy = HistoricalCursorIdentity::new(
            read_snapshot(1_000),
            table_id,
            HistoricalQueryShape::full_scan("messages.by_created_at:asc:limit=32").unwrap(),
            PolicySnapshotId::new("policy-revision-b").unwrap(),
            RetentionFloor::with_timestamp(sequence(10), commit_timestamp(900)),
            support,
            1,
        )
        .unwrap();

        let error = changed_policy.ensure_resume_identity(&base).unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::CursorMismatch)
        );
    }

    #[test]
    fn historical_cursor_identity_rejects_unknown_format_generation() {
        let error = HistoricalCursorIdentity::new(
            read_snapshot(1_000),
            TableId::new(),
            HistoricalQueryShape::full_scan("messages.all").unwrap(),
            PolicySnapshotId::new("policy-revision-a").unwrap(),
            RetentionFloor::new(sequence(1)),
            HistoricalReadSupport::supported("redb", "native").unwrap(),
            0,
        )
        .unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
    }

    #[test]
    fn unsupported_backend_and_adapter_fail_closed_with_typed_errors() {
        let backend = HistoricalReadSupport::fail_unsupported_backend("mongo");
        let adapter = HistoricalReadSupport::fail_unsupported_adapter("cloud-functions");

        assert_eq!(
            backend.historical_read_kind(),
            Some(HistoricalReadErrorKind::UnsupportedBackend)
        );
        assert_eq!(
            adapter.historical_read_kind(),
            Some(HistoricalReadErrorKind::UnsupportedAdapter)
        );
    }

    #[test]
    fn pending_versions_are_never_visible_to_historical_reads() {
        let pending = HistoricalVersionVisibility::Pending;
        let committed = HistoricalVersionVisibility::committed(sequence(12), commit_timestamp(500));

        assert!(!pending.is_visible_at(sequence(20)));
        assert!(!committed.is_visible_at(sequence(11)));
        assert!(committed.is_visible_at(sequence(12)));
    }
}
