//! Split from scrub/tests.rs per the modularity thresholds. Helpers live
//! in the parent `tests` module; `use super::*` re-imports them.

use super::*;

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
        .with_clock(Arc::new(nimbus_core::ManualWallClock::new(Timestamp(
            10_000,
        ))))
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
    // The structurally corrupt record is RETAINED as a quarantined claim
    // (fail-closed read), never silently dropped to NotFound.
    assert!(store.has(&corrupt).await.unwrap());
    let err = store.get(&corrupt).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
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
    // The corrupt record's claim is RETAINED (salvaged prefix supplies the
    // entry; direct verification fails -> quarantined, fail-closed).
    assert!(store.has(&corrupt).await.unwrap());
    let err = store.get(&corrupt).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
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
async fn rebuild_invalidates_stale_scrub_checkpoint() {
    let (dir, store) = open_temp(64 * 1024);
    store.put(Bytes::from_static(b"some record")).await.unwrap();

    // A stale incomplete checkpoint from an interrupted scrub...
    let checkpoint_path = dir.path().join("scrub-checkpoint.nbls");
    fs::write(&checkpoint_path, craft_checkpoint(0, 0, false)).unwrap();

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

#[tokio::test]
async fn corrupt_index_repair_retains_truncated_quarantined_claim() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store.put(Bytes::from(vec![7u8; 2048])).await.unwrap();
    let entry = entry_for(&store, victim).await;

    // Truncate the pack so the victim's body extends past EOF; scrub
    // quarantines it (TruncatedRecord with fully known coordinates).
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(entry.offset + 4 + 32 + 8 + 100).unwrap();
        file.sync_data().unwrap();
    }
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    drop(store);

    // Corrupt the index at its first record: no salvageable prefix. The
    // claim must survive via the truncated record's recorded coordinates.
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
        "truncated quarantined claim survives repair"
    );
    assert!(store.get(&victim).await.is_err(), "still fails closed");
}

#[tokio::test]
async fn rebuild_retains_live_claim_behind_corrupt_pack_header() {
    let (dir, store) = open_temp(64 * 1024);
    let a = store.put(Bytes::from_static(b"claim a")).await.unwrap();
    let b = store.put(Bytes::from_static(b"claim b")).await.unwrap();
    drop(store);

    // Corrupt pack 0's header (both claims live behind it), then corrupt the
    // index so rebuild_index_in_root runs.
    {
        let store = LocalPackStore::open(dir.path()).unwrap();
        let entry = entry_for(&store, a).await;
        let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();
        drop(store);
    }
    // Corrupt the index by APPENDING an unknown-tag record: the parseable
    // prefix (both a and b entries) still salvages, but a normal open
    // refuses, so rebuild_index_in_root runs. The salvaged entries point at
    // the header-corrupt pack — they must be retained + quarantined, not
    // skipped.
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.write_all(&[0u8; crate::BLAKE3_HASH_LEN]).unwrap();
    file.sync_data().unwrap();
    assert!(LocalPackStore::open(dir.path()).is_err(), "open refuses");

    LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
        .await
        .unwrap();

    // Both live claims survive repair as quarantined (fail-closed), never
    // dropped to NotFound where compaction could delete the bytes.
    let store = LocalPackStore::open(dir.path()).unwrap();
    for hash in [a, b] {
        assert!(store.has(&hash).await.unwrap(), "claim retained: {hash}");
        let err = store.get(&hash).await.unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }
}

#[tokio::test]
async fn corrupt_index_repair_fails_closed_on_unrecoverable_claim() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"doomed but claimed"))
        .await
        .unwrap();
    let entry = entry_for(&store, victim).await;
    // Corrupt the body so scrub quarantines it (a locatable record).
    flip_first_body_byte(&dir, &store, victim).await;
    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.quarantined_hashes.contains(&victim));
    drop(store);

    // Now DESTROY the record's framing (magic) so the pack scan can no longer
    // walk it, AND corrupt the index at its first record so nothing salvages.
    // The quarantined claim becomes genuinely unrecoverable.
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(entry.offset)).unwrap();
        file.write_all(b"XXXX").unwrap();
        file.sync_data().unwrap();
    }
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().write(true).open(&index_path).unwrap();
    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.sync_data().unwrap();

    // Repair must FAIL CLOSED rather than publish an index that drops the
    // claim (which reopen would prune and compaction could then erase).
    let err =
        LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
            .await
            .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));
    assert!(
        err.to_string().contains("unrecoverable"),
        "the error names the unrecoverable claim: {err}"
    );
}

#[tokio::test]
async fn failed_missing_index_rebuild_leaves_no_provisional_index() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"unrecoverable claim"))
        .await
        .unwrap();
    let entry = entry_for(&store, victim).await;
    flip_first_body_byte(&dir, &store, victim).await;
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(store.get(&victim).await.is_err(), "quarantined");
    drop(store);

    // Destroy the record framing so the claim is unrecoverable, AND remove
    // the index entirely (the missing-index path).
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(entry.offset)).unwrap();
        file.write_all(b"XXXX").unwrap();
        file.sync_data().unwrap();
    }
    fs::remove_file(dir.path().join("index.log")).unwrap();

    // The rebuild must FAIL CLOSED and leave NO provisional empty index
    // behind — otherwise the next open would treat it as authoritative and
    // prune the quarantine claim.
    let err =
        LocalPackScrubber::rebuild_index_in_root(dir.path(), LocalPackStoreOptions::default())
            .await
            .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));
    assert!(
        !dir.path().join("index.log").exists(),
        "no provisional empty index is left after a failed rebuild"
    );

    // The quarantine side file still names the claim (evidence preserved).
    let quarantine = fs::read(dir.path().join("quarantine.nblq")).unwrap();
    assert!(
        quarantine.len() > b"NBLQ2\n".len(),
        "quarantine evidence retained"
    );
}

#[tokio::test]
async fn failed_open_then_rebuild_removes_provisional_index() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store.put(Bytes::from_static(b"claimed")).await.unwrap();
    let entry = entry_for(&store, victim).await;
    flip_first_body_byte(&dir, &store, victim).await;
    LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    drop(store);

    // Destroy the record framing (unrecoverable) and remove the index.
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(entry.offset)).unwrap();
        file.write_all(b"XXXX").unwrap();
        file.sync_data().unwrap();
    }
    fs::remove_file(dir.path().join("index.log")).unwrap();

    // The PUBLIC open-then-rebuild workflow: open (creates provisional empty
    // index) then rebuild_index_from_packs, which fails closed. The
    // provisional index must be removed so a later open still sees the index
    // as missing (needs repair) rather than pruning the quarantine claim.
    let store = LocalPackStore::open(dir.path()).unwrap();
    let err = LocalPackScrubber::new(store.clone())
        .rebuild_index_from_packs()
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));
    drop(store);
    assert!(
        !dir.path().join("index.log").exists(),
        "the provisional empty index is removed on fail-closed rebuild"
    );
    let quarantine = fs::read(dir.path().join("quarantine.nblq")).unwrap();
    assert!(
        quarantine.len() > b"NBLQ2\n".len(),
        "quarantine evidence retained"
    );
}
