//! Split from scrub/tests.rs per the modularity thresholds. Helpers live
//! in the parent `tests` module; `use super::*` re-imports them.

use super::*;

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
    fs::write(&checkpoint_path, craft_checkpoint(0, 0, false)).unwrap();

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
async fn corrupt_checkpoint_is_ignored_and_full_scan_runs() {
    let (dir, store) = open_temp(64 * 1024);
    let victim = store
        .put(Bytes::from_static(b"must be scanned"))
        .await
        .unwrap();
    flip_first_body_byte(&dir, &store, victim).await;

    // A syntactically plausible checkpoint whose counters claim everything is
    // verified — with a bogus integrity trailer. Trusting it would skip the
    // whole root; it must be ignored (fail-safe full scan).
    let checkpoint_path = dir.path().join("scrub-checkpoint.nbls");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NBLSCP1\n");
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    bytes.push(0u8);
    bytes.extend_from_slice(&[0u8; 32]);
    fs::write(&checkpoint_path, &bytes).unwrap();

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
    assert!(report.completed);
    assert_eq!(report.packs_skipped_via_checkpoint, 0, "nothing skipped");
    assert!(
        report.quarantined_hashes.contains(&victim),
        "the corruption is found despite the damaged checkpoint: {report:?}"
    );
}

#[tokio::test]
async fn quarantine_revalidation_streams_large_corrupt_record() {
    // A large record with a single flipped byte: revalidation must not
    // materialize the whole blob, and the streamed bytes are accounted.
    let (dir, store) = open_temp(16 * 1024 * 1024);
    let big: Vec<u8> = (0..4_000_000usize).map(|i| (i % 251) as u8).collect();
    let victim = store.put(Bytes::from(big.clone())).await.unwrap();
    flip_first_body_byte(&dir, &store, victim).await;

    let report = LocalPackScrubber::new(store.clone())
        .with_pacing(ScrubPacing::bytes_per_tick(64 * 1024).unwrap())
        .scrub()
        .await
        .unwrap();

    assert!(report.quarantined_hashes.contains(&victim));
    // Every tick — sequential scan AND streamed revalidation — stays under
    // the configured budget (proves neither path buffered the whole blob).
    assert!(
        report.pacing.bytes_per_tick.iter().all(|t| *t <= 64 * 1024),
        "no tick exceeds budget: {:?}",
        report.pacing.bytes_per_tick
    );
    let err = store.get(&victim).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn scrub_does_not_falsely_report_records_swallowed_by_bogus_length() {
    let (dir, store) = open_temp(64 * 1024);
    let first = store
        .put(Bytes::from_static(b"first record here"))
        .await
        .unwrap();
    let healthy = store
        .put(Bytes::from_static(b"second healthy record"))
        .await
        .unwrap();
    let first_entry = entry_for(&store, first).await;
    let healthy_entry = entry_for(&store, healthy).await;
    assert_eq!(first_entry.pack_id, healthy_entry.pack_id);

    // Inflate the FIRST record's on-disk length field (still within EOF) so a
    // sequential scan steps OVER the healthy second record. The healthy
    // record is still readable by its own index offset.
    let path = local::pack_path(&dir.path().join("packs"), first_entry.pack_id);
    let len_offset = first_entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64;
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(len_offset)).unwrap();
        // A larger-but-in-bounds length.
        file.write_all(&(first_entry.len + 20).to_le_bytes())
            .unwrap();
        file.sync_data().unwrap();
    }

    let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();

    // The first record is genuinely corrupt (its length no longer matches).
    assert!(report.quarantined_hashes.contains(&first));
    // The healthy record swallowed by the bogus length must NOT be reported
    // as corrupt or quarantined — direct verification proves it good.
    assert!(
        !report.quarantined_hashes.contains(&healthy),
        "healthy swallowed record must not be quarantined: {report:?}"
    );
    assert!(
        !report.findings.iter().any(|f| f.hash == Some(healthy)),
        "no false finding for the healthy swallowed record: {report:?}"
    );
    assert_eq!(
        store.get(&healthy).await.unwrap(),
        Bytes::from_static(b"second healthy record")
    );
}
