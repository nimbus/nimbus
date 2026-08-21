use nimbus_core::{Document, Result};
use nimbus_storage::{
    CanonicalMaterializedState, DurableJournalBootstrap, MaterializedJournalSnapshot,
    MaterializedPosition, MaterializedVerificationMetricsSnapshot, MaterializedVerificationTracker,
    TableIdentitySnapshotEntry,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyVerificationMode {
    FullScrub,
    Incremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyEscalationReason {
    ColdStart,
    AnchorExpired,
    IdleSessionExpired,
    AppliedSequenceRewind,
    RetentionGap,
    IndexInvalidated,
    RootMismatch,
    OperatorForced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyScope {
    AuthoritativeSnapshot,
    ShadowMaterializer,
    EmbeddedReplica,
    JournalBootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotFingerprint {
    /// The storage-owned position: applied sequence plus the digest of the
    /// state that sequence produced. Reports compare this, not a sequence.
    pub position: MaterializedPosition,
    pub snapshot_version: u16,
    /// Durable head stays outside the position. It describes how far the
    /// journal is durable, not which state the snapshot materialized.
    pub durable_head: u64,
    pub schema_table_count: usize,
    pub document_count: usize,
    pub scheduled_execution_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapFingerprint {
    pub snapshot_position: MaterializedPosition,
    pub resume_after_sequence: u64,
    pub bootstrap_cut_sequence: u64,
    pub cursor_floor_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsistencyMismatch {
    pub invariant: String,
    pub left_scope: ConsistencyScope,
    pub right_scope: ConsistencyScope,
    pub path: String,
    pub left_description: String,
    pub right_description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationRootFingerprint {
    pub version: u16,
    pub applied_sequence: u64,
    pub root_hash: String,
    pub leaf_count: usize,
    pub resident_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationAnchor {
    pub position: MaterializedPosition,
    pub age_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsistencyVerificationReport {
    pub tenant_id: String,
    pub ok: bool,
    pub mode: ConsistencyVerificationMode,
    pub anchor: VerificationAnchor,
    pub event_count: u64,
    pub escalation_reason: Option<ConsistencyEscalationReason>,
    pub authoritative_root: VerificationRootFingerprint,
    pub shadow_root: VerificationRootFingerprint,
    pub embedded_replica_root: VerificationRootFingerprint,
    /// Full-scrub evidence retained at `anchor`. Incremental reports do not
    /// claim that these counts were scanned again at the current sequence.
    pub authoritative: SnapshotFingerprint,
    pub shadow: SnapshotFingerprint,
    pub embedded_replica: SnapshotFingerprint,
    pub bootstrap: BootstrapFingerprint,
    pub mismatches: Vec<ConsistencyMismatch>,
    pub metrics: MaterializedVerificationMetricsSnapshot,
}

pub(crate) fn verification_root_fingerprint(
    tracker: &MaterializedVerificationTracker,
) -> Result<VerificationRootFingerprint> {
    let position = tracker.position().ok_or_else(|| {
        nimbus_core::Error::Internal("materialized verification tracker is invalidated".to_string())
    })?;
    Ok(VerificationRootFingerprint {
        version: position.version().as_u16(),
        applied_sequence: position.applied_sequence().0,
        root_hash: bytes_to_hex(position.root_hash()),
        leaf_count: tracker.leaf_count(),
        resident_bytes: tracker.resident_bytes(),
    })
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

pub fn snapshot_fingerprint(snapshot: &MaterializedJournalSnapshot) -> Result<SnapshotFingerprint> {
    Ok(SnapshotFingerprint {
        position: snapshot.materialized_position()?,
        snapshot_version: snapshot.version,
        durable_head: snapshot.durable_head.0,
        schema_table_count: snapshot.schema.tables.len(),
        document_count: snapshot.documents.len(),
        scheduled_execution_count: snapshot.scheduled_execution_ids.len(),
    })
}

pub fn bootstrap_fingerprint(bootstrap: &DurableJournalBootstrap) -> Result<BootstrapFingerprint> {
    Ok(BootstrapFingerprint {
        snapshot_position: snapshot_fingerprint(&bootstrap.snapshot)?.position,
        resume_after_sequence: bootstrap.resume_after.0,
        bootstrap_cut_sequence: bootstrap.bootstrap_cut.0,
        cursor_floor_sequence: bootstrap.cursor_floor.0,
    })
}

/// Compare two materialized snapshots and name the first difference.
///
/// The position decides equality: two snapshots agree when their applied
/// sequence and state digest agree. The field walk below exists only to name
/// *where* a digest difference came from, so an operator gets a path rather
/// than two opaque hashes.
pub fn compare_materialized_journal_snapshots(
    left_scope: ConsistencyScope,
    left: &MaterializedJournalSnapshot,
    right_scope: ConsistencyScope,
    right: &MaterializedJournalSnapshot,
) -> Result<Option<ConsistencyMismatch>> {
    let left_position = left.materialized_position()?;
    let right_position = right.materialized_position()?;

    if left_position.version() != right_position.version() {
        return Ok(Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "position.version",
            left_position.version(),
            right_position.version(),
        )));
    }
    if left_position.applied_sequence() != right_position.applied_sequence() {
        return Ok(Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "position.applied_sequence",
            left_position.applied_sequence().0,
            right_position.applied_sequence().0,
        )));
    }
    // Durable head is compared separately because it is deliberately not part
    // of the state digest.
    if left.durable_head != right.durable_head {
        return Ok(Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "durable_head",
            left.durable_head.0,
            right.durable_head.0,
        )));
    }
    if left_position.state_digest() == right_position.state_digest() {
        return Ok(None);
    }

    let left_state = left.canonical_state()?;
    let right_state = right.canonical_state()?;
    Ok(Some(
        locate_canonical_state_difference(left_scope, &left_state, right_scope, &right_state)
            .unwrap_or_else(|| {
                mismatch(
                    "materialized_snapshot_match",
                    left_scope,
                    right_scope,
                    "position.state_digest",
                    left_position.state_digest(),
                    right_position.state_digest(),
                )
            }),
    ))
}

/// Walk two canonical states in their shared canonical order and name the first
/// field that differs. Returns `None` only when the states are field-identical,
/// which the digest comparison above has already ruled out.
fn locate_canonical_state_difference(
    left_scope: ConsistencyScope,
    left: &CanonicalMaterializedState,
    right_scope: ConsistencyScope,
    right: &CanonicalMaterializedState,
) -> Option<ConsistencyMismatch> {
    if left.snapshot_version() != right.snapshot_version() {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "version",
            left.snapshot_version(),
            right.snapshot_version(),
        ));
    }

    // Table identities first: a table_id or lifecycle-state drift on a
    // same-named table changes the digest, so the walk has to be able to say so
    // rather than falling through to the opaque digest path.
    let left_identity_keys = left
        .table_identities()
        .iter()
        .map(table_identity_key)
        .collect::<Vec<_>>();
    let right_identity_keys = right
        .table_identities()
        .iter()
        .map(table_identity_key)
        .collect::<Vec<_>>();
    if left_identity_keys != right_identity_keys {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "table_identities",
            left_identity_keys,
            right_identity_keys,
        ));
    }
    for (left_identity, right_identity) in
        left.table_identities().iter().zip(right.table_identities())
    {
        if left_identity != right_identity {
            return Some(mismatch(
                "materialized_snapshot_match",
                left_scope,
                right_scope,
                &format!("table_identities.{}", table_identity_key(left_identity)),
                left_identity,
                right_identity,
            ));
        }
    }

    let left_schema_keys = left
        .schema_tables()
        .iter()
        .map(|table| table.table.to_string())
        .collect::<Vec<_>>();
    let right_schema_keys = right
        .schema_tables()
        .iter()
        .map(|table| table.table.to_string())
        .collect::<Vec<_>>();
    if left_schema_keys != right_schema_keys {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "schema.tables",
            left_schema_keys,
            right_schema_keys,
        ));
    }
    for (left_table, right_table) in left.schema_tables().iter().zip(right.schema_tables()) {
        if left_table != right_table {
            return Some(mismatch(
                "materialized_snapshot_match",
                left_scope,
                right_scope,
                &format!("schema.tables.{}", left_table.table),
                left_table,
                right_table,
            ));
        }
    }

    let left_document_keys = left
        .documents()
        .iter()
        .map(document_key)
        .collect::<Vec<_>>();
    let right_document_keys = right
        .documents()
        .iter()
        .map(document_key)
        .collect::<Vec<_>>();
    if left_document_keys != right_document_keys {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "documents",
            left_document_keys,
            right_document_keys,
        ));
    }
    for (left_document, right_document) in left.documents().iter().zip(right.documents()) {
        if left_document != right_document {
            return Some(mismatch(
                "materialized_snapshot_match",
                left_scope,
                right_scope,
                &format!("documents.{}", document_key(left_document)),
                left_document,
                right_document,
            ));
        }
    }

    if left.scheduled_execution_ids() != right.scheduled_execution_ids() {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "scheduled_execution_ids",
            left.scheduled_execution_ids(),
            right.scheduled_execution_ids(),
        ));
    }

    None
}

pub fn collect_durable_journal_bootstrap_mismatches(
    authoritative_snapshot: &MaterializedJournalSnapshot,
    bootstrap: &DurableJournalBootstrap,
) -> Result<Vec<ConsistencyMismatch>> {
    let mut mismatches = Vec::new();

    if let Some(snapshot_mismatch) = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        authoritative_snapshot,
        ConsistencyScope::JournalBootstrap,
        &bootstrap.snapshot,
    )? {
        mismatches.push(ConsistencyMismatch {
            invariant: "bootstrap_snapshot_match".to_string(),
            ..snapshot_mismatch
        });
    }
    if bootstrap.resume_after != authoritative_snapshot.applied_sequence {
        mismatches.push(mismatch(
            "bootstrap_metadata_match",
            ConsistencyScope::AuthoritativeSnapshot,
            ConsistencyScope::JournalBootstrap,
            "bootstrap.resume_after_sequence",
            authoritative_snapshot.applied_sequence.0,
            bootstrap.resume_after.0,
        ));
    }
    if bootstrap.bootstrap_cut != authoritative_snapshot.durable_head {
        mismatches.push(mismatch(
            "bootstrap_metadata_match",
            ConsistencyScope::AuthoritativeSnapshot,
            ConsistencyScope::JournalBootstrap,
            "bootstrap.bootstrap_cut_sequence",
            authoritative_snapshot.durable_head.0,
            bootstrap.bootstrap_cut.0,
        ));
    }
    if bootstrap.cursor_floor.0 > bootstrap.resume_after.0 {
        mismatches.push(mismatch(
            "bootstrap_metadata_match",
            ConsistencyScope::AuthoritativeSnapshot,
            ConsistencyScope::JournalBootstrap,
            "bootstrap.cursor_floor_sequence",
            format!("<= {}", bootstrap.resume_after.0),
            bootstrap.cursor_floor.0,
        ));
    }
    if bootstrap.resume_after.0 > bootstrap.bootstrap_cut.0 {
        mismatches.push(mismatch(
            "bootstrap_metadata_match",
            ConsistencyScope::AuthoritativeSnapshot,
            ConsistencyScope::JournalBootstrap,
            "bootstrap.sequence_window",
            format!(
                "{} <= {}",
                bootstrap.resume_after.0, bootstrap.bootstrap_cut.0
            ),
            format!(
                "{} > {}",
                bootstrap.resume_after.0, bootstrap.bootstrap_cut.0
            ),
        ));
    }

    Ok(mismatches)
}

fn document_key(document: &Document) -> String {
    format!("{}/{}", document.table, document.id)
}

fn table_identity_key(identity: &TableIdentitySnapshotEntry) -> String {
    format!("{}/{}", identity.namespace, identity.table)
}

fn mismatch<T, U>(
    invariant: &str,
    left_scope: ConsistencyScope,
    right_scope: ConsistencyScope,
    path: &str,
    left: T,
    right: U,
) -> ConsistencyMismatch
where
    T: Serialize,
    U: Serialize,
{
    ConsistencyMismatch {
        invariant: invariant.to_string(),
        left_scope,
        right_scope,
        path: path.to_string(),
        left_description: describe(&left),
        right_description: describe(&right),
    }
}

fn describe<T>(value: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<unserializable>".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn materialized_position_golden_matches_shipped_graph() {
        let position = nimbus_storage::materialized_position_golden_fixture()
            .expect("materialized position fixture should compute");
        assert_eq!(position.version(), 2);
        assert_eq!(
            position.state_digest(),
            "cc10a2a6579d2df620010321813fa1ca2bc715288280c0d62a502b5281a7ca68"
        );
    }
}
