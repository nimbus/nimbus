use nimbus_core::{CommitEntry, Error, Result, StorageErrorKind, TenantEventRecord};

/// Serializes a tenant event record for persistence.
pub fn serialize_tenant_event_record(entry: &TenantEventRecord) -> Result<Vec<u8>> {
    rmp_serde::to_vec_named(entry).map_err(|error| Error::Serialization(error.to_string()))
}

/// Deserializes a tenant event record from persistence bytes and verifies integrity.
pub fn deserialize_tenant_event_record(bytes: &[u8]) -> Result<TenantEventRecord> {
    let record: TenantEventRecord =
        rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))?;
    record.validate_integrity()?;
    Ok(record)
}

/// Verifies that an incoming replay at an already-applied sequence names the
/// exact durable record that originally claimed that sequence.
///
/// Both records are decoded and integrity-checked before their semantic fields
/// are compared. Comparing decoded fields keeps this independent of backend
/// encodings and serialized map-key order while still covering the complete
/// durable payload rather than only its materialized document effect.
pub(crate) fn ensure_applied_record_matches(
    incoming: &TenantEventRecord,
    durable: Option<&TenantEventRecord>,
) -> Result<()> {
    incoming.validate_integrity()?;
    let Some(durable) = durable else {
        return Err(applied_record_corruption(
            incoming,
            "has no corresponding durable journal record",
        ));
    };
    durable.validate_integrity()?;

    let same_content = incoming.version == durable.version
        && incoming.sequence == durable.sequence
        && incoming.timestamp == durable.timestamp
        && incoming.events == durable.events
        && incoming.writes == durable.writes
        && incoming.scheduled_execution_id == durable.scheduled_execution_id;
    if same_content {
        Ok(())
    } else {
        Err(applied_record_corruption(
            incoming,
            "diverges from the durable journal record",
        ))
    }
}

fn applied_record_corruption(record: &TenantEventRecord, reason: &str) -> Error {
    let mut document_ids: Vec<&str> = record
        .writes
        .iter()
        .map(|write| write.doc_id.as_str())
        .collect();
    document_ids.sort_unstable();
    document_ids.dedup();
    let subject = if document_ids.is_empty() {
        "without a document".to_string()
    } else {
        format!("for document {}", document_ids.join(", "))
    };
    Error::storage(
        StorageErrorKind::Corruption,
        format!(
            "durable journal replay at already-applied sequence {} {subject} {reason}",
            record.sequence.0
        ),
    )
}

/// Serializes a commit entry by first promoting it into the durable journal format.
pub fn serialize_commit(entry: &CommitEntry) -> Result<Vec<u8>> {
    let record =
        TenantEventRecord::new(entry.sequence, entry.timestamp, entry.writes.clone(), None)?;
    serialize_tenant_event_record(&record)
}

/// Deserializes a commit entry from persistence bytes.
pub fn deserialize_commit(bytes: &[u8]) -> Result<CommitEntry> {
    Ok(deserialize_tenant_event_record(bytes)?.as_commit_entry())
}
