use std::collections::BTreeSet;

use nimbus_blob::BlobHash;
use nimbus_core::{Error, Result};
use nimbus_storage::{
    OBJECT_MANIFEST_TABLE, ObjectBlobLayout, ObjectManifest, PointInTimeRestoreArchive,
};

/// Extracts committed object blob roots from a materialized PITR archive.
pub fn object_backup_roots(archive: &PointInTimeRestoreArchive) -> Result<Vec<BlobHash>> {
    if !archive.journal_tail.is_empty() {
        return Err(Error::InvalidInput(
            "object backup root extraction requires a materialized archive with an empty journal tail"
                .to_string(),
        ));
    }

    let mut roots = BTreeSet::new();
    for document in &archive.base_snapshot.documents {
        if document.table.as_str() != OBJECT_MANIFEST_TABLE {
            continue;
        }
        let manifest = ObjectManifest::from_document(document)?;
        match manifest.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } => {
                roots.insert(BlobHash::from_hex(&blob_hash)?);
            }
            ObjectBlobLayout::Chunked { chunks } => {
                for chunk in chunks {
                    roots.insert(BlobHash::from_hex(&chunk.blob_hash)?);
                }
            }
        }
    }
    Ok(roots.into_iter().collect())
}
