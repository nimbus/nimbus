//! Split from scrub/tests.rs per the modularity thresholds. Helpers live
//! in the parent `tests` module; `use super::*` re-imports them.

use super::*;

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
    assert!(
        inserted.inserted.is_empty(),
        "stale finding must not quarantine"
    );
    assert_eq!(
        store.get(&keep).await.unwrap(),
        Bytes::from_static(b"healthy mover")
    );
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
        inserted.inserted.is_empty(),
        "a record that verifies right now is never quarantined"
    );
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"healthy again")
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
async fn scrub_of_corrupt_active_header_retires_pack() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"behind bad header"))
        .await
        .unwrap();
    let old_entry = entry_for(&store, victim).await;

    // Smash the ACTIVE pack's header; scrub quarantines and must RETIRE the
    // pack (roll to a fresh validated active pack) in the same operation.
    let path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));

    // Unrelated NEW writes land in the fresh pack, never behind the
    // discredited header.
    let fresh = store
        .put(Bytes::from_static(b"unrelated write"))
        .await
        .unwrap();
    let fresh_entry = entry_for(&store, fresh).await;
    assert_ne!(fresh_entry.pack_id, old_entry.pack_id);
    assert_eq!(
        store.get(&fresh).await.unwrap(),
        Bytes::from_static(b"unrelated write")
    );

    // And reopen selects the fresh pack as active instead of refusing the
    // corrupt one (no restart brick).
    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.get(&fresh).await.unwrap(),
        Bytes::from_static(b"unrelated write")
    );
    let err = reopened.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn unconditional_quarantine_skips_released_hash() {
    let (dir, store) = open_temp(64 * 1024);
    let hash = store
        .put(Bytes::from_static(b"released racer"))
        .await
        .unwrap();
    store.release(&hash).await.unwrap();

    // A content-level finding whose claim was released before insertion must
    // not land: a stale side-file entry would poison a future
    // reintroduction of the same content hash.
    let inserted = store
        .quarantine_hashes(vec![(hash, QuarantineCheck::Unconditional)])
        .await
        .unwrap();
    assert!(inserted.inserted.is_empty());

    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(reopened.open_report().unwrap().quarantine_entries_loaded, 0);
    // Reintroducing the content works normally.
    let again = reopened
        .put(Bytes::from_static(b"released racer"))
        .await
        .unwrap();
    assert_eq!(again, hash);
    assert_eq!(
        reopened.get(&again).await.unwrap(),
        Bytes::from_static(b"released racer")
    );
}

#[tokio::test]
async fn open_retires_corrupt_pack_referenced_only_by_quarantined_claims() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"only quarantined"))
        .await
        .unwrap();
    let old_entry = entry_for(&store, victim).await;

    // Corrupt the active pack header; scrub quarantines + retires (creates
    // a fresh pack). Simulate a crash BEFORE the retirement roll landed by
    // deleting the fresh empty pack, leaving the corrupt pack as disk-max,
    // referenced only by the quarantined claim.
    let corrupt_path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&corrupt_path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    drop(store);
    let fresh_path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id + 1);
    if fresh_path.exists() {
        fs::remove_file(&fresh_path).unwrap();
    }

    // Reopen must not brick: the corrupt disk-max pack is referenced only by
    // a quarantined claim (which reads fail-closed regardless), so open
    // rolls past it.
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert!(reopened.has(&victim).await.unwrap(), "claim retained");
    let err = reopened.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    let fresh = reopened
        .put(Bytes::from_static(b"post-crash write"))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(&fresh).await.unwrap(),
        Bytes::from_static(b"post-crash write")
    );
}

#[tokio::test]
async fn repeat_scrub_of_corrupt_header_still_retires_active_pack() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"stuck behind header"))
        .await
        .unwrap();
    let old_entry = entry_for(&store, victim).await;

    let path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    // Simulate losing the retirement roll (crash shape): delete the fresh
    // pack and force the active id back onto the corrupt pack.
    let fresh_path = local::pack_path(&dir.path().join("packs"), old_entry.pack_id + 1);
    if fresh_path.exists() {
        fs::remove_file(&fresh_path).unwrap();
    }
    store
        .blocking(move |mut state| {
            state.active_pack_id = old_entry.pack_id;
            Ok(())
        })
        .await
        .unwrap();

    // A REPEAT scrub re-confirms the same (already-quarantined) hashes. Even
    // with zero new insertions it must retire the corrupt active pack.
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    let active = store
        .blocking(|state| Ok(state.active_pack_id))
        .await
        .unwrap();
    assert_ne!(
        active, old_entry.pack_id,
        "repeat scrub retires the corrupt active pack even with no new quarantines"
    );
    let fresh = store.put(Bytes::from_static(b"fresh write")).await.unwrap();
    let fresh_entry = entry_for(&store, fresh).await;
    assert_ne!(fresh_entry.pack_id, old_entry.pack_id);
}

#[tokio::test]
async fn quarantine_revalidation_bytes_are_accounted() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store.put(Bytes::from(vec![3u8; 2000])).await.unwrap();
    let entry = entry_for(&store, victim).await;
    flip_first_body_byte(&dir, &store, victim).await;

    let pack_len = fs::metadata(local::pack_path(&dir.path().join("packs"), entry.pack_id))
        .unwrap()
        .len();

    let report = LocalPackScrubber::new(store.clone())
        .with_pacing(ScrubPacing::bytes_per_tick(64).unwrap())
        .scrub()
        .await
        .unwrap();

    assert!(report.quarantined_hashes.contains(&victim));
    // The ground-truth revalidation re-read is part of the accounting
    // contract: total scanned bytes exceed the pure sequential scan.
    assert!(
        report.bytes_scanned > pack_len,
        "revalidation I/O is accounted: scanned {} vs pack {}",
        report.bytes_scanned,
        pack_len
    );
}

#[tokio::test]
async fn scrub_retires_empty_corrupt_active_pack() {
    let (dir, store) = open_temp(64 * 1024);
    // Active pack 0 exists but has NO indexed records; corrupt its header.
    let path = local::pack_path(&dir.path().join("packs"), 0);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }

    // Scrub reports the finding AND retires the active pack even though there
    // are zero hashes to quarantine.
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    let active = store
        .blocking(|state| Ok(state.active_pack_id))
        .await
        .unwrap();
    assert_ne!(active, 0, "corrupt empty active pack is retired");

    // New puts land in the fresh pack and survive reopen.
    let hash = store
        .put(Bytes::from_static(b"after retire"))
        .await
        .unwrap();
    let entry = entry_for(&store, hash).await;
    assert_ne!(entry.pack_id, 0);
    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.get(&hash).await.unwrap(),
        Bytes::from_static(b"after retire")
    );
}

#[tokio::test]
async fn scrub_retires_corrupt_active_pack_despite_release_race() {
    let (dir, store) = open_temp(64 * 1024);
    // The active pack has an indexed record at scrub-snapshot time...
    let victim = store
        .put(Bytes::from_static(b"released mid-scrub"))
        .await
        .unwrap();
    let entry = entry_for(&store, victim).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
    }

    // ...but the claim is released before the scrub's quarantine pass. The
    // CorruptPackHeader request is then skipped (index no longer maps it),
    // so no quarantine is inserted — yet the corrupt active pack must STILL
    // be retired, or a later put would append behind the bad header.
    store.release(&victim).await.unwrap();

    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    let active = store
        .blocking(|state| Ok(state.active_pack_id))
        .await
        .unwrap();
    assert_ne!(
        active, entry.pack_id,
        "corrupt active pack retires even when the release race skipped every quarantine insert"
    );

    let fresh = store.put(Bytes::from_static(b"safe write")).await.unwrap();
    let fresh_entry = entry_for(&store, fresh).await;
    assert_ne!(fresh_entry.pack_id, entry.pack_id);
    drop(store);
    let reopened = LocalPackStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.get(&fresh).await.unwrap(),
        Bytes::from_static(b"safe write")
    );
}

#[tokio::test]
async fn record_finding_does_not_downgrade_content_quarantine() {
    let (dir, store) = open_temp(64 * 1024);
    let encrypted = EncryptedBlobStore::new(store.clone(), key("tenant-a"));
    let original = encrypted
        .put(Bytes::from_static(b"authenticated"))
        .await
        .unwrap();
    let mut framed = store.get(&original).await.unwrap().to_vec();
    framed[nimbus_crypto::FRAMED_HEADER_LEN] ^= 0xff;
    let tampered_bytes = Bytes::from(framed);
    let tampered = store.put(tampered_bytes.clone()).await.unwrap();

    // Content-quarantine it via the encrypted scrubber.
    EncryptedBlobScrubber::new(store.clone(), key("tenant-a"))
        .scrub()
        .await
        .unwrap();
    assert!(store.get(&tampered).await.is_err());

    // A later local (no-key) scrub in a batch that also inserts a NEW hash
    // must not downgrade the Content reason to Record. Force a co-inserted
    // record finding by corrupting a second blob.
    let other = store
        .put(Bytes::from_static(b"second victim"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, other).await;
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    // The Content quarantine must STILL survive an identical-byte re-upload.
    let again = store.put(tampered_bytes).await.unwrap();
    assert_eq!(again, tampered);
    let err = store.get(&tampered).await.unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(StorageErrorKind::Corruption),
        "content quarantine was not downgraded to repairable record"
    );
}

#[tokio::test]
async fn missing_index_open_does_not_prune_quarantine_before_rebuild() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"claimed corrupt"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, victim).await;
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(store.get(&victim).await.is_err(), "quarantined");
    drop(store);

    // The index log is lost entirely (e.g. a partial-restore). Opening for
    // rebuild must NOT prune the quarantine before rebuild can carry it.
    fs::remove_file(dir.path().join("index.log")).unwrap();

    let report =
        LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
            .await
            .unwrap();
    assert!(report.completed);

    // The corrupt blob is still claim-tracked and fail-closed, not silently
    // turned into NotFound + compaction-reclaimable.
    let store = LocalPackStore::open(dir.path()).unwrap();
    assert!(
        store.has(&victim).await.unwrap(),
        "claim survived index loss"
    );
    let err = store.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn compaction_refused_while_quarantine_orphaned_after_index_loss() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"claim only in side file"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, victim).await;
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(store.get(&victim).await.is_err());
    drop(store);

    // Lose the index entirely: the quarantine claim now lives only in the
    // side file, its bytes only in the pack.
    fs::remove_file(dir.path().join("index.log")).unwrap();

    // Open (provisional empty index, no prune). Compaction MUST refuse rather
    // than delete the pack holding the orphaned quarantine claim's bytes.
    let store = LocalPackStore::open(dir.path()).unwrap();
    let err = store.compact().await.unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(StorageErrorKind::Busy),
        "a precondition refusal (not a disk fault) must not poison the store"
    );
    // The refusal did not poison the store: rebuild proceeds on the same
    // handle, and the pack survived so the claim is recoverable.
    let report = LocalPackScrubber::new(store.clone())
        .rebuild_index_from_packs()
        .await
        .unwrap();
    assert!(report.completed);
    assert!(
        store.has(&victim).await.unwrap(),
        "claim recovered by rebuild"
    );
}
