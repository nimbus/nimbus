//! RFS7 Phase C bench: `LocalPackStore` baseline vs `ErasureBlobStore`
//! (4+2 and 12+4) on put / get / ranged-read workloads.
//!
//! Custom harness (no criterion dependency) per the repo's bench rules:
//! deterministic payloads, tempdir-backed drive roots, wall-clock timing
//! over fixed iteration counts, throughput reported in MiB/s. Results are
//! recorded (with exact hardware and workload shapes) in the plan proof
//! directory — this runner exists so the numbers are reproducible with
//! `cargo bench -p nimbus-blob --bench local_pack_baremetal`.
//!
//! Erasure legs measure the FULL leg cost: stripe encode, per-drive pack
//! writes, replicated manifest publish (put); manifest quorum load, shard
//! reads, reassembly, whole-blob verify (get); covering-stripe reads only
//! (get_range).

use std::path::PathBuf;
use std::time::Instant;

use bytes::Bytes;
use nimbus_blob::{BlobStore, ErasureBlobStore, ErasureConfig, LocalPackStore};

const SMALL_LEN: usize = 64 * 1024; // 64 KiB objects
const LARGE_LEN: usize = 4 * 1024 * 1024; // 4 MiB objects
const RANGE_LEN: u64 = 64 * 1024; // 64 KiB window of a large object
const SMALL_ITERS: usize = 64;
const LARGE_ITERS: usize = 16;
const RANGE_ITERS: usize = 64;
const STRIPE_WIDTH: usize = 1024 * 1024; // matches the erasure default

fn payload(len: usize, seed: u8) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|index| {
                let mixed = index
                    .wrapping_mul(31)
                    .wrapping_add((seed as usize).wrapping_mul(17))
                    .wrapping_add(7);
                (mixed % 251) as u8
            })
            .collect::<Vec<_>>(),
    )
}

fn mib_per_sec(bytes: u64, elapsed_secs: f64) -> f64 {
    (bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs
}

struct Lane {
    name: &'static str,
    store: Box<dyn BlobStore>,
    /// The lane's ACTUAL stripe width (the k*2-floored value for erasure
    /// lanes; None for local-pack) — reported per lane, since e.g. 12+4
    /// floors 1 MiB to 1,048,560 and a 4 MiB object then carries a fifth
    /// 64-byte stripe, which is material to interpreting the numbers.
    stripe_width: Option<usize>,
    _dirs: Vec<tempfile::TempDir>,
}

fn local_lane() -> Lane {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LocalPackStore::open(dir.path()).expect("open local pack store");
    Lane {
        name: "local-pack",
        store: Box::new(store),
        stripe_width: None,
        _dirs: vec![dir],
    }
}

fn erasure_lane(name: &'static str, data: usize, parity: usize) -> Lane {
    let dirs: Vec<tempfile::TempDir> = (0..data + parity)
        .map(|_| tempfile::tempdir().expect("tempdir"))
        .collect();
    let roots: Vec<PathBuf> = dirs.iter().map(|dir| dir.path().to_path_buf()).collect();
    // Stripe width must be a multiple of data_shards * 2 (even shard
    // lengths); floor the 1 MiB target to each layout's alignment.
    let unit = data * 2;
    let stripe_width = (STRIPE_WIDTH / unit) * unit;
    let config =
        ErasureConfig::new(name, roots, data, parity, stripe_width).expect("erasure config");
    let store = ErasureBlobStore::open(config).expect("open erasure store");
    Lane {
        name,
        store: Box::new(store),
        stripe_width: Some(stripe_width),
        _dirs: dirs,
    }
}

async fn bench_lane(lane: &Lane) {
    // -- small-object put/get --
    let small: Vec<Bytes> = (0..SMALL_ITERS)
        .map(|index| payload(SMALL_LEN, index as u8))
        .collect();
    let start = Instant::now();
    let mut hashes = Vec::with_capacity(SMALL_ITERS);
    for bytes in &small {
        hashes.push(lane.store.put(bytes.clone()).await.expect("put"));
    }
    let put_small = start.elapsed().as_secs_f64();

    let start = Instant::now();
    for hash in &hashes {
        let bytes = lane.store.get(hash).await.expect("get");
        assert_eq!(bytes.len(), SMALL_LEN);
    }
    let get_small = start.elapsed().as_secs_f64();

    // -- large-object put/get --
    // Lengths vary per object (still even) so no two objects share a
    // degenerate tail stripe: identical all-zero-padding shards would
    // dedup in the pack stores and silently inflate put throughput.
    let large: Vec<Bytes> = (0..LARGE_ITERS)
        .map(|index| payload(LARGE_LEN + index * 8192, 100 + index as u8))
        .collect();
    let start = Instant::now();
    let mut large_hashes = Vec::with_capacity(LARGE_ITERS);
    for bytes in &large {
        large_hashes.push(lane.store.put(bytes.clone()).await.expect("put"));
    }
    let put_large = start.elapsed().as_secs_f64();

    let start = Instant::now();
    for (source, hash) in large.iter().zip(&large_hashes) {
        let bytes = lane.store.get(hash).await.expect("get");
        assert_eq!(bytes.len(), source.len());
    }
    let get_large = start.elapsed().as_secs_f64();

    // -- ranged reads over large objects --
    // Stride the window across the FULL object span so every stripe is
    // exercised (a small fixed stride never left stripe 0 — review fix).
    let span = (LARGE_LEN as u64) - RANGE_LEN;
    let stride = (span / RANGE_ITERS as u64).max(2) & !1;
    let start = Instant::now();
    for iteration in 0..RANGE_ITERS {
        let hash = &large_hashes[iteration % large_hashes.len()];
        let offset = ((iteration as u64 * stride) % span) & !1;
        let slice = lane
            .store
            .get_range(hash, offset..offset + RANGE_LEN)
            .await
            .expect("get_range");
        assert_eq!(slice.len() as u64, RANGE_LEN);
    }
    let range_reads = start.elapsed().as_secs_f64();

    // UNTIMED validation: benchmark evidence is only meaningful if the
    // lanes returned the RIGHT bytes — hashes match inputs, full gets
    // equal their payloads, and sampled ranges equal the source slices.
    for (bytes, hash) in small.iter().zip(&hashes) {
        assert_eq!(*hash, nimbus_blob::BlobHash::of(bytes), "put hash mismatch");
        assert_eq!(&lane.store.get(hash).await.expect("verify get"), bytes);
    }
    for (bytes, hash) in large.iter().zip(&large_hashes) {
        assert_eq!(*hash, nimbus_blob::BlobHash::of(bytes), "put hash mismatch");
        assert_eq!(&lane.store.get(hash).await.expect("verify get"), bytes);
    }
    for iteration in 0..RANGE_ITERS {
        let object = iteration % large_hashes.len();
        let object_span = (large[object].len() as u64) - RANGE_LEN;
        let offset = ((iteration as u64 * stride) % object_span.min(span)) & !1;
        let slice = lane
            .store
            .get_range(&large_hashes[object], offset..offset + RANGE_LEN)
            .await
            .expect("verify get_range");
        assert_eq!(
            slice,
            large[object].slice(offset as usize..(offset + RANGE_LEN) as usize),
            "range window content mismatch"
        );
    }

    let small_bytes = (SMALL_ITERS * SMALL_LEN) as u64;
    let large_bytes: u64 = large.iter().map(|bytes| bytes.len() as u64).sum();
    let range_bytes = RANGE_ITERS as u64 * RANGE_LEN;
    let stripe = lane
        .stripe_width
        .map(|width| format!("stripe {width}B"))
        .unwrap_or_else(|| "no striping".to_string());
    println!(
        "{:<14} [{stripe}] | put64K {:>8.1} MiB/s | get64K {:>8.1} MiB/s | put~4M {:>8.1} MiB/s | get~4M {:>8.1} MiB/s | range64K {:>8.1} MiB/s",
        lane.name,
        mib_per_sec(small_bytes, put_small),
        mib_per_sec(small_bytes, get_small),
        mib_per_sec(large_bytes, put_large),
        mib_per_sec(large_bytes, get_large),
        mib_per_sec(range_bytes, range_reads),
    );
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async {
        println!(
            "workloads: {SMALL_ITERS}x{SMALL_LEN}B put/get, {LARGE_ITERS}x(~{LARGE_LEN}B, +8KiB/obj) put/get, {RANGE_ITERS}x{RANGE_LEN}B ranged reads striding the full span (stripe target {STRIPE_WIDTH}B, floored per lane)"
        );
        for lane in [
            local_lane(),
            erasure_lane("erasure-4p2", 4, 2),
            erasure_lane("erasure-12p4", 12, 4),
        ] {
            bench_lane(&lane).await;
        }
    });
}
