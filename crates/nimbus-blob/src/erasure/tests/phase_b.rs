use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

use bytes::Bytes;
use nimbus_core::StorageErrorKind;

use super::*;
use crate::{ErasureHealer, HealPacing, LocalPackScrubber};

fn payload_with_seed(len: usize, seed: u8) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|index| {
                let mixed = index
                    .wrapping_mul(37)
                    .wrapping_add((seed as usize).wrapping_mul(19))
                    .wrapping_add(11);
                (mixed % 251) as u8
            })
            .collect::<Vec<_>>(),
    )
}

async fn write_orphan_shards(
    store: &ErasureBlobStore,
    bytes: Bytes,
) -> BTreeSet<(usize, BlobHash)> {
    let mut written = BTreeSet::new();
    for (stripe_index, chunk) in bytes.chunks(STRIPE).enumerate() {
        let shards = stripe::encode_stripe(chunk, K, M).unwrap();
        for (shard_index, shard) in shards.into_iter().enumerate() {
            let drive = stripe::drive_for(shard_index, stripe_index, K + M);
            let hash = store.drive_store(drive).put(shard).await.unwrap();
            written.insert((drive, hash));
        }
    }
    written
}

fn manifest_shards(
    store: &ErasureBlobStore,
    manifest: &ErasureManifest,
) -> BTreeSet<(usize, BlobHash)> {
    let mut shards = BTreeSet::new();
    for (stripe_index, stripe) in manifest.stripes.iter().enumerate() {
        for shard in stripe {
            let drive = stripe::drive_for(
                shard.shard_index as usize,
                stripe_index,
                store.drive_roots().len(),
            );
            shards.insert((drive, shard.shard_hash));
        }
    }
    shards
}

fn manifest_shard_indices(
    store: &ErasureBlobStore,
    manifest: &ErasureManifest,
) -> BTreeSet<(usize, usize, BlobHash)> {
    let mut shards = BTreeSet::new();
    for (stripe_index, stripe) in manifest.stripes.iter().enumerate() {
        for shard in stripe {
            let shard_index = shard.shard_index as usize;
            let drive = stripe::drive_for(shard_index, stripe_index, store.drive_roots().len());
            shards.insert((drive, shard_index, shard.shard_hash));
        }
    }
    shards
}

async fn sweep_all(store: &ErasureBlobStore, grace: Duration) -> usize {
    let mut swept = 0usize;
    for drive in 0..K + M {
        let report = store.sweep_drive(drive, grace).await.unwrap();
        swept += report.swept;
    }
    swept
}

async fn sweep_all_reports(store: &ErasureBlobStore, grace: Duration) -> Vec<crate::BlobGcReport> {
    let mut reports = Vec::new();
    for drive in 0..K + M {
        reports.push(store.sweep_drive(drive, grace).await.unwrap());
    }
    reports
}

fn manifest_generations(store: &ErasureBlobStore, hash: &BlobHash) -> Vec<u64> {
    (0..K + M)
        .map(|drive| {
            let path = manifest::manifest_path(&store.drive_root(drive), hash);
            ErasureManifest::decode(&fs::read(path).unwrap())
                .unwrap()
                .generation
        })
        .collect()
}

async fn assert_shards_present(store: &ErasureBlobStore, shards: &BTreeSet<(usize, BlobHash)>) {
    for (drive, hash) in shards {
        assert!(
            store.drive_store(*drive).has(hash).await.unwrap(),
            "drive {drive} should retain shard {hash}"
        );
    }
}

async fn assert_shards_absent(store: &ErasureBlobStore, shards: &BTreeSet<(usize, BlobHash)>) {
    for (drive, hash) in shards {
        assert!(
            !store.drive_store(*drive).has(hash).await.unwrap(),
            "drive {drive} should reclaim shard {hash}"
        );
    }
}

#[tokio::test]
async fn erasure_gc_reclaims_orphan_shards_after_failed_put() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let kept = payload_with_seed(STRIPE + 5, 1);
    let kept_hash = store.put(kept).await.unwrap();
    let kept_manifest = store.load_manifest_for_test(&kept_hash).await.unwrap();
    let kept_shards = manifest_shards(&store, &kept_manifest);
    let orphans = write_orphan_shards(&store, payload_with_seed(STRIPE + 9, 91)).await;
    let reclaimable = orphans
        .difference(&kept_shards)
        .copied()
        .collect::<BTreeSet<_>>();

    let swept = sweep_all(&store, Duration::ZERO).await;

    assert_eq!(swept, reclaimable.len());
    assert_shards_present(&store, &kept_shards).await;
    assert_shards_absent(&store, &reclaimable).await;
}

#[tokio::test]
async fn erasure_gc_respects_visible_manifest_roots() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let keep_hash = store.put(payload_with_seed(STRIPE + 3, 2)).await.unwrap();
    let drop_hash = store.put(payload_with_seed(STRIPE + 11, 77)).await.unwrap();
    let keep_manifest = store.load_manifest_for_test(&keep_hash).await.unwrap();
    let drop_manifest = store.load_manifest_for_test(&drop_hash).await.unwrap();
    let keep_shards = manifest_shards(&store, &keep_manifest);
    let drop_shards = manifest_shards(&store, &drop_manifest);
    let drop_unique = drop_shards
        .difference(&keep_shards)
        .copied()
        .collect::<BTreeSet<_>>();

    store.release(&drop_hash).await.unwrap();
    let swept = sweep_all(&store, Duration::ZERO).await;

    assert_eq!(swept, drop_unique.len());
    assert_shards_present(&store, &keep_shards).await;
    assert_shards_absent(&store, &drop_unique).await;
}

#[tokio::test]
async fn erasure_gc_grace_retains_young_orphans() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let orphans = write_orphan_shards(&store, payload_with_seed(STRIPE + 7, 13)).await;

    let reports = sweep_all_reports(&store, Duration::from_secs(60)).await;

    assert_eq!(reports.iter().map(|report| report.swept).sum::<usize>(), 0);
    assert_eq!(
        reports
            .iter()
            .map(|report| report.grace_retained)
            .sum::<usize>(),
        orphans.len()
    );
    assert_shards_present(&store, &orphans).await;
}

#[tokio::test]
async fn erasure_heal_restores_missing_shard() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload_with_seed(STRIPE + 17, 3);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 0);
    let drive = stripe::drive_for(0, 0, K + M);
    release_shard(&store, &manifest, 0, 0).await;
    assert!(
        !store
            .drive_store(drive)
            .has(&shard.shard_hash)
            .await
            .unwrap()
    );

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();

    assert_eq!(report.blobs_examined, 1);
    assert_eq!(report.degraded, 1);
    assert_eq!(report.stripes_repaired, 1);
    assert_eq!(report.shards_rewritten, 1);
    assert!(report.beyond_repair.is_empty());
    assert!(!report.exhausted);
    assert_eq!(manifest_generations(&store, &hash), vec![2; K + M]);
    assert!(
        store
            .drive_store(drive)
            .has(&shard.shard_hash)
            .await
            .unwrap()
    );
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_heal_lifts_quarantine_via_reupload() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload_with_seed(STRIPE + 5, 4);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 1);
    let drive = stripe::drive_for(1, 0, K + M);

    flip_shard_body_byte(&store.drive_root(drive), &shard.shard_hash);
    let scrub = LocalPackScrubber::new(store.drive_store(drive))
        .scrub()
        .await
        .unwrap();
    assert!(scrub.quarantined_hashes.contains(&shard.shard_hash));
    let err = store
        .drive_store(drive)
        .get(&shard.shard_hash)
        .await
        .unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();

    assert_eq!(report.shards_rewritten, 1);
    assert!(
        store
            .drive_store(drive)
            .get(&shard.shard_hash)
            .await
            .is_ok()
    );
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test]
async fn erasure_heal_reports_beyond_repair_without_deleting() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let hash = store.put(payload_with_seed(STRIPE, 5)).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let all = manifest_shard_indices(&store, &manifest);
    let removed_indices = BTreeSet::from([0usize, 1, K]);
    for shard_index in &removed_indices {
        release_shard(&store, &manifest, 0, *shard_index).await;
    }
    let remaining = all
        .into_iter()
        .filter(|(_, shard_index, _)| !removed_indices.contains(shard_index))
        .map(|(drive, _, hash)| (drive, hash))
        .collect::<BTreeSet<_>>();

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();

    assert_eq!(report.beyond_repair, vec![hash]);
    assert_eq!(report.stripes_repaired, 0);
    assert_eq!(report.shards_rewritten, 0);
    assert_shards_present(&store, &remaining).await;
    let err = store.get(&hash).await.unwrap_err();
    assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
}

#[tokio::test]
async fn erasure_heal_verifies_before_writing() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let hash = store.put(payload_with_seed(STRIPE, 6)).await.unwrap();
    let mut manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = shard_ref(&manifest, 0, 0);
    let drive = stripe::drive_for(0, 0, K + M);
    release_shard(&store, &manifest, 0, 0).await;
    manifest.generation += 1;
    manifest.stripe_hashes[0] = BlobHash::of(b"wrong stripe hash");
    store.publish_manifest_for_test(manifest).await.unwrap();

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();

    assert_eq!(report.beyond_repair, vec![hash]);
    assert_eq!(report.stripes_repaired, 0);
    assert_eq!(report.shards_rewritten, 0);
    assert!(
        !store
            .drive_store(drive)
            .has(&shard.shard_hash)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .load_manifest_for_test(&hash)
            .await
            .unwrap()
            .generation,
        2
    );
}

#[tokio::test]
async fn erasure_heal_window_blocks_gc() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let hash = store.put(payload_with_seed(STRIPE, 7)).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shards = manifest_shards(&store, &manifest);
    let window = store
        .heal_pin_registry()
        .pin_all(shards.iter().map(|(_, hash)| *hash));
    store.release(&hash).await.unwrap();

    let during = sweep_all_reports(&store, Duration::ZERO).await;

    assert_eq!(
        during
            .iter()
            .map(|report| report.intent_retained)
            .sum::<usize>(),
        shards.len()
    );
    assert_eq!(during.iter().map(|report| report.swept).sum::<usize>(), 0);
    assert_shards_present(&store, &shards).await;

    drop(window);
    let swept = sweep_all(&store, Duration::ZERO).await;
    assert_eq!(swept, shards.len());
    assert_shards_absent(&store, &shards).await;
}

#[tokio::test]
async fn erasure_heal_pacing_stops_at_budget() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let mut expected = std::collections::HashMap::new();
    for seed in [8u8, 9] {
        let bytes = payload_with_seed(STRIPE, seed);
        let hash = store.put(bytes.clone()).await.unwrap();
        let manifest = store.load_manifest_for_test(&hash).await.unwrap();
        release_shard(&store, &manifest, 0, 0).await;
        expected.insert(hash, bytes);
    }

    let pacing = HealPacing::max_bytes_per_run(STRIPE as u64).unwrap();
    let first = ErasureHealer::new(store.clone())
        .with_pacing(pacing)
        .heal()
        .await
        .unwrap();
    assert!(first.exhausted);
    assert_eq!(first.stripes_repaired, 1);
    assert_eq!(first.shards_rewritten, 1);

    let second = ErasureHealer::new(store.clone())
        .with_pacing(pacing)
        .heal()
        .await
        .unwrap();
    assert!(!second.exhausted);
    assert_eq!(second.stripes_repaired, 1);
    assert_eq!(second.shards_rewritten, 1);
    for (hash, bytes) in expected {
        assert_eq!(store.get(&hash).await.unwrap(), bytes);
    }
}

#[tokio::test]
async fn erasure_stats_aggregates_per_drive_and_heal() {
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let degraded_hash = store.put(payload_with_seed(STRIPE, 10)).await.unwrap();
    let degraded_manifest = store.load_manifest_for_test(&degraded_hash).await.unwrap();
    release_shard(&store, &degraded_manifest, 0, 0).await;
    let beyond_hash = store.put(payload_with_seed(STRIPE, 11)).await.unwrap();
    let beyond_manifest = store.load_manifest_for_test(&beyond_hash).await.unwrap();
    for shard_index in [0usize, 1, K] {
        release_shard(&store, &beyond_manifest, 0, shard_index).await;
    }

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();
    let stats = store.stats().await.unwrap();

    assert_eq!(report.degraded, 1);
    assert_eq!(report.beyond_repair, vec![beyond_hash]);
    assert_eq!(stats.per_drive.len(), K + M);
    assert_eq!(stats.blob_count, 2);
    assert_eq!(stats.degraded_blobs, 1);
    assert_eq!(stats.beyond_repair_blobs, 1);
    let summary = stats.last_heal.expect("heal summary recorded");
    assert_eq!(summary.blobs_examined, report.blobs_examined);
    assert_eq!(summary.stripes_repaired, report.stripes_repaired);
    assert_eq!(summary.shards_rewritten, report.shards_rewritten);
    assert_eq!(summary.degraded_blobs, report.degraded);
    assert_eq!(summary.beyond_repair_blobs, report.beyond_repair.len());
    assert_eq!(summary.exhausted, report.exhausted);
    assert_eq!(summary.at_millis, report.at_millis);
}

#[cfg(unix)]
#[tokio::test]
async fn erasure_poisoned_leg_refuses_shard_gc() {
    // Review fix (Phase B round 1, P1): the shard GC surface honors the
    // poison fail-stop end to end — a poisoned leg's manifest view is
    // ambiguous, and sweeping against it could reclaim shards whose
    // manifests resurface after a crash. Construction refuses when already
    // poisoned; an ALREADY-CONSTRUCTED sweep fails at root enumeration
    // when the leg poisons afterwards, and reclaims nothing.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let committed = store.put(payload(STRIPE + 3)).await.unwrap();

    // Stage an aged orphan shard on drive 0 so a rogue sweep would have
    // something to reclaim.
    let orphan = store
        .drive_store(0)
        .put(Bytes::from_static(b"orphan shard bytes"))
        .await
        .unwrap();

    // Construct the sweep BEFORE poisoning.
    let gc = store.shard_gc(0, std::time::Duration::ZERO).unwrap();

    // Poison via a nondurable-rollback publish failure (round-9 seam).
    let blocked = manifest::manifest_dir(&store.drive_root(K + M - 1));
    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&blocked, perms).unwrap();
    store.arm_nondurable_rollback();
    store.put(payload(2 * STRIPE + 1)).await.unwrap_err();
    assert!(store.is_poisoned());

    // The pre-constructed sweep fails closed at enumeration; the orphan
    // and every committed shard survive.
    let err = gc.sweep().await.unwrap_err();
    assert!(err.to_string().contains("poisoned"));
    assert!(store.drive_store(0).has(&orphan).await.unwrap());

    // New construction refuses outright — via the public sweep_drive path
    // and the crate-private constructor alike.
    let err = store
        .sweep_drive(0, std::time::Duration::ZERO)
        .await
        .expect_err("poisoned leg must refuse sweep_drive");
    assert!(err.to_string().contains("poisoned"));
    let err = match store.shard_gc(0, std::time::Duration::ZERO) {
        Ok(_) => panic!("poisoned leg must refuse shard_gc construction"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("poisoned"));

    // A second same-leg handle sees the SAME poison (leg state, not handle
    // state).
    let second = ErasureBlobStore::open(
        ErasureConfig::new("test-leg", store.drive_roots(), K, M, STRIPE).unwrap(),
    )
    .unwrap();
    assert!(second.is_poisoned(), "poison must be shared per leg");

    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&blocked, perms).unwrap();
    let _ = committed;
}

#[tokio::test]
async fn erasure_heal_rewrites_unquarantined_corrupt_shard() {
    // Review fix (Phase B round 2, P1): a bit-flipped shard whose record is
    // corrupt on disk but still INDEXED must be released before rewrite —
    // LocalPackStore::put is an idempotent no-op for an indexed hash, so a
    // bare re-put would report "repaired" while the bad bytes stayed. The
    // healer also reads the shard back and only counts repairs it can
    // serve.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 9);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();

    // Corrupt shard 0's record body on disk WITHOUT scrubbing (stays
    // indexed, unquarantined).
    let shard = manifest.stripes[0]
        .iter()
        .find(|candidate| candidate.shard_index == 0)
        .unwrap();
    let drive = stripe::drive_for(0, 0, K + M);
    flip_shard_body_byte(&store.drive_root(drive), &shard.shard_hash);
    assert_eq!(
        store
            .drive_store(stripe::drive_for(0, 0, K + M))
            .get(&shard.shard_hash)
            .await
            .unwrap_err()
            .storage_kind(),
        Some(StorageErrorKind::Corruption),
        "precondition: shard is corrupt but indexed"
    );

    let report = ErasureHealer::new(store.clone()).heal().await.unwrap();
    assert_eq!(report.shards_rewritten, 1);
    assert!(report.beyond_repair.is_empty());

    // The drive ACTUALLY serves the healed shard now, and the blob reads
    // non-degraded.
    assert!(
        store
            .drive_store(drive)
            .get(&shard.shard_hash)
            .await
            .is_ok(),
        "healed shard must be servable, not an idempotent no-op"
    );
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn erasure_gc_never_sweeps_inflight_put_shards() {
    // Review fix (Phase B round 2, P1): put pins its shard hashes before
    // any byte lands, so even a ZERO-grace sweep racing a put cannot
    // reclaim pre-publish shards — every acknowledged put remains fully
    // readable.
    let (_dir, store, _roots) = open_temp(2, 1, STRIPE);
    for round in 0..10 {
        let blob = payload(4 * STRIPE + round);
        let put_store = store.clone();
        let put_blob = blob.clone();
        let put_task = tokio::spawn(async move { put_store.put(put_blob).await });

        let gc_store = store.clone();
        let gc_task = tokio::spawn(async move {
            for drive in 0..3 {
                let _ = gc_store
                    .sweep_drive(drive, std::time::Duration::ZERO)
                    .await
                    .unwrap();
            }
        });

        let hash = put_task.await.unwrap().unwrap();
        gc_task.await.unwrap();
        assert_eq!(
            store.get(&hash).await.unwrap(),
            blob,
            "an acknowledged put must survive a racing zero-grace sweep (round {round})"
        );
        store.release(&hash).await.unwrap();
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn erasure_heal_rechecks_poison_under_the_mutation_lock() {
    // Review fix (Phase B round 4, P1): a healer that passed its initial
    // liveness check and then waited behind a put must re-check the
    // fail-stop AFTER acquiring the leg mutation lock — the put ahead of
    // it can poison the leg (nondurable rollback), and healing against the
    // ambiguous manifest view would violate the poison contract. Relies on
    // tokio::sync::Mutex fairness (FIFO waiters) for the interleaving.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload(STRIPE + 17);
    let hash = store.put(bytes.clone()).await.unwrap();
    // Degrade the blob so heal has real work queued.
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    let shard = manifest.stripes[0]
        .iter()
        .find(|candidate| candidate.shard_index == 0)
        .unwrap();
    store
        .drive_store(stripe::drive_for(0, 0, K + M))
        .release(&shard.shard_hash)
        .await
        .unwrap();

    // Arm a poisoning put (fresh blob, nondurable rollback on the last
    // drive), then interleave: hold the lock, queue the put (waiter 1),
    // queue the heal (waiter 2), release.
    let blocked = manifest::manifest_dir(&store.drive_root(K + M - 1));
    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&blocked, perms).unwrap();
    store.arm_nondurable_rollback();

    let guard = store.mutation_lock().lock_owned().await;
    let put_store = store.clone();
    let put_task = tokio::spawn(async move { put_store.put(payload(2 * STRIPE + 23)).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let heal_store = store.clone();
    let heal_task = tokio::spawn(async move { ErasureHealer::new(heal_store).heal().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    drop(guard);

    put_task.await.unwrap().unwrap_err();
    assert!(store.is_poisoned(), "the queued put poisoned the leg");
    let heal_err = heal_task
        .await
        .unwrap()
        .expect_err("heal behind the poisoning put must fail-stop");
    assert!(heal_err.to_string().contains("poisoned"));

    // No generation bump was published over the ambiguous view.
    let after = manifest::load_newest(
        &hash,
        &store.drive_roots(),
        // parity+1 quorum for K=4, M=2
        M + 1,
    )
    .unwrap()
    .expect("committed blob still visible");
    assert_eq!(after.generation, manifest.generation, "no heal publish");

    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&blocked, perms).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn erasure_sweep_fails_closed_when_leg_poisons_mid_enumeration() {
    // Review fix (Phase B round 5, P1): the poison gate covers the RELEASE
    // phase, not just sweep start — a leg that poisons while roots are
    // being enumerated aborts the sweep before any shard is reclaimed.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    // Stage a stale orphan shard (pre-snapshot, unrooted, past any grace).
    let orphan = store
        .drive_store(0)
        .put(Bytes::from_static(b"ambiguous-state evidence"))
        .await
        .unwrap();

    // Trip the poison DURING the sweep via a nondurable-rollback put run
    // from inside a root provider look-alike: simplest deterministic form —
    // poison BEFORE the release loop by arming and failing a put between
    // construction and sweep.
    let gc = store.shard_gc(0, std::time::Duration::ZERO).unwrap();
    let blocked = manifest::manifest_dir(&store.drive_root(K + M - 1));
    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&blocked, perms).unwrap();
    store.arm_nondurable_rollback();
    store.put(payload(STRIPE + 41)).await.unwrap_err();
    assert!(store.is_poisoned());

    let err = gc.sweep().await.unwrap_err();
    assert!(err.to_string().contains("poisoned"));
    assert!(
        store.drive_store(0).has(&orphan).await.unwrap(),
        "no shard reclaimed after the leg fail-stopped"
    );

    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&blocked, perms).unwrap();
}

#[tokio::test]
async fn erasure_heal_pacing_never_exceeds_the_byte_cap() {
    // Review fix (Phase B round 6, P3): max_bytes_per_run is a strict
    // MAXIMUM — a budget smaller than one stripe repairs nothing and
    // reports exhausted-without-progress (the operator signal to raise the
    // budget), instead of silently overshooting on the first stripe.
    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let bytes = payload_with_seed(STRIPE, 21);
    let hash = store.put(bytes.clone()).await.unwrap();
    let manifest = store.load_manifest_for_test(&hash).await.unwrap();
    release_shard(&store, &manifest, 0, 0).await;

    let tiny = HealPacing::max_bytes_per_run(STRIPE as u64 - 1).unwrap();
    let report = ErasureHealer::new(store.clone())
        .with_pacing(tiny)
        .heal()
        .await
        .unwrap();
    assert!(report.exhausted, "cap smaller than the stripe: exhausted");
    assert_eq!(report.stripes_repaired, 0, "strict cap: zero overshoot");
    assert_eq!(report.shards_rewritten, 0);

    // Raising the budget to one stripe completes the repair.
    let enough = HealPacing::max_bytes_per_run(STRIPE as u64).unwrap();
    let report = ErasureHealer::new(store.clone())
        .with_pacing(enough)
        .heal()
        .await
        .unwrap();
    assert_eq!(report.stripes_repaired, 1);
    assert_eq!(store.get(&hash).await.unwrap(), bytes);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn erasure_put_and_release_recheck_poison_under_the_mutation_lock() {
    // Review fix (Phase B round 7, P1): put and release queued behind a
    // poisoning mutation must fail-stop after acquiring the lock, exactly
    // like heal and sweep_drive. FIFO-fair tokio Mutex gives the
    // deterministic interleaving.
    use std::os::unix::fs::PermissionsExt;

    let (_dir, store, _roots) = open_temp(K, M, STRIPE);
    let committed = store.put(payload(STRIPE + 51)).await.unwrap();

    let blocked = manifest::manifest_dir(&store.drive_root(K + M - 1));
    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(&blocked, perms).unwrap();
    store.arm_nondurable_rollback();

    let guard = store.mutation_lock().lock_owned().await;
    let poisoner = store.clone();
    let poison_task = tokio::spawn(async move { poisoner.put(payload(2 * STRIPE + 7)).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let putter = store.clone();
    let put_task = tokio::spawn(async move { putter.put(payload(3 * STRIPE + 5)).await });
    let releaser = store.clone();
    let release_task = tokio::spawn(async move { releaser.release(&committed).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    drop(guard);

    poison_task.await.unwrap().unwrap_err();
    assert!(store.is_poisoned());
    let put_err = put_task
        .await
        .unwrap()
        .expect_err("queued put must fail-stop");
    assert!(put_err.to_string().contains("poisoned"));
    let release_err = release_task
        .await
        .unwrap()
        .expect_err("queued release must fail-stop");
    assert!(release_err.to_string().contains("poisoned"));

    // The committed blob's manifests were NOT touched by the refused
    // release.
    for index in 0..(K + M) {
        assert!(
            manifest::manifest_path(&store.drive_root(index), &committed).exists(),
            "refused release must not remove replica {index}"
        );
    }

    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&blocked, perms).unwrap();
}
