use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use nimbus_core::{Error, StorageErrorKind};
use tempfile::TempDir;

use super::config::ErasureConfig;
use super::heal::ErasureHealer;
use super::manifest::{self, ErasureManifest, ShardRef};
use super::store::ErasureBlobStore;
use super::stripe;
use crate::hash::BlobHash;
use crate::local::{INDEX_MAGIC, INDEX_PUT, INDEX_RELEASE, PackEntry, RECORD_MAGIC, pack_path};
use crate::store::BlobStore;

mod phase_b;

const K: usize = 4;
const M: usize = 2;
const STRIPE: usize = 64;

fn open_temp(k: usize, m: usize, stripe_width: usize) -> (TempDir, ErasureBlobStore, Vec<PathBuf>) {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let roots = (0..k + m)
        .map(|index| dir.path().join(format!("drive-{index}")))
        .collect::<Vec<_>>();
    let config = ErasureConfig::new("test-leg", roots.clone(), k, m, stripe_width).unwrap();
    let store = ErasureBlobStore::open(config).unwrap();
    (dir, store, roots)
}

fn payload(len: usize) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|index| ((index * 31 + 17) % 251) as u8)
            .collect::<Vec<_>>(),
    )
}

fn shard_ref(manifest: &ErasureManifest, stripe_index: usize, shard_index: usize) -> ShardRef {
    manifest.stripes[stripe_index]
        .iter()
        .find(|shard| shard.shard_index as usize == shard_index)
        .cloned()
        .expect("manifest should contain requested shard")
}

async fn release_shard(
    store: &ErasureBlobStore,
    manifest: &ErasureManifest,
    stripe_index: usize,
    shard_index: usize,
) {
    let shard = shard_ref(manifest, stripe_index, shard_index);
    let drive = stripe::drive_for(shard_index, stripe_index, store.drive_roots().len());
    store
        .drive_store(drive)
        .release(&shard.shard_hash)
        .await
        .unwrap();
}

fn pack_entry(root: &Path, hash: &BlobHash) -> PackEntry {
    let index_path = root.join("index.log");
    let bytes = fs::read(&index_path).unwrap();
    assert!(
        bytes.starts_with(INDEX_MAGIC),
        "index should carry expected magic"
    );
    let mut cursor = INDEX_MAGIC.len();
    let mut entries = HashMap::new();
    while cursor < bytes.len() {
        let tag = bytes[cursor];
        cursor += 1;
        let record_hash = read_hash(&bytes, &mut cursor);
        match tag {
            INDEX_PUT => {
                let pack_id = read_u64(&bytes, &mut cursor);
                let offset = read_u64(&bytes, &mut cursor);
                let len = read_u64(&bytes, &mut cursor);
                let written_at_millis = read_u64(&bytes, &mut cursor);
                entries.insert(
                    record_hash,
                    PackEntry {
                        pack_id,
                        offset,
                        len,
                        written_at_millis,
                    },
                );
            }
            INDEX_RELEASE => {
                entries.remove(&record_hash);
            }
            other => panic!("unexpected index tag {other}"),
        }
    }
    entries
        .remove(hash)
        .unwrap_or_else(|| panic!("index should contain shard {hash}"))
}

fn read_hash(bytes: &[u8], cursor: &mut usize) -> BlobHash {
    let mut hash = [0u8; crate::BLAKE3_HASH_LEN];
    hash.copy_from_slice(&bytes[*cursor..*cursor + crate::BLAKE3_HASH_LEN]);
    *cursor += crate::BLAKE3_HASH_LEN;
    BlobHash::from_bytes(hash)
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[*cursor..*cursor + 8]);
    *cursor += 8;
    u64::from_le_bytes(raw)
}

fn shard_body_offset(entry: PackEntry) -> u64 {
    entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8
}

fn flip_shard_body_byte(root: &Path, hash: &BlobHash) {
    let entry = pack_entry(root, hash);
    let path = pack_path(&root.join("packs"), entry.pack_id);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let offset = shard_body_offset(entry);
    file.seek(SeekFrom::Start(offset)).unwrap();
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 0x5a;
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&byte).unwrap();
    file.sync_data().unwrap();
}

fn shorten_shard_record(root: &Path, hash: &BlobHash) {
    let entry = pack_entry(root, hash);
    assert!(entry.len > 1, "test shard should be long enough to shorten");
    let path = pack_path(&root.join("packs"), entry.pack_id);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    let len_offset = entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64;
    file.seek(SeekFrom::Start(len_offset)).unwrap();
    file.write_all(&(entry.len - 1).to_le_bytes()).unwrap();
    file.set_len(shard_body_offset(entry) + entry.len - 1)
        .unwrap();
    file.sync_data().unwrap();
}

fn corrupt_manifest_copy(store: &ErasureBlobStore, hash: &BlobHash, drive: usize) {
    let path = manifest::manifest_path(&store.drive_root(drive), hash);
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(3).unwrap();
    file.sync_data().unwrap();
}

fn manifest_file_count(root: &Path) -> usize {
    fs::read_dir(root.join("manifests"))
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "nblm"))
        .count()
}

async fn drive_lengths(store: &ErasureBlobStore) -> Vec<usize> {
    let mut lengths = Vec::new();
    for index in 0..store.drive_roots().len() {
        lengths.push(store.drive_store(index).len().unwrap());
    }
    lengths
}

#[tokio::test]
async fn erasure_recovers_missing_data_shard() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 17);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();

    release_shard(&store, &manifest, 0, 0).await;

    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_recovers_missing_parity_shard() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 9);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();

    release_shard(&store, &manifest, 0, K).await;

    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_short_shard_read_fails_closed_then_recovers() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(51);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 0);
    let drive = stripe::drive_for(0, 0, K + M);

    shorten_shard_record(&store.drive_root(drive), &shard.shard_hash);

    let err = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_shard_checksum_mismatch_degrades() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(57);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 1);
    let drive = stripe::drive_for(1, 0, K + M);

    flip_shard_body_byte(&store.drive_root(drive), &shard.shard_hash);

    let err = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_insufficient_quorum_fails_closed() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let hash = store.put(payload(STRIPE + 11)).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();

    for shard_index in [0usize, 1, K] {
        release_shard(&store, &manifest, 0, shard_index).await;
    }

    let err = store.get(&hash).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn erasure_inconsistent_parity_source_detected() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(49);
    let hash = store.put(bytes).await.unwrap();
    let mut manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 0);
    let drive = stripe::drive_for(0, 0, K + M);
    let original = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap();
    let decoy = Bytes::from(original.iter().map(|byte| byte ^ 0xa5).collect::<Vec<_>>());
    let decoy_hash = store.drive_store(drive).put(decoy).await.unwrap();

    manifest.generation += 1;
    manifest.stripes[0]
        .iter_mut()
        .find(|candidate| candidate.shard_index == 0)
        .unwrap()
        .shard_hash = decoy_hash;
    store.publish_manifest_for_test(manifest).await.unwrap();

    let err = store.get(&hash).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn erasure_put_get_roundtrip_across_stripe_boundaries() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    for size in [
        0,
        1,
        STRIPE - 1,
        STRIPE,
        STRIPE + 1,
        17,
        127,
        STRIPE * 3 + STRIPE / 2,
    ] {
        let bytes = payload(size);
        let hash = store.put(bytes.clone()).await.unwrap();
        assert_eq!(hash, BlobHash::of(&bytes));
        assert_eq!(store.get(&hash).await.unwrap(), bytes);
    }
}

#[tokio::test]
async fn erasure_get_range_reads_only_covering_stripes() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE * 4);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    for shard_index in 0..K + M {
        release_shard(&store, &manifest, 0, shard_index).await;
    }

    let start = STRIPE as u64 * 2 + 5;
    let end = start + 17;
    assert_eq!(
        store.get_range(&hash, start..end).await.unwrap(),
        Bytes::copy_from_slice(&bytes[start as usize..end as usize])
    );

    let err = store
        .get_range(&hash, 0..(bytes.len() as u64 + 1))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    let start = 20;
    let end = 10;
    let err = store.get_range(&hash, start..end).await.unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[tokio::test]
async fn erasure_put_is_idempotent() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 3);
    let first = store.put(bytes.clone()).await.unwrap();
    let lengths = drive_lengths(&store).await;
    let manifest_counts = store
        .drive_roots()
        .iter()
        .map(|root| manifest_file_count(root))
        .collect::<Vec<_>>();

    let second = store.put(bytes).await.unwrap();

    assert_eq!(first, second);
    assert_eq!(drive_lengths(&store).await, lengths);
    assert_eq!(
        store
            .drive_roots()
            .iter()
            .map(|root| manifest_file_count(root))
            .collect::<Vec<_>>(),
        manifest_counts
    );
    assert!(manifest_counts.iter().all(|count| *count == 1));
}

#[tokio::test]
async fn erasure_release_removes_manifest_everywhere() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let hash = store.put(payload(71)).await.unwrap();
    assert!(store.has(&hash).await.unwrap());

    store.release(&hash).await.unwrap();

    assert!(!store.has(&hash).await.unwrap());
    let err = store.get(&hash).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));
    assert!(
        drive_lengths(&store).await.iter().any(|len| *len > 0),
        "release removes only manifests in Phase A; shards remain for GC"
    );
    for root in store.drive_roots() {
        assert_eq!(manifest_file_count(&root), 0);
    }
}

mod codec;
mod read_only;

#[tokio::test]
async fn erasure_config_rejects_nested_drive_roots() {
    // Review fix (EOW round 17, P1): nested drive roots nest the
    // per-tenant trees, and recursive tenant deletion on the ancestor
    // would destroy the descendant drive's data.
    let dir = tempfile::tempdir().unwrap();
    let mut roots = (0..K + M - 1)
        .map(|index| dir.path().join(format!("drive-{index}")))
        .collect::<Vec<_>>();
    roots.push(dir.path().join("drive-0").join("nested"));
    let err = ErasureConfig::new("test-leg", roots, K, M, STRIPE)
        .expect_err("nested drive roots must be rejected");
    assert!(err.to_string().contains("must not nest"), "{err}");
}
