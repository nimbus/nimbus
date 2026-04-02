use neovex_core::{Document, Error, Result, Schema, TableSchema};
use neovex_storage::{DurableJournalBootstrap, MaterializedJournalSnapshot};
use serde::Serialize;
use sha2::{Digest, Sha256};

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
    pub digest: String,
    pub version: u16,
    pub applied_sequence: u64,
    pub durable_head: u64,
    pub schema_table_count: usize,
    pub document_count: usize,
    pub scheduled_execution_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootstrapFingerprint {
    pub snapshot_digest: String,
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
pub struct ConsistencyVerificationReport {
    pub tenant_id: String,
    pub ok: bool,
    pub authoritative: SnapshotFingerprint,
    pub shadow: SnapshotFingerprint,
    pub embedded_replica: SnapshotFingerprint,
    pub bootstrap: BootstrapFingerprint,
    pub mismatches: Vec<ConsistencyMismatch>,
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalMaterializedJournalSnapshot {
    version: u16,
    applied_sequence: u64,
    durable_head: u64,
    schema: Vec<TableSchema>,
    documents: Vec<Document>,
    scheduled_execution_ids: Vec<String>,
}

pub fn snapshot_fingerprint(snapshot: &MaterializedJournalSnapshot) -> Result<SnapshotFingerprint> {
    let canonical = canonicalize_materialized_journal_snapshot(snapshot);
    let payload =
        serde_json::to_vec(&canonical).map_err(|error| Error::Serialization(error.to_string()))?;
    let digest = hex_encode(Sha256::digest(payload));

    Ok(SnapshotFingerprint {
        digest,
        version: snapshot.version,
        applied_sequence: snapshot.applied_sequence.0,
        durable_head: snapshot.durable_head.0,
        schema_table_count: snapshot.schema.tables.len(),
        document_count: snapshot.documents.len(),
        scheduled_execution_count: snapshot.scheduled_execution_ids.len(),
    })
}

pub fn bootstrap_fingerprint(bootstrap: &DurableJournalBootstrap) -> Result<BootstrapFingerprint> {
    Ok(BootstrapFingerprint {
        snapshot_digest: snapshot_fingerprint(&bootstrap.snapshot)?.digest,
        resume_after_sequence: bootstrap.resume_after.0,
        bootstrap_cut_sequence: bootstrap.bootstrap_cut.0,
        cursor_floor_sequence: bootstrap.cursor_floor.0,
    })
}

pub fn compare_materialized_journal_snapshots(
    left_scope: ConsistencyScope,
    left: &MaterializedJournalSnapshot,
    right_scope: ConsistencyScope,
    right: &MaterializedJournalSnapshot,
) -> Option<ConsistencyMismatch> {
    let left_canonical = canonicalize_materialized_journal_snapshot(left);
    let right_canonical = canonicalize_materialized_journal_snapshot(right);

    if left_canonical.version != right_canonical.version {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "version",
            left_canonical.version,
            right_canonical.version,
        ));
    }
    if left_canonical.applied_sequence != right_canonical.applied_sequence {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "applied_sequence",
            left_canonical.applied_sequence,
            right_canonical.applied_sequence,
        ));
    }
    if left_canonical.durable_head != right_canonical.durable_head {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "durable_head",
            left_canonical.durable_head,
            right_canonical.durable_head,
        ));
    }

    let left_schema_keys = left_canonical
        .schema
        .iter()
        .map(|table| table.table.to_string())
        .collect::<Vec<_>>();
    let right_schema_keys = right_canonical
        .schema
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
    for (left_table, right_table) in left_canonical.schema.iter().zip(&right_canonical.schema) {
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

    let left_document_keys = left_canonical
        .documents
        .iter()
        .map(document_key)
        .collect::<Vec<_>>();
    let right_document_keys = right_canonical
        .documents
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
    for (left_document, right_document) in left_canonical
        .documents
        .iter()
        .zip(&right_canonical.documents)
    {
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

    if left_canonical.scheduled_execution_ids != right_canonical.scheduled_execution_ids {
        return Some(mismatch(
            "materialized_snapshot_match",
            left_scope,
            right_scope,
            "scheduled_execution_ids",
            left_canonical.scheduled_execution_ids,
            right_canonical.scheduled_execution_ids,
        ));
    }

    None
}

pub fn collect_durable_journal_bootstrap_mismatches(
    authoritative_snapshot: &MaterializedJournalSnapshot,
    bootstrap: &DurableJournalBootstrap,
) -> Vec<ConsistencyMismatch> {
    let mut mismatches = Vec::new();

    if let Some(snapshot_mismatch) = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        authoritative_snapshot,
        ConsistencyScope::JournalBootstrap,
        &bootstrap.snapshot,
    ) {
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

    mismatches
}

fn canonicalize_materialized_journal_snapshot(
    snapshot: &MaterializedJournalSnapshot,
) -> CanonicalMaterializedJournalSnapshot {
    CanonicalMaterializedJournalSnapshot {
        version: snapshot.version,
        applied_sequence: snapshot.applied_sequence.0,
        durable_head: snapshot.durable_head.0,
        schema: canonicalize_schema(&snapshot.schema),
        documents: canonicalize_documents(&snapshot.documents),
        scheduled_execution_ids: canonicalize_scheduled_execution_ids(
            &snapshot.scheduled_execution_ids,
        ),
    }
}

fn canonicalize_schema(schema: &Schema) -> Vec<TableSchema> {
    let mut tables = schema.tables.values().cloned().collect::<Vec<_>>();
    tables.sort_by(|left, right| left.table.to_string().cmp(&right.table.to_string()));
    tables
}

fn canonicalize_documents(documents: &[Document]) -> Vec<Document> {
    let mut sorted = documents.to_vec();
    sorted.sort_by_key(document_key);
    sorted
}

fn canonicalize_scheduled_execution_ids(ids: &[String]) -> Vec<String> {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted
}

fn document_key(document: &Document) -> String {
    format!("{}/{}", document.table, document.id)
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

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(bytes.as_ref().len().saturating_mul(2));
    for byte in bytes.as_ref() {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}
