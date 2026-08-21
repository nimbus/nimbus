use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, TableName, TenantId};
use nimbus_engine::Engine;
use serde::Serialize;
use serde_json::json;

const FORMAT_VERSION: u16 = 1;
const BASELINE_COMMIT: &str = "137cc632a1c8585545d200ea49f44bd236478175";
const QUICK_DOCUMENTS: usize = 10_000;
const QUICK_PAYLOAD_BYTES: usize = 1_024;
const CHURN_BASIS_POINTS: [u32; 4] = [0, 10, 100, 1_000];

struct CountingAllocator;

static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: this wrapper delegates every operation to the process System
// allocator. The counters do not affect allocation results or pointer use.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies the GlobalAlloc contract to this method.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout came from the delegated allocator.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies the GlobalAlloc contract to this method.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the pointer and layout came from the delegated allocator.
        let next = unsafe { System.realloc(pointer, layout, new_size) };
        if !next.is_null() {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        next
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Debug)]
struct Arguments {
    output: PathBuf,
    quick: bool,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    format_version: u16,
    baseline_commit: &'static str,
    mode: &'static str,
    target_os: &'static str,
    target_arch: &'static str,
    quick: bool,
    measurements: Vec<FullVerificationMeasurement>,
}

#[derive(Debug, Serialize)]
struct FullVerificationMeasurement {
    documents: usize,
    payload_bytes: usize,
    payload_state_bytes: u64,
    churn_basis_points: u32,
    elapsed_ns: u128,
    process_cpu_ns: u128,
    allocation_count: u64,
    allocated_bytes: u64,
    peak_rss_bytes: Option<u64>,
    bytes_read: Option<u64>,
    bytes_read_status: &'static str,
    report_ok: bool,
    mismatch_count: usize,
    authoritative_document_count: usize,
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Internal(format!("failed to build benchmark runtime: {error}")))?;

    let rungs = if arguments.quick {
        vec![(QUICK_DOCUMENTS, QUICK_PAYLOAD_BYTES)]
    } else {
        vec![
            (10_000, 256),
            (10_000, 1_024),
            (10_000, 8 * 1_024),
            (100_000, 256),
            (100_000, 1_024),
            (100_000, 8 * 1_024),
            (1_000_000, 256),
            (1_000_000, 1_024),
            (1_000_000, 8 * 1_024),
        ]
    };

    let mut measurements = Vec::with_capacity(if arguments.quick {
        1
    } else {
        rungs.len() * CHURN_BASIS_POINTS.len()
    });
    for (documents, payload_bytes) in rungs {
        measurements.extend(runtime.block_on(measure_full_verification_series(
            documents,
            payload_bytes,
            arguments.quick,
        ))?);
    }

    let report = BenchmarkReport {
        format_version: FORMAT_VERSION,
        baseline_commit: BASELINE_COMMIT,
        mode: "full",
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        quick: arguments.quick,
        measurements,
    };
    write_report(&arguments.output, &report)?;
    println!("wrote {}", arguments.output.display());
    Ok(())
}

async fn measure_full_verification_series(
    documents: usize,
    payload_bytes: usize,
    quick: bool,
) -> Result<Vec<FullVerificationMeasurement>> {
    let data_dir = tempfile::tempdir()
        .map_err(|error| Error::Internal(format!("failed to create benchmark root: {error}")))?;
    let engine = Arc::new(Engine::new(data_dir.path())?);
    let tenant_id = TenantId::new("materialized-verification".to_string())?;
    let table = TableName::new("documents")?;
    engine.create_tenant_async(tenant_id.clone()).await?;

    let payload = "x".repeat(payload_bytes);
    let mut document_ids = Vec::with_capacity(documents);
    for rank in 0..documents {
        document_ids.push(
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    table.clone(),
                    serde_json::Map::from_iter([
                        ("rank".to_string(), json!(rank)),
                        ("payload".to_string(), json!(payload)),
                    ]),
                )
                .await?,
        );
    }

    let churn_rungs: &[u32] = if quick {
        &CHURN_BASIS_POINTS[..1]
    } else {
        &CHURN_BASIS_POINTS
    };
    let mut measurements = Vec::with_capacity(churn_rungs.len());
    let mut churn_start = 0;
    for &churn_basis_points in churn_rungs {
        let churn_documents = documents
            .saturating_mul(churn_basis_points as usize)
            .div_ceil(10_000)
            .min(documents);
        for offset in 0..churn_documents {
            let index = (churn_start + offset) % documents;
            engine
                .update_document_async(
                    tenant_id.clone(),
                    table.clone(),
                    document_ids[index].clone(),
                    serde_json::Map::from_iter([(
                        "churn".to_string(),
                        json!({"basis_points": churn_basis_points, "rank": index}),
                    )]),
                )
                .await?;
        }
        churn_start = (churn_start + churn_documents) % documents;

        measurements.push(
            measure_full_verification(
                &engine,
                tenant_id.clone(),
                documents,
                payload_bytes,
                churn_basis_points,
            )
            .await?,
        );
    }
    Ok(measurements)
}

async fn measure_full_verification(
    engine: &Arc<Engine>,
    tenant_id: TenantId,
    documents: usize,
    payload_bytes: usize,
    churn_basis_points: u32,
) -> Result<FullVerificationMeasurement> {
    reset_allocation_counters();
    let cpu_before = process_cpu_time();
    let started = Instant::now();
    let report = engine.verify_consistency_async(tenant_id).await?;
    let elapsed = started.elapsed();
    let cpu_after = process_cpu_time();

    Ok(FullVerificationMeasurement {
        documents,
        payload_bytes,
        payload_state_bytes: (documents as u64).saturating_mul(payload_bytes as u64),
        churn_basis_points,
        elapsed_ns: elapsed.as_nanos(),
        process_cpu_ns: cpu_after.saturating_sub(cpu_before).as_nanos(),
        allocation_count: ALLOCATION_COUNT.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        peak_rss_bytes: peak_rss_bytes(),
        bytes_read: None,
        bytes_read_status: "UNVERIFIED: current verifier exposes no byte-read counter",
        report_ok: report.ok,
        mismatch_count: report.mismatches.len(),
        authoritative_document_count: report.authoritative.document_count,
    })
}

fn parse_arguments() -> Result<Arguments> {
    let mut output = None;
    let mut quick = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--bench" => {}
            "--quick" => quick = true,
            "--output" => {
                output = Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    Error::InvalidInput("--output requires a path".to_string())
                })?));
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "unknown materialized-verification argument: {argument}"
                )));
            }
        }
    }
    Ok(Arguments {
        output: output.ok_or_else(|| {
            Error::InvalidInput("materialized-verification requires --output <path>".to_string())
        })?,
        quick,
    })
}

fn write_report(path: &Path, report: &BenchmarkReport) -> Result<()> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("nimbus-engine must be two directories below the repository root")
            .join(path)
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create benchmark output directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    fs::write(&path, bytes).map_err(|error| {
        Error::Internal(format!(
            "failed to write benchmark report {}: {error}",
            path.display()
        ))
    })
}

fn reset_allocation_counters() {
    ALLOCATION_COUNT.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

#[cfg(unix)]
fn process_cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage on success.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return Duration::ZERO;
    }
    // SAFETY: status zero means getrusage initialized the value.
    let usage = unsafe { usage.assume_init() };
    timeval_duration(usage.ru_utime).saturating_add(timeval_duration(usage.ru_stime))
}

#[cfg(not(unix))]
fn process_cpu_time() -> Duration {
    Duration::ZERO
}

#[cfg(unix)]
fn timeval_duration(value: libc::timeval) -> Duration {
    Duration::from_secs(value.tv_sec.max(0) as u64)
        .saturating_add(Duration::from_micros(value.tv_usec.max(0) as u64))
}

#[cfg(target_os = "macos")]
fn peak_rss_bytes() -> Option<u64> {
    peak_rss_native().map(|value| value as u64)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn peak_rss_bytes() -> Option<u64> {
    peak_rss_native().map(|value| (value as u64).saturating_mul(1_024))
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(unix)]
fn peak_rss_native() -> Option<libc::c_long> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage on success.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    // SAFETY: status zero means getrusage initialized the value.
    Some(unsafe { usage.assume_init() }.ru_maxrss)
}
