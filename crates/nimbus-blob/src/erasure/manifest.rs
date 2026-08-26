use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use nimbus_core::{Error, Result, StorageErrorKind};

use crate::disk::{self, SyncObserver};
use crate::hash::BlobHash;

// NBLE1 implicitly commits to reed-solomon-simd=3.1.0 parity bytes. If the
// codec output changes, use NBLE2 so heal never re-encodes an old manifest
// under a different durable format.
const MANIFEST_MAGIC: &[u8] = b"NBLE1";
const MANIFEST_EXT: &str = "nblm";
const CHECKSUM_LEN: usize = crate::BLAKE3_HASH_LEN;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShardRef {
    pub(crate) shard_index: u16,
    pub(crate) shard_hash: BlobHash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ErasureManifest {
    pub(crate) generation: u64,
    pub(crate) blob_hash: BlobHash,
    pub(crate) blob_len: u64,
    pub(crate) data_shards: usize,
    pub(crate) parity_shards: usize,
    pub(crate) stripe_width: usize,
    /// BLAKE3 of each stripe's TRUE payload (pre-padding). Range reads
    /// verify reassembled stripes against these instead of the whole-blob
    /// hash (which would require reading every stripe), closing the
    /// wrong-shard-ref bug class for partial reads: a manifest whose shard
    /// refs drifted (e.g. a heal bug) fails the stripe hash instead of
    /// serving wrong bytes. A deliberately forged manifest is out of scope —
    /// in the shipped composition the encryption layer above authenticates
    /// bytes end-to-end via AEAD frames.
    pub(crate) stripe_hashes: Vec<BlobHash>,
    pub(crate) stripes: Vec<Vec<ShardRef>>,
}

impl ErasureManifest {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.generation.to_le_bytes());
        body.extend_from_slice(self.blob_hash.as_bytes());
        body.extend_from_slice(&self.blob_len.to_le_bytes());
        body.extend_from_slice(&(self.data_shards as u16).to_le_bytes());
        body.extend_from_slice(&(self.parity_shards as u16).to_le_bytes());
        body.extend_from_slice(&(self.stripe_width as u64).to_le_bytes());
        body.extend_from_slice(&(self.stripes.len() as u64).to_le_bytes());
        for (stripe, stripe_hash) in self.stripes.iter().zip(&self.stripe_hashes) {
            body.extend_from_slice(stripe_hash.as_bytes());
            body.extend_from_slice(&(stripe.len() as u16).to_le_bytes());
            for shard in stripe {
                body.extend_from_slice(&shard.shard_index.to_le_bytes());
                body.extend_from_slice(shard.shard_hash.as_bytes());
            }
        }

        let checksum = blake3::hash(&body);
        let mut out = Vec::with_capacity(MANIFEST_MAGIC.len() + body.len() + CHECKSUM_LEN);
        out.extend_from_slice(MANIFEST_MAGIC);
        out.extend_from_slice(&body);
        out.extend_from_slice(checksum.as_bytes());
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if !bytes.starts_with(MANIFEST_MAGIC) {
            return Err(corruption("erasure manifest has invalid magic"));
        }
        if bytes.len() < MANIFEST_MAGIC.len() + CHECKSUM_LEN {
            return Err(corruption("erasure manifest is truncated"));
        }
        let checksum_start = bytes.len() - CHECKSUM_LEN;
        let body = &bytes[MANIFEST_MAGIC.len()..checksum_start];
        let expected = blake3::hash(body);
        if expected.as_bytes() != &bytes[checksum_start..] {
            return Err(corruption("erasure manifest checksum mismatch"));
        }

        let mut cursor = 0usize;
        let generation = read_u64(body, &mut cursor)?;
        let blob_hash = read_hash(body, &mut cursor)?;
        let blob_len = read_u64(body, &mut cursor)?;
        let data_shards = read_u16(body, &mut cursor)? as usize;
        let parity_shards = read_u16(body, &mut cursor)? as usize;
        let stripe_width = usize::try_from(read_u64(body, &mut cursor)?)
            .map_err(|_| corruption("erasure manifest stripe width overflows usize"))?;
        validate_layout(data_shards, parity_shards, stripe_width)?;
        let stripe_count = usize::try_from(read_u64(body, &mut cursor)?)
            .map_err(|_| corruption("erasure manifest stripe count overflows usize"))?;
        let expected_stripes = if blob_len == 0 {
            0
        } else {
            usize::try_from(blob_len.div_ceil(stripe_width as u64))
                .map_err(|_| corruption("erasure manifest expected stripe count overflows usize"))?
        };
        if stripe_count != expected_stripes {
            return Err(corruption(format!(
                "erasure manifest has {stripe_count} stripes for blob_len {blob_len} and stripe_width {stripe_width}; expected {expected_stripes}"
            )));
        }

        let total = data_shards + parity_shards;
        // Bound the count by what the body can physically hold BEFORE any
        // allocation: a crafted (checksum-valid) manifest with a huge
        // blob_len/stripe_count must fail structurally, not via a
        // capacity-overflow panic or an enormous reservation.
        let stripe_record_len = CHECKSUM_LEN + 2 + total * (2 + crate::BLAKE3_HASH_LEN);
        let max_stripes = body.len().saturating_sub(cursor) / stripe_record_len;
        if stripe_count > max_stripes {
            return Err(corruption(format!(
                "erasure manifest claims {stripe_count} stripes but body holds at most {max_stripes}"
            )));
        }
        let mut stripe_hashes = Vec::with_capacity(stripe_count);
        let mut stripes = Vec::with_capacity(stripe_count);
        for _ in 0..stripe_count {
            stripe_hashes.push(read_hash(body, &mut cursor)?);
            let refs = read_u16(body, &mut cursor)? as usize;
            if refs != total {
                return Err(corruption(format!(
                    "erasure manifest stripe has {refs} shard refs, expected {total}"
                )));
            }
            let mut seen = BTreeSet::new();
            let mut stripe = Vec::with_capacity(refs);
            for _ in 0..refs {
                let shard_index = read_u16(body, &mut cursor)?;
                if shard_index as usize >= total {
                    return Err(corruption(format!(
                        "erasure manifest shard index {shard_index} out of bounds for {total}"
                    )));
                }
                if !seen.insert(shard_index) {
                    return Err(corruption(format!(
                        "erasure manifest duplicates shard index {shard_index}"
                    )));
                }
                stripe.push(ShardRef {
                    shard_index,
                    shard_hash: read_hash(body, &mut cursor)?,
                });
            }
            stripe.sort_by_key(|shard| shard.shard_index);
            stripes.push(stripe);
        }
        if cursor != body.len() {
            return Err(corruption("erasure manifest has trailing bytes"));
        }

        Ok(Self {
            generation,
            blob_hash,
            blob_len,
            data_shards,
            parity_shards,
            stripe_width,
            stripe_hashes,
            stripes,
        })
    }
}

/// A publish failure, carrying whether the rollback provably reached
/// durable storage. `rollback_durable == false` means the failed put's
/// replicas may resurface after a crash (unlinks not fsynced) — the store
/// must fail-stop rather than report the put invisible.
pub(crate) struct PublishError {
    pub(crate) rollback_durable: bool,
    pub(crate) error: Error,
}

/// Publishes the manifest to EVERY drive root, preserving committed state
/// on failure:
///
/// - a drive already holding IDENTICAL bytes is skipped (idempotent
///   republish is a no-op there);
/// - every non-identical replica's preimage is read before the first write;
///   only `NotFound` means there is no prior replica;
/// - on a write failure, every path this call changed is rolled back —
///   restored to its prior bytes if one existed (committed replicas survive
///   a failed republish), removed if the call created it;
/// - after rollback the surviving byte-identical replica count decides the
///   outcome: `>= quorum` returns Ok — the manifest set is durably visible
///   (shards were written before any manifest, so the blob is complete)
///   and treating it as an error would leave an acknowledged-invisible
///   contradiction if cleanup itself failed; `< quorum` returns Err, which
///   the visibility rule keeps invisible. Err therefore always means
///   not-visible, even when rollback is interrupted or partially fails.
pub(crate) fn publish(
    manifest: &ErasureManifest,
    drive_roots: &[PathBuf],
    observer: &dyn SyncObserver,
    quorum: usize,
) -> std::result::Result<(), PublishError> {
    publish_with_reader(manifest, drive_roots, observer, quorum, |path| {
        fs::read(path)
    })
}

pub(super) fn publish_with_reader(
    manifest: &ErasureManifest,
    drive_roots: &[PathBuf],
    observer: &dyn SyncObserver,
    quorum: usize,
    mut read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>,
) -> std::result::Result<(), PublishError> {
    let bytes = manifest.encode();
    let mut pending = Vec::with_capacity(drive_roots.len());
    for root in drive_roots {
        let path = manifest_path(root, &manifest.blob_hash);
        let prior = match read(&path) {
            Ok(existing) => {
                if existing == bytes {
                    continue; // identical replica already committed here
                }
                Some(existing)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(PublishError {
                    // All preimages are collected before the first write, so
                    // this failure has no state to roll back.
                    rollback_durable: true,
                    error: Error::storage(
                        StorageErrorKind::Io,
                        format!("read erasure manifest {}: {err}", path.display()),
                    ),
                });
            }
        };
        pending.push((path, prior));
    }

    let mut changed: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::with_capacity(pending.len());
    for (path, prior) in pending {
        if let Err(err) = disk::write_replace_durable(&path, &bytes, observer) {
            // Roll back, TRACKING durability: an unlink that is not followed
            // by a successful directory fsync can un-happen after a crash.
            let mut rollback_durable = true;
            for (undo, prior) in &changed {
                match prior {
                    Some(prior_bytes) => {
                        if disk::write_replace_durable(undo, prior_bytes, observer).is_err() {
                            rollback_durable = false;
                        }
                    }
                    None => {
                        match fs::remove_file(undo) {
                            Ok(()) => {}
                            Err(remove_err)
                                if remove_err.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => rollback_durable = false,
                        }
                        match undo.parent() {
                            Some(dir) => {
                                if disk::fsync_dir(dir, observer).is_err() {
                                    rollback_durable = false;
                                }
                            }
                            None => rollback_durable = false,
                        }
                    }
                }
            }
            if count_identical_replicas(&manifest.blob_hash, drive_roots, &bytes) >= quorum {
                return Ok(());
            }
            return Err(PublishError {
                rollback_durable,
                error: Error::storage(
                    StorageErrorKind::Io,
                    format!("write erasure manifest {}: {err}", path.display()),
                ),
            });
        }
        changed.push((path, prior));
    }
    Ok(())
}

/// Counts drive roots holding a byte-identical valid replica of `bytes`.
fn count_identical_replicas(hash: &BlobHash, drive_roots: &[PathBuf], bytes: &[u8]) -> usize {
    drive_roots
        .iter()
        .filter(|root| {
            fs::read(manifest_path(root, hash))
                .map(|existing| existing == bytes)
                .unwrap_or(false)
        })
        .count()
}

pub(crate) fn load_newest(
    hash: &BlobHash,
    drive_roots: &[PathBuf],
    quorum: usize,
) -> Result<Option<ErasureManifest>> {
    // Quorum is over IDENTICAL manifest content (encoded-byte digest), not
    // just matching generation numbers: a single divergent or forged
    // checksum-valid copy must not piggyback on legitimate replicas' count
    // and become the exemplar. Committed publishes write byte-identical
    // replicas, so content-grouping costs nothing in the healthy path.
    let mut groups: Vec<(blake3::Hash, u64, usize, ErasureManifest)> = Vec::new();
    for root in drive_roots {
        let path = manifest_path(root, hash);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let manifest = match ErasureManifest::decode(&bytes) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        if manifest.blob_hash != *hash {
            // The file's NAME is the content address callers asked for; a
            // checksum-valid manifest embedding a different blob_hash is
            // misplaced/forged and must not be served under this name
            // (backup-root enumeration would otherwise substitute another
            // blob's hash for the requested one).
            continue;
        }
        let digest = blake3::hash(&bytes);
        match groups.iter_mut().find(|(existing, ..)| *existing == digest) {
            Some((_, _, count, _)) => *count += 1,
            None => groups.push((digest, manifest.generation, 1, manifest)),
        }
    }
    Ok(groups
        .into_iter()
        .filter(|(_, _, count, _)| *count >= quorum)
        // Highest generation wins; among (pathological) same-generation
        // content splits, the better-replicated group, then the smaller
        // digest — any deterministic rule suffices, publish never creates
        // divergent same-generation quorums.
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.2.cmp(&b.2))
                .then(b.0.as_bytes().cmp(a.0.as_bytes()))
        })
        .map(|(_, _, _, manifest)| manifest))
}

pub(crate) fn list_visible(drive_roots: &[PathBuf], quorum: usize) -> Result<Vec<ErasureManifest>> {
    let mut hashes = BTreeSet::new();
    for root in drive_roots {
        let dir = manifest_dir(root);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(Error::storage(
                    StorageErrorKind::Io,
                    format!("read erasure manifest dir {}: {err}", dir.display()),
                ));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|err| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!("read erasure manifest dir entry {}: {err}", dir.display()),
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(MANIFEST_EXT) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if let Ok(hash) = BlobHash::from_hex(stem) {
                hashes.insert(hash);
            }
        }
    }

    let mut manifests = Vec::new();
    for hash in hashes {
        if let Some(manifest) = load_newest(&hash, drive_roots, quorum)? {
            manifests.push(manifest);
        }
    }
    manifests.sort_by_key(|manifest| manifest.blob_hash);
    Ok(manifests)
}

pub(crate) fn manifest_dir(root: &Path) -> PathBuf {
    root.join("manifests")
}

pub(crate) fn manifest_path(root: &Path, hash: &BlobHash) -> PathBuf {
    manifest_dir(root).join(format!("{}.{}", hash.to_hex(), MANIFEST_EXT))
}

fn validate_layout(data_shards: usize, parity_shards: usize, stripe_width: usize) -> Result<()> {
    if !(2..=16).contains(&data_shards) {
        return Err(corruption(format!(
            "erasure manifest data shard count {data_shards} out of range"
        )));
    }
    if !(1..=4).contains(&parity_shards) {
        return Err(corruption(format!(
            "erasure manifest parity shard count {parity_shards} out of range"
        )));
    }
    if stripe_width == 0 || stripe_width % (data_shards * 2) != 0 {
        return Err(corruption(format!(
            "erasure manifest stripe width {stripe_width} is invalid for {data_shards} data shards"
        )));
    }
    Ok(())
}

fn read_exact<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8]> {
    if bytes.len().saturating_sub(*cursor) < len {
        return Err(corruption("erasure manifest is truncated"));
    }
    let out = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(out)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16> {
    let raw = read_exact(bytes, cursor, 2)?;
    Ok(u16::from_le_bytes(raw.try_into().expect("sliced 2 bytes")))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let raw = read_exact(bytes, cursor, 8)?;
    Ok(u64::from_le_bytes(raw.try_into().expect("sliced 8 bytes")))
}

fn read_hash(bytes: &[u8], cursor: &mut usize) -> Result<BlobHash> {
    let raw = read_exact(bytes, cursor, crate::BLAKE3_HASH_LEN)?;
    let mut hash = [0u8; crate::BLAKE3_HASH_LEN];
    hash.copy_from_slice(raw);
    Ok(BlobHash::from_bytes(hash))
}

fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}
