//! Concurrent write-throughput benchmark — measures the group-commit ceiling.
//!
//! WHY THIS EXISTS. The embedded-provider CRUD benchmark issues one mutation at
//! a time (await, then issue the next), so the per-tenant journal worker only
//! ever has a batch of 1 to commit — one fsync per write. That measures per-op
//! durability latency, NOT the group-commit throughput, because Nimbus's journal
//! worker coalesces up to `MUTATION_JOURNAL_BATCH_SIZE` (32) concurrently-queued
//! mutations into a single fsync. To see that benefit you must present the
//! worker with concurrent in-flight mutations.
//!
//! METHODOLOGY (canonical closed-loop concurrency sweep):
//!   * CLOSED-LOOP load: a fixed pool of N async workers, each in a tight loop
//!     {insert -> await durable ack -> repeat}, zero think time. In-flight
//!     concurrency is pinned at N by construction — exactly the batch-formation
//!     condition we want to characterize.
//!   * GEOMETRIC LADDER over N (1,2,4,...,256) — we sweep to find the knee/peak
//!     of the throughput-vs-concurrency curve rather than betting on one N.
//!   * SINGLE TENANT: group commit coalesces PER TENANT, so all load drives one
//!     journal worker. (A cross-tenant sweep is a different experiment.)
//!   * N=1 IS THE SEQUENTIAL ANCHOR. The N=1 rung is, by definition, this
//!     harness's one-op-at-a-time sequential path (batch size 1). Every higher-N
//!     result is a speedup S(N)=X(N)/X(1) relative to it — which is what makes
//!     the group-commit payoff a valid, workload-independent multiple. The
//!     default CRUD workload replays the sequential CRUD baseline's shape
//!     (schemaless "tasks" table, phased insert/update/delete over 300 docs,
//!     same fields, no pre-seed), so N=1 should land NEAR the published ~2,661
//!     mutations/s figure as a cross-check — not bit-identical (separate
//!     harness, machine state), so treat a modest difference as expected.
//!   * LITTLE'S LAW check per rung: N ~= X * R (concurrency = throughput x mean
//!     latency). A rung where this fails by >~10% signals a measurement bug.
//!   * STATISTICS match the house style: warmup rounds discarded, then R
//!     measured rounds; report mean + median throughput, a two-sided Student-t
//!     95% CI on the round means, and the coefficient of variation (CV). CV
//!     above ~10% means the environment is too noisy to trust — fix it first.
//!
//! HONEST CAVEAT ON LATENCY. The p50/p95/p99 reported here are CLOSED-LOOP
//! (queue) latencies. At saturated rungs they suffer coordinated omission and
//! are NOT service-latency SLAs — read them as "how long a client waits at this
//! concurrency," not as the engine's service time. A faithful SLA-latency number
//! needs a separate open-loop / constant-rate run below Cmax (follow-up).
//!
//! Effective batch size (ops/fsync) is measured directly from the journal
//! worker's batch counters when `NIMBUS_CWB_SPLIT_PHASES=1`.
//!
//! Env overrides (all optional):
//!   NIMBUS_CWB_WORKLOAD=crud|insert|hotkey    unit = CRUD (default), insert, or one shared-doc update
//!   NIMBUS_CWB_LADDER=1,2,4,8,...              concurrency ladder (N=1 always forced in)
//!   NIMBUS_CWB_OPS_PER_WORKER=300              base work units/worker per round (300 = baseline docs)
//!   NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=24000   per-round mutation cap (bounds high-N runtime)
//!   NIMBUS_CWB_MEASURE_ROUNDS=10               measured rounds per rung
//!   NIMBUS_CWB_WARMUP_ROUNDS=2                 discarded warmup rounds per rung
//!   NIMBUS_CWB_SEED_DOCS=0                      pre-seed docs (0 = match baseline; >0 pre-ages the store)
//!   NIMBUS_CWB_BACKEND=sqlite|redb             embedded backend (default sqlite)
//!   NIMBUS_CWB_SPLIT_PHASES=1                   add plan-CPU/apply/fsync phase shares (default off)
//!   NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION=1     add SQLite checkpoint diagnostics (default off)
//!   NIMBUS_CWB_OUT=<path>                      also write the markdown report to <path>
//!
//! Open-loop mode (SUC6.1) — coordinated-omission-free service latency:
//!   NIMBUS_CWB_OPEN_LOOP_RATES=0.5,0.75        run open-loop after the ladder; each value is a
//!                                              fraction of the top rung's measured closed-loop
//!                                              capacity, used as a constant arrival rate
//!   NIMBUS_CWB_OPEN_LOOP_SECONDS=30            duration of each open-loop round
//!   NIMBUS_CWB_OPEN_LOOP_ROUNDS=3              measured rounds per rate (CV gate applies)
//!
//! Open-loop rounds drive single-document inserts on a fixed arrival schedule
//! (arrival_i = start + i/rate) and measure each latency FROM THE SCHEDULED
//! ARRIVAL, not from dispatch — a slow engine cannot slow the arrival process,
//! so percentiles are service-latency SLAs, unlike the closed-loop queue
//! latencies above. A round aborts as saturation-breached if in-flight work
//! exceeds a bound, rather than reporting numbers from an unstable regime.

use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nimbus_core::{DocumentId, Retryability, TableName, TenantId};
use nimbus_engine::{CommitPhaseMetricsSnapshot, EmbeddedProviderKind, Engine};
use nimbus_storage::{
    SqlitePassiveCheckpointProbe, SqliteWalCheckpointObservationSnapshot,
    disable_sqlite_wal_checkpoint_observation, probe_sqlite_passive_checkpoint,
    reset_sqlite_wal_checkpoint_observation, sqlite_wal_checkpoint_observation_snapshot,
};
use serde_json::json;
use tokio::task::JoinSet;

#[path = "support/concurrent_write_phase_split.rs"]
mod concurrent_write_phase_split;

use concurrent_write_phase_split::{PhaseSplit, PhaseTotals, render_phase_split_section};

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn ladder() -> Vec<usize> {
    let mut levels = match std::env::var("NIMBUS_CWB_LADDER") {
        Ok(raw) => {
            let parsed: Vec<usize> = raw
                .split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .filter(|n| *n > 0)
                .collect();
            if parsed.is_empty() {
                default_ladder()
            } else {
                parsed
            }
        }
        Err(_) => default_ladder(),
    };
    // The N=1 rung is the MANDATORY sequential anchor: every speedup is reported
    // relative to it, so it must always be measured and must be the smallest N.
    // Force it in, then sort ascending + dedup so the anchor is first and a
    // custom (possibly unsorted, N=1-less) ladder can never silently baseline
    // the speedup on some N>1 rung.
    levels.push(1);
    levels.sort_unstable();
    levels.dedup();
    levels
}

fn default_ladder() -> Vec<usize> {
    // Geometric, densified around the 32-coalesce cap where the knee is expected.
    vec![1, 2, 4, 8, 16, 24, 32, 48, 64, 96, 128, 192, 256]
}

fn backend() -> EmbeddedProviderKind {
    match std::env::var("NIMBUS_CWB_BACKEND")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "redb" => EmbeddedProviderKind::Redb,
        _ => EmbeddedProviderKind::Sqlite,
    }
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
}

fn split_phases_enabled() -> bool {
    env_flag("NIMBUS_CWB_SPLIT_PHASES")
}

fn wal_checkpoint_observation_enabled() -> bool {
    env_flag("NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION")
}

/// Which mutation shape one "unit" of work is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Workload {
    /// One durable insert per unit.
    Insert,
    /// insert + update + delete per unit (3 durable mutations), run PHASED per
    /// worker — the same shape and fields as the sequential CRUD baseline, so N=1
    /// cross-checks against the published ~2,661 mutations/s figure.
    Crud,
    /// One durable update per unit, with every worker contending on the same
    /// document. This isolates the stale-prepare wait/retry path.
    HotKey,
}

impl Workload {
    fn mutations_per_unit(self) -> usize {
        match self {
            Workload::Insert => 1,
            Workload::Crud => 3,
            Workload::HotKey => 1,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Workload::Insert => "insert",
            Workload::Crud => "crud (insert+update+delete)",
            Workload::HotKey => "hotkey (one shared-document update)",
        }
    }
}

fn workload() -> Workload {
    match std::env::var("NIMBUS_CWB_WORKLOAD")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "insert" => Workload::Insert,
        "hotkey" => Workload::HotKey,
        _ => Workload::Crud,
    }
}

fn insert_fields(unit: usize) -> serde_json::Map<String, serde_json::Value> {
    // Exactly the sequential CRUD baseline's insert shape (exercise_crud_sample):
    // {status, rank, title} on a schemaless "tasks" table — no extra fields.
    serde_json::Map::from_iter([
        ("status".to_string(), json!("open")),
        ("rank".to_string(), json!(unit)),
        ("title".to_string(), json!(format!("task-{unit:05}"))),
    ])
}

fn patch_fields(unit: usize) -> serde_json::Map<String, serde_json::Value> {
    // Baseline's update patch: {rank: rank + CRUD_DOCUMENTS} (CRUD_DOCUMENTS = 300).
    serde_json::Map::from_iter([("rank".to_string(), json!(unit + 300))])
}

async fn update_hot_key_until_committed(
    engine: &Arc<Engine>,
    tenant: &TenantId,
    table: &TableName,
    document_id: &DocumentId,
    unit: usize,
) {
    loop {
        let result = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let tenant = tenant.clone();
            let table = table.clone();
            let document_id = document_id.clone();
            move || engine.update_document(&tenant, table, document_id, patch_fields(unit))
        })
        .await
        .expect("hot-key blocking update task should join");
        match result {
            Ok(_) => return,
            Err(error) if error.retryability() == Retryability::Retryable => {
                // The regular API deliberately caps one call's OCC attempts.
                // Hot-key saturation can exceed that cap while still making
                // progress, so the workload's client retries and charges the
                // entire wait/reprepare interval to this mutation's latency.
                tokio::task::yield_now().await;
            }
            Err(error) if error.retryability() == Retryability::RetryableAfterBackoff => {
                // N=256 intentionally exceeds the bounded committer inbox and
                // exercises overload recovery. Treat that expected pressure as
                // client-visible wait, honoring an explicit retry delay when
                // the error carries one and otherwise avoiding a busy loop.
                tokio::time::sleep(
                    error
                        .retry_after()
                        .unwrap_or_else(|| Duration::from_millis(1)),
                )
                .await;
            }
            Err(error) => panic!("hot-key update_document_async should succeed: {error}"),
        }
    }
}

/// One measured concurrency rung.
struct Rung {
    n: usize,
    throughputs: Vec<f64>, // per-round ops/sec (measured rounds only)
    latencies_ns: Vec<u64>,
    phase_split: Option<PhaseSplit>,
    wal_checkpoint_observation: Option<WalCheckpointObservation>,
}

struct RungStats {
    n: usize,
    raw_throughputs: Vec<f64>,
    mean_tps: f64,
    median_tps: f64,
    ci95_low: f64,
    ci95_high: f64,
    cv_percent: f64,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    mean_latency_s: f64,
    phase_split: Option<PhaseSplit>,
    wal_checkpoint_observation: Option<WalCheckpointObservation>,
}

#[derive(Clone, Copy)]
struct WalCheckpointObservation {
    foreground: SqliteWalCheckpointObservationSnapshot,
    passive: SqlitePassiveCheckpointProbe,
    measured_round_nanos: u64,
}

/// Perform one closed-loop round: `n` workers each perform `units_per_worker`
/// work units against a single tenant/table, recording EVERY durable mutation's
/// latency. For `Crud` the unit expands PHASED per worker — bulk-insert all
/// units, then bulk-update all, then bulk-delete all (3 mutations/unit) —
/// matching the sequential CRUD baseline; for `Insert` a unit is one insert;
/// for `HotKey` every unit updates the same seeded document.
/// Returns (total_mutations, wall_elapsed, per-mutation latencies in ns).
async fn run_round(
    engine: &Arc<Engine>,
    tenant: &TenantId,
    table: &TableName,
    n: usize,
    units_per_worker: usize,
    workload: Workload,
    hot_key_document_id: Option<&DocumentId>,
) -> (usize, Duration, Vec<u64>) {
    let started = Instant::now();
    let mut set: JoinSet<Vec<u64>> = JoinSet::new();
    for _worker in 0..n {
        let engine = engine.clone();
        let tenant = tenant.clone();
        let table = table.clone();
        let hot_key_document_id = hot_key_document_id.cloned();
        set.spawn(async move {
            let mut lat = Vec::with_capacity(units_per_worker * workload.mutations_per_unit());
            if workload == Workload::HotKey {
                let document_id = hot_key_document_id
                    .expect("hot-key workload should receive its shared document id");
                for unit in 0..units_per_worker {
                    let t = Instant::now();
                    update_hot_key_until_committed(&engine, &tenant, &table, &document_id, unit)
                        .await;
                    lat.push(t.elapsed().as_nanos() as u64);
                }
                return lat;
            }
            // Phase 1 — bulk insert, collecting ids. This PHASED shape (all
            // inserts, then all updates, then all deletes) mirrors the sequential
            // CRUD baseline exactly, so the update/delete phases operate over a
            // populated live set rather than a just-inserted-then-deleted doc.
            let mut ids = Vec::with_capacity(units_per_worker);
            for unit in 0..units_per_worker {
                let t = Instant::now();
                let id = engine
                    .insert_document_async(tenant.clone(), table.clone(), insert_fields(unit))
                    .await
                    .expect("insert_document_async should succeed");
                lat.push(t.elapsed().as_nanos() as u64);
                ids.push(id);
            }
            if workload == Workload::Crud {
                // Phase 2 — bulk update over the now-populated live set.
                for (unit, id) in ids.iter().cloned().enumerate() {
                    let t = Instant::now();
                    engine
                        .update_document_async(
                            tenant.clone(),
                            table.clone(),
                            id,
                            patch_fields(unit),
                        )
                        .await
                        .expect("update_document_async should succeed");
                    lat.push(t.elapsed().as_nanos() as u64);
                }
                // Phase 3 — bulk delete.
                for id in ids {
                    let t = Instant::now();
                    engine
                        .delete_document_async(tenant.clone(), table.clone(), id)
                        .await
                        .expect("delete_document_async should succeed");
                    lat.push(t.elapsed().as_nanos() as u64);
                }
            }
            lat
        });
    }

    let mut latencies = Vec::with_capacity(n * units_per_worker * workload.mutations_per_unit());
    while let Some(joined) = set.join_next().await {
        latencies.extend(joined.expect("worker task should not panic"));
    }
    let elapsed = started.elapsed();
    (latencies.len(), elapsed, latencies)
}

#[allow(
    clippy::too_many_arguments,
    reason = "benchmark rung driver threads the full sweep configuration"
)]
async fn measure_rung(
    engine: &Arc<Engine>,
    tenant: &TenantId,
    table: &TableName,
    n: usize,
    units_per_worker: usize,
    warmup_rounds: usize,
    measure_rounds: usize,
    workload: Workload,
    split_phases: bool,
    hot_key_document_id: Option<&DocumentId>,
    wal_checkpoint_observation_path: Option<&Path>,
) -> Rung {
    for _ in 0..warmup_rounds {
        let (ops, elapsed, lat) = run_round(
            engine,
            tenant,
            table,
            n,
            units_per_worker,
            workload,
            hot_key_document_id,
        )
        .await;
        black_box((ops, elapsed, lat.len()));
    }
    if let Some(path) = wal_checkpoint_observation_path {
        reset_sqlite_wal_checkpoint_observation(path);
    }
    let phase_before = split_phases.then(|| {
        engine
            .tenant_engine_diagnostics(tenant)
            .expect("phase snapshot before measured rounds should load")
            .commit_phases
    });
    let mut throughputs = Vec::with_capacity(measure_rounds);
    let mut latencies_ns = Vec::new();
    let mut measured_round_nanos = 0_u128;
    for _ in 0..measure_rounds {
        let (ops, elapsed, mut lat) = run_round(
            engine,
            tenant,
            table,
            n,
            units_per_worker,
            workload,
            hot_key_document_id,
        )
        .await;
        measured_round_nanos = measured_round_nanos.saturating_add(elapsed.as_nanos());
        let secs = elapsed.as_secs_f64();
        throughputs.push(if secs > 0.0 { ops as f64 / secs } else { 0.0 });
        latencies_ns.append(&mut lat);
    }
    let phase_split = phase_before.map(|before| {
        let after = engine
            .tenant_engine_diagnostics(tenant)
            .expect("phase snapshot after measured rounds should load")
            .commit_phases;
        PhaseSplit::between(phase_totals(before), phase_totals(after))
    });
    let wal_checkpoint_observation = wal_checkpoint_observation_path.map(|path| {
        let foreground = sqlite_wal_checkpoint_observation_snapshot(path);
        let passive = probe_sqlite_passive_checkpoint(path);
        disable_sqlite_wal_checkpoint_observation();
        WalCheckpointObservation {
            foreground,
            passive: passive.expect("post-run SQLite passive checkpoint probe should succeed"),
            measured_round_nanos: u64::try_from(measured_round_nanos).unwrap_or(u64::MAX),
        }
    });
    Rung {
        n,
        throughputs,
        latencies_ns,
        phase_split,
        wal_checkpoint_observation,
    }
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn median(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn percentile_ns(sorted: &[u64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f64 * pct / 100.0).round() as usize;
    sorted[rank.min(sorted.len() - 1)] as f64
}

/// Two-sided Student-t critical value at 95% confidence for `n-1` d.o.f.
/// Small table (matches the embedded-provider harness's approach); large-n
/// falls back to the normal approximation.
fn student_t_critical_95(n: usize) -> f64 {
    const TABLE: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    if n <= 1 {
        return 0.0;
    }
    let df = n - 1;
    if df <= 30 { TABLE[df - 1] } else { 1.96 }
}

fn summarize(rung: &Rung) -> RungStats {
    let mut tput_sorted = rung.throughputs.clone();
    tput_sorted.sort_by(f64::total_cmp);
    let mean_tps = mean(&rung.throughputs);
    let median_tps = median(&tput_sorted);

    let count = rung.throughputs.len();
    let stddev = if count > 1 {
        let var = rung
            .throughputs
            .iter()
            .map(|x| (x - mean_tps).powi(2))
            .sum::<f64>()
            / (count - 1) as f64;
        var.sqrt()
    } else {
        0.0
    };
    let sem = if count > 1 {
        stddev / (count as f64).sqrt()
    } else {
        0.0
    };
    let radius = student_t_critical_95(count) * sem;
    let cv_percent = if mean_tps > 0.0 {
        stddev / mean_tps * 100.0
    } else {
        0.0
    };

    let mut lat_sorted = rung.latencies_ns.clone();
    lat_sorted.sort_unstable();
    let mean_latency_s = if lat_sorted.is_empty() {
        0.0
    } else {
        lat_sorted.iter().sum::<u64>() as f64 / lat_sorted.len() as f64 / 1e9
    };

    RungStats {
        n: rung.n,
        raw_throughputs: rung.throughputs.clone(),
        mean_tps,
        median_tps,
        ci95_low: (mean_tps - radius).max(0.0),
        ci95_high: mean_tps + radius,
        cv_percent,
        p50_us: percentile_ns(&lat_sorted, 50.0) / 1000.0,
        p95_us: percentile_ns(&lat_sorted, 95.0) / 1000.0,
        p99_us: percentile_ns(&lat_sorted, 99.0) / 1000.0,
        mean_latency_s,
        phase_split: rung.phase_split,
        wal_checkpoint_observation: rung.wal_checkpoint_observation,
    }
}

fn render_report(
    stats: &[RungStats],
    backend: EmbeddedProviderKind,
    cfg: &str,
    workload: Workload,
) -> String {
    // Baseline is the N=1 rung specifically — NOT stats.first() — so a custom or
    // unsorted ladder can never anchor the speedup on an N>1 rung and report an
    // inflated (false) speedup. `ladder()` guarantees N=1 is always measured.
    let baseline = stats
        .iter()
        .find(|s| s.n == 1)
        .map(|s| s.mean_tps)
        .unwrap_or(0.0);
    let mut out = String::new();
    out.push_str("# Concurrent write-throughput (group-commit sweep)\n\n");
    out.push_str(&format!(
        "backend: `{}`  |  {}\n\n",
        match backend {
            EmbeddedProviderKind::Sqlite => "sqlite",
            EmbeddedProviderKind::Redb => "redb",
        },
        cfg,
    ));
    out.push_str(&format!(
        "Closed-loop, single-tenant, workload = `{}`. Throughput is durable mutations/sec. ",
        workload.label(),
    ));
    out.push_str("N=1 (batch size 1) is this harness's own sequential anchor");
    if workload == Workload::Crud {
        out.push_str("; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness)");
    }
    out.push_str(
        ";\n`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.\n",
    );
    out.push_str("Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).\n\n");
    out.push_str("| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
    for s in stats {
        let speedup = if baseline > 0.0 {
            s.mean_tps / baseline
        } else {
            0.0
        };
        let little = s.mean_tps * s.mean_latency_s; // should ≈ N
        out.push_str(&format!(
            "| {} | {:.0} | [{:.0}, {:.0}] | {:.0} | {:.1} | {:.2}× | {:.1} | {:.1} | {:.1} | {:.1} |\n",
            s.n,
            s.mean_tps,
            s.ci95_low,
            s.ci95_high,
            s.median_tps,
            s.cv_percent,
            speedup,
            s.p50_us,
            s.p95_us,
            s.p99_us,
            little,
        ));
    }
    out.push_str("\n## Raw measured-round samples\n\n");
    out.push_str(
        "These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.\n\n",
    );
    out.push_str("| N | measured mut/s samples |\n");
    out.push_str("|---:|---|\n");
    for s in stats {
        let samples = s
            .raw_throughputs
            .iter()
            .map(|sample| format!("{sample:.3}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("| {} | {} |\n", s.n, samples));
    }

    // Headline: peak throughput + the rung achieving it.
    if let Some(peak) = stats
        .iter()
        .max_by(|a, b| a.mean_tps.total_cmp(&b.mean_tps))
    {
        let speedup = if baseline > 0.0 {
            peak.mean_tps / baseline
        } else {
            0.0
        };
        out.push_str(&format!(
            "\n**Peak:** {:.0} mut/s at N={} — {:.2}× the sequential (N=1) baseline of {:.0} mut/s.\n",
            peak.mean_tps, peak.n, speedup, baseline,
        ));
    }

    let phase_rows = stats
        .iter()
        .filter_map(|stats| stats.phase_split.map(|split| (stats.n, split)))
        .collect::<Vec<_>>();
    out.push_str(&render_phase_split_section(&phase_rows));
    out.push_str(&render_wal_checkpoint_observation_section(stats));
    out
}

fn render_wal_checkpoint_observation_section(stats: &[RungStats]) -> String {
    let rows = stats
        .iter()
        .filter_map(|stats| {
            stats
                .wal_checkpoint_observation
                .map(|observation| (stats.n, observation))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("\n## WAL/checkpoint observation (diagnostic only)\n\n");
    out.push_str(
        "Enabled explicitly with `NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION=1`. \
         It is off in canonical timed runs, and the test-only statement counters \
         are not compiled into this benchmark. During measured rounds, each \
         successful foreground COMMIT runs a read-only `wal_checkpoint(NOOP)` \
         status query. `NOOP probe share` is that query's measured wall time \
         divided by measured-round wall time, so throughput from this diagnostic \
         mode is not acceptance evidence. The one `PASSIVE` probe runs only after \
         each rung and is reported separately. A nonzero `probe errors` count \
         means the frame and checkpoint columns are incomplete; treat that \
         rung's diagnostic as invalid and rerun.\n\n",
    );
    out.push_str(
        "`automatic checkpoints` counts post-COMMIT samples whose WAL frame \
         count reached the connection's auto-checkpoint threshold. Sampling runs \
         after COMMIT releases the writer lock, so per-commit attribution relies \
         on the per-tenant committer serializing writers, which holds for this \
         benchmark's workloads; treat the columns as sampled aggregate WAL \
         state. SQLite does not expose checkpoint-only COMMIT time, so \
         `automatic COMMIT upper bound` includes all work in those COMMIT \
         calls.\n\n",
    );
    out.push_str(
        "| N | foreground commits | automatic checkpoints | automatic COMMIT upper bound ms | WAL high-water frames | checkpointed high-water frames | auto threshold pages | NOOP probes | NOOP probe ms | NOOP probe share | probe errors | post-run PASSIVE busy/log/checkpointed | PASSIVE ms |\n",
    );
    out.push_str("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for (n, observation) in rows {
        let foreground = observation.foreground;
        let probe_share = if observation.measured_round_nanos == 0 {
            0.0
        } else {
            foreground.observation_probe_nanos as f64 / observation.measured_round_nanos as f64
                * 100.0
        };
        out.push_str(&format!(
            "| {n} | {} | {} | {:.3} | {} | {} | {} | {} | {:.3} | {:.3}% | {} | {}/{}/{} | {:.3} |\n",
            foreground.foreground_commit_count,
            foreground.automatic_checkpoint_count,
            foreground.automatic_checkpoint_commit_upper_bound_nanos as f64 / 1e6,
            foreground.wal_high_water_frames,
            foreground.checkpointed_high_water_frames,
            foreground.auto_checkpoint_pages,
            foreground.observation_probe_count,
            foreground.observation_probe_nanos as f64 / 1e6,
            probe_share,
            foreground.observation_probe_error_count,
            observation.passive.busy,
            observation.passive.wal_frames,
            observation.passive.checkpointed_frames,
            observation.passive.elapsed_nanos as f64 / 1e6,
        ));
    }
    out
}

fn phase_totals(snapshot: CommitPhaseMetricsSnapshot) -> PhaseTotals {
    PhaseTotals {
        journal_batch_size_sum: snapshot.journal_batch_size_sum,
        journal_batch_count: snapshot.journal_batch_count,
        prepare_nanos: snapshot.prepare_nanos,
        conflict_check_nanos: snapshot.conflict_check_nanos,
        apply_nanos: snapshot.apply_nanos,
        publish_nanos: snapshot.publish_nanos,
        durable_append_nanos: snapshot.durable_append_nanos,
        window_prepare_total: snapshot.window_prepare_total,
        storage_prepare_total: snapshot.storage_prepare_total,
    }
}

struct OpenLoopRound {
    target_rate: f64,
    achieved_rate: f64,
    scheduled: usize,
    completed: usize,
    saturation_breached: bool,
    latencies_ns: Vec<u64>,
}

/// One constant-rate open-loop round: `rate` inserts/second for `duration`.
/// Latency is measured from each arrival's SCHEDULED instant, so dispatcher or
/// engine lag inflates the recorded latency instead of thinning the arrivals
/// (the coordinated-omission fix). In-flight work above `max_in_flight` means
/// the offered rate is not sustainable at this margin; the round is flagged
/// rather than trusted.
async fn run_open_loop_round(
    engine: &Arc<Engine>,
    tenant: &TenantId,
    table: &TableName,
    rate: f64,
    duration: Duration,
    max_in_flight: usize,
) -> OpenLoopRound {
    let total = (rate * duration.as_secs_f64()).floor() as usize;
    let start = Instant::now() + Duration::from_millis(50);
    let mut set: JoinSet<u64> = JoinSet::new();
    let mut latencies = Vec::with_capacity(total);
    let mut saturation_breached = false;
    let mut completed = 0usize;
    for i in 0..total {
        let scheduled = start + Duration::from_secs_f64(i as f64 / rate);
        tokio::time::sleep_until(tokio::time::Instant::from_std(scheduled)).await;
        // Drain finished work without blocking the arrival schedule.
        while let Some(joined) = set.try_join_next() {
            latencies.push(joined.expect("open-loop worker should not panic"));
            completed += 1;
        }
        if set.len() >= max_in_flight {
            saturation_breached = true;
            break;
        }
        let engine = engine.clone();
        let tenant = tenant.clone();
        let table = table.clone();
        set.spawn(async move {
            engine
                .insert_document_async(tenant, table, insert_fields(i))
                .await
                .expect("open-loop insert should succeed");
            scheduled.elapsed().as_nanos() as u64
        });
    }
    let scheduled_count = if saturation_breached {
        completed + set.len()
    } else {
        total
    };
    while let Some(joined) = set.join_next().await {
        latencies.push(joined.expect("open-loop worker should not panic"));
        completed += 1;
    }
    let wall = start.elapsed().as_secs_f64();
    OpenLoopRound {
        target_rate: rate,
        achieved_rate: completed as f64 / wall,
        scheduled: scheduled_count,
        completed,
        saturation_breached,
        latencies_ns: latencies,
    }
}

fn render_open_loop_section(
    capacity: f64,
    rates: &[f64],
    rounds_per_rate: &[Vec<OpenLoopRound>],
    duration: Duration,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "\n## Open-loop service latency (SUC6.1)\n");
    let _ = writeln!(
        out,
        "Calibrated closed-loop capacity at the top rung: **{capacity:.0} mut/s**. Each round drives single-document inserts on a fixed arrival schedule for {}s; latency is measured from the scheduled arrival (coordinated-omission-free). A saturation-breached round means the offered rate was not sustainable and its numbers are not service-latency evidence.\n",
        duration.as_secs()
    );
    let _ = writeln!(
        out,
        "| Fraction | Target mut/s | Round | Sched | Done | Achieved | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | Verdict |"
    );
    let _ = writeln!(
        out,
        "| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"
    );
    for (fraction, rounds) in rates.iter().zip(rounds_per_rate) {
        for (round_index, round) in rounds.iter().enumerate() {
            let mut sorted = round.latencies_ns.clone();
            sorted.sort_unstable();
            let ms = |v: f64| v / 1_000_000.0;
            let verdict = if round.saturation_breached {
                "SATURATION BREACH"
            } else if (round.achieved_rate - round.target_rate).abs() / round.target_rate > 0.02 {
                "rate drift >2%"
            } else {
                "ok"
            };
            let _ = writeln!(
                out,
                "| {fraction} | {:.0} | {} | {} | {} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {verdict} |",
                round.target_rate,
                round_index + 1,
                round.scheduled,
                round.completed,
                round.achieved_rate,
                ms(percentile_ns(&sorted, 50.0)),
                ms(percentile_ns(&sorted, 90.0)),
                ms(percentile_ns(&sorted, 99.0)),
                ms(percentile_ns(&sorted, 99.9)),
                ms(*sorted.last().unwrap_or(&0) as f64),
            );
        }
    }
    out
}

async fn run() -> String {
    let ladder = ladder();
    let base_units = env_usize("NIMBUS_CWB_OPS_PER_WORKER", 300);
    let max_mut_per_round = env_usize("NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND", 24_000);
    let measure_rounds = env_usize("NIMBUS_CWB_MEASURE_ROUNDS", 10);
    let warmup_rounds = std::env::var("NIMBUS_CWB_WARMUP_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(2);
    // Default 0: the sequential CRUD baseline does NOT pre-seed, so N=1 matches
    // it. Set >0 for a pre-aged (deliberately non-baseline-matching) variant.
    let seed_docs = env_usize("NIMBUS_CWB_SEED_DOCS", 0);
    let backend = backend();
    let workload = workload();
    let split_phases = split_phases_enabled();
    let wal_checkpoint_observation = wal_checkpoint_observation_enabled();
    assert!(
        !wal_checkpoint_observation || backend == EmbeddedProviderKind::Sqlite,
        "WAL/checkpoint observation requires NIMBUS_CWB_BACKEND=sqlite"
    );

    let cfg = format!(
        "workload={}, base_units/worker={base_units}, max_mut/round={max_mut_per_round}, measure_rounds={measure_rounds}, warmup_rounds={warmup_rounds}, seed_docs={seed_docs}, ladder={ladder:?}, wal_checkpoint_observation={wal_checkpoint_observation}",
        workload.label(),
    );
    eprintln!("[cwb] {cfg}");
    if split_phases {
        eprintln!("[cwb] commit phase split enabled");
    }
    if wal_checkpoint_observation {
        eprintln!(
            "[cwb] WAL/checkpoint observation enabled; results are diagnostic, not timed acceptance"
        );
    }

    let dir = tempfile::tempdir().expect("tempdir should build");
    let engine = Arc::new(
        Engine::new_with_embedded_provider(dir.path(), backend)
            .expect("engine should open with the embedded provider"),
    );
    let tenant = TenantId::new("cwb-tenant").expect("tenant id should build");
    let table = TableName::new("tasks").expect("table name should build");
    engine
        .create_tenant_async(tenant.clone())
        .await
        .expect("tenant creation should succeed");
    let wal_checkpoint_observation_path =
        wal_checkpoint_observation.then(|| dir.path().join(format!("{}.sqlite3", tenant.as_str())));

    // Pre-age the store so we are not measuring the empty-file fast path.
    if seed_docs > 0 {
        eprintln!("[cwb] seeding {seed_docs} documents…");
        run_round(
            &engine,
            &tenant,
            &table,
            1,
            seed_docs,
            Workload::Insert,
            None,
        )
        .await;
    }

    let hot_key_document_id = if workload == Workload::HotKey {
        Some(
            engine
                .insert_document_async(tenant.clone(), table.clone(), insert_fields(0))
                .await
                .expect("hot-key seed insert should succeed"),
        )
    } else {
        None
    };

    let mut_per_unit = workload.mutations_per_unit();
    let mut stats = Vec::with_capacity(ladder.len());
    for n in &ladder {
        // Cap total mutations per round so high-N (overload) rungs don't blow up
        // wall time, while low-N rungs still do `base_units` of work. Always >=1.
        let capped = (max_mut_per_round / (mut_per_unit * *n)).max(1);
        let units_per_worker = base_units.min(capped);
        eprintln!("[cwb] rung N={n} (units/worker={units_per_worker})…");
        let rung = measure_rung(
            &engine,
            &tenant,
            &table,
            *n,
            units_per_worker,
            warmup_rounds,
            measure_rounds,
            workload,
            split_phases,
            hot_key_document_id.as_ref(),
            wal_checkpoint_observation_path.as_deref(),
        )
        .await;
        stats.push(summarize(&rung));
    }

    let mut report = render_report(&stats, backend, &cfg, workload);

    if let Ok(raw) = std::env::var("NIMBUS_CWB_OPEN_LOOP_RATES") {
        let fractions: Vec<f64> = raw
            .split(',')
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .filter(|v| *v > 0.0 && *v < 1.0)
            .collect();
        assert!(
            !fractions.is_empty(),
            "NIMBUS_CWB_OPEN_LOOP_RATES must contain fractions in (0, 1)"
        );
        let top = stats
            .last()
            .expect("open-loop mode requires at least one closed-loop rung");
        let capacity = top.mean_tps;
        let duration = Duration::from_secs(env_usize("NIMBUS_CWB_OPEN_LOOP_SECONDS", 30) as u64);
        let rounds = env_usize("NIMBUS_CWB_OPEN_LOOP_ROUNDS", 3);
        let mut rounds_per_rate = Vec::with_capacity(fractions.len());
        for fraction in &fractions {
            let rate = capacity * fraction;
            let mut collected = Vec::with_capacity(rounds);
            for round_index in 0..rounds {
                eprintln!(
                    "[cwb] open-loop fraction={fraction} rate={rate:.0}/s round {}/{rounds}…",
                    round_index + 1
                );
                collected.push(
                    run_open_loop_round(&engine, &tenant, &table, rate, duration, 10_000).await,
                );
            }
            rounds_per_rate.push(collected);
        }
        report.push_str(&render_open_loop_section(
            capacity,
            &fractions,
            &rounds_per_rate,
            duration,
        ));
    }

    if let Ok(path) = std::env::var("NIMBUS_CWB_OUT") {
        if let Err(e) = std::fs::write(&path, &report) {
            eprintln!("[cwb] could not write report to {path}: {e}");
        } else {
            eprintln!("[cwb] report written to {path}");
        }
    }
    report
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let report = runtime.block_on(run());
    println!("{report}");
}
