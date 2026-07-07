use std::collections::BTreeSet;

use bytes::Bytes;
use nimbus_blob::{BlobHash, BlobStore};
use nimbus_core::{Error, Result};
use nimbus_storage::{ObjectBlobLayout, ObjectManifest, ObjectMultipartUpload};

pub(crate) async fn read_manifest_bytes(
    blobs: &dyn BlobStore,
    manifest: &ObjectManifest,
) -> Result<Bytes> {
    match &manifest.blob_layout {
        ObjectBlobLayout::Whole { blob_hash } => blobs.get(&parse_blob_hash(blob_hash)?).await,
        ObjectBlobLayout::Chunked { chunks } => {
            let mut out = Vec::with_capacity(manifest.size as usize);
            let mut expected_offset = 0_u64;
            for chunk in chunks {
                if chunk.offset != expected_offset {
                    return Err(corrupt_manifest(format!(
                        "chunk offset {} does not match expected offset {expected_offset}",
                        chunk.offset
                    )));
                }
                let bytes = blobs.get(&parse_blob_hash(&chunk.blob_hash)?).await?;
                if bytes.len() as u64 != chunk.len {
                    return Err(corrupt_manifest(format!(
                        "chunk at offset {} expected {} bytes but blob returned {} bytes",
                        chunk.offset,
                        chunk.len,
                        bytes.len()
                    )));
                }
                out.extend_from_slice(&bytes);
                expected_offset = expected_offset
                    .checked_add(chunk.len)
                    .ok_or_else(|| corrupt_manifest("chunk offsets overflow u64".to_string()))?;
            }
            if expected_offset != manifest.size {
                return Err(corrupt_manifest(format!(
                    "chunked object size {expected_offset} does not match manifest size {}",
                    manifest.size
                )));
            }
            Ok(Bytes::from(out))
        }
    }
}

pub(crate) async fn release_manifest_blobs(
    blobs: &dyn BlobStore,
    manifest: &ObjectManifest,
) -> Result<()> {
    release_manifest_blobs_except(blobs, manifest, None).await
}

pub(crate) async fn release_manifest_blobs_except(
    blobs: &dyn BlobStore,
    manifest: &ObjectManifest,
    retained: Option<&ObjectManifest>,
) -> Result<()> {
    let retained = match retained {
        Some(manifest) => manifest_blob_hashes(manifest)?,
        None => BTreeSet::new(),
    };
    match &manifest.blob_layout {
        ObjectBlobLayout::Whole { blob_hash } => {
            let hash = parse_blob_hash(blob_hash)?;
            if !retained.contains(&hash) {
                blobs.release(&hash).await?;
            }
        }
        ObjectBlobLayout::Chunked { chunks } => {
            for chunk in chunks {
                let hash = parse_blob_hash(&chunk.blob_hash)?;
                if !retained.contains(&hash) {
                    blobs.release(&hash).await?;
                }
            }
        }
    }
    Ok(())
}

fn manifest_blob_hashes(manifest: &ObjectManifest) -> Result<BTreeSet<BlobHash>> {
    let mut hashes = BTreeSet::new();
    match &manifest.blob_layout {
        ObjectBlobLayout::Whole { blob_hash } => {
            hashes.insert(parse_blob_hash(blob_hash)?);
        }
        ObjectBlobLayout::Chunked { chunks } => {
            for chunk in chunks {
                hashes.insert(parse_blob_hash(&chunk.blob_hash)?);
            }
        }
    }
    Ok(hashes)
}

pub(crate) fn manifest_contains_blob(manifest: &ObjectManifest, hash: &BlobHash) -> Result<bool> {
    Ok(manifest_blob_hashes(manifest)?.contains(hash))
}

pub(crate) fn multipart_upload_contains_blob(
    upload: &ObjectMultipartUpload,
    hash: &BlobHash,
) -> Result<bool> {
    for part in &upload.parts {
        if parse_blob_hash(&part.blob_hash)? == *hash {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn parse_blob_hash(value: &str) -> Result<BlobHash> {
    BlobHash::from_hex(value)
}

fn corrupt_manifest(message: String) -> Error {
    Error::storage(
        nimbus_core::StorageErrorKind::Corruption,
        format!("object manifest corruption: {message}"),
    )
}
