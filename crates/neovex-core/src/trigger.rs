use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Document, DocumentPath, Error, PrincipalContext, Result, SequenceNumber, Timestamp};

/// CloudEvent spec version for shared trigger events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CloudEventSpecVersion {
    #[default]
    #[serde(rename = "1.0")]
    V1,
}

impl CloudEventSpecVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "1.0",
        }
    }
}

/// Standard Firestore CloudEvent type strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirestoreCloudEventType {
    #[serde(rename = "google.cloud.firestore.document.v1.created")]
    Created,
    #[serde(rename = "google.cloud.firestore.document.v1.updated")]
    Updated,
    #[serde(rename = "google.cloud.firestore.document.v1.deleted")]
    Deleted,
    #[serde(rename = "google.cloud.firestore.document.v1.written")]
    Written,
}

impl FirestoreCloudEventType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "google.cloud.firestore.document.v1.created",
            Self::Updated => "google.cloud.firestore.document.v1.updated",
            Self::Deleted => "google.cloud.firestore.document.v1.deleted",
            Self::Written => "google.cloud.firestore.document.v1.written",
        }
    }
}

/// Shared CloudEvent identity for Firestore-backed trigger dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerCloudEvent {
    pub id: String,
    pub source: String,
    #[serde(default)]
    pub specversion: CloudEventSpecVersion,
    #[serde(rename = "type")]
    pub event_type: FirestoreCloudEventType,
    pub time: Timestamp,
    pub subject: String,
}

impl TriggerCloudEvent {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        event_type: FirestoreCloudEventType,
        time: Timestamp,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            specversion: CloudEventSpecVersion::V1,
            event_type,
            time,
            subject: subject.into(),
        }
    }
}

/// Firestore document state included in a `DocumentEventData` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentEventDocument {
    pub path: DocumentPath,
    pub document: Document,
}

impl DocumentEventDocument {
    pub fn new(path: DocumentPath, document: Document) -> Self {
        Self { path, document }
    }
}

/// Firestore-style update mask for changed document fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DocumentEventUpdateMask {
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "fieldPaths")]
    pub field_paths: Vec<String>,
}

impl DocumentEventUpdateMask {
    pub fn new(field_paths: Vec<String>) -> Self {
        Self { field_paths }
    }
}

/// Shared `DocumentEventData` payload consumed by both trigger authoring
/// surfaces later in the Cloud Functions plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentEventData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<DocumentEventDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "oldValue")]
    pub old_value: Option<DocumentEventDocument>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "updateMask"
    )]
    pub update_mask: Option<DocumentEventUpdateMask>,
}

impl DocumentEventData {
    pub fn new(
        value: Option<DocumentEventDocument>,
        old_value: Option<DocumentEventDocument>,
        update_mask: Option<DocumentEventUpdateMask>,
    ) -> Self {
        Self {
            value,
            old_value,
            update_mask,
        }
    }
}

/// Firestore-specific trigger metadata kept beside the CloudEvent identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirestoreTriggerMetadata {
    pub project_id: String,
    pub database_id: String,
    pub document_path: DocumentPath,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl FirestoreTriggerMetadata {
    pub fn new(
        project_id: impl Into<String>,
        database_id: impl Into<String>,
        document_path: DocumentPath,
        params: BTreeMap<String, String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            database_id: database_id.into(),
            document_path,
            params,
        }
    }
}

/// Commit metadata preserved for durable trigger replay and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerCommitMetadata {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
}

impl TriggerCommitMetadata {
    pub fn new(sequence: SequenceNumber, timestamp: Timestamp) -> Self {
        Self {
            sequence,
            timestamp,
        }
    }
}

/// Trigger execution principal contract.
///
/// Base document triggers run as a trusted service principal rather than as the
/// end user whose write caused the commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerExecutionPrincipal {
    Service { principal: PrincipalContext },
}

impl TriggerExecutionPrincipal {
    pub fn service(principal: PrincipalContext) -> Self {
        Self::Service { principal }
    }

    pub fn principal(&self) -> &PrincipalContext {
        match self {
            Self::Service { principal } => principal,
        }
    }

    pub fn is_service(&self) -> bool {
        matches!(self, Self::Service { .. })
    }
}

/// Shared trigger event envelope for Cloud Functions-compatible dispatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerEvent {
    pub cloud_event: TriggerCloudEvent,
    pub firestore: FirestoreTriggerMetadata,
    pub data: DocumentEventData,
    pub commit: TriggerCommitMetadata,
    pub execution: TriggerExecutionPrincipal,
}

impl TriggerEvent {
    pub fn new(
        cloud_event: TriggerCloudEvent,
        firestore: FirestoreTriggerMetadata,
        data: DocumentEventData,
        commit: TriggerCommitMetadata,
        execution: TriggerExecutionPrincipal,
    ) -> Self {
        Self {
            cloud_event,
            firestore,
            data,
            commit,
            execution,
        }
    }
}

/// Trigger-origin metadata persisted beside committed writes.
///
/// The stored depth is the depth of the invocation that produced the write.
/// Child trigger invocations derived from that write advance to `depth + 1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerWriteOrigin {
    pub invocation: TriggerInvocationKey,
    pub depth: u32,
}

impl TriggerWriteOrigin {
    pub fn new(invocation: TriggerInvocationKey, depth: u32) -> Self {
        Self { invocation, depth }
    }

    pub fn child_depth(&self) -> u32 {
        self.depth.saturating_add(1)
    }
}

/// Per-tenant trigger delivery progress.
///
/// The durable journal remains the authoritative source of committed writes.
/// This cursor only records how far the dispatcher has expanded committed
/// writes into durable invocation records. It is not, by itself, the
/// completion contract for trigger execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TriggerDeliveryCursor {
    pub materialized_through: SequenceNumber,
}

impl TriggerDeliveryCursor {
    pub const fn new(materialized_through: SequenceNumber) -> Self {
        Self {
            materialized_through,
        }
    }

    pub fn advance_to(&mut self, sequence: SequenceNumber) -> Result<()> {
        if sequence.0 < self.materialized_through.0 {
            return Err(Error::InvalidInput(format!(
                "trigger delivery cursor cannot move backwards from {} to {}",
                self.materialized_through.0, sequence.0
            )));
        }
        self.materialized_through = sequence;
        Ok(())
    }
}

/// Stable identifier for one durable handler invocation.
///
/// CloudEvent ids identify the source document change, while `registration_id`
/// identifies the matched trigger registration or bound handler target. The
/// pair forms a deterministic dedupe key across crash recovery and replay.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TriggerInvocationKey {
    pub registration_id: String,
    pub event_id: String,
}

impl TriggerInvocationKey {
    pub fn new(registration_id: impl Into<String>, event_id: impl Into<String>) -> Result<Self> {
        let registration_id = registration_id.into();
        let event_id = event_id.into();
        if registration_id.is_empty() {
            return Err(Error::InvalidInput(
                "trigger invocation registration id cannot be empty".to_string(),
            ));
        }
        if event_id.is_empty() {
            return Err(Error::InvalidInput(
                "trigger invocation event id cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            registration_id,
            event_id,
        })
    }
}

/// Durable ancestry metadata for one trigger invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerInvocationAncestry {
    pub parent: TriggerInvocationKey,
    pub depth: u32,
}

impl TriggerInvocationAncestry {
    pub fn new(parent: TriggerInvocationKey, depth: u32) -> Self {
        Self { parent, depth }
    }
}

/// Durable lifecycle for one matched trigger invocation.
///
/// The dispatcher advances the journal-backed materialization cursor once the
/// matching invocation records are persisted. Each record then moves through
/// this state machine independently so retries and completion survive restart
/// without forcing the cursor to stall at the source commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerInvocationState {
    Pending,
    Running {
        attempt: u32,
        started_at: Timestamp,
    },
    RetryPending {
        attempt: u32,
        failed_at: Timestamp,
        next_attempt_at: Timestamp,
        error: String,
    },
    Completed {
        attempt: u32,
        completed_at: Timestamp,
    },
    TerminalFailure {
        attempt: u32,
        failed_at: Timestamp,
        error: String,
    },
}

impl TriggerInvocationState {
    pub fn attempt(&self) -> u32 {
        match self {
            Self::Pending => 0,
            Self::Running { attempt, .. }
            | Self::RetryPending { attempt, .. }
            | Self::Completed { attempt, .. }
            | Self::TerminalFailure { attempt, .. } => *attempt,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::TerminalFailure { .. })
    }
}

/// Durable record for one handler invocation emitted from a committed write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerInvocationRecord {
    pub key: TriggerInvocationKey,
    pub commit_sequence: SequenceNumber,
    pub event: TriggerEvent,
    #[serde(default)]
    pub ancestry: Option<TriggerInvocationAncestry>,
    pub state: TriggerInvocationState,
}

impl TriggerInvocationRecord {
    pub fn pending(
        key: TriggerInvocationKey,
        commit_sequence: SequenceNumber,
        event: TriggerEvent,
    ) -> Self {
        Self::pending_with_ancestry(key, commit_sequence, event, None)
    }

    pub fn pending_with_ancestry(
        key: TriggerInvocationKey,
        commit_sequence: SequenceNumber,
        event: TriggerEvent,
        ancestry: Option<TriggerInvocationAncestry>,
    ) -> Self {
        Self {
            key,
            commit_sequence,
            event,
            ancestry,
            state: TriggerInvocationState::Pending,
        }
    }

    pub fn terminal_with_ancestry(
        key: TriggerInvocationKey,
        commit_sequence: SequenceNumber,
        event: TriggerEvent,
        ancestry: Option<TriggerInvocationAncestry>,
        failed_at: Timestamp,
        error: impl Into<String>,
    ) -> Self {
        Self {
            key,
            commit_sequence,
            event,
            ancestry,
            state: TriggerInvocationState::TerminalFailure {
                attempt: 0,
                failed_at,
                error: error.into(),
            },
        }
    }

    pub fn depth(&self) -> u32 {
        self.ancestry.as_ref().map_or(0, |ancestry| ancestry.depth)
    }

    pub fn begin_attempt(&mut self, started_at: Timestamp) -> Result<u32> {
        let attempt = match self.state {
            TriggerInvocationState::Pending => 1,
            TriggerInvocationState::RetryPending { attempt, .. } => attempt.saturating_add(1),
            TriggerInvocationState::Running { .. } => {
                return Err(Error::InvalidInput(
                    "trigger invocation attempt is already running".to_string(),
                ));
            }
            TriggerInvocationState::Completed { .. } => {
                return Err(Error::InvalidInput(
                    "completed trigger invocation cannot be restarted".to_string(),
                ));
            }
            TriggerInvocationState::TerminalFailure { .. } => {
                return Err(Error::InvalidInput(
                    "terminal trigger invocation cannot be restarted".to_string(),
                ));
            }
        };
        self.state = TriggerInvocationState::Running {
            attempt,
            started_at,
        };
        Ok(attempt)
    }

    pub fn schedule_retry(
        &mut self,
        failed_at: Timestamp,
        next_attempt_at: Timestamp,
        error: impl Into<String>,
    ) -> Result<()> {
        let attempt = match self.state {
            TriggerInvocationState::Running { attempt, .. } => attempt,
            _ => {
                return Err(Error::InvalidInput(
                    "trigger invocation retry requires a running attempt".to_string(),
                ));
            }
        };
        self.state = TriggerInvocationState::RetryPending {
            attempt,
            failed_at,
            next_attempt_at,
            error: error.into(),
        };
        Ok(())
    }

    pub fn complete(&mut self, completed_at: Timestamp) -> Result<()> {
        let attempt = match self.state {
            TriggerInvocationState::Running { attempt, .. } => attempt,
            _ => {
                return Err(Error::InvalidInput(
                    "trigger invocation completion requires a running attempt".to_string(),
                ));
            }
        };
        self.state = TriggerInvocationState::Completed {
            attempt,
            completed_at,
        };
        Ok(())
    }

    pub fn fail_terminal(&mut self, failed_at: Timestamp, error: impl Into<String>) -> Result<()> {
        let attempt = match self.state {
            TriggerInvocationState::Running { attempt, .. } => attempt,
            _ => {
                return Err(Error::InvalidInput(
                    "terminal trigger failure requires a running attempt".to_string(),
                ));
            }
        };
        self.state = TriggerInvocationState::TerminalFailure {
            attempt,
            failed_at,
            error: error.into(),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rmp_serde::{from_slice, to_vec};
    use serde_json::json;

    use super::{
        CloudEventSpecVersion, DocumentEventData, DocumentEventDocument, DocumentEventUpdateMask,
        FirestoreCloudEventType, FirestoreTriggerMetadata, TriggerCloudEvent,
        TriggerCommitMetadata, TriggerDeliveryCursor, TriggerEvent, TriggerExecutionPrincipal,
        TriggerInvocationAncestry, TriggerInvocationKey, TriggerInvocationRecord,
        TriggerInvocationState, TriggerWriteOrigin,
    };
    use crate::{
        Document, DocumentId, DocumentPath, PrincipalContext, SequenceNumber, TableName, Timestamp,
    };

    #[test]
    fn firestore_cloud_event_types_use_standard_gcp_strings() {
        assert_eq!(
            FirestoreCloudEventType::Created.as_str(),
            "google.cloud.firestore.document.v1.created"
        );
        assert_eq!(
            FirestoreCloudEventType::Updated.as_str(),
            "google.cloud.firestore.document.v1.updated"
        );
        assert_eq!(
            FirestoreCloudEventType::Deleted.as_str(),
            "google.cloud.firestore.document.v1.deleted"
        );
        assert_eq!(
            FirestoreCloudEventType::Written.as_str(),
            "google.cloud.firestore.document.v1.written"
        );
    }

    #[test]
    fn cloud_event_specversion_stays_at_v1() {
        let event = TriggerCloudEvent::new(
            "evt-1",
            "//firestore.googleapis.com/projects/demo/databases/(default)",
            FirestoreCloudEventType::Written,
            Timestamp(42),
            "documents/users/alice",
        );

        assert_eq!(event.specversion, CloudEventSpecVersion::V1);
        assert_eq!(event.specversion.as_str(), "1.0");
    }

    #[test]
    fn document_event_data_roundtrips_through_serialization() {
        let path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");
        let document = Document::with_id(
            DocumentId::from_key("alice".to_string()).expect("document id should parse"),
            TableName::new("users_table").expect("table should parse"),
            serde_json::Map::from_iter([("name".to_string(), json!("Alice"))]),
        );
        let data = DocumentEventData::new(
            Some(DocumentEventDocument::new(path.clone(), document.clone())),
            Some(DocumentEventDocument::new(path, document)),
            Some(DocumentEventUpdateMask::new(vec!["name".to_string()])),
        );

        let encoded = serde_json::to_value(&data).expect("payload should encode");
        let decoded =
            serde_json::from_value::<DocumentEventData>(encoded).expect("payload should decode");

        assert_eq!(decoded, data);
    }

    #[test]
    fn trigger_events_model_service_principal_execution_explicitly() {
        let path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");
        let event = TriggerEvent::new(
            TriggerCloudEvent::new(
                "evt-2",
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Created,
                Timestamp(99),
                "documents/users/alice",
            ),
            FirestoreTriggerMetadata::new(
                "demo",
                "(default)",
                path.clone(),
                BTreeMap::from([("userId".to_string(), "alice".to_string())]),
            ),
            DocumentEventData::new(None, None, None),
            TriggerCommitMetadata::new(SequenceNumber(7), Timestamp(99)),
            TriggerExecutionPrincipal::service(PrincipalContext::anonymous()),
        );

        assert!(event.execution.is_service());
        assert_eq!(event.execution.principal(), &PrincipalContext::anonymous());
        assert_eq!(event.firestore.document_path, path);
        assert_eq!(
            event.firestore.params.get("userId"),
            Some(&"alice".to_string())
        );
    }

    #[test]
    fn trigger_delivery_cursor_only_advances_forward() {
        let mut cursor = TriggerDeliveryCursor::new(SequenceNumber(7));
        cursor
            .advance_to(SequenceNumber(9))
            .expect("cursor should advance");
        assert_eq!(cursor.materialized_through, SequenceNumber(9));

        let error = cursor
            .advance_to(SequenceNumber(8))
            .expect_err("cursor should reject moving backwards");
        assert!(
            matches!(error, crate::Error::InvalidInput(message) if message.contains("cannot move backwards"))
        );
    }

    #[test]
    fn trigger_invocation_record_roundtrips_through_msgpack_persistence() {
        let path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");
        let event = TriggerEvent::new(
            TriggerCloudEvent::new(
                "evt-3",
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Written,
                Timestamp(111),
                "documents/users/alice",
            ),
            FirestoreTriggerMetadata::new("demo", "(default)", path, BTreeMap::new()),
            DocumentEventData::new(None, None, None),
            TriggerCommitMetadata::new(SequenceNumber(12), Timestamp(111)),
            TriggerExecutionPrincipal::service(PrincipalContext::anonymous()),
        );
        let record = TriggerInvocationRecord::pending(
            TriggerInvocationKey::new("deploy:users-writer", "evt-3")
                .expect("invocation key should build"),
            SequenceNumber(12),
            event,
        );

        let encoded = to_vec(&record).expect("record should encode");
        let decoded: TriggerInvocationRecord = from_slice(&encoded).expect("record should decode");

        assert_eq!(decoded, record);
    }

    #[test]
    fn trigger_write_origin_and_invocation_depth_stay_distinct() {
        let parent = TriggerInvocationKey::new("deploy:users-writer", "evt-7")
            .expect("invocation key should build");
        let origin = TriggerWriteOrigin::new(parent.clone(), 2);
        let ancestry = TriggerInvocationAncestry::new(parent, origin.child_depth());

        assert_eq!(origin.depth, 2);
        assert_eq!(origin.child_depth(), 3);
        assert_eq!(ancestry.depth, 3);
    }

    #[test]
    fn trigger_invocation_depth_defaults_to_root_and_tracks_ancestry() {
        let root = TriggerInvocationRecord::pending(
            TriggerInvocationKey::new("deploy:users-updated", "evt-8")
                .expect("invocation key should build"),
            SequenceNumber(13),
            sample_trigger_event("evt-8", SequenceNumber(13), Timestamp(130)),
        );
        assert_eq!(root.depth(), 0);

        let child = TriggerInvocationRecord::pending_with_ancestry(
            TriggerInvocationKey::new("deploy:users-updated", "evt-9")
                .expect("invocation key should build"),
            SequenceNumber(14),
            sample_trigger_event("evt-9", SequenceNumber(14), Timestamp(140)),
            Some(TriggerInvocationAncestry::new(
                TriggerInvocationKey::new("deploy:users-updated", "evt-8")
                    .expect("parent invocation key should build"),
                1,
            )),
        );
        assert_eq!(child.depth(), 1);
    }

    #[test]
    fn trigger_invocation_state_machine_supports_retry_then_completion() {
        let event = TriggerEvent::new(
            TriggerCloudEvent::new(
                "evt-4",
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Updated,
                Timestamp(200),
                "documents/users/alice",
            ),
            FirestoreTriggerMetadata::new(
                "demo",
                "(default)",
                DocumentPath::from_segments(["users", "alice"]).expect("path should parse"),
                BTreeMap::from([("userId".to_string(), "alice".to_string())]),
            ),
            DocumentEventData::new(None, None, None),
            TriggerCommitMetadata::new(SequenceNumber(21), Timestamp(200)),
            TriggerExecutionPrincipal::service(PrincipalContext::anonymous()),
        );
        let mut record = TriggerInvocationRecord::pending(
            TriggerInvocationKey::new("deploy:users-updated", "evt-4")
                .expect("invocation key should build"),
            SequenceNumber(21),
            event,
        );

        assert_eq!(record.state, TriggerInvocationState::Pending);
        assert_eq!(
            record
                .begin_attempt(Timestamp(201))
                .expect("first attempt should start"),
            1
        );
        assert_eq!(
            record.state,
            TriggerInvocationState::Running {
                attempt: 1,
                started_at: Timestamp(201),
            }
        );

        record
            .schedule_retry(Timestamp(202), Timestamp(210), "transient failure")
            .expect("retry should schedule");
        assert_eq!(
            record.state,
            TriggerInvocationState::RetryPending {
                attempt: 1,
                failed_at: Timestamp(202),
                next_attempt_at: Timestamp(210),
                error: "transient failure".to_string(),
            }
        );

        assert_eq!(
            record
                .begin_attempt(Timestamp(211))
                .expect("second attempt should start"),
            2
        );
        record
            .complete(Timestamp(212))
            .expect("running attempt should complete");
        assert_eq!(
            record.state,
            TriggerInvocationState::Completed {
                attempt: 2,
                completed_at: Timestamp(212),
            }
        );
        assert!(record.state.is_terminal());
    }

    #[test]
    fn trigger_invocation_state_machine_rejects_invalid_transitions() {
        let event = TriggerEvent::new(
            TriggerCloudEvent::new(
                "evt-5",
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Deleted,
                Timestamp(300),
                "documents/users/alice",
            ),
            FirestoreTriggerMetadata::new(
                "demo",
                "(default)",
                DocumentPath::from_segments(["users", "alice"]).expect("path should parse"),
                BTreeMap::new(),
            ),
            DocumentEventData::new(None, None, None),
            TriggerCommitMetadata::new(SequenceNumber(31), Timestamp(300)),
            TriggerExecutionPrincipal::service(PrincipalContext::anonymous()),
        );
        let mut record = TriggerInvocationRecord::pending(
            TriggerInvocationKey::new("deploy:users-deleted", "evt-5")
                .expect("invocation key should build"),
            SequenceNumber(31),
            event,
        );

        let error = record
            .complete(Timestamp(301))
            .expect_err("completion before start should fail");
        assert!(
            matches!(error, crate::Error::InvalidInput(message) if message.contains("requires a running attempt"))
        );

        record
            .begin_attempt(Timestamp(302))
            .expect("attempt should start");
        record
            .fail_terminal(Timestamp(303), "permanent failure")
            .expect("terminal failure should record");
        assert_eq!(
            record.state,
            TriggerInvocationState::TerminalFailure {
                attempt: 1,
                failed_at: Timestamp(303),
                error: "permanent failure".to_string(),
            }
        );

        let error = record
            .begin_attempt(Timestamp(304))
            .expect_err("terminal failure should not restart");
        assert!(
            matches!(error, crate::Error::InvalidInput(message) if message.contains("cannot be restarted"))
        );
    }

    fn sample_trigger_event(
        event_id: &str,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    ) -> TriggerEvent {
        let path = DocumentPath::from_segments(["users", "alice"]).expect("path should parse");
        let document = Document::with_id(
            DocumentId::from_key("alice".to_string()).expect("document id should parse"),
            TableName::new("users_table").expect("table should parse"),
            serde_json::Map::from_iter([("name".to_string(), json!("Alice"))]),
        );
        TriggerEvent::new(
            TriggerCloudEvent::new(
                event_id,
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Written,
                timestamp,
                "documents/users/alice",
            ),
            FirestoreTriggerMetadata::new("demo", "(default)", path.clone(), BTreeMap::new()),
            DocumentEventData::new(Some(DocumentEventDocument::new(path, document)), None, None),
            TriggerCommitMetadata::new(sequence, timestamp),
            TriggerExecutionPrincipal::service(PrincipalContext::anonymous()),
        )
    }
}
