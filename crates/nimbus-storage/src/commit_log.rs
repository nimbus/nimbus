use nimbus_core::{CommitEntry, DurableMutationRecord, Error, Result};

/// Serializes a durable mutation record for persistence.
pub fn serialize_durable_record(entry: &DurableMutationRecord) -> Result<Vec<u8>> {
    rmp_serde::to_vec_named(entry).map_err(|error| Error::Serialization(error.to_string()))
}

/// Deserializes a durable mutation record from persistence bytes and verifies integrity.
pub fn deserialize_durable_record(bytes: &[u8]) -> Result<DurableMutationRecord> {
    let record: DurableMutationRecord =
        rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))?;
    record.validate_integrity()?;
    Ok(record)
}

/// Serializes a commit entry by first promoting it into the durable journal format.
pub fn serialize_commit(entry: &CommitEntry) -> Result<Vec<u8>> {
    let record =
        DurableMutationRecord::new(entry.sequence, entry.timestamp, entry.writes.clone(), None)?;
    serialize_durable_record(&record)
}

/// Deserializes a commit entry from persistence bytes.
pub fn deserialize_commit(bytes: &[u8]) -> Result<CommitEntry> {
    Ok(deserialize_durable_record(bytes)?.as_commit_entry())
}
