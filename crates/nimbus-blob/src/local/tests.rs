//! Behavior, durability-ordering, crash-window, and root-ownership tests
//! for [`LocalPackStore`]. Split out of `local.rs` per the modularity
//! thresholds (files >= 1,500 lines need an owning-plan exception).

use super::*;
use crate::disk::SyncEvent;
use crate::disk::recorder::RecordingSyncObserver;
use crate::root_guard::FORMAT_FILE;

fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let store =
        LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
    (dir, store)
}

#[tokio::test]
async fn put_reopen_get_round_trips() {
    let (dir, store) = open_temp(256);
    let hash = store
        .put(Bytes::from_static(b"durable payload"))
        .await
        .unwrap();
    drop(store);

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert_eq!(
        reopened.get(&hash).await.unwrap(),
        Bytes::from_static(b"durable payload")
    );
}

#[tokio::test]
async fn put_is_idempotent_and_indexes_once() {
    let (_dir, store) = open_temp(256);
    let first = store.put(Bytes::from_static(b"same")).await.unwrap();
    let second = store.put(Bytes::from_static(b"same")).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(store.len().unwrap(), 1);
    assert_eq!(store.live_entries().unwrap().len(), 1);
}

#[tokio::test]
async fn get_range_slices_verified_bytes() {
    let (_dir, store) = open_temp(256);
    let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
    assert_eq!(
        store.get_range(&hash, 4..8).await.unwrap(),
        Bytes::from_static(b"4567")
    );
}

#[tokio::test]
async fn local_pack_store_range_read_transfers_only_inner_bytes_served() {
    let (_dir, store) = open_temp(64 * 1024 * 1024);
    let big: Vec<u8> = (0..1_048_576usize).map(|i| (i % 251) as u8).collect();
    let hash = store.put(Bytes::from(big.clone())).await.unwrap();
    // `put` only writes; drain any bytes the write path itself may have
    // touched so the counter below reflects only the `get_range` call.
    store.take_body_bytes_read().await.unwrap();

    let slice = store.get_range(&hash, 4096..8192).await.unwrap();

    assert_eq!(slice, Bytes::copy_from_slice(&big[4096..8192]));
    let body_bytes = store.take_body_bytes_read().await.unwrap();
    assert_eq!(
        body_bytes, 4096,
        "range read should transfer exactly the requested body window, not the whole 1MiB blob"
    );
}

#[tokio::test]
async fn get_range_rejects_end_past_blob_length() {
    let (_dir, store) = open_temp(256);
    let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
    let err = store.get_range(&hash, 4..100).await.unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[tokio::test]
#[allow(clippy::reversed_empty_ranges)]
async fn get_range_rejects_start_after_end() {
    let (_dir, store) = open_temp(256);
    let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
    let err = store.get_range(&hash, 8..4).await.unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[tokio::test]
async fn release_removes_index_entry_without_deleting_other_blobs() {
    let (_dir, store) = open_temp(128);
    let keep = store.put(Bytes::from_static(b"keep")).await.unwrap();
    let drop_hash = store.put(Bytes::from_static(b"drop")).await.unwrap();

    store.release(&drop_hash).await.unwrap();

    assert!(!store.has(&drop_hash).await.unwrap());
    assert_eq!(store.get(&keep).await.unwrap(), Bytes::from_static(b"keep"));
}

#[tokio::test]
async fn open_retires_unreferenced_corrupt_header_pack() {
    // No live claim references pack 0; its corrupt header retires at open
    // (rolled past, reported) instead of bricking the root over a file no
    // data depends on.
    let (dir, store) = open_temp(256);
    drop(store);

    let path = pack_path(&dir.path().join("packs"), 0);
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"BAD").unwrap();
    file.sync_data().unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert_eq!(
        reopened
            .open_report()
            .unwrap()
            .unreferenced_corrupt_packs_retired,
        1
    );
    let hash = reopened
        .put(Bytes::from_static(b"fresh start"))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(&hash).await.unwrap(),
        Bytes::from_static(b"fresh start")
    );
}

#[tokio::test]
async fn open_fails_closed_on_referenced_corrupt_header_pack() {
    // A corrupt header on a pack that LIVE claims reference still fails
    // closed at open; scrub owns its quarantine + retirement.
    let (dir, store) = open_temp(256);
    store.put(Bytes::from_static(b"referenced")).await.unwrap();
    drop(store);

    let path = pack_path(&dir.path().join("packs"), 0);
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"BAD").unwrap();
    file.sync_data().unwrap();

    let err = match LocalPackStore::open_with_pack_target(dir.path(), 256) {
        Ok(_) => panic!("a referenced corrupt pack header must fail closed"),
        Err(err) => err,
    };
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn compact_rewrites_live_blobs_and_removes_dead_packs() {
    let (dir, store) = open_temp(96);
    let keep = store
        .put(Bytes::from_static(b"keep this payload"))
        .await
        .unwrap();
    let drop_hash = store
        .put(Bytes::from_static(b"drop this payload"))
        .await
        .unwrap();
    store.release(&drop_hash).await.unwrap();

    let stats = store.compact().await.unwrap();

    assert_eq!(stats.blobs_rewritten, 1);
    assert!(stats.packs_removed >= 1);
    assert_eq!(
        store.get(&keep).await.unwrap(),
        Bytes::from_static(b"keep this payload")
    );
    assert!(!store.has(&drop_hash).await.unwrap());
    let pack_count = fs::read_dir(dir.path().join("packs"))
        .unwrap()
        .filter_map(std::result::Result::ok)
        .count();
    assert_eq!(pack_count, 1, "dead packs should be removed");
}

#[tokio::test]
async fn put_stream_and_get_stream_round_trip() {
    let (_dir, store) = open_temp(256);
    let src: ByteStream = Box::new(std::io::Cursor::new(Bytes::from_static(b"streamed")));
    let hash = store.put_stream(src).await.unwrap();

    let mut reader = store.get_stream(&hash).await.unwrap();
    let mut out = Vec::new();
    reader.read_to_end(&mut out).await.unwrap();
    assert_eq!(out, b"streamed");
}

// ---- RFS2: root ownership and format guard ----

#[tokio::test]
async fn local_pack_second_open_shares_live_state() {
    let (dir, store) = open_temp(256);
    let hash = store.put(Bytes::from_static(b"shared")).await.unwrap();

    // A second same-process writable open aliases the SAME live state:
    // no Busy, immediate visibility, one flock, one writer mutex.
    let second = LocalPackStore::open_with_pack_target(dir.path(), 256)
        .expect("same-process open shares the live root state");
    assert_eq!(
        second.get(&hash).await.unwrap(),
        Bytes::from_static(b"shared")
    );
    let via_second = second.put(Bytes::from_static(b"both ways")).await.unwrap();
    assert!(store.has(&via_second).await.unwrap());

    // Dropping every handle releases the state; a fresh open re-reads disk.
    drop(store);
    drop(second);
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert!(reopened.has(&hash).await.unwrap());
}

#[tokio::test]
async fn root_lock_excludes_second_process() {
    use fs2::FileExt;

    let (dir, store) = open_temp(256);

    // Probe the flock the way another process would: a separate file
    // description on root/lock. flock conflicts across descriptions, so
    // this is exactly the cross-process exclusion contract.
    let lock_path = dir.path().canonicalize().unwrap().join("lock");
    let probe = OpenOptions::new().write(true).open(&lock_path).unwrap();
    assert!(
        probe.try_lock_exclusive().is_err(),
        "a live store holds the exclusive root flock"
    );

    drop(store);
    probe
        .try_lock_exclusive()
        .expect("dropping the last handle releases the flock");
    fs2::FileExt::unlock(&probe).unwrap();
}

#[test]
fn local_pack_format_marker_roundtrip() {
    let (dir, store) = open_temp(256);
    drop(store);

    assert!(
        dir.path().join(FORMAT_FILE).exists(),
        "open stamps a marker"
    );
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert_eq!(reopened.open_report().unwrap(), OpenReport::default());
}

#[test]
fn local_pack_rejects_foreign_or_future_marker() {
    let (dir, store) = open_temp(256);
    drop(store);
    let marker_path = dir.path().join(FORMAT_FILE);
    let valid = fs::read(&marker_path).unwrap();

    // Foreign marker: not ours at all.
    fs::write(&marker_path, b"someone else's format file").unwrap();
    let err = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    // Future-versioned marker: fail closed instead of guessing.
    let mut future = valid.clone();
    future[8..12].copy_from_slice(&99u32.to_le_bytes());
    fs::write(&marker_path, &future).unwrap();
    let err = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    // The valid marker still opens.
    fs::write(&marker_path, &valid).unwrap();
    LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
}

#[test]
fn local_pack_startup_cleanup_removes_stale_temp() {
    let (dir, store) = open_temp(256);
    drop(store);

    let root_temp = dir.path().join(format!("{}stale", disk::TMP_PREFIX));
    let packs_temp = dir
        .path()
        .join("packs")
        .join(format!("{}stale", disk::TMP_PREFIX));
    fs::write(&root_temp, b"crash leftover").unwrap();
    fs::write(&packs_temp, b"crash leftover").unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert!(!root_temp.exists(), "root temp removed");
    assert!(!packs_temp.exists(), "packs temp removed");
    assert_eq!(
        reopened.open_report().unwrap().stale_temp_files_removed,
        2,
        "cleanup is reported, not silent"
    );
}

#[cfg(unix)]
#[test]
fn local_pack_rejects_symlinked_root() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir_all(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let err = LocalPackStore::open(&link).unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[tokio::test]
async fn local_pack_read_only_serves_reads_and_rejects_writes() {
    let (dir, owner) = open_temp(256);
    let hash = owner.put(Bytes::from_static(b"inspect me")).await.unwrap();

    // Coexists with the live writable owner: no lock conflict.
    let inspector = LocalPackStore::open_read_only(dir.path()).unwrap();
    assert_eq!(
        inspector.get(&hash).await.unwrap(),
        Bytes::from_static(b"inspect me")
    );
    assert_eq!(inspector.len().unwrap(), 1);

    for err in [
        inspector
            .put(Bytes::from_static(b"nope"))
            .await
            .unwrap_err(),
        inspector.release(&hash).await.unwrap_err(),
        inspector.compact().await.unwrap_err(),
    ] {
        assert_eq!(
            err.storage_kind(),
            Some(StorageErrorKind::Busy),
            "read-only handle refuses mutations"
        );
    }

    // The owner is unaffected.
    assert_eq!(
        owner.get(&hash).await.unwrap(),
        Bytes::from_static(b"inspect me")
    );
}

#[tokio::test]
async fn shared_open_still_refuses_foreign_identity() {
    let dir = tempfile::tempdir().unwrap();
    let bound = LocalPackStoreOptions {
        pack_target_bytes: 256,
        identity: Some([7u8; 32]),
        ..LocalPackStoreOptions::default()
    };
    let _owner = LocalPackStore::open_with_options(dir.path(), bound).unwrap();

    // Root is live and bound to identity 7; a same-process open claiming a
    // different identity must NOT silently alias it.
    let foreign = LocalPackStoreOptions {
        pack_target_bytes: 256,
        identity: Some([8u8; 32]),
        ..LocalPackStoreOptions::default()
    };
    let err = LocalPackStore::open_with_options(dir.path(), foreign).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn read_only_put_stream_refuses_before_consuming_input() {
    let (dir, _owner) = open_temp(256);
    let inspector = LocalPackStore::open_read_only(dir.path()).unwrap();

    // A reader that panics if polled proves the gate fires before the
    // stream is consumed.
    struct Unpollable;
    impl tokio::io::AsyncRead for Unpollable {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            panic!("read-only put_stream must refuse before reading input");
        }
    }
    let err = inspector
        .put_stream(Box::new(Unpollable))
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));
}

#[tokio::test]
async fn crash_index_unknown_tag_torn_at_eof_still_fails_closed() {
    let (dir, store) = open_temp(4096);
    store.put(Bytes::from_static(b"fine")).await.unwrap();
    drop(store);

    // Unknown tag followed by only a partial hash: EOF-torn, but the tag
    // itself is garbage — corruption, never a healable torn tail.
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.write_all(&[0u8; 10]).unwrap();
    file.sync_data().unwrap();

    let err = match LocalPackStore::open_with_pack_target(dir.path(), 4096) {
        Ok(_) => panic!("unknown tag torn at EOF must fail closed"),
        Err(err) => err,
    };
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn read_only_refuses_unowned_data_bearing_root() {
    // Data without a marker: unowned/foreign — inspection refuses.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packs")).unwrap();
    let err = LocalPackStore::open_read_only(dir.path()).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    // An empty root inspects as an empty store.
    let empty = tempfile::tempdir().unwrap();
    let inspector = LocalPackStore::open_read_only(empty.path()).unwrap();
    assert_eq!(inspector.len().unwrap(), 0);
}

#[tokio::test]
async fn writable_open_refuses_unowned_data_bearing_root() {
    // Pack data without a marker: unowned/foreign — a writable open must not
    // silently adopt (and identity-bind) it.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packs")).unwrap();
    let err = LocalPackStore::open(dir.path()).unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[test]
fn fresh_root_creation_fsyncs_new_directory_entries() {
    use crate::disk::recorder::RecordingSyncObserver;

    // Two missing levels: parent/child. Every created level's parent must be
    // fsynced so the new entries survive power loss.
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("object-blobs").join("tenant-x");
    let recorder = RecordingSyncObserver::new();
    crate::disk::create_dir_all_durable(&root, &recorder).unwrap();

    assert!(root.is_dir());
    let events = recorder.events();
    let parent_synced = events
        .iter()
        .any(|e| matches!(e, SyncEvent::DirSync(p) if p == &base.path().join("object-blobs")));
    let grandparent_synced = events
        .iter()
        .any(|e| matches!(e, SyncEvent::DirSync(p) if p == base.path()));
    assert!(
        parent_synced && grandparent_synced,
        "each created level's parent is fsynced: {events:?}"
    );
}

// ---- RFS3: durable commit-point writes and crash windows ----

#[tokio::test]
async fn durable_write_fsync_order() {
    let (dir, store) = open_temp(4096);
    let recorder = Arc::new(RecordingSyncObserver::new());
    store.set_sync_observer(recorder.clone());

    store
        .put(Bytes::from_static(b"ordered durability"))
        .await
        .unwrap();

    // The store canonicalizes its root (macOS /var -> /private/var), so
    // compare against canonical paths.
    let canonical = dir.path().canonicalize().unwrap();
    let packs_dir = canonical.join("packs");
    let index_path = canonical.join("index.log");
    let pack_sync = recorder
        .index_where(|e| matches!(e, SyncEvent::FileSync(path) if path.starts_with(&packs_dir)));
    let index_sync =
        recorder.index_where(|e| matches!(e, SyncEvent::FileSync(path) if path == &index_path));
    assert!(
        pack_sync < index_sync,
        "pack bytes must be durable before the index record is published: {:?}",
        recorder.events()
    );
}

#[tokio::test]
async fn crash_bytes_written_index_missing() {
    let (dir, store) = open_temp(4096);
    let visible = store
        .put(Bytes::from_static(b"acknowledged"))
        .await
        .unwrap();
    drop(store);

    // Simulate the crash window: pack record fully written and synced,
    // index record never published.
    let orphan_body = b"never acknowledged";
    let orphan_hash = BlobHash::of(orphan_body);
    let path = pack_path(&dir.path().join("packs"), 0);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(RECORD_MAGIC).unwrap();
    file.write_all(orphan_hash.as_bytes()).unwrap();
    file.write_all(&(orphan_body.len() as u64).to_le_bytes())
        .unwrap();
    file.write_all(orphan_body).unwrap();
    file.sync_data().unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert!(
        !reopened.has(&orphan_hash).await.unwrap(),
        "an unindexed orphan record is invisible"
    );
    assert_eq!(
        reopened.get(&visible).await.unwrap(),
        Bytes::from_static(b"acknowledged")
    );
    // The store keeps working; a fresh put lands after the orphan bytes.
    let next = reopened
        .put(Bytes::from_static(b"after crash"))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(&next).await.unwrap(),
        Bytes::from_static(b"after crash")
    );
}

#[tokio::test]
async fn crash_index_partially_written() {
    let (dir, store) = open_temp(4096);
    let keep = store.put(Bytes::from_static(b"kept")).await.unwrap();
    let torn = store.put(Bytes::from_static(b"torn away")).await.unwrap();
    drop(store);

    // Tear the tail: cut 3 bytes out of the last index record.
    let index_path = dir.path().join("index.log");
    let full_len = fs::metadata(&index_path).unwrap().len();
    let file = OpenOptions::new().write(true).open(&index_path).unwrap();
    file.set_len(full_len - 3).unwrap();
    file.sync_data().unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert!(reopened.has(&keep).await.unwrap(), "whole records survive");
    assert!(
        !reopened.has(&torn).await.unwrap(),
        "the torn record was never acknowledged and is dropped"
    );
    // A PUT record is 1 (tag) + 32 (hash) + 4*8 (fields) = 65 bytes; the
    // tear removed 3, so the heal truncates the remaining 62.
    assert_eq!(
        reopened.open_report().unwrap().torn_index_bytes_truncated,
        62
    );
    assert_eq!(
        fs::metadata(&index_path).unwrap().len(),
        full_len - 65,
        "the index is truncated back to the last whole record"
    );

    // The heal is durable: the next open sees a clean index.
    drop(reopened);
    let again = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert_eq!(again.open_report().unwrap().torn_index_bytes_truncated, 0);
    // And the store accepts the blob again.
    let rewritten = again.put(Bytes::from_static(b"torn away")).await.unwrap();
    assert_eq!(rewritten, torn);
    assert_eq!(
        again.get(&rewritten).await.unwrap(),
        Bytes::from_static(b"torn away")
    );
}

#[tokio::test]
async fn crash_active_pack_truncated() {
    let (dir, store) = open_temp(64 * 1024);
    let victim_body: Vec<u8> = (0..2048usize).map(|i| (i % 251) as u8).collect();
    let victim = store.put(Bytes::from(victim_body)).await.unwrap();
    drop(store);

    // Truncate the pack mid-record-body.
    let path = pack_path(&dir.path().join("packs"), 0);
    let full_len = fs::metadata(&path).unwrap().len();
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(full_len - 100).unwrap();
    file.sync_data().unwrap();

    // The store still opens; the truncated blob fails closed on read.
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 64 * 1024).unwrap();
    let err = reopened.get(&victim).await.unwrap_err();
    assert!(
        matches!(
            err.storage_kind(),
            Some(StorageErrorKind::Io) | Some(StorageErrorKind::Corruption)
        ),
        "no partial bytes are ever served: {err}"
    );

    // New writes still work.
    let fresh = reopened.put(Bytes::from_static(b"fresh")).await.unwrap();
    assert_eq!(
        reopened.get(&fresh).await.unwrap(),
        Bytes::from_static(b"fresh")
    );
}

#[tokio::test]
async fn crash_temp_file_left_behind() {
    let (dir, store) = open_temp(256);
    drop(store);
    let leftover = dir.path().join(format!("{}crashed", disk::TMP_PREFIX));
    fs::write(&leftover, b"half-written marker").unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    assert!(!leftover.exists(), "crash leftovers are swept at open");
    assert_eq!(reopened.open_report().unwrap().stale_temp_files_removed, 1);
    let hash = reopened
        .put(Bytes::from_static(b"back to work"))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(&hash).await.unwrap(),
        Bytes::from_static(b"back to work")
    );
}

#[tokio::test]
async fn crash_index_points_at_corrupt_bytes() {
    let (dir, store) = open_temp(256);
    let hash = store.put(Bytes::from_static(b"authentic")).await.unwrap();
    let entry = lock(&store.state)
        .unwrap()
        .index
        .get(&hash)
        .copied()
        .unwrap();
    drop(store);

    let path = pack_path(&dir.path().join("packs"), entry.pack_id);
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    let body_offset = entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;
    file.seek(SeekFrom::Start(body_offset)).unwrap();
    file.write_all(b"X").unwrap();
    file.sync_data().unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    let err = reopened.get(&hash).await.unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(StorageErrorKind::Corruption),
        "a content-address mismatch fails closed; no partial bytes"
    );
}
#[tokio::test]
async fn crash_index_torn_release_tail_truncated() {
    let (dir, store) = open_temp(4096);
    let hash = store.put(Bytes::from_static(b"released?")).await.unwrap();
    store.release(&hash).await.unwrap();
    drop(store);

    // Tear the trailing RELEASE record (1 tag + 32 hash = 33 bytes).
    let index_path = dir.path().join("index.log");
    let full_len = fs::metadata(&index_path).unwrap().len();
    let file = OpenOptions::new().write(true).open(&index_path).unwrap();
    file.set_len(full_len - 3).unwrap();
    file.sync_data().unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert!(
        reopened.has(&hash).await.unwrap(),
        "a torn release was never acknowledged, so the blob stays live"
    );
    assert_eq!(
        reopened.open_report().unwrap().torn_index_bytes_truncated,
        30
    );
}

#[tokio::test]
async fn crash_index_unknown_tag_fails_closed() {
    let (dir, store) = open_temp(4096);
    store.put(Bytes::from_static(b"fine")).await.unwrap();
    drop(store);

    // Append a structurally complete record with an unknown tag: this is
    // not a torn tail, it is corruption, and the open must refuse.
    let index_path = dir.path().join("index.log");
    let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
    file.write_all(&[9u8]).unwrap();
    file.write_all(&[0u8; crate::BLAKE3_HASH_LEN]).unwrap();
    file.sync_data().unwrap();

    let err = match LocalPackStore::open_with_pack_target(dir.path(), 4096) {
        Ok(_) => panic!("unknown index tag must fail closed"),
        Err(err) => err,
    };
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn crash_release_replay_order_preserved() {
    let (dir, store) = open_temp(4096);
    let hash = store.put(Bytes::from_static(b"cycled")).await.unwrap();
    store.release(&hash).await.unwrap();
    let again = store.put(Bytes::from_static(b"cycled")).await.unwrap();
    assert_eq!(hash, again);
    drop(store);

    // Replay must apply PUT / RELEASE / PUT in log order: live at the end.
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert!(reopened.has(&hash).await.unwrap());
    assert_eq!(
        reopened.get(&hash).await.unwrap(),
        Bytes::from_static(b"cycled")
    );
    assert_eq!(
        reopened.len().unwrap(),
        1,
        "duplicate PUTs collapse to one entry"
    );
}

#[tokio::test]
async fn compaction_crash_replay_prefers_rewritten_records() {
    let (dir, store) = open_temp(4096);
    let keep = store.put(Bytes::from_static(b"survivor")).await.unwrap();
    let dead = store.put(Bytes::from_static(b"garbage")).await.unwrap();
    store.release(&dead).await.unwrap();
    store.compact().await.unwrap();
    drop(store);

    // Simulate a crash where an old pack's delete never persisted: the
    // orphan pack reappears next to the rewritten one.
    let packs_dir = dir.path().join("packs");
    let orphan = pack_path(&packs_dir, 0);
    assert!(!orphan.exists(), "compaction removed the original pack");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&orphan)
        .unwrap();
    file.write_all(PACK_MAGIC).unwrap();
    file.sync_data().unwrap();

    // Replay resolves the survivor to the rewritten (last-wins) record,
    // and the resurrected orphan is inert.
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert_eq!(
        reopened.get(&keep).await.unwrap(),
        Bytes::from_static(b"survivor")
    );
    assert!(!reopened.has(&dead).await.unwrap());

    // The next compaction removes the orphan pack.
    reopened.compact().await.unwrap();
    assert!(!orphan.exists(), "orphan pack is reclaimed by compaction");
    assert_eq!(
        reopened.get(&keep).await.unwrap(),
        Bytes::from_static(b"survivor")
    );
}

#[tokio::test]
async fn local_pack_concurrent_same_hash_dedups_under_mutex() {
    let (_dir, store) = open_temp(64 * 1024);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            store.put(Bytes::from_static(b"same payload")).await
        }));
    }
    let mut hashes = Vec::new();
    for handle in handles {
        hashes.push(handle.await.unwrap().unwrap());
    }
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(store.len().unwrap(), 1, "concurrent identical puts dedup");
}

#[cfg(unix)]
#[tokio::test]
async fn durable_write_fsync_error_poisons_store() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, store) = open_temp(4096);
    let hash = store
        .put(Bytes::from_static(b"before failure"))
        .await
        .unwrap();

    // Make the index unwritable so the next index append fails.
    let index_path = dir.path().join("index.log");
    fs::set_permissions(&index_path, fs::Permissions::from_mode(0o400)).unwrap();
    let err = store.release(&hash).await.unwrap_err();
    assert!(
        matches!(err.storage_kind(), Some(StorageErrorKind::Io)),
        "the failing write surfaces as Io: {err}"
    );

    // Fail-stop: even a write that would touch different files is refused.
    fs::set_permissions(&index_path, fs::Permissions::from_mode(0o644)).unwrap();
    let err = store
        .put(Bytes::from_static(b"after failure"))
        .await
        .unwrap_err();
    assert_eq!(
        err.storage_kind(),
        Some(StorageErrorKind::Unavailable),
        "a poisoned store refuses further mutations until reopened"
    );

    // Reads stay available (content-verified).
    assert_eq!(
        store.get(&hash).await.unwrap(),
        Bytes::from_static(b"before failure")
    );

    // Reopening recovers: state is revalidated from disk.
    drop(store);
    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
    assert!(reopened.has(&hash).await.unwrap(), "release never landed");
    let fresh = reopened
        .put(Bytes::from_static(b"after reopen"))
        .await
        .unwrap();
    assert_eq!(
        reopened.get(&fresh).await.unwrap(),
        Bytes::from_static(b"after reopen")
    );
}

#[tokio::test]
async fn open_prunes_stale_quarantine_entries() {
    // Simulate the crash window between release's index tombstone and the
    // quarantine side-file rewrite: a quarantine entry for an absent claim.
    // A second LIVE blob keeps the index non-empty (authoritative), which is
    // the condition under which the open-time prune applies.
    let (dir, store) = open_temp(256);
    let keep = store.put(Bytes::from_static(b"keeper")).await.unwrap();
    let hash = store
        .put(Bytes::from_static(b"reintroduce me"))
        .await
        .unwrap();
    store.release(&hash).await.unwrap();
    drop(store);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NBLQ2\n");
    bytes.extend_from_slice(hash.as_bytes());
    bytes.push(2u8); // Content reason: the sticky kind.
    fs::write(dir.path().join("quarantine.nblq"), &bytes).unwrap();

    let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
    let report = reopened.open_report().unwrap();
    assert_eq!(report.stale_quarantine_entries_pruned, 1);
    assert_eq!(report.quarantine_entries_loaded, 0);

    // Reintroducing the same content hash works and reads back.
    let again = reopened
        .put(Bytes::from_static(b"reintroduce me"))
        .await
        .unwrap();
    assert_eq!(again, hash);
    assert_eq!(
        reopened.get(&again).await.unwrap(),
        Bytes::from_static(b"reintroduce me")
    );
    assert_eq!(
        reopened.get(&keep).await.unwrap(),
        Bytes::from_static(b"keeper")
    );
}
