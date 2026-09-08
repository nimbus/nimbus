//! Object lifecycle for native callers over one resolved tenant.
//!
//! The S3 service and the Convex storage binding each own a wire surface, but
//! the blob lifecycle underneath them is one contract: a manifest publishes a
//! blob, a superseded manifest releases the blobs the replacement does not
//! retain, and a deleted manifest releases all of its blobs. A caller that is
//! neither S3 nor Convex (the operator console, an embedder) needs that same
//! contract without a wire protocol in front of it. This module is that seam.

use bytes::Bytes;
use nimbus_core::Result;
use nimbus_storage::{ObjectManifest, ObjectManifestAttributes};

use crate::backend::{S3TenantObjects, delete_manifest_unconditional, put_manifest_unconditional};
use crate::checksum::ComputedChecksums;
use crate::object_io::{
    manifest_contains_blob, read_manifest_bytes, release_manifest_blobs,
    release_manifest_blobs_except,
};

/// Stores `bytes` as one whole-blob object at `bucket`/`key`, replacing any
/// object already there.
///
/// The manifest ETag is the hex MD5 of the bytes, as the S3 surface writes
/// it, so an object stored here and read back over S3 carries the ETag an S3
/// client expects. `last_modified_millis` is the system clock at the commit.
///
/// # Errors
/// Fails if the bucket or key is invalid, if the tenant is mid-deletion, or
/// if the byte plane or the commit path fails. A blob whose manifest commit
/// failed is released again unless the manifest that is current after the
/// failure still retains it.
pub async fn put_whole_object(
    ctx: &S3TenantObjects,
    bucket: &str,
    key: &str,
    bytes: Bytes,
    content_type: Option<String>,
) -> Result<ObjectManifest> {
    let byte_len = bytes.len() as u64;
    let computed = ComputedChecksums::for_bytes(&bytes);
    let blobs = ctx.blobs().await?;
    let hash = blobs.put(bytes).await?;
    let mut attributes = ObjectManifestAttributes::new(
        computed.md5_hex.clone(),
        nimbus_core::clock::system_now_millis(),
    );
    attributes.content_type = content_type;
    attributes.checksums = computed.object_checksums();
    let manifest = match ObjectManifest::whole(bucket, key, byte_len, hash.to_hex(), attributes) {
        Ok(manifest) => manifest,
        Err(error) => {
            blobs.release(&hash).await?;
            return Err(error);
        }
    };
    let previous = match put_manifest_unconditional(ctx.meta.as_ref(), manifest.clone()).await {
        Ok(previous) => previous,
        Err(error) => {
            // The commit did not land, but a concurrent writer may have
            // published the same bytes under this key in the meantime; only
            // release the blob when no current manifest retains it.
            let current = ctx.meta.get_manifest(bucket, key).await?;
            let retained = match current.as_ref() {
                Some(current) => manifest_contains_blob(current, &hash)?,
                None => false,
            };
            if !retained {
                blobs.release(&hash).await?;
            }
            return Err(error);
        }
    };
    if let Some(previous) = previous {
        release_manifest_blobs_except(blobs.as_ref(), &previous, Some(&manifest)).await?;
    }
    Ok(manifest)
}

/// Reads the bytes a manifest describes, whole or chunked.
///
/// # Errors
/// Fails if the tenant is mid-deletion, a blob is missing, or the manifest
/// and its blobs disagree on size or layout.
pub async fn read_object_bytes(ctx: &S3TenantObjects, manifest: &ObjectManifest) -> Result<Bytes> {
    let blobs = ctx.blobs().await?;
    read_manifest_bytes(blobs.as_ref(), manifest).await
}

/// Deletes the object at `bucket`/`key` and releases its blobs. Returns the
/// deleted manifest, or `None` when there was no object to delete.
///
/// # Errors
/// Fails if the bucket or key is invalid, the tenant is mid-deletion, or the
/// commit or the release fails.
pub async fn delete_object(
    ctx: &S3TenantObjects,
    bucket: &str,
    key: &str,
) -> Result<Option<ObjectManifest>> {
    let Some((_, manifest)) = delete_manifest_unconditional(ctx.meta.as_ref(), bucket, key).await?
    else {
        return Ok(None);
    };
    let blobs = ctx.blobs().await?;
    release_manifest_blobs(blobs.as_ref(), &manifest).await?;
    Ok(Some(manifest))
}
