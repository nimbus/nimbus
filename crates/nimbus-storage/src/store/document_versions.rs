use nimbus_core::{Document, DocumentId, Result, SequenceNumber, TableId, Timestamp, WriteOp};
use redb::{ReadableTable, TableError};
use serde::{Deserialize, Serialize};

use crate::diagnostics::DocumentVersionStorageDiagnostic;
use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::keys::{document_version_key, document_version_prefix, prefix_end};
use crate::store::{DOCUMENT_VERSIONS, METADATA, TenantReadSnapshot, TenantStore, map_redb_error};
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64,
    validate_document_version_storage_format, validate_document_version_storage_format_state,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocumentVersionValue {
    commit_timestamp: Timestamp,
    document: Option<Vec<u8>>,
}

impl TenantStore {
    pub fn get_document_version_at(
        &self,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        self.read_snapshot()?
            .get_document_version_at(table_id, document_id, sequence)
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        self.read_snapshot()?.document_version_storage_diagnostic()
    }
}

impl TenantReadSnapshot {
    pub fn get_document_version_at(
        &self,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        get_document_version_at_in_read_txn(&self.read_txn, table_id, document_id, sequence)
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        document_version_storage_diagnostic_in_read_txn(&self.read_txn)
    }
}

pub(super) fn record_document_versions_for_writes(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &[WriteOp],
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    ensure_document_version_storage_format_in_write_txn(write_txn)?;
    let mut versions = write_txn
        .open_table(DOCUMENT_VERSIONS)
        .map_err(map_redb_error)?;
    for write in writes {
        let value = DocumentVersionValue {
            commit_timestamp: timestamp,
            document: write
                .current
                .as_ref()
                .map(encode_document_msgpack)
                .transpose()
                .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?,
        };
        let encoded = rmp_serde::to_vec_named(&value)
            .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
        versions
            .insert(
                document_version_key(&write.table_id, &write.doc_id, sequence).as_slice(),
                encoded.as_slice(),
            )
            .map_err(map_redb_error)?;
    }
    Ok(())
}

fn get_document_version_at_in_read_txn(
    read_txn: &redb::ReadTransaction,
    table_id: &TableId,
    document_id: &DocumentId,
    sequence: SequenceNumber,
) -> Result<Option<Document>> {
    validate_document_version_storage_format_for_read_txn(read_txn)?;
    let versions = match read_txn.open_table(DOCUMENT_VERSIONS) {
        Ok(versions) => versions,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    let prefix = document_version_prefix(table_id, document_id);
    let mut latest = None;
    match prefix_end(&prefix) {
        Some(end) => {
            for item in versions
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if version_sequence_from_key(key.value())? <= sequence {
                    latest = Some(value.value().to_vec());
                } else {
                    break;
                }
            }
        }
        None => {
            for item in versions
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                if version_sequence_from_key(key.value())? <= sequence {
                    latest = Some(value.value().to_vec());
                } else {
                    break;
                }
            }
        }
    }

    latest
        .map(|bytes| decode_version_value(bytes.as_slice()))
        .transpose()
        .map(|value| value.and_then(|value| value.document))
}

fn document_version_storage_diagnostic_in_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<DocumentVersionStorageDiagnostic> {
    let format_version = load_document_version_storage_format_in_read_txn(read_txn)?;
    let mut version_count = 0_u64;
    let mut min_sequence = None;
    let mut max_sequence = None;

    match read_txn.open_table(DOCUMENT_VERSIONS) {
        Ok(versions) => {
            for item in versions.iter().map_err(map_redb_error)? {
                let (key, _) = item.map_err(map_redb_error)?;
                let sequence = version_sequence_from_key(key.value())?;
                version_count = version_count.saturating_add(1);
                min_sequence = Some(
                    min_sequence.map_or(sequence, |current: SequenceNumber| current.min(sequence)),
                );
                max_sequence = Some(
                    max_sequence.map_or(sequence, |current: SequenceNumber| current.max(sequence)),
                );
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(error) => return Err(map_redb_error(error)),
    }

    validate_document_version_storage_format_state(format_version, version_count > 0)?;
    Ok(DocumentVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

fn validate_document_version_storage_format_for_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<()> {
    let format_version = load_document_version_storage_format_in_read_txn(read_txn)?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_document_version_storage_format(format_version)?;
            false
        }
        None => document_versions_have_rows_in_read_txn(read_txn)?,
    };
    validate_document_version_storage_format_state(format_version, has_versions)
}

fn ensure_document_version_storage_format_in_write_txn(
    write_txn: &redb::WriteTransaction,
) -> Result<()> {
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    let existing = metadata
        .get(DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .map_err(map_redb_error)?
        .map(|value| value.value().to_vec());
    if let Some(bytes) = existing {
        let version = storage_format_version_from_u64(decode_format_u64(bytes.as_slice())?)?;
        validate_document_version_storage_format(version)?;
        return Ok(());
    }

    metadata
        .insert(
            DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
            encode_format_u64(CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0.into()).as_slice(),
        )
        .map_err(map_redb_error)?;
    Ok(())
}

fn load_document_version_storage_format_in_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<Option<StorageFormatVersion>> {
    let metadata = match read_txn.open_table(METADATA) {
        Ok(metadata) => metadata,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    metadata
        .get(DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .map_err(map_redb_error)?
        .map(|value| storage_format_version_from_u64(decode_format_u64(value.value())?))
        .transpose()
}

fn document_versions_have_rows_in_read_txn(read_txn: &redb::ReadTransaction) -> Result<bool> {
    let versions = match read_txn.open_table(DOCUMENT_VERSIONS) {
        Ok(versions) => versions,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(error) => return Err(map_redb_error(error)),
    };
    Ok(versions
        .iter()
        .map_err(map_redb_error)?
        .next()
        .transpose()
        .map_err(map_redb_error)?
        .is_some())
}

fn decode_version_value(bytes: &[u8]) -> Result<DecodedDocumentVersion> {
    let value: DocumentVersionValue = rmp_serde::from_slice(bytes)
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
    Ok(DecodedDocumentVersion {
        document: value
            .document
            .map(|bytes| decode_document_msgpack(bytes.as_slice()))
            .transpose()
            .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?,
    })
}

struct DecodedDocumentVersion {
    document: Option<Document>,
}

fn version_sequence_from_key(key: &[u8]) -> Result<SequenceNumber> {
    let sequence_bytes = key.get(key.len().saturating_sub(8)..).ok_or_else(|| {
        nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "document version key is too short",
        )
    })?;
    let array: [u8; 8] = sequence_bytes.try_into().map_err(|_| {
        nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "document version key has invalid sequence suffix",
        )
    })?;
    Ok(SequenceNumber(u64::from_be_bytes(array)))
}

fn encode_format_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_format_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes.try_into().map_err(|_| {
        nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "document-version storage format marker is not a u64",
        )
    })?;
    Ok(u64::from_be_bytes(array))
}
