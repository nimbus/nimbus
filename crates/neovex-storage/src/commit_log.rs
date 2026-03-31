use neovex_core::{CommitEntry, Error, Result};

/// Serializes a commit entry for persistence.
pub fn serialize_commit(entry: &CommitEntry) -> Result<Vec<u8>> {
    rmp_serde::to_vec(entry).map_err(|error| Error::Serialization(error.to_string()))
}

/// Deserializes a commit entry from persistence bytes.
pub fn deserialize_commit(bytes: &[u8]) -> Result<CommitEntry> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}
