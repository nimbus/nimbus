//! GC root enumeration for the object byte plane (RFS6).
//!
//! The local pack GC ([`nimbus_blob::BlobGc`]) marks live blobs against a
//! [`nimbus_blob::BlobGcRoots`] set. This module turns the storage-coupled
//! root sources — committed object manifests and **open multipart uploads** —
//! into that set, above the `nimbus-blob` dependency fence. A caller composes
//! the result into a [`nimbus_blob::CompositeBlobRoots`] alongside snapshot
//! roots and the in-flight-backup / write-intent pin registries.
//!
//! In-flight multipart parts are live roots for exactly as long as the upload
//! is enumerable here: a part uploaded but not yet committed into a manifest
//! would otherwise be unrooted and reclaimable mid-upload.

use std::collections::BTreeSet;

use nimbus_blob::{BlobHash, StaticBlobRoots};
use nimbus_core::Result;
use nimbus_storage::{ObjectBlobLayout, ObjectManifest, ObjectMultipartUpload};

/// Unions every live blob root named by `manifests` (whole + chunked parts)
/// and `multipart_uploads` (each uploaded part) into one set.
///
/// This is the exact enumeration a GC scheduler runs before a sweep. It is
/// consumer-agnostic (takes already-listed manifests/uploads) so it has no
/// engine or bucket-iteration coupling; the caller supplies the listing.
pub fn object_gc_roots(
    manifests: &[ObjectManifest],
    multipart_uploads: &[ObjectMultipartUpload],
) -> Result<BTreeSet<BlobHash>> {
    let mut roots = BTreeSet::new();
    for manifest in manifests {
        match &manifest.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } => {
                roots.insert(BlobHash::from_hex(blob_hash)?);
            }
            ObjectBlobLayout::Chunked { chunks } => {
                for chunk in chunks {
                    roots.insert(BlobHash::from_hex(&chunk.blob_hash)?);
                }
            }
        }
    }
    for upload in multipart_uploads {
        for part in &upload.parts {
            roots.insert(BlobHash::from_hex(&part.blob_hash)?);
        }
    }
    Ok(roots)
}

/// Builds a [`StaticBlobRoots`] provider from the same sources, ready to drop
/// into a [`nimbus_blob::CompositeBlobRoots`].
pub fn object_gc_roots_provider(
    manifests: &[ObjectManifest],
    multipart_uploads: &[ObjectMultipartUpload],
) -> Result<StaticBlobRoots> {
    Ok(StaticBlobRoots::new(object_gc_roots(
        manifests,
        multipart_uploads,
    )?))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use nimbus_blob::{BlobGc, BlobGcRoots, BlobStore, CompositeBlobRoots, LocalPackStore};
    use nimbus_storage::{ObjectManifestAttributes, ObjectMultipartPart};

    use super::*;

    fn manifest_whole(bucket: &str, key: &str, hash: &BlobHash, len: u64) -> ObjectManifest {
        ObjectManifest::whole(
            bucket,
            key,
            len,
            hash.to_hex(),
            ObjectManifestAttributes::new("\"etag\"", 1),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn gc_roots_union_manifest_and_multipart_part_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalPackStore::open(dir.path()).unwrap();
        let committed = store
            .put(Bytes::from_static(b"committed object"))
            .await
            .unwrap();
        let in_flight = store
            .put(Bytes::from_static(b"multipart part"))
            .await
            .unwrap();
        let orphan = store
            .put(Bytes::from_static(b"orphan, unrooted"))
            .await
            .unwrap();

        let manifests = vec![manifest_whole("bucket", "committed.txt", &committed, 16)];
        let mut upload =
            ObjectMultipartUpload::new("upl-1", "bucket", "big.bin", None, Default::default(), 0)
                .unwrap();
        upload.parts.push(ObjectMultipartPart {
            part_number: 1,
            blob_hash: in_flight.to_hex(),
            size: 14,
            etag: "\"part-etag\"".to_string(),
            checksums: nimbus_storage::ObjectChecksums::default(),
            last_modified_millis: 0,
        });

        let roots = object_gc_roots(&manifests, &[upload.clone()]).unwrap();
        assert!(
            roots.contains(&committed),
            "committed manifest blob is a root"
        );
        assert!(
            roots.contains(&in_flight),
            "in-flight multipart part is a root"
        );
        assert!(
            !roots.contains(&orphan),
            "an unreferenced blob is not a root"
        );

        // Composed with the nimbus-blob GC, the orphan is reclaimed while the
        // committed + in-flight roots survive.
        let provider = object_gc_roots_provider(&manifests, &[upload]).unwrap();
        let roots_for_gc: CompositeBlobRoots =
            CompositeBlobRoots::new().with(Arc::new(provider) as Arc<dyn BlobGcRoots>);
        let gc = BlobGc::new(store.clone(), roots_for_gc, std::time::Duration::ZERO);
        let report = gc.sweep().await.unwrap();

        assert_eq!(report.referenced_retained, 2);
        assert_eq!(report.swept, 1);
        assert!(store.has(&committed).await.unwrap());
        assert!(store.has(&in_flight).await.unwrap());
        assert!(!store.has(&orphan).await.unwrap());
    }
}
