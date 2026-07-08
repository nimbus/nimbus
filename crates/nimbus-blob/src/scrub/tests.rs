use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use nimbus_core::{StorageErrorKind, Timestamp};
use nimbus_crypto::{DataEncryptionKey, FramedBlobKey};

use super::*;
use crate::local::{self, QuarantineCheck, RECORD_MAGIC};
use crate::{
    BlobGc, BlobStore, EncryptedBlobStore, LocalPackStore, LocalPackStoreOptions, StaticBlobRoots,
};

fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let store =
        LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
    (dir, store)
}

async fn entry_for(store: &LocalPackStore, hash: BlobHash) -> PackEntry {
    store
        .blocking(move |state| Ok(state.index.get(&hash).copied().expect("hash is indexed")))
        .await
        .expect("entry lookup succeeds")
}

async fn flip_first_body_byte(
    dir: &tempfile::TempDir,
    store: &LocalPackStore,
    hash: BlobHash,
) -> PackEntry {
    let entry = entry_for(store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let body_offset = entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("pack opens for corruption");
    file.seek(SeekFrom::Start(body_offset))
        .expect("seek to body byte");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("read body byte");
    byte[0] ^= 0xff;
    file.seek(SeekFrom::Start(body_offset))
        .expect("seek back to body byte");
    file.write_all(&byte).expect("write flipped body byte");
    file.sync_data().expect("corruption lands on disk");
    entry
}

fn key(seed: &str) -> FramedBlobKey {
    FramedBlobKey::new(DataEncryptionKey::new(
        *blake3::hash(seed.as_bytes()).as_bytes(),
    ))
}

#[tokio::test]
async fn scrub_detects_flipped_byte() {
    let (dir, store) = open_temp(4096);
    let hash = store
        .put(Bytes::from_static(b"authentic scrub payload"))
        .await
        .unwrap();
    let entry = flip_first_body_byte(&dir, &store, hash).await;

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    assert!(
        report.findings.iter().any(|finding| {
            finding.kind == ScrubFindingKind::HashMismatch
                && finding.pack_id == Some(entry.pack_id)
                && finding.offset == Some(entry.offset)
                && finding.expected_hash == Some(hash)
        }),
        "hash mismatch finding should name the corrupt pack record: {report:?}"
    );
    assert_eq!(report.quarantined_hashes, vec![hash]);
    let err = store.get(&hash).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn scrub_quarantines_corrupt_record() {
    let (dir, store) = open_temp(4096);
    let bad = store.put(Bytes::from_static(b"bad record")).await.unwrap();
    let healthy = store
        .put(Bytes::from_static(b"healthy record"))
        .await
        .unwrap();
    let bad_entry = flip_first_body_byte(&dir, &store, bad).await;
    let healthy_entry = entry_for(&store, healthy).await;
    assert_eq!(
        bad_entry.pack_id, healthy_entry.pack_id,
        "test needs two records in the same pack"
    );

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    assert_eq!(report.quarantined_hashes, vec![bad]);
    let err = store.get(&bad).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    assert_eq!(
        store.get(&healthy).await.unwrap(),
        Bytes::from_static(b"healthy record"),
        "healthy record in the same pack remains readable"
    );
}

#[tokio::test]
async fn scrub_rebuilds_index_from_packs() {
    let (dir, store) = open_temp(4096);
    let keep = store.put(Bytes::from_static(b"rooted")).await.unwrap();
    let released = store
        .put(Bytes::from_static(b"released but not compacted"))
        .await
        .unwrap();
    store.release(&released).await.unwrap();
    assert!(!store.has(&released).await.unwrap());
    drop(store);

    fs::remove_file(dir.path().join("index.log")).unwrap();
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert_eq!(
        reopened.len().unwrap(),
        0,
        "missing index reopens as an empty claim set before rebuild"
    );

    let report = LocalPackScrubber::new(reopened.clone())
        .with_clock(Arc::new(nimbus_core::ManualClock::new(Timestamp(10_000))))
        .rebuild_index_from_packs()
        .await
        .unwrap();

    assert!(report.completed);
    assert!(reopened.has(&keep).await.unwrap());
    assert!(reopened.has(&released).await.unwrap());
    assert_eq!(
        reopened.get(&keep).await.unwrap(),
        Bytes::from_static(b"rooted")
    );
    assert_eq!(
        reopened.get(&released).await.unwrap(),
        Bytes::from_static(b"released but not compacted")
    );

    // Rebuild intentionally resurrects released-but-uncompacted pack bytes as
    // live claims because release tombstones live only in the index log. A GC
    // sweep with `keep` rooted re-reclaims the unrooted resurrected claim.
    let gc = BlobGc::new(
        reopened.clone(),
        StaticBlobRoots::new([keep]),
        Duration::ZERO,
    );
    let gc_report = gc.sweep().await.unwrap();
    assert_eq!(gc_report.swept, 1);
    assert!(reopened.has(&keep).await.unwrap());
    assert!(!reopened.has(&released).await.unwrap());
}

#[tokio::test]
async fn scrub_rebuilds_corrupt_index_from_packs() {
    let (dir, store) = open_temp(4096);
    let first = store.put(Bytes::from_static(b"first")).await.unwrap();
    let second = store.put(Bytes::from_static(b"second")).await.unwrap();
    drop(store);

    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.write_all(&[0u8; crate::BLAKE3_HASH_LEN]).unwrap();
    file.sync_data().unwrap();

    let err = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    let report = LocalPackScrubber::rebuild_index_in_root(
        dir.path(),
        LocalPackStoreOptions {
            pack_target_bytes: 4096,
            ..LocalPackStoreOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(report.completed);

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert_eq!(
        reopened.get(&first).await.unwrap(),
        Bytes::from_static(b"first")
    );
    assert_eq!(
        reopened.get(&second).await.unwrap(),
        Bytes::from_static(b"second")
    );
}

#[tokio::test]
async fn scrub_resumes_from_checkpoint() {
    let (_dir, store) = open_temp(72);
    let h0 = store.put(Bytes::from_static(b"first pack")).await.unwrap();
    let h1 = store.put(Bytes::from_static(b"second pack")).await.unwrap();
    let h2 = store.put(Bytes::from_static(b"third pack")).await.unwrap();
    let e0 = entry_for(&store, h0).await;
    let e1 = entry_for(&store, h1).await;
    let e2 = entry_for(&store, h2).await;
    assert!(e0.pack_id < e1.pack_id && e1.pack_id < e2.pack_id);

    let first = LocalPackScrubber::new(store.clone())
        .scrub_with_pack_limit(1)
        .await
        .unwrap();
    assert!(!first.completed);
    assert_eq!(first.packs_scanned, 1);
    assert_eq!(first.checkpoint.last_completed_pack_id, Some(e0.pack_id));

    let resumed = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(resumed.completed);
    assert_eq!(resumed.checkpoint.resumed_after_pack_id, Some(e0.pack_id));
    assert_eq!(resumed.packs_skipped_via_checkpoint, 1);
    assert_eq!(resumed.first_scanned_pack_id, Some(e1.pack_id));
}

#[tokio::test]
async fn scrub_pacing_bounds_io() {
    let (_dir, store) = open_temp(4096);
    let payload: Vec<u8> = (0..512usize).map(|i| (i % 251) as u8).collect();
    store.put(Bytes::from(payload)).await.unwrap();
    let pacing = ScrubPacing::bytes_per_tick(64).unwrap();

    let report = LocalPackScrubber::new(store)
        .with_pacing(pacing)
        .scrub()
        .await
        .unwrap();

    assert_eq!(report.pacing.bytes_per_tick_budget, Some(64));
    assert!(
        !report.pacing.bytes_per_tick.is_empty(),
        "scrub should report deterministic pacing ticks"
    );
    assert!(
        report
            .pacing
            .bytes_per_tick
            .iter()
            .all(|bytes| *bytes <= 64),
        "every tick must stay under the configured budget: {:?}",
        report.pacing.bytes_per_tick
    );
}

#[tokio::test]
async fn scrub_detects_truncated_record() {
    let (dir, store) = open_temp(4096);
    let hash = store
        .put(Bytes::from_static(b"truncate me during scrub"))
        .await
        .unwrap();
    let entry = entry_for(&store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let full_len = fs::metadata(&path).unwrap().len();
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(full_len - 2).unwrap();
    file.sync_data().unwrap();

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    assert!(
        report.findings.iter().any(|finding| {
            finding.kind == ScrubFindingKind::TruncatedRecord
                && finding.pack_id == Some(entry.pack_id)
                && finding.offset == Some(entry.offset)
        }),
        "truncated record should be reported with pack id and offset: {report:?}"
    );
    assert_eq!(report.quarantined_hashes, vec![hash]);
    let err = store.get(&hash).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn scrub_encrypted_layer_detects_aead_failure() {
    let (_dir, store) = open_temp(4096);
    let encrypted = EncryptedBlobStore::new(store.clone(), key("tenant-a"));
    let original = encrypted
        .put(Bytes::from_static(b"authenticated plaintext"))
        .await
        .unwrap();
    let mut framed = store.get(&original).await.unwrap().to_vec();
    framed[nimbus_crypto::FRAMED_HEADER_LEN] ^= 0xff;
    let tampered = store.put(Bytes::from(framed)).await.unwrap();

    let report = EncryptedBlobScrubber::new(store.clone(), key("tenant-a"))
        .scrub()
        .await
        .unwrap();

    assert!(
        report.findings.iter().any(|finding| {
            finding.kind == ScrubFindingKind::AeadOpenFailed && finding.hash == Some(tampered)
        }),
        "AEAD failure should be reported by the encrypted scrubber: {report:?}"
    );
    assert_eq!(report.quarantined_hashes, vec![tampered]);
    let err = store.get(&tampered).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    assert_eq!(
        encrypted.get(&original).await.unwrap(),
        Bytes::from_static(b"authenticated plaintext")
    );
}

#[tokio::test]
async fn scrub_quarantine_survives_compaction_without_poisoning() {
    let (dir, store) = open_temp(64 * 1024);
    let healthy = store
        .put(Bytes::from_static(b"healthy bytes"))
        .await
        .unwrap();
    let victim = store
        .put(Bytes::from_static(b"victim bytes"))
        .await
        .unwrap();
    let released = store
        .put(Bytes::from_static(b"released bytes"))
        .await
        .unwrap();
    store.release(&released).await.unwrap();

    flip_first_body_byte(&dir, &store, victim).await;
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));

    // Compaction must succeed (not poison the store), keep healthy bytes
    // readable, keep the quarantined hash refusing, and RETAIN the pack that
    // still holds the corrupt bytes (RFS6: no deletion before repair).
    let stats = store.compact().await.unwrap();
    assert!(stats.blobs_rewritten >= 1);
    assert_eq!(
        store.get(&healthy).await.unwrap(),
        Bytes::from_static(b"healthy bytes")
    );
    let err = store.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    // The store is NOT poisoned: writes still work.
    let after = store
        .put(Bytes::from_static(b"post-compact write"))
        .await
        .unwrap();
    assert_eq!(
        store.get(&after).await.unwrap(),
        Bytes::from_static(b"post-compact write")
    );
    // The quarantined entry's pack survived compaction.
    let entry = entry_for(&store, victim).await;
    assert!(
        local::pack_path(&dir.path().join("packs"), entry.pack_id).exists(),
        "the pack holding quarantined bytes is retained"
    );
}

#[tokio::test]
async fn scrub_reupload_clears_quarantine() {
    let (dir, store) = open_temp(64 * 1024);
    let victim_bytes = Bytes::from_static(b"repairable bytes");
    let victim = store.put(victim_bytes.clone()).await.unwrap();

    flip_first_body_byte(&dir, &store, victim).await;
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    assert!(
        store.get(&victim).await.is_err(),
        "quarantined read refuses"
    );

    // Content-addressed self-repair: re-uploading the exact bytes writes a
    // fresh record and lifts the quarantine.
    let again = store.put(victim_bytes.clone()).await.unwrap();
    assert_eq!(again, victim);
    assert_eq!(store.get(&victim).await.unwrap(), victim_bytes);

    // The heal is durable: a fresh open serves the blob and loads no
    // quarantine entry for it.
    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(reopened.get(&victim).await.unwrap(), victim_bytes);
    assert_eq!(reopened.open_report().unwrap().quarantine_entries_loaded, 0);
}

#[tokio::test]
async fn scrub_stale_snapshot_cannot_quarantine_relocated_blob() {
    let (_dir, store) = open_temp(64 * 1024);
    let keep = store
        .put(Bytes::from_static(b"healthy mover"))
        .await
        .unwrap();
    let junk = store.put(Bytes::from_static(b"junk")).await.unwrap();
    store.release(&junk).await.unwrap();

    // A stale scrub snapshot captured this entry...
    let stale_entry = entry_for(&store, keep).await;
    // ...then compaction legitimately rewrote the blob to a new pack.
    store.compact().await.unwrap();
    let moved_entry = entry_for(&store, keep).await;
    assert_ne!(
        (stale_entry.pack_id, stale_entry.offset),
        (moved_entry.pack_id, moved_entry.offset),
        "compaction relocated the record"
    );

    // A location-bound quarantine request from the stale snapshot must be a
    // no-op: the current record is healthy.
    let inserted = store
        .quarantine_hashes(vec![(keep, QuarantineCheck::CorruptRecord(stale_entry))])
        .await
        .unwrap();
    assert!(inserted.is_empty(), "stale finding must not quarantine");
    assert_eq!(
        store.get(&keep).await.unwrap(),
        Bytes::from_static(b"healthy mover")
    );
}

#[tokio::test]
async fn scrub_resume_rescans_growable_pack() {
    let (dir, store) = open_temp(64 * 1024);
    store
        .put(Bytes::from_static(b"first record"))
        .await
        .unwrap();

    // Simulate a scrub that crashed right after checkpointing pack 0 while
    // pack 0 was still the active (appendable) pack: last_completed == 0,
    // max_pack_seen == 0, complete == false. (The pack-limit early-return can
    // never produce this shape — it always stops BEFORE a further pack — so
    // this is a crash-window state, hand-crafted like the other crash tests.)
    let checkpoint_path = dir.path().join("scrub-checkpoint.nbls");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NBLSCP1\n");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(0u8);
    fs::write(&checkpoint_path, &bytes).unwrap();

    // The pack grows after the checkpoint, and the new record is corrupted.
    let grown = store
        .put(Bytes::from_static(b"appended later"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, grown).await;

    // Resume must NOT skip pack 0 (it was appendable when checkpointed): the
    // corruption is found and quarantined.
    let resumed = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(resumed.completed);
    assert_eq!(
        resumed.packs_skipped_via_checkpoint, 0,
        "an appendable pack is never checkpoint-skipped"
    );
    assert!(resumed.quarantined_hashes.contains(&grown));
    let err = store.get(&grown).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn released_quarantined_blob_reads_not_found() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"doomed bytes"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, victim).await;
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));

    // Releasing the claim lifts the quarantine entry with it: the hash reads
    // as absent (NotFound), not as corruption, and the lift is durable.
    store.release(&victim).await.unwrap();
    let err = store.get(&victim).await.unwrap_err();
    assert!(matches!(err, nimbus_core::Error::NotFound(_)), "{err}");

    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(reopened.open_report().unwrap().quarantine_entries_loaded, 0);
    let err = reopened.get(&victim).await.unwrap_err();
    assert!(matches!(err, nimbus_core::Error::NotFound(_)), "{err}");
}

async fn smash_record_magic(dir: &tempfile::TempDir, store: &LocalPackStore, hash: BlobHash) {
    let entry = entry_for(store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("pack opens for corruption");
    file.seek(SeekFrom::Start(entry.offset))
        .expect("seek to record magic");
    file.write_all(b"XXXX").expect("smash record magic");
    file.sync_data().expect("corruption lands on disk");
}

#[tokio::test]
async fn scrub_does_not_quarantine_healthy_records_after_corrupt_segment() {
    let (dir, store) = open_temp(64 * 1024);
    let before = store
        .put(Bytes::from_static(b"before segment"))
        .await
        .unwrap();
    let corrupt = store
        .put(Bytes::from_static(b"corrupt segment"))
        .await
        .unwrap();
    let after = store
        .put(Bytes::from_static(b"after segment"))
        .await
        .unwrap();

    // Destroy the middle record's MAGIC: the sequential scan cannot walk past
    // it, but the record after it is still readable by direct index offset.
    smash_record_magic(&dir, &store, corrupt).await;

    let report = LocalPackScrubber::new(store.clone())
        .with_pacing(ScrubPacing::bytes_per_tick(64).unwrap())
        .scrub()
        .await
        .unwrap();

    assert!(report.quarantined_hashes.contains(&corrupt));
    assert!(
        !report.quarantined_hashes.contains(&after),
        "a healthy record past the corrupt segment must not be quarantined: {report:?}"
    );
    // Direct verification honors the same pacing budget and byte accounting
    // as sequential scanning.
    assert!(
        report.pacing.bytes_per_tick.iter().all(|tick| *tick <= 64),
        "every tick (including direct verification) stays under budget: {report:?}"
    );
    assert!(
        !report.quarantined_hashes.contains(&before),
        "records before the corrupt segment are unaffected"
    );
    assert_eq!(
        store.get(&before).await.unwrap(),
        Bytes::from_static(b"before segment")
    );
    assert_eq!(
        store.get(&after).await.unwrap(),
        Bytes::from_static(b"after segment"),
        "direct-offset reads past the corrupt segment keep working"
    );
    let err = store.get(&corrupt).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    // Both healthy records were verified (one sequentially, one directly).
    assert!(report.records_verified >= 2, "{report:?}");
}

#[tokio::test]
async fn compaction_invalidates_stale_scrub_checkpoint() {
    let (dir, store) = open_temp(64 * 1024);
    let keep = store.put(Bytes::from_static(b"keep me")).await.unwrap();
    let junk = store.put(Bytes::from_static(b"junk")).await.unwrap();
    store.release(&junk).await.unwrap();

    // A completed scrub leaves a checkpoint on disk.
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    let checkpoint = dir.path().join("scrub-checkpoint.nbls");
    assert!(checkpoint.exists());

    // Compaction restructures pack ids (the empty branch even reuses id 0),
    // so it must invalidate the checkpoint — a stale one could let a resumed
    // scrub skip a REUSED pack id as "already verified".
    store.compact().await.unwrap();
    assert!(
        !checkpoint.exists(),
        "compaction removes the stale scrub checkpoint"
    );
    assert_eq!(
        store.get(&keep).await.unwrap(),
        Bytes::from_static(b"keep me")
    );
}

#[tokio::test]
async fn scrub_ignores_bytes_past_snapshot_active_length() {
    let (dir, store) = open_temp(64 * 1024);
    let hash = store
        .put(Bytes::from_static(b"settled record"))
        .await
        .unwrap();

    // Simulate an in-flight append racing the scrub: raw bytes land in the
    // active pack past the snapshot's recorded length (a torn record, were
    // the scanner to walk into it).
    let entry = entry_for(&store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(RECORD_MAGIC).unwrap();
    file.write_all(&[0u8; 7]).unwrap();
    file.sync_data().unwrap();

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.completed);
    assert!(
        report.findings.is_empty(),
        "bytes past the snapshot active length are not misreported: {report:?}"
    );
    assert!(report.quarantined_hashes.is_empty());
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"settled record")
    );
}

#[tokio::test]
async fn scrub_quarantines_records_behind_corrupt_pack_header() {
    let (dir, store) = open_temp(64 * 1024);
    let a = store.put(Bytes::from_static(b"first blob")).await.unwrap();
    let b = store.put(Bytes::from_static(b"second blob")).await.unwrap();

    // Smash the PACK header (not a record): the whole file is discredited.
    let entry = entry_for(&store, a).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"BAD").unwrap();
    file.sync_data().unwrap();

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.kind == ScrubFindingKind::InvalidPackHeader),
        "{report:?}"
    );
    for hash in [a, b] {
        assert!(
            report.quarantined_hashes.contains(&hash),
            "every record behind a corrupt pack header is quarantined: {report:?}"
        );
        let err = store.get(&hash).await.unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }
}

#[tokio::test]
async fn interrupted_checkpoint_records_snapshot_active_pack() {
    // Tiny pack target: each put rolls the active pack, giving us 3 packs
    // with pack 2 active.
    let dir = tempfile::tempdir().unwrap();
    let store = LocalPackStore::open_with_pack_target(dir.path(), 64).unwrap();
    for payload in [&b"one"[..], b"two", b"three"] {
        store.put(Bytes::copy_from_slice(payload)).await.unwrap();
    }

    let partial = LocalPackScrubber::new(store.clone())
        .scrub_with_pack_limit(1)
        .await
        .unwrap();
    assert!(!partial.completed);

    // The checkpoint's sealed-boundary field must be the ACTIVE pack id from
    // the locked snapshot (2), never a post-snapshot directory listing: only
    // packs strictly below it are checkpoint-skippable on resume.
    let bytes = fs::read(dir.path().join("scrub-checkpoint.nbls")).unwrap();
    let magic_len = b"NBLSCP1\n".len();
    let mut raw_max = [0u8; 8];
    raw_max.copy_from_slice(&bytes[magic_len + 8..magic_len + 16]);
    let active = store
        .blocking(|state| Ok(state.active_pack_id))
        .await
        .unwrap();
    assert_eq!(u64::from_le_bytes(raw_max), active);
}

#[tokio::test]
async fn quarantine_reverifies_record_before_inserting() {
    // A stale corrupt-record finding whose coordinates match a NOW-HEALTHY
    // record (the pack-id-reuse ABA: release + empty-compact + reupload can
    // reproduce identical entry coordinates) must not quarantine: the check
    // is ground-truth re-verification under the lock, not entry equality.
    let (_dir, store) = open_temp(64 * 1024);
    let hash = store
        .put(Bytes::from_static(b"healthy again"))
        .await
        .unwrap();
    let entry = entry_for(&store, hash).await;

    let inserted = store
        .quarantine_hashes(vec![(hash, QuarantineCheck::CorruptRecord(entry))])
        .await
        .unwrap();
    assert!(
        inserted.is_empty(),
        "a record that verifies right now is never quarantined"
    );
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"healthy again")
    );
}

#[tokio::test]
async fn rebuild_preserves_healthy_records_after_corrupt_segment() {
    let (dir, store) = open_temp(64 * 1024);
    let before = store
        .put(Bytes::from_static(b"before segment"))
        .await
        .unwrap();
    let corrupt = store
        .put(Bytes::from_static(b"corrupt segment"))
        .await
        .unwrap();
    let after = store
        .put(Bytes::from_static(b"after segment"))
        .await
        .unwrap();

    // Structural corruption in the middle stops the sequential rebuild scan.
    smash_record_magic(&dir, &store, corrupt).await;

    LocalPackScrubber::new(store.clone())
        .rebuild_index_from_packs()
        .await
        .unwrap();

    // Healthy records on BOTH sides of the corrupt segment survive the
    // rebuild (the later one via direct verification of its known offset);
    // publishing only the scanned prefix would have made it NotFound and
    // eventually deletable.
    assert_eq!(
        store.get(&before).await.unwrap(),
        Bytes::from_static(b"before segment")
    );
    assert_eq!(
        store.get(&after).await.unwrap(),
        Bytes::from_static(b"after segment")
    );
    // The structurally corrupt record itself is dropped from the index.
    assert!(matches!(
        store.get(&corrupt).await.unwrap_err(),
        nimbus_core::Error::NotFound(_)
    ));
}

#[tokio::test]
async fn corrupt_index_rebuild_salvages_prefix_offsets() {
    let (dir, store) = open_temp(64 * 1024);
    let before = store
        .put(Bytes::from_static(b"before segment"))
        .await
        .unwrap();
    let corrupt = store
        .put(Bytes::from_static(b"corrupt segment"))
        .await
        .unwrap();
    let after = store
        .put(Bytes::from_static(b"after segment"))
        .await
        .unwrap();
    drop(store);

    // Structural corruption in the pack stops the sequential rebuild scan...
    let index_path = dir.path().join("index.log");
    {
        let store = LocalPackStore::open(dir.path()).unwrap();
        smash_record_magic(&dir, &store, corrupt).await;
        drop(store);
    }
    // ...and the index log ALSO goes corrupt (unknown tag appended), so a
    // normal open refuses and rebuild_index_in_root's repair path runs.
    let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.write_all(&[0u8; crate::BLAKE3_HASH_LEN]).unwrap();
    file.sync_data().unwrap();
    assert!(LocalPackStore::open(dir.path()).is_err(), "open refuses");

    let report =
        LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
            .await
            .unwrap();
    assert!(report.completed);

    // The salvaged index prefix carried the offset of the record PAST the
    // corrupt pack segment; direct verification preserved it. Only the
    // structurally corrupt record itself is gone.
    let store = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(
        store.get(&before).await.unwrap(),
        Bytes::from_static(b"before segment")
    );
    assert_eq!(
        store.get(&after).await.unwrap(),
        Bytes::from_static(b"after segment")
    );
    assert!(matches!(
        store.get(&corrupt).await.unwrap_err(),
        nimbus_core::Error::NotFound(_)
    ));
}

#[tokio::test]
async fn corrupt_index_rebuild_retains_quarantined_claim() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"claimed corrupt"))
        .await
        .unwrap();
    let healthy = store
        .put(Bytes::from_static(b"healthy sibling"))
        .await
        .unwrap();

    // Corrupt the victim's body and quarantine it via scrub.
    flip_first_body_byte(&dir, &store, victim).await;
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    drop(store);

    // Corrupt the index at its FIRST record: the salvageable prefix is empty,
    // so the quarantined claim's entry can only be recovered from the pack
    // scan's corrupt-record coordinates.
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().write(true).open(&index_path).unwrap();
    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.sync_data().unwrap();
    assert!(LocalPackStore::open(dir.path()).is_err(), "open refuses");

    LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
        .await
        .unwrap();

    // The quarantined claim survived the repair: still indexed (claim
    // tracked, pack retained), still failing closed on read.
    let store = LocalPackStore::open(dir.path()).unwrap();
    assert!(store.has(&victim).await.unwrap(), "claim survives repair");
    let err = store.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    assert_eq!(
        store.get(&healthy).await.unwrap(),
        Bytes::from_static(b"healthy sibling")
    );
    // And compaction still retains its pack instead of deleting the bytes.
    store.compact().await.unwrap();
    assert!(store.has(&victim).await.unwrap());
}

#[tokio::test]
async fn checkpoint_publication_refused_after_compaction_epoch_moves() {
    let (dir, store) = open_temp(64 * 1024);
    let keep = store.put(Bytes::from_static(b"keep")).await.unwrap();
    let junk = store.put(Bytes::from_static(b"junk")).await.unwrap();
    store.release(&junk).await.unwrap();

    let scrubber = LocalPackScrubber::new(store.clone());
    let snapshot_epoch = store
        .blocking(|state| Ok(state.compaction_epoch))
        .await
        .unwrap();

    // A compaction lands between the scrub snapshot and its checkpoint write
    // (and invalidates any on-disk checkpoint as it restructures packs).
    store.compact().await.unwrap();

    // Publishing a checkpoint derived from the dead layout is refused...
    let stale = ScrubCheckpoint {
        last_completed_pack_id: Some(0),
        max_pack_seen: Some(0),
        complete: false,
    };
    let wrote = scrubber
        .write_checkpoint(stale, snapshot_epoch)
        .await
        .unwrap();
    assert!(!wrote, "stale-layout checkpoint publication is refused");
    assert!(!dir.path().join("scrub-checkpoint.nbls").exists());

    // ...while a checkpoint carrying the CURRENT epoch lands normally.
    let current_epoch = store
        .blocking(|state| Ok(state.compaction_epoch))
        .await
        .unwrap();
    let wrote = scrubber
        .write_checkpoint(stale, current_epoch)
        .await
        .unwrap();
    assert!(wrote);
    assert!(dir.path().join("scrub-checkpoint.nbls").exists());
    assert!(store.has(&keep).await.unwrap());
}

#[tokio::test]
async fn resume_rescans_packs_with_findings() {
    // Tiny pack target: each put rolls the active pack -> packs 0,1,2.
    let dir = tempfile::tempdir().unwrap();
    let store = LocalPackStore::open_with_pack_target(dir.path(), 64).unwrap();
    for payload in [&b"pack zero"[..], b"pack one", b"pack two"] {
        store.put(Bytes::copy_from_slice(payload)).await.unwrap();
    }

    // Orphan structural corruption in sealed pack 1: a finding with no
    // durable quarantine entry (the indexed record itself stays valid).
    let path = local::pack_path(&dir.path().join("packs"), 1);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"XXXXGARBAGE").unwrap();
    file.sync_data().unwrap();

    // Interrupted run scans packs 0 (clean) and 1 (dirty): the resume
    // checkpoint must freeze at pack 0 — findings for unindexed corrupt
    // bytes are not durable anywhere else.
    let scrubber = LocalPackScrubber::new(store.clone());
    let partial = scrubber.scrub_with_pack_limit(2).await.unwrap();
    assert!(!partial.completed);
    assert_eq!(partial.checkpoint.last_completed_pack_id, Some(0));

    // The resumed run rescans pack 1 and re-surfaces the finding in its own
    // (completed) report instead of silently omitting it.
    let resumed = scrubber.scrub().await.unwrap();
    assert!(resumed.completed);
    assert_eq!(
        resumed.packs_skipped_via_checkpoint, 1,
        "only clean pack 0 skips"
    );
    assert!(
        resumed.findings.iter().any(|finding| {
            finding.kind == ScrubFindingKind::InvalidRecordMagic && finding.pack_id == Some(1)
        }),
        "the dirty pack's finding re-surfaces on resume: {resumed:?}"
    );
}

#[tokio::test]
async fn repeat_scrub_reports_previously_quarantined() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"persistent corrupt"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, victim).await;

    // First run quarantines.
    let first = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(first.quarantined_hashes.contains(&victim));

    // Every subsequent run keeps the live quarantined claim operator-visible
    // instead of reporting a clean store while a blob is unreadable.
    let second = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(
        second.previously_quarantined.contains(&victim),
        "persistent corruption stays visible: {second:?}"
    );

    // The encrypted-layer scrub composes the local pass, so it inherits the
    // same persistent visibility.
    let encrypted = EncryptedBlobScrubber::new(store.clone(), key("tenant-visibility"))
        .scrub()
        .await
        .unwrap();
    assert!(encrypted.previously_quarantined.contains(&victim));
}

#[tokio::test]
async fn raw_reupload_does_not_clear_aead_quarantine() {
    let (_dir, store) = open_temp(4096);
    let encrypted = EncryptedBlobStore::new(store.clone(), key("tenant-a"));
    let original = encrypted
        .put(Bytes::from_static(b"authenticated plaintext"))
        .await
        .unwrap();
    let mut framed = store.get(&original).await.unwrap().to_vec();
    framed[nimbus_crypto::FRAMED_HEADER_LEN] ^= 0xff;
    let tampered_bytes = Bytes::from(framed);
    let tampered = store.put(tampered_bytes.clone()).await.unwrap();

    let report = EncryptedBlobScrubber::new(store.clone(), key("tenant-a"))
        .scrub()
        .await
        .unwrap();
    assert!(report.quarantined_hashes.contains(&tampered));

    // A raw local re-upload of the IDENTICAL bytes reproduces the identical
    // AEAD failure — it must NOT lift a content-level quarantine the way a
    // re-upload lifts a record-level one.
    let again = store.put(tampered_bytes).await.unwrap();
    assert_eq!(again, tampered);
    let err = store.get(&tampered).await.unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(StorageErrorKind::Corruption),
        "AEAD quarantine survives a raw re-upload of the same bad bytes"
    );
}

#[tokio::test]
async fn rebuild_invalidates_stale_scrub_checkpoint() {
    let (dir, store) = open_temp(64 * 1024);
    store.put(Bytes::from_static(b"some record")).await.unwrap();

    // A stale incomplete checkpoint from an interrupted scrub...
    let checkpoint_path = dir.path().join("scrub-checkpoint.nbls");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NBLSCP1\n");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.push(0u8);
    fs::write(&checkpoint_path, &bytes).unwrap();

    // ...must not survive an index rebuild: its evidence describes the
    // pre-rebuild index/scan state.
    LocalPackScrubber::new(store.clone())
        .rebuild_index_from_packs()
        .await
        .unwrap();
    assert!(
        !checkpoint_path.exists(),
        "rebuild durably invalidates the resume checkpoint"
    );
}

#[tokio::test]
async fn reupload_after_pack_header_corruption_heals_into_fresh_pack() {
    let (dir, store) = open_temp(64 * 1024);
    let payload = Bytes::from_static(b"headline victim");
    let victim = store.put(payload.clone()).await.unwrap();
    let old_entry = entry_for(&store, victim).await;

    // Smash the ACTIVE pack's header and quarantine via scrub.
    let path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));

    // The heal must land in a FRESH validated pack, never behind the
    // discredited header.
    let again = store.put(payload.clone()).await.unwrap();
    assert_eq!(again, victim);
    let new_entry = entry_for(&store, victim).await;
    assert_ne!(
        new_entry.pack_id, old_entry.pack_id,
        "heal rolls to a fresh pack: {new_entry:?} vs {old_entry:?}"
    );
    assert_eq!(store.get(&victim).await.unwrap(), payload);

    // And the store survives reopen: the corrupt pack is no longer the
    // active pack, so open's active-header validation does not brick.
    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(reopened.get(&victim).await.unwrap(), payload);
}

#[tokio::test]
async fn corrupt_index_repair_recovers_claim_when_hash_field_corrupted() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"true content"))
        .await
        .unwrap();
    let entry = entry_for(&store, victim).await;

    // Corrupt the record's HASH FIELD (the body still hashes to `victim`).
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(entry.offset + RECORD_MAGIC.len() as u64))
            .unwrap();
        file.write_all(&[0xEEu8; 8]).unwrap();
        file.sync_data().unwrap();
    }
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    drop(store);

    // Corrupt the index at its first record: no salvageable prefix, so the
    // quarantined claim is only recoverable via the corrupt-record evidence,
    // which must be findable by the blob's TRUE (body) hash.
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().write(true).open(&index_path).unwrap();
    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.sync_data().unwrap();

    LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
        .await
        .unwrap();

    let store = LocalPackStore::open(dir.path()).unwrap();
    assert!(
        store.has(&victim).await.unwrap(),
        "the quarantined claim survives repair via its body hash"
    );
    let err = store.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}
