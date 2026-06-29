//! Structural backup bundle for the Nimbus object byte plane.
//!
//! A bundle is independent of placement: export reads a pinned set of blob
//! hashes from any [`BlobStore`], records the commit-log segment needed for
//! PITR, carries caller-provided key escrow, and restores into any other
//! [`BlobStore`] only when matching escrow material is presented.

use std::collections::BTreeSet;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use nimbus_core::{Error, Result, StorageErrorKind};

use crate::hash::{BLAKE3_HASH_LEN, BlobHash};
use crate::store::BlobStore;

const BUNDLE_MAGIC: &[u8] = b"NIMBUSOBJBACKUP1\n";

/// Opaque key escrow record carried by an object backup bundle.
///
/// The bytes are intentionally opaque at this layer. NOS5 owns how node master
/// keys wrap tenant DEKs; NOS4 requires backup/restore to fail closed unless the
/// same escrow record is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEscrow {
    id: String,
    wrapped_key_material: Bytes,
}

impl KeyEscrow {
    /// Creates a non-empty escrow record.
    pub fn new(id: impl Into<String>, wrapped_key_material: Bytes) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(Error::InvalidInput("key escrow id is required".to_string()));
        }
        if wrapped_key_material.is_empty() {
            return Err(Error::InvalidInput(
                "key escrow material is required".to_string(),
            ));
        }
        Ok(Self {
            id,
            wrapped_key_material,
        })
    }

    /// Stable operator-visible escrow identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Opaque wrapped key material.
    pub fn wrapped_key_material(&self) -> &[u8] {
        &self.wrapped_key_material
    }
}

/// Input to a backup export.
#[derive(Clone, Debug)]
pub struct BackupRequest {
    root_hashes: Vec<BlobHash>,
    manifest_snapshot: Bytes,
    commit_log_segment: Bytes,
    key_escrow: KeyEscrow,
}

impl BackupRequest {
    /// Builds a backup request from the snapshot roots and structural metadata.
    pub fn new(
        root_hashes: impl IntoIterator<Item = BlobHash>,
        manifest_snapshot: Bytes,
        commit_log_segment: Bytes,
        key_escrow: KeyEscrow,
    ) -> Result<Self> {
        if commit_log_segment.is_empty() {
            return Err(Error::InvalidInput(
                "backup bundle requires a commit_log segment".to_string(),
            ));
        }
        Ok(Self {
            root_hashes: root_hashes.into_iter().collect(),
            manifest_snapshot,
            commit_log_segment,
            key_escrow,
        })
    }
}

/// One content-addressed chunk carried in a backup bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupChunk {
    pub hash: BlobHash,
    pub bytes: Bytes,
}

/// Portable object-storage backup artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupBundle {
    manifest_snapshot: Bytes,
    commit_log_segment: Bytes,
    key_escrow: KeyEscrow,
    chunks: Vec<BackupChunk>,
}

impl BackupBundle {
    /// Manifest snapshot bytes captured at the backup root.
    pub fn manifest_snapshot(&self) -> &[u8] {
        &self.manifest_snapshot
    }

    /// Commit-log segment that makes the bundle point-in-time restorable.
    pub fn commit_log_segment(&self) -> &[u8] {
        &self.commit_log_segment
    }

    /// Escrow record required for restore.
    pub fn key_escrow(&self) -> &KeyEscrow {
        &self.key_escrow
    }

    /// Chunks carried by this bundle.
    pub fn chunks(&self) -> &[BackupChunk] {
        &self.chunks
    }

    /// Encodes this bundle as one portable byte artifact.
    pub fn encode(&self) -> Bytes {
        let mut out = BytesMut::new();
        out.extend_from_slice(BUNDLE_MAGIC);
        put_bytes(&mut out, self.key_escrow.id.as_bytes());
        put_bytes(&mut out, &self.key_escrow.wrapped_key_material);
        put_bytes(&mut out, &self.manifest_snapshot);
        put_bytes(&mut out, &self.commit_log_segment);
        out.put_u64(self.chunks.len() as u64);
        for chunk in &self.chunks {
            out.extend_from_slice(chunk.hash.as_bytes());
            put_bytes(&mut out, &chunk.bytes);
        }
        out.freeze()
    }

    /// Decodes a bundle previously produced by [`encode`](Self::encode).
    pub fn decode(mut bytes: Bytes) -> Result<Self> {
        if bytes.len() < BUNDLE_MAGIC.len() || &bytes[..BUNDLE_MAGIC.len()] != BUNDLE_MAGIC {
            return Err(corruption("backup bundle has invalid magic"));
        }
        bytes.advance(BUNDLE_MAGIC.len());
        let escrow_id = String::from_utf8(read_bytes(&mut bytes)?.to_vec())
            .map_err(|err| Error::InvalidInput(format!("key escrow id is not UTF-8: {err}")))?;
        let key_escrow = KeyEscrow::new(escrow_id, read_bytes(&mut bytes)?)?;
        let manifest_snapshot = read_bytes(&mut bytes)?;
        let commit_log_segment = read_bytes(&mut bytes)?;
        if bytes.remaining() < 8 {
            return Err(corruption("backup bundle ended before chunk count"));
        }
        let chunk_count = bytes.get_u64();
        let mut chunks = Vec::new();
        for _ in 0..chunk_count {
            if bytes.remaining() < BLAKE3_HASH_LEN {
                return Err(corruption("backup bundle ended mid chunk hash"));
            }
            let mut raw_hash = [0u8; BLAKE3_HASH_LEN];
            bytes.copy_to_slice(&mut raw_hash);
            let hash = BlobHash::from_bytes(raw_hash);
            let chunk_bytes = read_bytes(&mut bytes)?;
            let actual = BlobHash::of(&chunk_bytes);
            if actual != hash {
                return Err(corruption(format!(
                    "backup chunk {hash} contains bytes hashing to {actual}"
                )));
            }
            chunks.push(BackupChunk {
                hash,
                bytes: chunk_bytes,
            });
        }
        if bytes.has_remaining() {
            return Err(corruption("backup bundle has trailing bytes"));
        }
        Ok(Self {
            manifest_snapshot,
            commit_log_segment,
            key_escrow,
            chunks,
        })
    }
}

/// Restore statistics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackupRestoreReport {
    pub restored_chunks: usize,
    pub restored_bytes: u64,
}

/// Backup/restore operations over any [`BlobStore`] placement.
pub struct ObjectBackup;

impl ObjectBackup {
    /// Exports a bundle from a pinned set of blob roots.
    pub async fn export_bundle(
        source: &dyn BlobStore,
        request: BackupRequest,
    ) -> Result<BackupBundle> {
        let mut chunks = Vec::new();
        for hash in request.root_hashes.into_iter().collect::<BTreeSet<_>>() {
            let bytes = source.get(&hash).await?;
            let actual = BlobHash::of(&bytes);
            if actual != hash {
                return Err(corruption(format!(
                    "source blob {hash} contains bytes hashing to {actual}"
                )));
            }
            chunks.push(BackupChunk { hash, bytes });
        }
        Ok(BackupBundle {
            manifest_snapshot: request.manifest_snapshot,
            commit_log_segment: request.commit_log_segment,
            key_escrow: request.key_escrow,
            chunks,
        })
    }

    /// Restores a bundle into `target`, failing closed without matching escrow.
    pub async fn restore_bundle(
        target: &dyn BlobStore,
        bundle: &BackupBundle,
        key_escrow: Option<&KeyEscrow>,
    ) -> Result<BackupRestoreReport> {
        let presented = key_escrow.ok_or_else(|| {
            Error::PreconditionFailed("restore requires key escrow material".to_string())
        })?;
        if presented != bundle.key_escrow() {
            return Err(Error::PermissionDenied(
                "restore key escrow does not match bundle".to_string(),
            ));
        }

        let mut report = BackupRestoreReport::default();
        for chunk in bundle.chunks() {
            let restored = target.put(chunk.bytes.clone()).await?;
            if restored != chunk.hash {
                return Err(corruption(format!(
                    "restore wrote chunk {} but target returned {restored}",
                    chunk.hash
                )));
            }
            report.restored_chunks += 1;
            report.restored_bytes = report
                .restored_bytes
                .saturating_add(chunk.bytes.len() as u64);
        }
        Ok(report)
    }
}

fn put_bytes(out: &mut BytesMut, bytes: &[u8]) {
    out.put_u64(bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn read_bytes(bytes: &mut Bytes) -> Result<Bytes> {
    if bytes.remaining() < 8 {
        return Err(corruption("backup bundle ended before length"));
    }
    let len = bytes.get_u64();
    if len > usize::MAX as u64 {
        return Err(corruption("backup bundle length exceeds platform usize"));
    }
    let len = len as usize;
    if bytes.remaining() < len {
        return Err(corruption("backup bundle ended mid field"));
    }
    Ok(bytes.split_to(len))
}

fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryBlobStore;

    #[tokio::test]
    async fn backup_bundle_round_trips_chunks_commit_log_segment_and_key_escrow() {
        let source = MemoryBlobStore::new();
        let first = source.put(Bytes::from_static(b"first")).await.unwrap();
        let second = source.put(Bytes::from_static(b"second")).await.unwrap();
        let escrow = KeyEscrow::new(
            "tenant-a",
            Bytes::from_static(b"wrapped-master-kek-and-dek"),
        )
        .unwrap();
        let request = BackupRequest::new(
            [second, first, first],
            Bytes::from_static(b"manifest-snapshot"),
            Bytes::from_static(b"commit_log segment: lsn 41..44"),
            escrow.clone(),
        )
        .unwrap();

        let bundle = ObjectBackup::export_bundle(&source, request).await.unwrap();
        assert_eq!(bundle.chunks().len(), 2, "duplicate roots are deduped");
        assert_eq!(
            bundle.commit_log_segment(),
            b"commit_log segment: lsn 41..44"
        );
        assert_eq!(bundle.key_escrow().id(), "tenant-a");

        let encoded = bundle.encode();
        let decoded = BackupBundle::decode(encoded).unwrap();
        assert_eq!(decoded.manifest_snapshot(), b"manifest-snapshot");

        let target = MemoryBlobStore::new();
        let missing_escrow = ObjectBackup::restore_bundle(&target, &decoded, None)
            .await
            .unwrap_err();
        assert!(matches!(missing_escrow, Error::PreconditionFailed(_)));

        let report = ObjectBackup::restore_bundle(&target, &decoded, Some(&escrow))
            .await
            .unwrap();
        assert_eq!(report.restored_chunks, 2);
        assert_eq!(
            target.get(&first).await.unwrap(),
            Bytes::from_static(b"first")
        );
        assert_eq!(
            target.get(&second).await.unwrap(),
            Bytes::from_static(b"second")
        );
    }

    #[test]
    fn backup_bundle_decode_rejects_corrupt_chunk_bytes() {
        let hash = BlobHash::of(b"expected");
        let escrow = KeyEscrow::new("tenant-a", Bytes::from_static(b"wrapped-key")).unwrap();
        let bundle = BackupBundle {
            manifest_snapshot: Bytes::new(),
            commit_log_segment: Bytes::from_static(b"commit_log segment"),
            key_escrow: escrow,
            chunks: vec![BackupChunk {
                hash,
                bytes: Bytes::from_static(b"tampered"),
            }],
        };

        let err = BackupBundle::decode(bundle.encode()).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }
}
