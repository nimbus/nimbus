use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use nimbus_core::{Error, StorageErrorKind};
use tempfile::TempDir;

use super::config::ErasureConfig;
use super::manifest::{self, ErasureManifest, ShardRef};
use super::store::ErasureBlobStore;
use super::stripe;
use crate::hash::BlobHash;
use crate::local::{INDEX_MAGIC, INDEX_PUT, INDEX_RELEASE, PackEntry, RECORD_MAGIC, pack_path};
use crate::store::BlobStore;

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

#[tokio::test]
async fn erasure_drive_identity_refuses_swapped_roots() {
    let (_dir, _store, roots) = open_temp(K, M, STRIPE);
    let mut swapped = roots.clone();
    swapped.swap(0, 1);

    let err =
        ErasureBlobStore::open(ErasureConfig::new("test-leg", swapped, K, M, STRIPE).unwrap())
            .unwrap_err();

    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn erasure_manifest_torn_write_ignored() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 7);
    let hash = store.put(bytes.clone()).await.unwrap();

    corrupt_manifest_copy(&store, &hash, 0);

    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_crash_before_manifest_publish_is_invisible() {
    let (dir, store, roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 5);
    let hash = BlobHash::of(&bytes);
    for (stripe_index, chunk) in bytes.chunks(STRIPE).enumerate() {
        let shards = stripe::encode_stripe(chunk, K, M).unwrap();
        for (shard_index, shard) in shards.into_iter().enumerate() {
            let drive = stripe::drive_for(shard_index, stripe_index, K + M);
            store.drive_store(drive).put(shard).await.unwrap();
        }
    }
    drop(store);

    let reopened =
        ErasureBlobStore::open(ErasureConfig::new("test-leg", roots, K, M, STRIPE).unwrap())
            .unwrap();

    assert!(!reopened.has(&hash).await.unwrap());
    let err = reopened.get(&hash).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));
    drop(reopened);
    drop(dir);
}

#[tokio::test]
async fn erasure_random_loss_within_parity_always_roundtrips() {
    let mut rng = TestRng::new(0x9e37_79b9_7f4a_7c15);
    for (k, m) in [(2usize, 1usize), (4, 2), (8, 3), (12, 4)] {
        let stripe_width = k * 16;
        for _case in 0..6 {
            let (_dir, store, _roots) = open_temp(k, m, stripe_width);
            let stripe_count = 1 + rng.usize(4);
            let max = stripe_width * stripe_count;
            let min = stripe_width * (stripe_count - 1) + 1;
            let len = min + rng.usize(max - min + 1);
            let mut bytes = vec![0u8; len];
            rng.fill(&mut bytes);
            let bytes = Bytes::from(bytes);
            let hash = store.put(bytes.clone()).await.unwrap();
            let manifest = store.load_manifest_for_test(&hash).await.unwrap();
            let occurrences = shard_occurrences(&store, &manifest);

            for stripe_index in 0..manifest.stripes.len() {
                let mut candidates = (0..k + m).collect::<Vec<_>>();
                rng.shuffle(&mut candidates);
                let loss_target = rng.usize(m + 1);
                let mut removed = 0usize;
                for shard_index in candidates {
                    if removed == loss_target {
                        break;
                    }
                    let shard = shard_ref(&manifest, stripe_index, shard_index);
                    let drive = stripe::drive_for(shard_index, stripe_index, k + m);
                    if occurrences
                        .get(&(drive, shard.shard_hash))
                        .copied()
                        .unwrap_or(0)
                        != 1
                    {
                        continue;
                    }
                    store
                        .drive_store(drive)
                        .release(&shard.shard_hash)
                        .await
                        .unwrap();
                    removed += 1;
                }
                assert!(
                    removed <= m,
                    "test helper must never remove more than parity shards"
                );
            }

            assert_eq!(store.get(&hash).await.unwrap(), bytes);
        }
    }
}

fn shard_occurrences(
    store: &ErasureBlobStore,
    manifest: &ErasureManifest,
) -> HashMap<(usize, BlobHash), usize> {
    let mut occurrences = HashMap::new();
    for (stripe_index, stripe) in manifest.stripes.iter().enumerate() {
        for shard in stripe {
            let drive = stripe::drive_for(
                shard.shard_index as usize,
                stripe_index,
                store.drive_roots().len(),
            );
            *occurrences.entry((drive, shard.shard_hash)).or_insert(0) += 1;
        }
    }
    occurrences
}

struct TestRng {
    state: u64,
}

impl TestRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn usize(&mut self, upper_exclusive: usize) -> usize {
        assert!(upper_exclusive > 0);
        (self.next_u64() as usize) % upper_exclusive
    }

    fn fill(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            *byte = self.next_u64() as u8;
        }
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            let swap = self.usize(index + 1);
            values.swap(index, swap);
        }
    }
}

#[tokio::test]
async fn erasure_get_range_detects_wrong_shard_manifest() {
    // Review fix (round 1, P1): a manifest whose shard ref drifted to a
    // wrong-but-valid shard must fail RANGE reads closed too — the
    // per-stripe payload hash catches what the whole-blob hash (full-get
    // only) used to be the sole guard for.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(3 * STRIPE);
    let hash = store.put(bytes.clone()).await.unwrap();
    let mut manifest = store.load_manifest_for_test(&hash).await.unwrap();

    // Decoy shard: same length, valid content address, wrong bytes.
    let shard = shard_ref(&manifest, 1, 0);
    let drive = stripe::drive_for(0, 1, K + M);
    let original = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap();
    let decoy = Bytes::from(original.iter().map(|byte| byte ^ 0x5a).collect::<Vec<_>>());
    let decoy_hash = store.drive_store(drive).put(decoy).await.unwrap();
    manifest.generation += 1;
    manifest.stripes[1]
        .iter_mut()
        .find(|candidate| candidate.shard_index == 0)
        .unwrap()
        .shard_hash = decoy_hash;
    store.publish_manifest_for_test(manifest).await.unwrap();

    // A range confined to the poisoned stripe fails closed...
    let err = store
        .get_range(&hash, (STRIPE as u64)..(STRIPE as u64 + 8))
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    // ...while a range over healthy stripes still serves exact bytes.
    let healthy = store.get_range(&hash, 0..(STRIPE as u64)).await.unwrap();
    assert_eq!(healthy, bytes.slice(0..STRIPE));
}

#[tokio::test]
async fn erasure_put_repairs_partially_replicated_manifest() {
    // Review fix (round 1, P2): the idempotent put path must re-publish the
    // manifest to every drive, healing crash-mid-publish / partial-release
    // windows instead of blessing a single surviving copy.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 3);
    let hash = store.put(bytes.clone()).await.unwrap();

    // Simulate a partial publish: strip the manifest below quorum (m+1=3),
    // leaving copies on drives 0 and 1 only.
    for index in 2..(K + M) {
        let path = manifest::manifest_path(&store.drive_root(index), &hash);
        fs::remove_file(&path).unwrap();
    }
    assert!(
        !store.has(&hash).await.unwrap(),
        "a below-quorum manifest minority must be invisible (round-2 P1)"
    );

    let again = store.put(bytes.clone()).await.unwrap();
    assert_eq!(again, hash);
    for index in 0..(K + M) {
        let path = manifest::manifest_path(&store.drive_root(index), &hash);
        assert!(
            path.exists(),
            "idempotent put must repair the manifest replica on drive {index}"
        );
    }
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_foreign_leg_root_refused() {
    // Review fix (round 1, P2): identity binds the LEG INSTANCE, not just
    // the drive index — a root provisioned for another leg refuses to open
    // even at the same index.
    let dir = tempfile::tempdir().unwrap();
    let roots = (0..K + M)
        .map(|index| dir.path().join(format!("drive-{index}")))
        .collect::<Vec<_>>();
    let first =
        ErasureBlobStore::open(ErasureConfig::new("leg-a", roots.clone(), K, M, STRIPE).unwrap())
            .unwrap();
    drop(first);

    let err = ErasureBlobStore::open(ErasureConfig::new("leg-b", roots, K, M, STRIPE).unwrap())
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn erasure_manifest_huge_stripe_count_rejected() {
    // Review fix (round 1, P2): a checksum-valid manifest claiming a huge
    // stripe count must fail structurally (bounded by body size) instead of
    // panicking or reserving enormous capacity.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE);
    let hash = store.put(bytes).await.unwrap();

    // Hand-encode a manifest frame: magic + body + BLAKE3 checksum, where
    // blob_len/stripe_count are astronomically large but the body is tiny.
    let huge_stripes: u64 = u64::MAX / (STRIPE as u64); // consistent with blob_len below
    let mut body = Vec::new();
    body.extend_from_slice(&2u64.to_le_bytes()); // generation
    body.extend_from_slice(hash.as_bytes()); // blob_hash
    body.extend_from_slice(&u64::MAX.to_le_bytes()); // blob_len (huge)
    body.extend_from_slice(&(K as u16).to_le_bytes());
    body.extend_from_slice(&(M as u16).to_le_bytes());
    body.extend_from_slice(&(STRIPE as u64).to_le_bytes()); // stripe_width
    body.extend_from_slice(&huge_stripes.to_le_bytes()); // stripe_count (huge)
    let checksum = blake3::hash(&body);
    let mut frame = Vec::new();
    frame.extend_from_slice(b"NBLE1");
    frame.extend_from_slice(&body);
    frame.extend_from_slice(checksum.as_bytes());

    let err = ErasureManifest::decode(&frame).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    // The store keeps serving the healthy generation-1 manifest even if the
    // crafted frame lands on a drive.
    let path = manifest::manifest_path(&store.drive_root(0), &hash);
    fs::write(&path, &frame).unwrap();
    assert!(store.has(&hash).await.unwrap());
}

#[tokio::test]
async fn erasure_under_quorum_manifest_is_invisible() {
    // Review fix (round 2, P1): visibility requires parity+1 valid manifest
    // replicas, so the minority an interrupted put can leave behind is
    // never observable as committed — while a committed blob tolerates
    // k-1 manifest losses (more than the m shard losses data tolerates).
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 11);
    let hash = store.put(bytes.clone()).await.unwrap();

    // k+m = 6 copies; quorum = m+1 = 3. Removing 3 leaves exactly quorum.
    for index in 0..M + 1 {
        let path = manifest::manifest_path(&store.drive_root(index), &hash);
        fs::remove_file(&path).unwrap();
    }
    assert!(
        store.has(&hash).await.unwrap(),
        "quorum copies stay visible"
    );
    assert_eq!(store.get(&hash).await.unwrap(), bytes);

    // One more removal drops below quorum: invisible, fail-closed.
    let path = manifest::manifest_path(&store.drive_root(M + 1), &hash);
    fs::remove_file(&path).unwrap();
    assert!(!store.has(&hash).await.unwrap());
    assert!(matches!(
        store.get(&hash).await.unwrap_err(),
        Error::NotFound(_)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn erasure_failed_publish_leaves_put_invisible() {
    // Review fix (round 2, P1): a put whose manifest publish fails partway
    // returns Err AND stays invisible — publish undoes the copies it
    // already wrote, and the quorum rule bounds any cleanup remnant.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(2 * STRIPE + 9);
    let hash = BlobHash::of(&bytes);

    // Make the LAST drive's manifests dir unwritable so every earlier
    // replica writes before the failure.
    let last = store.drive_root(K + M - 1);
    let dir = manifest::manifest_dir(&last);
    let mut perms = fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&dir, perms).unwrap();

    let err = store.put(bytes.clone()).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Io));
    assert!(
        !store.has(&hash).await.unwrap(),
        "an errored put must not be observable as committed"
    );
    for index in 0..(K + M - 1) {
        let path = manifest::manifest_path(&store.drive_root(index), &hash);
        assert!(!path.exists(), "publish cleanup removed drive {index} copy");
    }

    // Restore and retry: the same put commits cleanly.
    let mut perms = fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).unwrap();
    assert_eq!(store.put(bytes.clone()).await.unwrap(), hash);
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_quorum_requires_identical_manifest_content() {
    // Review fix (round 3, P1): quorum groups by encoded-manifest content —
    // a single divergent same-generation copy neither piggybacks on the
    // legitimate replicas' count nor becomes the exemplar.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(2 * STRIPE);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();

    // Divergent same-generation manifest: point stripe 0 shard 0 at a decoy
    // shard and fix up the stripe hash so the copy is fully self-consistent.
    let shard = shard_ref(&manifest, 0, 0);
    let drive = stripe::drive_for(0, 0, K + M);
    let original = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap();
    let decoy = Bytes::from(original.iter().map(|byte| byte ^ 0x3c).collect::<Vec<_>>());
    let decoy_hash = store.drive_store(drive).put(decoy.clone()).await.unwrap();
    let mut forged = manifest.clone();
    forged.stripes[0]
        .iter_mut()
        .find(|candidate| candidate.shard_index == 0)
        .unwrap()
        .shard_hash = decoy_hash;
    // Recompute the forged stripe-0 payload hash over the decoy layout.
    let mut poisoned_shards = Vec::new();
    for index in 0..K {
        let reference = forged.stripes[0]
            .iter()
            .find(|candidate| candidate.shard_index as usize == index)
            .unwrap();
        let drive = stripe::drive_for(index, 0, K + M);
        poisoned_shards.push(
            store
                .drive_store(drive)
                .get(&reference.shard_hash)
                .await
                .unwrap(),
        );
    }
    let poisoned_stripe = stripe::reassemble_stripe(&poisoned_shards, STRIPE).unwrap();
    forged.stripe_hashes[0] = BlobHash::of(&poisoned_stripe);

    // Plant the forged copy on drive 0 (overwriting the legitimate replica).
    let path = manifest::manifest_path(&store.drive_root(0), &hash);
    fs::write(&path, forged.encode()).unwrap();

    // 5 legitimate identical copies (>= quorum 3) vs 1 forged: reads serve
    // the TRUE bytes, including ranges inside the poisoned stripe.
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
    assert_eq!(
        store.get_range(&hash, 4..12).await.unwrap(),
        bytes.slice(4..12)
    );

    // Strip legitimate copies down to 2 (below quorum 3): the forged
    // single never becomes visible, the blob goes invisible instead of
    // serving forged bytes.
    for index in 1..=M + 1 {
        let path = manifest::manifest_path(&store.drive_root(index), &hash);
        fs::remove_file(&path).unwrap();
    }
    assert!(!store.has(&hash).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn erasure_failed_republish_preserves_committed_replicas() {
    // Review fix (round 3, P1): a FAILED idempotent republish must not roll
    // back committed replicas — the blob stays fully visible and readable.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 21);
    let hash = store.put(bytes.clone()).await.unwrap();

    // Strip ONE replica so the idempotent path has real work, then make the
    // last drive's manifests dir unwritable so the republish fails partway.
    fs::remove_file(manifest::manifest_path(&store.drive_root(0), &hash)).unwrap();
    let last_dir = manifest::manifest_dir(&store.drive_root(K + M - 1));
    let mut perms = fs::metadata(&last_dir).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&last_dir, perms).unwrap();
    // Unwritable dir only blocks CREATING the temp file; the last drive
    // already holds an identical replica, so publish skips it. Force real
    // failure: also strip that replica... which needs the dir writable.
    let mut perms = fs::metadata(&last_dir).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&last_dir, perms).unwrap();
    fs::remove_file(manifest::manifest_path(&store.drive_root(K + M - 1), &hash)).unwrap();
    let mut perms = fs::metadata(&last_dir).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&last_dir, perms).unwrap();

    let err = store.put(bytes.clone()).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Io));

    // The four untouched committed replicas survived the rollback (drive 0's
    // freshly created copy was removed again): still >= quorum, readable.
    assert!(
        store.has(&hash).await.unwrap(),
        "failed republish must not drop a committed blob below quorum"
    );
    assert_eq!(store.get(&hash).await.unwrap(), bytes);

    // Restore and repair fully.
    let mut perms = fs::metadata(&last_dir).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&last_dir, perms).unwrap();
    assert_eq!(store.put(bytes.clone()).await.unwrap(), hash);
    for index in 0..(K + M) {
        assert!(
            manifest::manifest_path(&store.drive_root(index), &hash).exists(),
            "repair restored replica on drive {index}"
        );
    }
}

#[tokio::test]
async fn erasure_high_parity_manifests_survive_parity_drive_losses() {
    // Review fix (round 5, P2): with k=2,m=4 the quorum caps at k=2 so the
    // manifest plane tolerates the SAME m=4 drive losses the data does —
    // parity+1 would have demanded 5/6 manifests and lost visibility after
    // only two failures while shards were still recoverable.
    let (_dir, store, _roots) = open_temp(2, 4, STRIPE);
    let bytes = payload(STRIPE + 13);
    let hash = store.put(bytes.clone()).await.unwrap();

    // Lose m=4 drives' manifests (and their shards, via quarantine-free
    // removal of the manifest copies — shard loss is covered elsewhere):
    // 2 replicas remain = quorum, blob stays visible and readable.
    for index in 0..4 {
        fs::remove_file(manifest::manifest_path(&store.drive_root(index), &hash)).unwrap();
    }
    assert!(store.has(&hash).await.unwrap());
    assert_eq!(store.get(&hash).await.unwrap(), bytes);

    // One more loss drops below quorum: fail-closed invisibility.
    fs::remove_file(manifest::manifest_path(&store.drive_root(4), &hash)).unwrap();
    assert!(!store.has(&hash).await.unwrap());
}
