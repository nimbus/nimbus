use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use nimbus_core::{Error, Result, StorageErrorKind};

use crate::disk::{self, SyncObserver};
use crate::hash::BlobHash;

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

pub(crate) fn publish(
    manifest: &ErasureManifest,
    drive_roots: &[PathBuf],
    observer: &dyn SyncObserver,
) -> Result<()> {
    let bytes = manifest.encode();
    for root in drive_roots {
        let path = manifest_path(root, &manifest.blob_hash);
        disk::write_replace_durable(&path, &bytes, observer).map_err(|err| {
            Error::storage(
                StorageErrorKind::Io,
                format!("write erasure manifest {}: {err}", path.display()),
            )
        })?;
    }
    Ok(())
}

pub(crate) fn load_newest(
    hash: &BlobHash,
    drive_roots: &[PathBuf],
) -> Result<Option<ErasureManifest>> {
    let mut newest: Option<ErasureManifest> = None;
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
        if newest
            .as_ref()
            .map(|current| manifest.generation > current.generation)
            .unwrap_or(true)
        {
            newest = Some(manifest);
        }
    }
    Ok(newest)
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
