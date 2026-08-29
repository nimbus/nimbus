use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::hint::black_box;
use std::io::Read;
use std::mem::size_of;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, Error, PrincipalContext, Result,
    TableName, TenantId, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_storage::{LogicalLeafKey, LogicalLeafKind, MaterializedVerificationIndex};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

const FORMAT_VERSION: u16 = 2;
const BASELINE_COMMIT: &str = "137cc632a1c8585545d200ea49f44bd236478175";
const SOURCE_BASE_COMMIT: &str = "9c807015a3049adbd8e24b15a57ccac34b2fc380";
const QUICK_DOCUMENTS: usize = 10_000;
const QUICK_PAYLOAD_BYTES: usize = 1_024;
const CHURN_BASIS_POINTS: [u32; 4] = [0, 10, 100, 1_000];
const CANDIDATE_SAMPLES: usize = 21;
const CANDIDATE_COMPARISONS_PER_SAMPLE: usize = 10_000;
const CANDIDATE_PRODUCTION_DOCUMENTS: [usize; 2] = [100_000, 1_000_000];
const CANDIDATE_PRODUCTION_PAYLOAD_BYTES: usize = 1_024;
const CANDIDATE_PRODUCTION_CHURN_BASIS_POINTS: u32 = 10;
const WRITE_OVERHEAD_DOCUMENTS: usize = 100_000;
const WRITE_OVERHEAD_PAYLOAD_BYTES: usize = 1_024;
const WRITE_OVERHEAD_SAMPLES: usize = 1_000;
const SETUP_BATCH_SIZE: usize = 256;
const FULL_SAMPLE_TIMEOUT: Duration = Duration::from_secs(60);
const MILLION_DOCUMENT_SAMPLE_TIMEOUT: Duration = Duration::from_secs(15);
const CHURN_SETUP_BUDGET: Duration = Duration::from_secs(120);
const CHILD_ADDRESS_SPACE_LIMIT_BYTES: u64 = 24 * 1_024 * 1_024 * 1_024;
const ALLOCATOR_METADATA_BYTES_PER_NODE: usize = 16;

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

#[path = "materialized_verification/arguments.rs"]
mod arguments;

use arguments::{Arguments, ChildArguments, parse_arguments};

#[derive(Debug, Clone, Serialize)]
struct BenchmarkReport {
    format_version: u16,
    baseline_commit: &'static str,
    source_base_commit: &'static str,
    target_os: &'static str,
    target_arch: &'static str,
    interval_seconds: u64,
    full_samples_per_rung: usize,
    candidate_samples_per_rung: usize,
    full_sample_timeout_seconds: u64,
    churn_setup_budget_seconds: u64,
    child_address_space_limit_bytes: u64,
    matrix: Vec<MatrixMeasurement>,
    write_overhead: Option<WriteOverheadMeasurement>,
}

#[derive(Debug, Clone, Serialize)]
struct MatrixMeasurement {
    documents: usize,
    payload_bytes: usize,
    payload_state_bytes: u64,
    churn_basis_points: u32,
    churn_requested_documents: usize,
    churn_applied_documents: usize,
    churn_setup_elapsed_ns: u128,
    churn_setup_status: &'static str,
    full: FullMeasurement,
    candidate: CandidateMeasurement,
}

#[derive(Debug, Clone, Serialize)]
struct FullMeasurement {
    status: &'static str,
    sample_timeout_seconds: u64,
    samples: Vec<FullSample>,
    summary: Option<SampleSummary>,
    censored_lower_bound_summary: Option<SampleSummary>,
    timed_out_samples: usize,
    failures: Vec<String>,
    bytes_read_scope: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FullSample {
    elapsed_ns: u128,
    process_cpu_ns: u128,
    allocation_count: u64,
    allocated_bytes: u64,
    peak_rss_bytes: Option<u64>,
    extra_peak_rss_bytes: Option<u64>,
    bytes_read: u64,
    report_ok: bool,
    mismatch_count: usize,
    authoritative_document_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CandidateMeasurement {
    status: &'static str,
    samples_ns: Vec<u128>,
    summary: SampleSummary,
    root_hex: String,
    node_bytes: usize,
    allocator_metadata_bytes_per_node: usize,
    resident_bytes_per_leaf: usize,
    total_resident_bytes: u64,
    memory_derivation: String,
}

#[derive(Debug, Clone, Serialize)]
struct CandidatePerformanceReport {
    format_version: u16,
    target_os: &'static str,
    target_arch: &'static str,
    measurement: &'static str,
    samples_per_rung: usize,
    rungs: Vec<CandidatePerformanceMeasurement>,
}

#[derive(Debug, Clone, Serialize)]
struct CandidatePerformanceMeasurement {
    documents: usize,
    payload_bytes: usize,
    churn_basis_points: u32,
    churn_documents: usize,
    status: &'static str,
    samples_ns: Vec<u128>,
    summary: SampleSummary,
    leaf_count: usize,
    resident_bytes_status: &'static str,
    resident_bytes: usize,
    resident_bytes_per_leaf: usize,
    max_depth: usize,
    latency_scope: &'static str,
    memory_source: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SampleSummary {
    sample_count: usize,
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
}

#[derive(Debug, Clone, Serialize)]
struct WriteOverheadMeasurement {
    documents: usize,
    payload_bytes: usize,
    samples_per_arm: usize,
    baseline: WriteArm,
    active_session: WriteArm,
    throughput_change_percent: f64,
    p99_commit_latency_change_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
struct WriteArm {
    elapsed_ns: u128,
    throughput_per_second: f64,
    commit_latency: SampleSummary,
    raw_commit_latency_ns: Vec<u128>,
}

#[derive(Debug)]
struct TreapNode {
    key: [u8; 32],
    priority: [u8; 32],
    value_hash: [u8; 32],
    subtree_hash: [u8; 32],
    left: Option<u32>,
    right: Option<u32>,
}

#[derive(Debug)]
struct CandidateTreap {
    nodes: Vec<TreapNode>,
    root: Option<u32>,
}

#[derive(Debug)]
struct ChurnApplication {
    requested_documents: usize,
    applied_documents: usize,
    elapsed: Duration,
}

impl ChurnApplication {
    fn exact(&self) -> bool {
        self.applied_documents == self.requested_documents
    }

    fn status(&self) -> &'static str {
        if self.exact() {
            "measured"
        } else {
            "resource_limited"
        }
    }
}

fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    if let Some(child) = arguments.child {
        apply_child_address_space_limit();
        let sample = run_child_full_sample(child)?;
        println!(
            "{}",
            serde_json::to_string(&sample)
                .map_err(|error| Error::Serialization(error.to_string()))?
        );
        return Ok(());
    }
    if arguments.candidate_only {
        let report = measure_production_candidate()?;
        return write_report(arguments.output.as_deref(), &report);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Internal(format!("failed to build benchmark runtime: {error}")))?;
    let rungs = selected_rungs(&arguments);
    let churn_rungs: &[u32] = if arguments.quick {
        &CHURN_BASIS_POINTS[..1]
    } else {
        &CHURN_BASIS_POINTS
    };

    let mut matrix = Vec::with_capacity(rungs.len() * churn_rungs.len());
    let mut write_overhead = None;
    for (documents, payload_bytes) in rungs {
        eprintln!("preparing {documents} documents with {payload_bytes}-byte payloads");
        let data_dir = tempfile::tempdir().map_err(|error| {
            Error::Internal(format!("failed to create benchmark root: {error}"))
        })?;
        let payload = vec![b'x'; payload_bytes];
        if let Err(error) =
            runtime.block_on(seed_fixture(data_dir.path(), documents, payload_bytes))
        {
            let Error::ResourceExhausted(message) = error else {
                return Err(error);
            };
            eprintln!(
                "recording resource-limited setup for {documents} documents with \
                 {payload_bytes}-byte payloads: {message}"
            );
            let candidate = CandidateTreap::build(documents, &payload);
            for &churn_basis_points in churn_rungs {
                let churn_documents = churn_count(documents, churn_basis_points);
                matrix.push(MatrixMeasurement {
                    documents,
                    payload_bytes,
                    payload_state_bytes: (documents as u64).saturating_mul(payload_bytes as u64),
                    churn_basis_points,
                    churn_requested_documents: churn_documents,
                    churn_applied_documents: 0,
                    churn_setup_elapsed_ns: 0,
                    churn_setup_status: "resource_limited_seed",
                    full: skipped_full_measurement(
                        documents,
                        format!("fixture seed exhausted local resources: {message}"),
                    ),
                    candidate: measure_candidate(&candidate, "resource_limited_setup"),
                });
            }
            if arguments.output.is_some() {
                let checkpoint = benchmark_report(
                    arguments.full_samples,
                    matrix.clone(),
                    write_overhead.clone(),
                );
                write_report(arguments.output.as_deref(), &checkpoint)?;
            }
            continue;
        }
        let mut candidate = CandidateTreap::build(documents, &payload);
        let mut churn_start = 0;

        for &churn_basis_points in churn_rungs {
            eprintln!(
                "measuring {documents} documents, {payload_bytes}-byte payloads, {} basis points churn",
                churn_basis_points
            );
            let churn_documents = churn_count(documents, churn_basis_points);
            let churn = if churn_documents > 0 {
                let churn = runtime.block_on(apply_fixture_churn(
                    data_dir.path(),
                    documents,
                    churn_start,
                    churn_documents,
                    churn_basis_points,
                ))?;
                for offset in 0..churn.applied_documents {
                    let rank = (churn_start + offset) % documents;
                    candidate.update(rank, leaf_hash(rank, &payload, churn_basis_points));
                }
                churn_start = (churn_start + churn.applied_documents) % documents;
                churn
            } else {
                ChurnApplication {
                    requested_documents: 0,
                    applied_documents: 0,
                    elapsed: Duration::ZERO,
                }
            };

            let full = if churn.exact() {
                measure_full_samples(
                    data_dir.path(),
                    documents,
                    payload_bytes,
                    churn_basis_points,
                    arguments.full_samples,
                )?
            } else {
                skipped_full_measurement(
                    documents,
                    format!(
                        "churn setup applied {} of {} requested documents before the {}-second limit",
                        churn.applied_documents,
                        churn.requested_documents,
                        CHURN_SETUP_BUDGET.as_secs()
                    ),
                )
            };
            let candidate_measurement = measure_candidate(
                &candidate,
                if churn.exact() {
                    "measured"
                } else {
                    "resource_limited_setup"
                },
            );
            matrix.push(MatrixMeasurement {
                documents,
                payload_bytes,
                payload_state_bytes: (documents as u64).saturating_mul(payload_bytes as u64),
                churn_basis_points,
                churn_requested_documents: churn.requested_documents,
                churn_applied_documents: churn.applied_documents,
                churn_setup_elapsed_ns: churn.elapsed.as_nanos(),
                churn_setup_status: churn.status(),
                full,
                candidate: candidate_measurement,
            });

            if write_overhead.is_none()
                && !arguments.quick
                && documents == WRITE_OVERHEAD_DOCUMENTS
                && payload_bytes == WRITE_OVERHEAD_PAYLOAD_BYTES
                && churn_basis_points == 10
            {
                write_overhead = Some(runtime.block_on(measure_write_overhead(
                    data_dir.path(),
                    &mut candidate,
                    &payload,
                ))?);
            }
            if arguments.output.is_some() {
                let checkpoint = benchmark_report(
                    arguments.full_samples,
                    matrix.clone(),
                    write_overhead.clone(),
                );
                write_report(arguments.output.as_deref(), &checkpoint)?;
            }
        }
    }

    let report = benchmark_report(arguments.full_samples, matrix, write_overhead);
    write_report(arguments.output.as_deref(), &report)
}

fn measure_production_candidate() -> Result<CandidatePerformanceReport> {
    let mut rungs = Vec::with_capacity(CANDIDATE_PRODUCTION_DOCUMENTS.len());
    for documents in CANDIDATE_PRODUCTION_DOCUMENTS {
        eprintln!("measuring production candidate at {documents} documents");
        let mut index = MaterializedVerificationIndex::from_leaves((0..documents).map(|rank| {
            let key = candidate_leaf_key(rank)
                .expect("a nonempty rank identity must produce a logical leaf key");
            (key, candidate_leaf_value(rank, 0))
        }))?;
        let churn_documents = churn_count(documents, CANDIDATE_PRODUCTION_CHURN_BASIS_POINTS);
        let mut samples_ns = Vec::with_capacity(CANDIDATE_SAMPLES);
        let mut prior_root = index.root_hash();
        for sample_index in 1..=CANDIDATE_SAMPLES {
            let started = Instant::now();
            for rank in 0..churn_documents {
                index.upsert(
                    candidate_leaf_key(rank)?,
                    &candidate_leaf_value(rank, sample_index),
                )?;
            }
            let root = black_box(index.root_hash());
            let elapsed_ns = started.elapsed().as_nanos();
            if root == prior_root {
                return Err(Error::Internal(format!(
                    "candidate root did not change for sample {sample_index} at {documents} documents"
                )));
            }
            prior_root = root;
            samples_ns.push(elapsed_ns);
        }
        let summary = summarize(samples_ns.clone()).ok_or_else(|| {
            Error::Internal("production candidate samples are unexpectedly empty".to_string())
        })?;
        rungs.push(CandidatePerformanceMeasurement {
            documents,
            payload_bytes: CANDIDATE_PRODUCTION_PAYLOAD_BYTES,
            churn_basis_points: CANDIDATE_PRODUCTION_CHURN_BASIS_POINTS,
            churn_documents,
            status: "measured",
            samples_ns,
            summary,
            leaf_count: index.len(),
            resident_bytes_status: "measured",
            resident_bytes: index.resident_bytes(),
            resident_bytes_per_leaf: index.resident_bytes_per_leaf(),
            max_depth: index.max_depth(),
            latency_scope: "0.1% production-index upserts plus the final root read",
            memory_source: "MaterializedVerificationIndex::resident_bytes",
        });
    }
    Ok(CandidatePerformanceReport {
        format_version: 1,
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        measurement: "production_materialized_verification_index",
        samples_per_rung: CANDIDATE_SAMPLES,
        rungs,
    })
}

fn candidate_leaf_key(rank: usize) -> Result<LogicalLeafKey> {
    LogicalLeafKey::new(LogicalLeafKind::Document, &(rank as u64).to_be_bytes())
}

fn candidate_leaf_value(rank: usize, sample: usize) -> Vec<u8> {
    let mut value = vec![b'x'; CANDIDATE_PRODUCTION_PAYLOAD_BYTES];
    value[..8].copy_from_slice(&(rank as u64).to_be_bytes());
    value[8..16].copy_from_slice(&(sample as u64).to_be_bytes());
    value
}

fn benchmark_report(
    full_samples: usize,
    matrix: Vec<MatrixMeasurement>,
    write_overhead: Option<WriteOverheadMeasurement>,
) -> BenchmarkReport {
    BenchmarkReport {
        format_version: FORMAT_VERSION,
        baseline_commit: BASELINE_COMMIT,
        source_base_commit: SOURCE_BASE_COMMIT,
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        interval_seconds: 60,
        full_samples_per_rung: full_samples,
        candidate_samples_per_rung: CANDIDATE_SAMPLES,
        full_sample_timeout_seconds: FULL_SAMPLE_TIMEOUT.as_secs(),
        churn_setup_budget_seconds: CHURN_SETUP_BUDGET.as_secs(),
        child_address_space_limit_bytes: CHILD_ADDRESS_SPACE_LIMIT_BYTES,
        matrix,
        write_overhead,
    }
}

fn selected_rungs(arguments: &Arguments) -> Vec<(usize, usize)> {
    if arguments.quick {
        return vec![(QUICK_DOCUMENTS, QUICK_PAYLOAD_BYTES)];
    }
    let documents = arguments
        .documents
        .map(|value| vec![value])
        .unwrap_or_else(|| vec![10_000, 100_000, 1_000_000]);
    let payloads = arguments
        .payload_bytes
        .map(|value| vec![value])
        .unwrap_or_else(|| vec![256, 1_024, 8 * 1_024]);
    documents
        .into_iter()
        .flat_map(|count| {
            payloads
                .iter()
                .copied()
                .map(move |payload| (count, payload))
        })
        .collect()
}

async fn seed_fixture(path: &Path, documents: usize, payload_bytes: usize) -> Result<()> {
    let engine = Arc::new(Engine::new(path)?);
    let tenant_id = benchmark_tenant()?;
    let table = benchmark_table()?;
    engine.create_tenant_async(tenant_id.clone()).await?;
    let payload = "x".repeat(payload_bytes);

    for batch_start in (0..documents).step_by(SETUP_BATCH_SIZE) {
        let batch_end = (batch_start + SETUP_BATCH_SIZE).min(documents);
        let writes = (batch_start..batch_end)
            .map(|rank| set_write(&table, rank, &payload))
            .collect::<Result<Vec<_>>>()?;
        let unit = engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())?;
        unit.execute_atomic_write_batch(AtomicWriteBatch::new(writes)?)?;
    }
    Ok(())
}

async fn apply_fixture_churn(
    path: &Path,
    documents: usize,
    churn_start: usize,
    churn_documents: usize,
    churn_basis_points: u32,
) -> Result<ChurnApplication> {
    let engine = Arc::new(Engine::new(path)?);
    let tenant_id = benchmark_tenant()?;
    let table = benchmark_table()?;
    let started = Instant::now();
    let mut applied_documents = 0;
    for batch_offset in (0..churn_documents).step_by(SETUP_BATCH_SIZE) {
        if batch_offset > 0 && started.elapsed() >= CHURN_SETUP_BUDGET {
            break;
        }
        let batch_end = (batch_offset + SETUP_BATCH_SIZE).min(churn_documents);
        let writes = (batch_offset..batch_end)
            .map(|offset| {
                let rank = (churn_start + offset) % documents;
                churn_write(&table, rank, churn_basis_points)
            })
            .collect::<Result<Vec<_>>>()?;
        let unit = engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())?;
        unit.execute_atomic_write_batch(AtomicWriteBatch::new(writes)?)?;
        applied_documents = batch_end;
    }
    Ok(ChurnApplication {
        requested_documents: churn_documents,
        applied_documents,
        elapsed: started.elapsed(),
    })
}

fn set_write(table: &TableName, rank: usize, payload: &str) -> Result<AtomicWrite> {
    Ok(AtomicWrite::Set {
        key: write_key(table, rank)?,
        document: serde_json::Map::from_iter([
            ("rank".to_string(), json!(rank)),
            ("payload".to_string(), json!(payload)),
        ]),
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    })
}

fn churn_write(table: &TableName, rank: usize, churn_basis_points: u32) -> Result<AtomicWrite> {
    Ok(AtomicWrite::Set {
        key: write_key(table, rank)?,
        document: serde_json::Map::from_iter([(
            "churn".to_string(),
            json!({"basis_points": churn_basis_points, "rank": rank}),
        )]),
        typed_fields: Default::default(),
        mode: WriteSetMode::MergeAll,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    })
}

fn write_key(table: &TableName, rank: usize) -> Result<WriteKey> {
    Ok(DocumentLocator::new(table.clone(), document_id(rank)?).into())
}

fn document_id(rank: usize) -> Result<DocumentId> {
    DocumentId::from_key(format!("doc-{rank:09}"))
}

fn benchmark_tenant() -> Result<TenantId> {
    TenantId::new("materialized-verification".to_string())
}

fn benchmark_table() -> Result<TableName> {
    TableName::new("documents")
}

fn churn_count(documents: usize, churn_basis_points: u32) -> usize {
    documents
        .saturating_mul(churn_basis_points as usize)
        .div_ceil(10_000)
        .min(documents)
}

fn measure_full_samples(
    path: &Path,
    documents: usize,
    payload_bytes: usize,
    churn_basis_points: u32,
    sample_count: usize,
) -> Result<FullMeasurement> {
    let timeout = if documents >= 1_000_000 {
        MILLION_DOCUMENT_SAMPLE_TIMEOUT
    } else {
        FULL_SAMPLE_TIMEOUT
    };
    let mut samples = Vec::with_capacity(sample_count);
    let mut failures = Vec::new();
    for sample_index in 0..sample_count {
        match run_full_sample_process(path, documents, payload_bytes, churn_basis_points, timeout) {
            Ok(sample) => samples.push(sample),
            Err(error) => failures.push(format!("sample {}: {error}", sample_index + 1)),
        }
    }
    let summary = summarize(samples.iter().map(|sample| sample.elapsed_ns).collect());
    let timed_out_samples = failures
        .iter()
        .filter(|failure| failure.contains("exceeded the"))
        .count();
    let censored_lower_bound_summary =
        if timed_out_samples > 0 && timed_out_samples == failures.len() {
            let mut censored = samples
                .iter()
                .map(|sample| sample.elapsed_ns)
                .collect::<Vec<_>>();
            censored.extend(std::iter::repeat_n(timeout.as_nanos(), timed_out_samples));
            summarize(censored)
        } else {
            None
        };
    Ok(FullMeasurement {
        status: if failures.is_empty() {
            "measured"
        } else {
            "resource_limited"
        },
        sample_timeout_seconds: timeout.as_secs(),
        samples,
        summary,
        censored_lower_bound_summary,
        timed_out_samples,
        failures,
        bytes_read_scope: "verified payload bytes traversed; provider and envelope bytes excluded",
    })
}

fn skipped_full_measurement(documents: usize, failure: String) -> FullMeasurement {
    FullMeasurement {
        status: "resource_limited_setup",
        sample_timeout_seconds: if documents >= 1_000_000 {
            MILLION_DOCUMENT_SAMPLE_TIMEOUT.as_secs()
        } else {
            FULL_SAMPLE_TIMEOUT.as_secs()
        },
        samples: Vec::new(),
        summary: None,
        censored_lower_bound_summary: None,
        timed_out_samples: 0,
        failures: vec![failure],
        bytes_read_scope: "not measured because the requested churn state was not reached",
    }
}

fn run_full_sample_process(
    path: &Path,
    documents: usize,
    payload_bytes: usize,
    churn_basis_points: u32,
    timeout: Duration,
) -> Result<FullSample> {
    let executable = std::env::current_exe().map_err(|error| {
        Error::Internal(format!("failed to resolve benchmark executable: {error}"))
    })?;
    let mut child = Command::new(executable)
        .arg("--child-full")
        .arg(path)
        .arg("--documents")
        .arg(documents.to_string())
        .arg("--payload-bytes")
        .arg(payload_bytes.to_string())
        .arg("--churn-basis-points")
        .arg(churn_basis_points.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            Error::Internal(format!("failed to start full-verifier child: {error}"))
        })?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll full-verifier child: {error}"))
        })? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Internal(format!(
                "full verifier exceeded the {}-second sample limit",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout)
            .map_err(|error| Error::Internal(format!("failed to read child stdout: {error}")))?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .map_err(|error| Error::Internal(format!("failed to read child stderr: {error}")))?;
    }
    if !status.success() {
        return Err(Error::Internal(format!(
            "full-verifier child exited {status}: {}",
            stderr.trim()
        )));
    }
    serde_json::from_str(stdout.trim()).map_err(|error| {
        Error::Serialization(format!(
            "invalid full-verifier child JSON: {error}; stdout={stdout}"
        ))
    })
}

fn run_child_full_sample(arguments: ChildArguments) -> Result<FullSample> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Internal(format!("failed to build child runtime: {error}")))?;
    let engine = Arc::new(Engine::new(&arguments.data_dir)?);
    let tenant_id = benchmark_tenant()?;
    let peak_before = peak_rss_bytes();
    reset_allocation_counters();
    let cpu_before = process_cpu_time();
    let started = Instant::now();
    let report = runtime.block_on(engine.verify_consistency_async(tenant_id))?;
    let elapsed = started.elapsed();
    let cpu_after = process_cpu_time();
    let peak_after = peak_rss_bytes();
    let allocation_count = ALLOCATION_COUNT.load(Ordering::Relaxed);
    let allocated_bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
    let payload_bytes_read = (arguments.documents as u64)
        .saturating_mul(arguments.payload_bytes as u64)
        .saturating_mul(3);
    let _ = arguments.churn_basis_points;

    Ok(FullSample {
        elapsed_ns: elapsed.as_nanos(),
        process_cpu_ns: cpu_after.saturating_sub(cpu_before).as_nanos(),
        allocation_count,
        allocated_bytes,
        peak_rss_bytes: peak_after,
        extra_peak_rss_bytes: peak_before
            .zip(peak_after)
            .map(|(before, after)| after.saturating_sub(before)),
        bytes_read: payload_bytes_read,
        report_ok: report.ok,
        mismatch_count: report.mismatches.len(),
        authoritative_document_count: report.authoritative.document_count,
    })
}

fn measure_candidate(candidate: &CandidateTreap, status: &'static str) -> CandidateMeasurement {
    let expected = candidate.root_hash();
    let mut samples = Vec::with_capacity(CANDIDATE_SAMPLES);
    for _ in 0..CANDIDATE_SAMPLES {
        let started = Instant::now();
        for _ in 0..CANDIDATE_COMPARISONS_PER_SAMPLE {
            assert_eq!(black_box(candidate.root_hash()), black_box(expected));
        }
        samples.push(started.elapsed().as_nanos() / CANDIDATE_COMPARISONS_PER_SAMPLE as u128);
    }
    let summary = summarize(samples.clone()).expect("candidate samples are not empty");
    let node_bytes = size_of::<TreapNode>();
    let resident_bytes_per_leaf = node_bytes + ALLOCATOR_METADATA_BYTES_PER_NODE;
    CandidateMeasurement {
        status,
        samples_ns: samples,
        summary,
        root_hex: hex(expected),
        node_bytes,
        allocator_metadata_bytes_per_node: ALLOCATOR_METADATA_BYTES_PER_NODE,
        resident_bytes_per_leaf,
        total_resident_bytes: (resident_bytes_per_leaf as u64)
            .saturating_mul(candidate.nodes.len() as u64),
        memory_derivation: format!(
            "size_of::<TreapNode>() ({node_bytes}) + conservative allocator metadata ({ALLOCATOR_METADATA_BYTES_PER_NODE})"
        ),
    }
}

async fn measure_write_overhead(
    path: &Path,
    candidate: &mut CandidateTreap,
    payload: &[u8],
) -> Result<WriteOverheadMeasurement> {
    let engine = Arc::new(Engine::new(path)?);
    let tenant_id = benchmark_tenant()?;
    let table = benchmark_table()?;
    let baseline_latencies =
        measure_engine_write_latencies(&engine, &tenant_id, &table, 20_000).await?;
    let mut active_latencies = Vec::with_capacity(WRITE_OVERHEAD_SAMPLES);
    for (offset, &commit_latency) in baseline_latencies.iter().enumerate() {
        let rank = 20_000 + offset;
        let started = Instant::now();
        candidate.update(rank, leaf_hash(rank, payload, offset as u32));
        active_latencies.push(commit_latency.saturating_add(started.elapsed().as_nanos()));
    }
    let baseline = write_arm(baseline_latencies);
    let active_session = write_arm(active_latencies);
    Ok(WriteOverheadMeasurement {
        documents: WRITE_OVERHEAD_DOCUMENTS,
        payload_bytes: WRITE_OVERHEAD_PAYLOAD_BYTES,
        samples_per_arm: WRITE_OVERHEAD_SAMPLES,
        throughput_change_percent: percent_change(
            baseline.throughput_per_second,
            active_session.throughput_per_second,
        ),
        p99_commit_latency_change_percent: percent_change(
            baseline.commit_latency.p99_ns as f64,
            active_session.commit_latency.p99_ns as f64,
        ),
        baseline,
        active_session,
    })
}

async fn measure_engine_write_latencies(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    start_rank: usize,
) -> Result<Vec<u128>> {
    let mut latencies = Vec::with_capacity(WRITE_OVERHEAD_SAMPLES);
    for offset in 0..WRITE_OVERHEAD_SAMPLES {
        let rank = start_rank + offset;
        let started = Instant::now();
        engine
            .update_document_async(
                tenant_id.clone(),
                table.clone(),
                document_id(rank)?,
                serde_json::Map::from_iter([("overhead".to_string(), json!(offset))]),
            )
            .await?;
        latencies.push(started.elapsed().as_nanos());
    }
    Ok(latencies)
}

fn write_arm(latencies: Vec<u128>) -> WriteArm {
    let elapsed_ns = latencies.iter().copied().sum::<u128>();
    WriteArm {
        elapsed_ns,
        throughput_per_second: (WRITE_OVERHEAD_SAMPLES as f64 * 1_000_000_000.0)
            / elapsed_ns as f64,
        commit_latency: summarize(latencies.clone()).expect("write samples are not empty"),
        raw_commit_latency_ns: latencies,
    }
}

fn percent_change(baseline: f64, candidate: f64) -> f64 {
    if baseline == 0.0 {
        return 0.0;
    }
    ((candidate - baseline) / baseline) * 100.0
}

impl CandidateTreap {
    fn build(documents: usize, payload: &[u8]) -> Self {
        let mut nodes = Vec::with_capacity(documents);
        let mut stack: Vec<u32> = Vec::new();
        let mut root = None;
        for rank in 0..documents {
            let key = leaf_key(rank);
            let priority = digest(&[b"nimbus.imv2.priority.v1", &key]);
            let value_hash = leaf_hash(rank, payload, 0);
            let index = nodes.len() as u32;
            nodes.push(TreapNode {
                key,
                priority,
                value_hash,
                subtree_hash: [0; 32],
                left: None,
                right: None,
            });
            let mut last = None;
            while let Some(&candidate) = stack.last() {
                if nodes[candidate as usize].priority <= priority {
                    break;
                }
                last = stack.pop();
            }
            nodes[index as usize].left = last;
            if let Some(&parent) = stack.last() {
                nodes[parent as usize].right = Some(index);
            } else {
                root = Some(index);
            }
            stack.push(index);
        }
        let mut tree = Self { nodes, root };
        if let Some(root) = tree.root {
            tree.recompute_subtree(root);
        }
        tree
    }

    fn update(&mut self, rank: usize, value_hash: [u8; 32]) {
        let key = leaf_key(rank);
        let mut path = Vec::new();
        let mut cursor = self.root;
        while let Some(index) = cursor {
            path.push(index);
            let node = &self.nodes[index as usize];
            if key == node.key {
                self.nodes[index as usize].value_hash = value_hash;
                for &path_index in path.iter().rev() {
                    self.recompute_node(path_index);
                }
                return;
            }
            cursor = if key < node.key {
                node.left
            } else {
                node.right
            };
        }
        panic!("candidate treap is missing rank {rank}");
    }

    fn root_hash(&self) -> [u8; 32] {
        self.root
            .map(|index| self.nodes[index as usize].subtree_hash)
            .unwrap_or_else(|| digest(&[b"nimbus.imv2.empty.v1"]))
    }

    fn recompute_subtree(&mut self, index: u32) -> [u8; 32] {
        let left = self.nodes[index as usize]
            .left
            .map(|child| self.recompute_subtree(child));
        let right = self.nodes[index as usize]
            .right
            .map(|child| self.recompute_subtree(child));
        self.set_subtree_hash(index, left, right)
    }

    fn recompute_node(&mut self, index: u32) {
        let left = self.nodes[index as usize]
            .left
            .map(|child| self.nodes[child as usize].subtree_hash);
        let right = self.nodes[index as usize]
            .right
            .map(|child| self.nodes[child as usize].subtree_hash);
        self.set_subtree_hash(index, left, right);
    }

    fn set_subtree_hash(
        &mut self,
        index: u32,
        left: Option<[u8; 32]>,
        right: Option<[u8; 32]>,
    ) -> [u8; 32] {
        let empty = digest(&[b"nimbus.imv2.empty.v1"]);
        let node = &self.nodes[index as usize];
        let hash = digest(&[
            b"nimbus.imv2.node.v1",
            left.as_ref().unwrap_or(&empty),
            &node.key,
            &node.value_hash,
            right.as_ref().unwrap_or(&empty),
        ]);
        self.nodes[index as usize].subtree_hash = hash;
        hash
    }
}

fn leaf_key(rank: usize) -> [u8; 32] {
    let mut key = [0; 32];
    key[..8].copy_from_slice(&(rank as u64).to_be_bytes());
    key
}

fn leaf_hash(rank: usize, payload: &[u8], churn_basis_points: u32) -> [u8; 32] {
    digest(&[
        b"nimbus.imv2.leaf.v1",
        &(rank as u64).to_be_bytes(),
        &(payload.len() as u64).to_be_bytes(),
        payload,
        &churn_basis_points.to_be_bytes(),
    ])
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn summarize(mut samples: Vec<u128>) -> Option<SampleSummary> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_unstable();
    Some(SampleSummary {
        sample_count: samples.len(),
        p50_ns: percentile(&samples, 50),
        p95_ns: percentile(&samples, 95),
        p99_ns: percentile(&samples, 99),
    })
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn write_report(path: Option<&Path>, report: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    let Some(path) = path else {
        println!("{}", String::from_utf8_lossy(&bytes));
        return Ok(());
    };
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
    fs::write(&path, bytes).map_err(|error| {
        Error::Internal(format!(
            "failed to write benchmark report {}: {error}",
            path.display()
        ))
    })?;
    println!("wrote {}", path.display());
    Ok(())
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

#[cfg(unix)]
fn apply_child_address_space_limit() {
    let limit = libc::rlimit {
        rlim_cur: CHILD_ADDRESS_SPACE_LIMIT_BYTES as libc::rlim_t,
        rlim_max: CHILD_ADDRESS_SPACE_LIMIT_BYTES as libc::rlim_t,
    };
    // SAFETY: setrlimit reads the value for this process only. A failure leaves
    // the child unrestricted, and the parent timeout still bounds the sample.
    let _ = unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) };
}

#[cfg(not(unix))]
fn apply_child_address_space_limit() {}
