use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::hint::black_box;
use std::io::Write;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationKind,
    InvocationRequest, NimbusRuntime, NimbusRuntimeError, Result, RuntimeExecutionModel,
    RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits, RuntimeMetricsSnapshot,
    RuntimeNodeFullRealmReusePolicy, RuntimePolicy, RuntimePoolKind, RuntimeRoutingAffinity,
};
use serde::Serialize;
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "runtime_pool_modes/post_pir.rs"]
mod runtime_pool_modes_post_pir;

const PIR0_TRACE_SCHEMA: &str = "nimbus.profile_aware_isolate_runtime.pir0.trace.v1";
const PIR5_RETAINED_DENSITY_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.pir5.retained_density.v1";
const NFR6_TRACE_SCHEMA: &str = "nimbus.node_full_substrate_realm.nfr6.benchmark.v1";
const WASMTIME_V8_COMPARISON_TRACE_SCHEMA: &str = "nimbus.wasmtime_backend.w7.v8_comparison.v1";
const PIR5_RETAINED_DENSITY_COUNT: usize = 4;
const NFR6_TENANT_LABEL: &str = "tenant-a";

#[derive(Clone, Copy)]
enum BenchmarkProfile {
    WebStandard,
    Node20,
    Node22,
    Node24,
    Node26,
}

impl BenchmarkProfile {
    fn label(self) -> &'static str {
        match self {
            Self::WebStandard => "web_standard",
            Self::Node20 => "node20",
            Self::Node22 => "node22",
            Self::Node24 => "node24",
            Self::Node26 => "node26",
        }
    }

    fn limits(self) -> RuntimeLimits {
        match self {
            Self::WebStandard => RuntimeLimits::application_web_standard(),
            Self::Node20 => RuntimeLimits::application_node20(),
            Self::Node22 => RuntimeLimits::application_node22(),
            Self::Node24 => RuntimeLimits::application_node24(),
            Self::Node26 => RuntimeLimits::application_node26(),
        }
    }

    fn is_node_full(self) -> bool {
        matches!(
            self,
            Self::Node20 | Self::Node22 | Self::Node24 | Self::Node26
        )
    }
}

#[derive(Clone, Copy)]
enum PureJsWorkloadKind {
    HostlessTrivial,
    ComputeBound,
    SetupHeavy,
}

impl PureJsWorkloadKind {
    fn label(self) -> &'static str {
        match self {
            Self::HostlessTrivial => "hostless_trivial",
            Self::ComputeBound => "compute_bound_jit_hot",
            Self::SetupHeavy => "setup_heavy_large_module",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::HostlessTrivial => {
                r#"
globalThis.__nimbusInvoke = function (request) {
  return {
    ok: true,
    functionName: request.function_name,
    kind: request.kind,
  };
};

export {};
"#
            }
            Self::ComputeBound => {
                r#"
function mix(value) {
  let acc = value >>> 0;
  for (let i = 0; i < 4096; i++) {
    acc = Math.imul(acc ^ i, 2654435761) >>> 0;
    acc = (acc + (acc >>> 13)) >>> 0;
  }
  return acc;
}

globalThis.__nimbusInvoke = function (request) {
  return {
    ok: true,
    functionName: request.function_name,
    value: mix(request.args && request.args.bench ? 17 : 3),
  };
};

export {};
"#
            }
            Self::SetupHeavy => {
                r#"
const lookup = new Map();
for (let i = 0; i < 1024; i++) {
  lookup.set(`key-${i}`, {
    ordinal: i,
    text: `value-${i}-${(i * 17).toString(36)}`,
    flags: [i % 2 === 0, i % 3 === 0, i % 5 === 0],
  });
}

globalThis.__nimbusInvoke = function (request) {
  return {
    ok: true,
    functionName: request.function_name,
    size: lookup.size,
    sample: lookup.get("key-511"),
  };
};

export {};
"#
            }
        }
    }
}

const NO_EXTRA_FILES: &[(&str, &[u8])] = &[];
const CJS_TRANSLATOR_EXTRA_FILES: &[(&str, &[u8])] = &[(
    "translator-target.cjs",
    br#"
module.exports = {
  value: 41,
  label: "cjs-translator-target",
  next(value) {
    return value + 1;
  },
};
"#,
)];

#[derive(Clone, Copy)]
enum NodeFullNfr6WorkloadKind {
    SetupHeavy,
    LoaderHookDynamicBuiltin,
    Node24CjsTranslatorBoundary,
}

impl NodeFullNfr6WorkloadKind {
    fn label(self) -> &'static str {
        match self {
            Self::SetupHeavy => "setup_heavy_large_module",
            Self::LoaderHookDynamicBuiltin => "loader_hook_dynamic_builtin",
            Self::Node24CjsTranslatorBoundary => "node24_cjs_translator_boundary",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::SetupHeavy => PureJsWorkloadKind::SetupHeavy.source(),
            Self::LoaderHookDynamicBuiltin => {
                r#"
import { createRequire, isBuiltin } from "node:module";

const require = createRequire(import.meta.url);
const path = require("node:path");
const moduleNamespace = await import("node:module");

globalThis.__nimbusInvoke = async function (request) {
  const dynamicModuleNamespace = await import("node:module");
  return {
    ok: true,
    functionName: request.function_name,
    builtin: isBuiltin("node:fs") && moduleNamespace.isBuiltin("node:path"),
    dynamicBuiltin: dynamicModuleNamespace.isBuiltin("node:module"),
    basename: path.basename("/nimbus/nfr6/loader-hook.js"),
  };
};

export {};
"#
            }
            Self::Node24CjsTranslatorBoundary => {
                r#"
const cjsModule = await import("./translator-target.cjs");

globalThis.__nimbusInvoke = async function (request) {
  const dynamicCjsModule = await import("./translator-target.cjs");
  return {
    ok: true,
    functionName: request.function_name,
    value: cjsModule.default.next(dynamicCjsModule.default.value),
    keys: Object.keys(dynamicCjsModule.default).sort(),
  };
};

export {};
"#
            }
        }
    }

    fn extra_files(self) -> &'static [(&'static str, &'static [u8])] {
        match self {
            Self::SetupHeavy | Self::LoaderHookDynamicBuiltin => NO_EXTRA_FILES,
            Self::Node24CjsTranslatorBoundary => CJS_TRANSLATOR_EXTRA_FILES,
        }
    }
}

#[derive(Clone, Copy)]
enum PoolMode {
    StartupSnapshotCache,
    WarmPool,
    WarmContextRecycle,
}

impl PoolMode {
    fn label(self) -> &'static str {
        match self {
            Self::StartupSnapshotCache => "startup_snapshot_cache",
            Self::WarmPool => "warm_pool",
            Self::WarmContextRecycle => "warm_context_recycle",
        }
    }

    fn runtime_pool_kind(self) -> RuntimePoolKind {
        match self {
            Self::StartupSnapshotCache => RuntimePoolKind::StartupSnapshotCache,
            Self::WarmPool => RuntimePoolKind::WarmPool,
            Self::WarmContextRecycle => RuntimePoolKind::WarmContextRecycle,
        }
    }
}

#[derive(Clone, Copy)]
enum CodeCacheState {
    FreshBundleEachInvocation,
    PrimedBundleCodeCache,
}

impl CodeCacheState {
    fn label(self) -> &'static str {
        match self {
            Self::FreshBundleEachInvocation => "fresh_bundle_each_invocation",
            Self::PrimedBundleCodeCache => "primed_bundle_code_cache",
        }
    }
}

fn include_blocked_pir0_await_rows() -> bool {
    std::env::var_os("NIMBUS_PIR0_INCLUDE_BLOCKED_AWAIT_ROWS").is_some()
        || std::env::var_os("NIMBUS_PIR0_INCLUDE_KNOWN_STALLED_AWAIT_ROWS").is_some()
}

fn is_blocked_pir0_await_row(
    profile: BenchmarkProfile,
    _synthetic_await: Duration,
    scenario_kind: AsyncHostBatchScenarioKind,
    pool_mode: PoolMode,
) -> bool {
    matches!(
        scenario_kind,
        AsyncHostBatchScenarioKind::CooperativeLockerFourTenants
    ) && (matches!(profile, BenchmarkProfile::WebStandard)
        || matches!(pool_mode, PoolMode::WarmPool))
}

fn execution_model_label(execution_model: RuntimeExecutionModel) -> &'static str {
    match execution_model {
        RuntimeExecutionModel::RunToCompletion => "run_to_completion",
        RuntimeExecutionModel::CooperativeLocker => "cooperative_locker",
        RuntimeExecutionModel::CooperativeFuel => "cooperative_fuel",
        RuntimeExecutionModel::BackendOwnedEventLoop => "backend_owned_event_loop",
    }
}

#[derive(Clone, Copy)]
enum PureJsScenarioKind {
    RunToCompletionSingleTenant,
    CooperativeLockerSingleTenant,
    CooperativeLockerFourTenants,
}

impl PureJsScenarioKind {
    fn label(self) -> &'static str {
        match self {
            Self::RunToCompletionSingleTenant => "run_to_completion_single_tenant",
            Self::CooperativeLockerSingleTenant => "cooperative_locker_single_tenant",
            Self::CooperativeLockerFourTenants => "cooperative_locker_four_tenants",
        }
    }

    fn execution_model(self) -> RuntimeExecutionModel {
        match self {
            Self::RunToCompletionSingleTenant => RuntimeExecutionModel::RunToCompletion,
            Self::CooperativeLockerSingleTenant | Self::CooperativeLockerFourTenants => {
                RuntimeExecutionModel::CooperativeLocker
            }
        }
    }

    fn tenant_labels(self) -> &'static [&'static str] {
        match self {
            Self::RunToCompletionSingleTenant | Self::CooperativeLockerSingleTenant => {
                &["tenant-a"]
            }
            Self::CooperativeLockerFourTenants => &["tenant-a", "tenant-b", "tenant-c", "tenant-d"],
        }
    }
}

#[derive(Clone, Copy)]
enum AsyncHostBatchScenarioKind {
    RunToCompletionFourTenants,
    CooperativeLockerFourTenants,
}

impl AsyncHostBatchScenarioKind {
    fn label(self) -> &'static str {
        match self {
            Self::RunToCompletionFourTenants => "run_to_completion_four_tenants",
            Self::CooperativeLockerFourTenants => "cooperative_locker_four_tenants",
        }
    }

    fn execution_model(self) -> RuntimeExecutionModel {
        match self {
            Self::RunToCompletionFourTenants => RuntimeExecutionModel::RunToCompletion,
            Self::CooperativeLockerFourTenants => RuntimeExecutionModel::CooperativeLocker,
        }
    }

    fn tenant_labels(self) -> &'static [&'static str] {
        &["tenant-a", "tenant-b", "tenant-c", "tenant-d"]
    }
}

#[derive(Default)]
struct NoopHost;

impl HostBridge for NoopHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(format!(
            "benchmark bundle should not issue host operations: {}",
            request.operation
        )))
    }
}

#[derive(Clone, Copy)]
struct DelayedAsyncHost {
    delay: Duration,
}

impl DelayedAsyncHost {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl HostBridge for DelayedAsyncHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(format!(
            "async benchmark should not use sync host path: {}",
            request.operation
        )))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(json!({
                "status": "ok",
                "value": {
                    "operation": request.operation,
                    "payload": request.payload,
                },
            }))
        })
    }
}

fn benchmark_request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: json!({ "bench": true }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn write_bundle(tempdir: &TempDir, source: &str) -> nimbus_runtime::RuntimeBundle {
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, source).expect("benchmark bundle should write");
    nimbus_runtime::RuntimeBundle::new(&bundle_path)
}

fn write_nfr6_workload_bundle(
    tempdir: &TempDir,
    workload: NodeFullNfr6WorkloadKind,
) -> nimbus_runtime::RuntimeBundle {
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, workload.source()).expect("NFR6 bundle should write");
    for &(relative_path, contents) in workload.extra_files() {
        let path = tempdir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("NFR6 extra file parent should exist");
        }
        std::fs::write(path, contents).expect("NFR6 extra file should write");
    }
    nimbus_runtime::RuntimeBundle::new(&bundle_path)
}

fn build_runtime(
    profile: BenchmarkProfile,
    host: Arc<dyn HostBridge>,
    pool_mode: PoolMode,
    execution_model: RuntimeExecutionModel,
) -> (NimbusRuntime, RuntimeExecutor) {
    build_runtime_with_config(profile, host, pool_mode, execution_model, |_| {})
}

fn build_runtime_with_config(
    profile: BenchmarkProfile,
    host: Arc<dyn HostBridge>,
    pool_mode: PoolMode,
    execution_model: RuntimeExecutionModel,
    configure: impl FnOnce(&mut RuntimeLimits),
) -> (NimbusRuntime, RuntimeExecutor) {
    let mut limits = profile.limits();
    limits.execution_model = execution_model;
    limits.runtime_pool_kind = pool_mode.runtime_pool_kind();
    if profile.is_node_full() && matches!(pool_mode, PoolMode::WarmContextRecycle) {
        limits.node_full_realm_reuse_policy =
            RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
    }
    limits.routing_affinity = RuntimeRoutingAffinity::Tenant;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_heap_mb = 256;
    // Criterion may run 16k+ iterations per closure call. Set the warm
    // reuse cap high enough that the benchmark doesn't hit retirement.
    limits.max_warm_reuses = 1_000_000;
    configure(&mut limits);
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime = NimbusRuntime::with_policy(host, policy.clone());
    let executor = RuntimeExecutor::new(policy);
    (runtime, executor)
}

fn maybe_report_phase_metrics_once(
    scenario_label: &str,
    pool_label: &str,
    snapshot: &RuntimeMetricsSnapshot,
    total_invocations: u64,
) {
    if std::env::var_os("NIMBUS_BENCH_REPORT_METRICS").is_none() || total_invocations == 0 {
        return;
    }

    // Keep reporting inside Nimbus's own metrics surface. Criterion scenarios
    // exercise worker threads, so mutating process-global env vars here would
    // be unsafe and can destabilize the benchmark process.
    static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let reported = REPORTED.get_or_init(|| Mutex::new(HashSet::new()));
    let key = format!("{scenario_label}/{pool_label}");
    let mut reported = reported
        .lock()
        .expect("benchmark phase metric report lock should not be poisoned");
    if !reported.insert(key.clone()) {
        return;
    }
    drop(reported);

    let per_invocation =
        |nanos_total: u64| nanos_total as f64 / total_invocations as f64 / 1_000_000.0;
    eprintln!(
        concat!(
            "phase-metrics {} schema={}: module_load={:.3}ms evaluation={:.3}ms ",
            "bundle_total={:.3}ms realm_create={:.3}ms ",
            "realm_bootstrap_install={:.3}ms realm_bootstrap_finalize={:.3}ms ",
            "realm_bootstrap_reset={:.3}ms realm_invoke_script={:.3}ms ",
            "realm_promise_resolve={:.3}ms realm_deserialize={:.3}ms realm_destroy={:.3}ms ",
            "wasmtime_module_cache_hits={} wasmtime_module_cache_misses={} ",
            "wasmtime_compilation_time_ns={} wasmtime_fuel_consumed_total={} ",
            "wasmtime_store_pool_hits={} wasmtime_store_pool_misses={} ",
            "comparison='Wasmtime WASM backend versus V8 path'"
        ),
        key,
        WASMTIME_V8_COMPARISON_TRACE_SCHEMA,
        per_invocation(snapshot.bundle_module_load_nanos_total),
        per_invocation(snapshot.bundle_evaluation_nanos_total),
        per_invocation(snapshot.bundle_load_nanos_total),
        per_invocation(snapshot.fresh_realm_create_nanos_total),
        per_invocation(snapshot.fresh_realm_bootstrap_install_nanos_total),
        per_invocation(snapshot.fresh_realm_bootstrap_finalize_nanos_total),
        per_invocation(snapshot.fresh_realm_bootstrap_reset_nanos_total),
        per_invocation(snapshot.fresh_realm_invocation_script_nanos_total),
        per_invocation(snapshot.fresh_realm_promise_resolve_nanos_total),
        per_invocation(snapshot.fresh_realm_deserialization_nanos_total),
        per_invocation(snapshot.fresh_realm_destroy_nanos_total),
        snapshot.wasmtime_module_cache_hits,
        snapshot.wasmtime_module_cache_misses,
        snapshot.wasmtime_module_compilation_nanos_total,
        snapshot.wasmtime_fuel_consumed_total,
        snapshot.wasmtime_store_pool_hits,
        snapshot.wasmtime_store_pool_misses,
    );
}

#[derive(Serialize)]
struct Pir0TraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'a str,
    benchmark_id: &'a str,
    profile: &'a str,
    workload: &'a str,
    pool_kind: &'a str,
    execution_model: &'a str,
    tenant_count: usize,
    measured_iterations: u64,
    total_invocations: u64,
    synthetic_await_ms: Option<u64>,
    rss_bytes: Option<u64>,
    bundle_loads: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    fresh_realm_creates: u64,
    fresh_realm_create_nanos_total: u64,
    fresh_realm_bootstrap_installs: u64,
    fresh_realm_bootstrap_install_nanos_total: u64,
    fresh_realm_bootstrap_finalizes: u64,
    fresh_realm_bootstrap_finalize_nanos_total: u64,
    fresh_realm_bootstrap_resets: u64,
    fresh_realm_bootstrap_reset_nanos_total: u64,
    fresh_realm_invocation_scripts: u64,
    fresh_realm_invocation_script_nanos_total: u64,
    fresh_realm_promise_resolves: u64,
    fresh_realm_promise_resolve_nanos_total: u64,
    fresh_realm_deserializations: u64,
    fresh_realm_deserialization_nanos_total: u64,
    fresh_realm_destroys: u64,
    fresh_realm_destroy_nanos_total: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
}

#[derive(Serialize)]
struct Pir5RetainedDensityTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'a str,
    benchmark_id: &'a str,
    profile: &'a str,
    workload: &'a str,
    rss_source: &'static str,
    fresh_process_profile_filter_enabled: bool,
    retained_runtime_count: usize,
    measured_iterations: u64,
    total_retained_invocations: u64,
    rss_before_bytes: Option<u64>,
    rss_after_bytes: Option<u64>,
    rss_delta_bytes: Option<u64>,
    measured_per_runtime_rss_bytes: Option<u64>,
    max_heap_mb: usize,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
}

#[derive(Serialize)]
struct Nfr6TraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'a str,
    benchmark_id: &'a str,
    profile: &'a str,
    workload: &'a str,
    pool_kind: &'a str,
    substrate: &'static str,
    execution_model: &'static str,
    tenant_count: usize,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_min_nanos: Option<u64>,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_max_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_bytes: Option<u64>,
    rss_after_bytes: Option<u64>,
    rss_delta_bytes: Option<u64>,
    max_heap_mb: usize,
    worker_threads: usize,
    max_concurrent_runtime_instances: usize,
    max_warm_pool_entries_per_worker: usize,
    max_warm_reuses: usize,
    owner_cap_hit_count: u64,
    owner_cap_hit_rate: f64,
    observed_dirty_return_count: u64,
    observed_condemn_count: u64,
    condemn_reason_distribution: BTreeMap<&'static str, u64>,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    runtime_pool_replacements: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    warm_pool_discard_unquiesced: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    fresh_realm_creates: u64,
    fresh_realm_create_nanos_total: u64,
    fresh_realm_bootstrap_installs: u64,
    fresh_realm_bootstrap_install_nanos_total: u64,
    fresh_realm_bootstrap_finalizes: u64,
    fresh_realm_bootstrap_finalize_nanos_total: u64,
    fresh_realm_bootstrap_resets: u64,
    fresh_realm_bootstrap_reset_nanos_total: u64,
    fresh_realm_invocation_scripts: u64,
    fresh_realm_invocation_script_nanos_total: u64,
    fresh_realm_promise_resolves: u64,
    fresh_realm_promise_resolve_nanos_total: u64,
    fresh_realm_deserializations: u64,
    fresh_realm_deserialization_nanos_total: u64,
    fresh_realm_destroys: u64,
    fresh_realm_destroy_nanos_total: u64,
    host_pressure_decisions: u64,
    host_pressure_high_decisions: u64,
    host_pressure_critical_decisions: u64,
    latest_effective_dispatch_seats: usize,
}

fn maybe_emit_pir0_trace_record(record: Pir0TraceRecord<'_>) {
    let Some(path) = std::env::var_os("NIMBUS_PIR0_TRACE_PATH") else {
        return;
    };
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("PIR0 trace emission lock should not be poisoned");
    if emitted
        .get(&key)
        .is_some_and(|previous| *previous >= record.measured_iterations)
    {
        return;
    }
    emitted.insert(key, record.measured_iterations);
    drop(emitted);

    let path = std::path::PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("PIR0 trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("PIR0 trace file should open");
    serde_json::to_writer(&mut file, &record).expect("PIR0 trace record should serialize");
    file.write_all(b"\n")
        .expect("PIR0 trace record should end with newline");
}

fn maybe_emit_pir5_retained_density_trace_record(record: Pir5RetainedDensityTraceRecord<'_>) {
    let Some(path) = std::env::var_os("NIMBUS_PIR5_RETAINED_DENSITY_TRACE_PATH") else {
        return;
    };
    static EMITTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut emitted = emitted
        .lock()
        .expect("PIR5 retained-density trace emission lock should not be poisoned");
    if !emitted.insert(key) {
        return;
    }
    drop(emitted);

    let path = std::path::PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .expect("PIR5 retained-density trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("PIR5 retained-density trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("PIR5 retained-density trace record should serialize");
    file.write_all(b"\n")
        .expect("PIR5 retained-density trace record should end with newline");
}

fn maybe_emit_nfr6_trace_record(record: Nfr6TraceRecord<'_>) {
    let Some(path) = std::env::var_os("NIMBUS_NFR6_TRACE_PATH") else {
        return;
    };
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("NFR6 trace emission lock should not be poisoned");
    if emitted
        .get(&key)
        .is_some_and(|previous| *previous >= record.measured_iterations)
    {
        return;
    }
    emitted.insert(key, record.measured_iterations);
    drop(emitted);

    let path = std::path::PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("NFR6 trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("NFR6 trace file should open");
    serde_json::to_writer(&mut file, &record).expect("NFR6 trace record should serialize");
    file.write_all(b"\n")
        .expect("NFR6 trace record should end with newline");
}

fn duration_nanos_u64(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn percentile_nanos(sorted_samples: &[u64], percentile: f64) -> Option<u64> {
    if sorted_samples.is_empty() {
        return None;
    }
    let max_index = sorted_samples.len() - 1;
    let index = ((max_index as f64) * percentile)
        .round()
        .clamp(0.0, max_index as f64) as usize;
    sorted_samples.get(index).copied()
}

#[cfg(target_os = "linux")]
fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    Some(rss_pages.saturating_mul(page_size as u64))
}

#[cfg(target_os = "macos")]
fn current_rss_bytes() -> Option<u64> {
    let mut task_info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::uninit();
    let mut task_info_count = libc::MACH_TASK_BASIC_INFO_COUNT;
    let rc = unsafe {
        libc::task_info(
            mach_task_self_port(),
            libc::MACH_TASK_BASIC_INFO,
            task_info.as_mut_ptr() as libc::task_info_t,
            &mut task_info_count,
        )
    };
    if rc != libc::KERN_SUCCESS {
        return None;
    }
    let task_info = unsafe { task_info.assume_init() };
    Some(task_info.resident_size)
}

#[cfg(target_os = "macos")]
fn mach_task_self_port() -> libc::mach_port_t {
    unsafe extern "C" {
        static mach_task_self_: libc::mach_port_t;
    }
    unsafe { mach_task_self_ }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_rss_bytes() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn current_rss_source_label() -> &'static str {
    "linux_proc_self_statm_resident_pages"
}

#[cfg(target_os = "macos")]
fn current_rss_source_label() -> &'static str {
    "macos_mach_task_basic_info_resident_size"
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_rss_source_label() -> &'static str {
    "unsupported"
}

fn env_filter_includes(var_name: &str, label: &str) -> bool {
    let Ok(filter) = std::env::var(var_name) else {
        return true;
    };
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    filter
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == label)
}

fn include_nfr6_profile(profile: BenchmarkProfile) -> bool {
    env_filter_includes("NIMBUS_NFR6_PROFILE", profile.label())
}

fn include_nfr6_pool_mode(pool_mode: PoolMode) -> bool {
    env_filter_includes("NIMBUS_NFR6_POOL_MODE", pool_mode.label())
}

fn include_nfr6_workload(workload: NodeFullNfr6WorkloadKind) -> bool {
    env_filter_includes("NIMBUS_NFR6_WORKLOAD", workload.label())
}

fn pir5_retained_density_profile_filter() -> Option<String> {
    std::env::var("NIMBUS_PIR5_RETAINED_DENSITY_PROFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn include_pir5_retained_density_profile(profile: BenchmarkProfile) -> bool {
    let Some(filter) = pir5_retained_density_profile_filter() else {
        return true;
    };
    filter
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == profile.label())
}

struct SequentialScenario {
    _tempdir: TempDir,
    runtime: NimbusRuntime,
    executor: RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    profile: BenchmarkProfile,
    tenant_labels: &'static [&'static str],
    next_tenant_index: usize,
    pool_mode: PoolMode,
    benchmark_group: &'static str,
    scenario_label: &'static str,
    workload_label: &'static str,
    execution_model: RuntimeExecutionModel,
}

impl SequentialScenario {
    fn new(pool_mode: PoolMode, scenario_kind: PureJsScenarioKind) -> Self {
        Self::new_profiled(
            BenchmarkProfile::WebStandard,
            pool_mode,
            scenario_kind.execution_model(),
            scenario_kind.tenant_labels(),
            "runtime_pool_modes_pure_js",
            scenario_kind.label(),
            PureJsWorkloadKind::HostlessTrivial,
        )
    }

    fn new_profiled(
        profile: BenchmarkProfile,
        pool_mode: PoolMode,
        execution_model: RuntimeExecutionModel,
        tenant_labels: &'static [&'static str],
        benchmark_group: &'static str,
        scenario_label: &'static str,
        workload: PureJsWorkloadKind,
    ) -> Self {
        let tempdir = tempfile::tempdir().expect("benchmark tempdir should build");
        let bundle = write_bundle(&tempdir, workload.source());
        let (runtime, executor) =
            build_runtime(profile, Arc::new(NoopHost), pool_mode, execution_model);
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: benchmark_request(),
            profile,
            tenant_labels,
            next_tenant_index: 0,
            pool_mode,
            benchmark_group,
            scenario_label,
            workload_label: workload.label(),
            execution_model,
        }
    }

    fn prime(&mut self) {
        for _ in 0..self.tenant_labels.len() {
            self.invoke_once();
        }
    }

    fn invoke_once(&mut self) {
        let tenant_label = self.tenant_labels[self.next_tenant_index % self.tenant_labels.len()];
        self.next_tenant_index = self.next_tenant_index.saturating_add(1);
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            self.bundle.clone(),
            self.request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&self.request, tenant_label),
        );
        let result = result.expect("benchmark invocation should succeed");
        black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn assert_metrics(&self, measured_iterations: u64) {
        let snapshot = self.metrics_snapshot();
        let total_invocations = self.tenant_labels.len() as u64 + measured_iterations;
        match self.pool_mode {
            PoolMode::WarmPool => {
                // Warm pool: cold miss only on first bundle load (all tenants
                // share the same bundle identity). All subsequent invocations
                // are warm hits that skip module loading entirely.
                assert_eq!(snapshot.bundle_loads, 1);
                assert_eq!(snapshot.warm_pool_misses, 1);
                assert_eq!(snapshot.warm_pool_hits, total_invocations - 1);
                assert_eq!(snapshot.warm_pool_discard_unquiesced, 0);
            }
            PoolMode::StartupSnapshotCache | PoolMode::WarmContextRecycle => {
                assert_eq!(snapshot.bundle_loads, total_invocations);
                assert_eq!(snapshot.bundle_module_loads, total_invocations);
                assert_eq!(snapshot.bundle_evaluations, total_invocations);
            }
        }
        match self.pool_mode {
            PoolMode::StartupSnapshotCache => {
                assert_eq!(snapshot.runtime_pool_misses, 1);
                assert_eq!(
                    snapshot.runtime_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.retained_runtime_pool_entries, 0);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmContextRecycle => {
                assert_eq!(snapshot.runtime_pool_misses, 1);
                assert_eq!(
                    snapshot.runtime_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.warm_pool_misses, 1);
                assert_eq!(snapshot.warm_pool_hits, total_invocations.saturating_sub(1));
                assert_eq!(snapshot.warm_pool_discard_unquiesced, 0);
                assert_eq!(snapshot.retained_runtime_pool_entries, 1);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmPool => {
                // Already asserted above
            }
        }
        maybe_report_phase_metrics_once(
            self.scenario_label,
            self.pool_mode.label(),
            &snapshot,
            total_invocations,
        );
        let benchmark_id = format!(
            "{}/{}/{}/{}",
            self.profile.label(),
            self.workload_label,
            self.scenario_label,
            self.pool_mode.label()
        );
        maybe_emit_pir0_trace_record(Pir0TraceRecord {
            schema: PIR0_TRACE_SCHEMA,
            benchmark_group: self.benchmark_group,
            benchmark_id: &benchmark_id,
            profile: self.profile.label(),
            workload: self.workload_label,
            pool_kind: self.pool_mode.label(),
            execution_model: execution_model_label(self.execution_model),
            tenant_count: self.tenant_labels.len(),
            measured_iterations,
            total_invocations,
            synthetic_await_ms: None,
            rss_bytes: current_rss_bytes(),
            bundle_loads: snapshot.bundle_loads,
            bundle_module_loads: snapshot.bundle_module_loads,
            bundle_evaluations: snapshot.bundle_evaluations,
            fresh_realm_creates: snapshot.fresh_realm_creates,
            fresh_realm_create_nanos_total: snapshot.fresh_realm_create_nanos_total,
            fresh_realm_bootstrap_installs: snapshot.fresh_realm_bootstrap_installs,
            fresh_realm_bootstrap_install_nanos_total: snapshot
                .fresh_realm_bootstrap_install_nanos_total,
            fresh_realm_bootstrap_finalizes: snapshot.fresh_realm_bootstrap_finalizes,
            fresh_realm_bootstrap_finalize_nanos_total: snapshot
                .fresh_realm_bootstrap_finalize_nanos_total,
            fresh_realm_bootstrap_resets: snapshot.fresh_realm_bootstrap_resets,
            fresh_realm_bootstrap_reset_nanos_total: snapshot
                .fresh_realm_bootstrap_reset_nanos_total,
            fresh_realm_invocation_scripts: snapshot.fresh_realm_invocation_scripts,
            fresh_realm_invocation_script_nanos_total: snapshot
                .fresh_realm_invocation_script_nanos_total,
            fresh_realm_promise_resolves: snapshot.fresh_realm_promise_resolves,
            fresh_realm_promise_resolve_nanos_total: snapshot
                .fresh_realm_promise_resolve_nanos_total,
            fresh_realm_deserializations: snapshot.fresh_realm_deserializations,
            fresh_realm_deserialization_nanos_total: snapshot
                .fresh_realm_deserialization_nanos_total,
            fresh_realm_destroys: snapshot.fresh_realm_destroys,
            fresh_realm_destroy_nanos_total: snapshot.fresh_realm_destroy_nanos_total,
            runtime_pool_hits: snapshot.runtime_pool_hits,
            runtime_pool_misses: snapshot.runtime_pool_misses,
            warm_pool_hits: snapshot.warm_pool_hits,
            warm_pool_misses: snapshot.warm_pool_misses,
            queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
            execution_nanos_total: snapshot.execution_nanos_total,
            retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
        });
    }
}

struct CodeCacheImpactScenario {
    _tempdir: TempDir,
    runtime: NimbusRuntime,
    executor: RuntimeExecutor,
    bundle_path: std::path::PathBuf,
    cached_bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
}

struct RetainedDensityScenario {
    _tempdir: TempDir,
    runtime: NimbusRuntime,
    executor: RuntimeExecutor,
    bundles: Vec<nimbus_runtime::RuntimeBundle>,
    request: InvocationRequest,
    profile: BenchmarkProfile,
    retained_runtime_count: usize,
    max_heap_mb: usize,
}

impl RetainedDensityScenario {
    fn new(profile: BenchmarkProfile, retained_runtime_count: usize) -> Self {
        let tempdir = tempfile::tempdir().expect("retained-density tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, PureJsWorkloadKind::HostlessTrivial.source())
            .expect("retained-density bundle should write");
        let expected_sha256 = nimbus_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
            .expect("retained-density bundle hash should load");
        let bundles = (0..retained_runtime_count)
            .map(|index| {
                nimbus_runtime::RuntimeBundle::for_tenant(
                    &bundle_path,
                    &expected_sha256,
                    format!("tenant-{index}"),
                )
                .expect("retained-density tenant bundle should build")
            })
            .collect();
        let mut limits = profile.limits();
        limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
        limits.runtime_pool_kind = RuntimePoolKind::WarmPool;
        limits.routing_affinity = RuntimeRoutingAffinity::Tenant;
        limits.max_concurrent_runtime_instances = 1;
        limits.worker_threads = 1;
        limits.max_warm_pool_entries_per_worker = retained_runtime_count;
        limits.max_heap_mb = 256;
        limits.max_warm_reuses = 1_000_000;
        let max_heap_mb = limits.max_heap_mb;
        let policy = Arc::new(RuntimePolicy::new(limits));
        let runtime = NimbusRuntime::with_policy(Arc::new(NoopHost), policy.clone());
        let executor = RuntimeExecutor::new(policy);
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundles,
            request: benchmark_request(),
            profile,
            retained_runtime_count,
            max_heap_mb,
        }
    }

    fn retain_runtimes(&self) {
        for (index, bundle) in self.bundles.iter().enumerate() {
            let tenant_label = format!("tenant-{index}");
            let result = self.executor.invoke_blocking(
                self.runtime.clone(),
                bundle.clone(),
                self.request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&self.request, &tenant_label),
            );
            black_box(result.expect("retained-density warm-pool invocation should succeed"));
        }
        let snapshot = self.metrics_snapshot();
        assert_eq!(
            snapshot.retained_runtime_pool_entries, self.retained_runtime_count,
            "retained-density scenario should hold the requested retained runtime count"
        );
        assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
        assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        rss_before_bytes: Option<u64>,
        rss_after_bytes: Option<u64>,
    ) {
        let snapshot = self.metrics_snapshot();
        let rss_delta_bytes = rss_before_bytes
            .zip(rss_after_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let measured_per_runtime_rss_bytes =
            rss_delta_bytes.map(|delta| delta / self.retained_runtime_count.max(1) as u64);
        let total_retained_invocations =
            measured_iterations.saturating_mul(self.retained_runtime_count as u64);
        let benchmark_id = format!(
            "{}/{}/retained_{}",
            self.profile.label(),
            PureJsWorkloadKind::HostlessTrivial.label(),
            self.retained_runtime_count
        );
        maybe_emit_pir5_retained_density_trace_record(Pir5RetainedDensityTraceRecord {
            schema: PIR5_RETAINED_DENSITY_TRACE_SCHEMA,
            benchmark_group: "runtime_pool_modes_pir5_retained_density",
            benchmark_id: &benchmark_id,
            profile: self.profile.label(),
            workload: PureJsWorkloadKind::HostlessTrivial.label(),
            rss_source: current_rss_source_label(),
            fresh_process_profile_filter_enabled: pir5_retained_density_profile_filter().is_some(),
            retained_runtime_count: self.retained_runtime_count,
            measured_iterations,
            total_retained_invocations,
            rss_before_bytes,
            rss_after_bytes,
            rss_delta_bytes,
            measured_per_runtime_rss_bytes,
            max_heap_mb: self.max_heap_mb,
            runtime_pool_hits: snapshot.runtime_pool_hits,
            runtime_pool_misses: snapshot.runtime_pool_misses,
            warm_pool_hits: snapshot.warm_pool_hits,
            warm_pool_misses: snapshot.warm_pool_misses,
            retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
        });
    }
}

struct Nfr6NodeFullScenario {
    _tempdir: TempDir,
    runtime: NimbusRuntime,
    executor: RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    profile: BenchmarkProfile,
    workload: NodeFullNfr6WorkloadKind,
    pool_mode: PoolMode,
    max_heap_mb: usize,
    worker_threads: usize,
    max_concurrent_runtime_instances: usize,
    max_warm_pool_entries_per_worker: usize,
    max_warm_reuses: usize,
}

impl Nfr6NodeFullScenario {
    fn new(
        profile: BenchmarkProfile,
        workload: NodeFullNfr6WorkloadKind,
        pool_mode: PoolMode,
    ) -> Self {
        debug_assert!(profile.is_node_full());
        let tempdir = tempfile::tempdir().expect("NFR6 tempdir should build");
        let bundle = write_nfr6_workload_bundle(&tempdir, workload);
        let (runtime, executor) = build_runtime(
            profile,
            Arc::new(NoopHost),
            pool_mode,
            RuntimeExecutionModel::CooperativeLocker,
        );
        let policy = executor.policy();
        let limits = policy.limits();
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: benchmark_request(),
            profile,
            workload,
            pool_mode,
            max_heap_mb: limits.max_heap_mb,
            worker_threads: limits.worker_threads,
            max_concurrent_runtime_instances: limits.max_concurrent_runtime_instances,
            max_warm_pool_entries_per_worker: limits.max_warm_pool_entries_per_worker,
            max_warm_reuses: limits.max_warm_reuses,
        }
    }

    fn prime(&mut self) {
        self.invoke_once();
    }

    fn invoke_once(&mut self) {
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            self.bundle.clone(),
            self.request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&self.request, NFR6_TENANT_LABEL),
        );
        black_box(result.expect("NFR6 NodeFull benchmark invocation should succeed"));
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn assert_metrics(&self, measured_iterations: u64) {
        let snapshot = self.metrics_snapshot();
        let total_invocations = 1 + measured_iterations;
        match self.pool_mode {
            PoolMode::WarmPool => {
                assert_eq!(snapshot.bundle_loads, 1);
                assert_eq!(snapshot.warm_pool_misses, 1);
                assert_eq!(snapshot.warm_pool_hits, total_invocations - 1);
                assert_eq!(snapshot.warm_pool_discard_unquiesced, 0);
            }
            PoolMode::StartupSnapshotCache | PoolMode::WarmContextRecycle => {
                assert_eq!(snapshot.bundle_loads, total_invocations);
                assert_eq!(snapshot.bundle_module_loads, total_invocations);
                assert_eq!(snapshot.bundle_evaluations, total_invocations);
            }
        }
        match self.pool_mode {
            PoolMode::StartupSnapshotCache => {
                assert_eq!(snapshot.runtime_pool_misses, 1);
                assert_eq!(
                    snapshot.runtime_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.retained_runtime_pool_entries, 0);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmPool => {}
            PoolMode::WarmContextRecycle => {
                assert_eq!(snapshot.runtime_pool_misses, 1);
                assert_eq!(
                    snapshot.runtime_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.warm_pool_misses, 1);
                assert_eq!(snapshot.warm_pool_hits, total_invocations.saturating_sub(1));
                assert_eq!(snapshot.warm_pool_discard_unquiesced, 0);
                assert_eq!(snapshot.retained_runtime_pool_entries, 1);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
        }
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_bytes: Option<u64>,
        rss_after_bytes: Option<u64>,
    ) {
        if std::env::var_os("NIMBUS_NFR6_TRACE_PATH").is_none() {
            return;
        }
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let rss_delta_bytes = rss_before_bytes
            .zip(rss_after_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let total_invocations = 1 + measured_iterations;
        let elapsed_secs = elapsed.as_secs_f64();
        let throughput_invocations_per_sec = if elapsed_secs > 0.0 {
            measured_iterations as f64 / elapsed_secs
        } else {
            0.0
        };
        let benchmark_id = format!(
            "{}/{}/{}/{}",
            self.profile.label(),
            self.workload.label(),
            execution_model_label(RuntimeExecutionModel::CooperativeLocker),
            self.pool_mode.label()
        );
        maybe_emit_nfr6_trace_record(Nfr6TraceRecord {
            schema: NFR6_TRACE_SCHEMA,
            benchmark_group: "runtime_pool_modes_nfr6_node_full_realm",
            benchmark_id: &benchmark_id,
            profile: self.profile.label(),
            workload: self.workload.label(),
            pool_kind: self.pool_mode.label(),
            substrate: "node_full_realm_lease",
            execution_model: execution_model_label(RuntimeExecutionModel::CooperativeLocker),
            tenant_count: 1,
            measured_iterations,
            total_invocations,
            elapsed_nanos: duration_nanos_u64(elapsed),
            throughput_invocations_per_sec,
            latency_min_nanos: sorted_latency_nanos.first().copied(),
            latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
            latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
            latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
            latency_max_nanos: sorted_latency_nanos.last().copied(),
            rss_source: current_rss_source_label(),
            rss_before_bytes,
            rss_after_bytes,
            rss_delta_bytes,
            max_heap_mb: self.max_heap_mb,
            worker_threads: self.worker_threads,
            max_concurrent_runtime_instances: self.max_concurrent_runtime_instances,
            max_warm_pool_entries_per_worker: self.max_warm_pool_entries_per_worker,
            max_warm_reuses: self.max_warm_reuses,
            owner_cap_hit_count: 0,
            owner_cap_hit_rate: 0.0,
            observed_dirty_return_count: snapshot.warm_pool_discard_unquiesced,
            observed_condemn_count: 0,
            condemn_reason_distribution: BTreeMap::new(),
            runtime_pool_hits: snapshot.runtime_pool_hits,
            runtime_pool_misses: snapshot.runtime_pool_misses,
            runtime_pool_replacements: snapshot.runtime_pool_replacements,
            warm_pool_hits: snapshot.warm_pool_hits,
            warm_pool_misses: snapshot.warm_pool_misses,
            warm_pool_retirements: snapshot.warm_pool_retirements,
            warm_pool_discard_unquiesced: snapshot.warm_pool_discard_unquiesced,
            retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
            queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
            execution_nanos_total: snapshot.execution_nanos_total,
            fresh_realm_creates: snapshot.fresh_realm_creates,
            fresh_realm_create_nanos_total: snapshot.fresh_realm_create_nanos_total,
            fresh_realm_bootstrap_installs: snapshot.fresh_realm_bootstrap_installs,
            fresh_realm_bootstrap_install_nanos_total: snapshot
                .fresh_realm_bootstrap_install_nanos_total,
            fresh_realm_bootstrap_finalizes: snapshot.fresh_realm_bootstrap_finalizes,
            fresh_realm_bootstrap_finalize_nanos_total: snapshot
                .fresh_realm_bootstrap_finalize_nanos_total,
            fresh_realm_bootstrap_resets: snapshot.fresh_realm_bootstrap_resets,
            fresh_realm_bootstrap_reset_nanos_total: snapshot
                .fresh_realm_bootstrap_reset_nanos_total,
            fresh_realm_invocation_scripts: snapshot.fresh_realm_invocation_scripts,
            fresh_realm_invocation_script_nanos_total: snapshot
                .fresh_realm_invocation_script_nanos_total,
            fresh_realm_promise_resolves: snapshot.fresh_realm_promise_resolves,
            fresh_realm_promise_resolve_nanos_total: snapshot
                .fresh_realm_promise_resolve_nanos_total,
            fresh_realm_deserializations: snapshot.fresh_realm_deserializations,
            fresh_realm_deserialization_nanos_total: snapshot
                .fresh_realm_deserialization_nanos_total,
            fresh_realm_destroys: snapshot.fresh_realm_destroys,
            fresh_realm_destroy_nanos_total: snapshot.fresh_realm_destroy_nanos_total,
            host_pressure_decisions: snapshot.host_pressure.decisions,
            host_pressure_high_decisions: snapshot.host_pressure.high_decisions,
            host_pressure_critical_decisions: snapshot.host_pressure.critical_decisions,
            latest_effective_dispatch_seats: snapshot.host_pressure.latest_effective_dispatch_seats,
        });
    }
}

impl CodeCacheImpactScenario {
    fn new(profile: BenchmarkProfile) -> Self {
        let tempdir = tempfile::tempdir().expect("benchmark tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, PureJsWorkloadKind::SetupHeavy.source())
            .expect("benchmark bundle should write");
        let cached_bundle = nimbus_runtime::RuntimeBundle::new(&bundle_path);
        let (runtime, executor) = build_runtime(
            profile,
            Arc::new(NoopHost),
            PoolMode::StartupSnapshotCache,
            RuntimeExecutionModel::CooperativeLocker,
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle_path,
            cached_bundle,
            request: benchmark_request(),
        }
    }

    fn prime_runtime_only(&self) {
        self.invoke_fresh_bundle_once();
    }

    fn prime_cached_bundle(&self) {
        self.invoke_cached_bundle_once();
    }

    fn invoke_fresh_bundle_once(&self) {
        let bundle = nimbus_runtime::RuntimeBundle::new(&self.bundle_path);
        self.invoke_bundle(bundle);
    }

    fn invoke_cached_bundle_once(&self) {
        self.invoke_bundle(self.cached_bundle.clone());
    }

    fn invoke_bundle(&self, bundle: nimbus_runtime::RuntimeBundle) {
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            bundle,
            self.request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&self.request, "tenant-a"),
        );
        black_box(result.expect("code-cache benchmark invocation should succeed"));
    }
}

fn prime_profile_bootstrap(profile: BenchmarkProfile) {
    let tempdir = tempfile::tempdir().expect("bootstrap-prime tempdir should build");
    let bundle = write_bundle(&tempdir, PureJsWorkloadKind::HostlessTrivial.source());
    let (runtime, executor) = build_runtime(
        profile,
        Arc::new(NoopHost),
        PoolMode::StartupSnapshotCache,
        RuntimeExecutionModel::CooperativeLocker,
    );
    let request = benchmark_request();
    let result = executor.invoke_blocking(
        runtime,
        bundle,
        request.clone(),
        RuntimeInvocationContext::top_level_for_tenant(&request, "bootstrap-prime"),
    );
    black_box(result.expect("profile bootstrap prime should succeed"));
}

struct AsyncHostBatchScenario {
    _tempdir: TempDir,
    runtime: NimbusRuntime,
    executor: RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    profile: BenchmarkProfile,
    tenant_labels: &'static [&'static str],
    next_tenant_index: usize,
    pool_mode: PoolMode,
    scenario_kind: AsyncHostBatchScenarioKind,
    benchmark_group: &'static str,
    synthetic_await_ms: u64,
}

impl AsyncHostBatchScenario {
    fn new(pool_mode: PoolMode, scenario_kind: AsyncHostBatchScenarioKind) -> Self {
        Self::new_profiled(
            BenchmarkProfile::WebStandard,
            pool_mode,
            scenario_kind,
            "runtime_pool_modes_async_host_batch",
            Duration::from_millis(1),
        )
    }

    fn new_profiled(
        profile: BenchmarkProfile,
        pool_mode: PoolMode,
        scenario_kind: AsyncHostBatchScenarioKind,
        benchmark_group: &'static str,
        synthetic_await: Duration,
    ) -> Self {
        let tempdir = tempfile::tempdir().expect("benchmark tempdir should build");
        let bundle = write_bundle(
            &tempdir,
            r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
        );
        let (runtime, executor) = build_runtime(
            profile,
            Arc::new(DelayedAsyncHost::new(synthetic_await)),
            pool_mode,
            scenario_kind.execution_model(),
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: benchmark_request(),
            profile,
            tenant_labels: scenario_kind.tenant_labels(),
            next_tenant_index: 0,
            pool_mode,
            scenario_kind,
            benchmark_group,
            synthetic_await_ms: synthetic_await.as_millis() as u64,
        }
    }

    fn prime(&mut self) {
        self.invoke_batch_once();
    }

    fn next_tenant_label(&mut self) -> &'static str {
        let tenant_label = self.tenant_labels[self.next_tenant_index % self.tenant_labels.len()];
        self.next_tenant_index = self.next_tenant_index.saturating_add(1);
        tenant_label
    }

    fn invoke_batch_once(&mut self) {
        let tenant_a = self.next_tenant_label();
        let tenant_b = self.next_tenant_label();
        let tenant_c = self.next_tenant_label();
        let tenant_d = self.next_tenant_label();
        let invocations = [tenant_a, tenant_b, tenant_c, tenant_d].map(|tenant_label| {
            let executor = self.executor.clone();
            let runtime = self.runtime.clone();
            let bundle = self.bundle.clone();
            let request = self.request.clone();
            std::thread::spawn(move || {
                executor.invoke_blocking(
                    runtime,
                    bundle,
                    request.clone(),
                    RuntimeInvocationContext::top_level_for_tenant(&request, tenant_label),
                )
            })
        });

        for handle in invocations {
            let result = handle
                .join()
                .expect("async benchmark caller thread should not panic")
                .expect("async benchmark invocation should succeed");
            black_box(result);
        }
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn assert_metrics(&self, measured_batches: u64) {
        let snapshot = self.metrics_snapshot();
        let total_invocations = 4 + measured_batches.saturating_mul(4);
        if !matches!(self.pool_mode, PoolMode::WarmPool) {
            assert_eq!(snapshot.bundle_loads, total_invocations);
            assert_eq!(snapshot.bundle_module_loads, total_invocations);
            assert_eq!(snapshot.bundle_evaluations, total_invocations);
        }
        match self.pool_mode {
            PoolMode::StartupSnapshotCache => {
                assert_eq!(snapshot.runtime_pool_misses, 1);
                assert_eq!(
                    snapshot.runtime_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.retained_runtime_pool_entries, 0);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmPool => {
                // Warm pool metrics are validated at the top-level match
            }
            PoolMode::WarmContextRecycle => {
                // Not used by the async-host matrix yet.
            }
        }
        maybe_report_phase_metrics_once(
            self.scenario_kind.label(),
            self.pool_mode.label(),
            &snapshot,
            total_invocations,
        );
        let benchmark_id = format!(
            "{}/await_{}ms/{}/{}",
            self.profile.label(),
            self.synthetic_await_ms,
            self.scenario_kind.label(),
            self.pool_mode.label()
        );
        maybe_emit_pir0_trace_record(Pir0TraceRecord {
            schema: PIR0_TRACE_SCHEMA,
            benchmark_group: self.benchmark_group,
            benchmark_id: &benchmark_id,
            profile: self.profile.label(),
            workload: "await_heavy_synthetic_host_call",
            pool_kind: self.pool_mode.label(),
            execution_model: execution_model_label(self.scenario_kind.execution_model()),
            tenant_count: self.tenant_labels.len(),
            measured_iterations: measured_batches,
            total_invocations,
            synthetic_await_ms: Some(self.synthetic_await_ms),
            rss_bytes: current_rss_bytes(),
            bundle_loads: snapshot.bundle_loads,
            bundle_module_loads: snapshot.bundle_module_loads,
            bundle_evaluations: snapshot.bundle_evaluations,
            fresh_realm_creates: snapshot.fresh_realm_creates,
            fresh_realm_create_nanos_total: snapshot.fresh_realm_create_nanos_total,
            fresh_realm_bootstrap_installs: snapshot.fresh_realm_bootstrap_installs,
            fresh_realm_bootstrap_install_nanos_total: snapshot
                .fresh_realm_bootstrap_install_nanos_total,
            fresh_realm_bootstrap_finalizes: snapshot.fresh_realm_bootstrap_finalizes,
            fresh_realm_bootstrap_finalize_nanos_total: snapshot
                .fresh_realm_bootstrap_finalize_nanos_total,
            fresh_realm_bootstrap_resets: snapshot.fresh_realm_bootstrap_resets,
            fresh_realm_bootstrap_reset_nanos_total: snapshot
                .fresh_realm_bootstrap_reset_nanos_total,
            fresh_realm_invocation_scripts: snapshot.fresh_realm_invocation_scripts,
            fresh_realm_invocation_script_nanos_total: snapshot
                .fresh_realm_invocation_script_nanos_total,
            fresh_realm_promise_resolves: snapshot.fresh_realm_promise_resolves,
            fresh_realm_promise_resolve_nanos_total: snapshot
                .fresh_realm_promise_resolve_nanos_total,
            fresh_realm_deserializations: snapshot.fresh_realm_deserializations,
            fresh_realm_deserialization_nanos_total: snapshot
                .fresh_realm_deserialization_nanos_total,
            fresh_realm_destroys: snapshot.fresh_realm_destroys,
            fresh_realm_destroy_nanos_total: snapshot.fresh_realm_destroy_nanos_total,
            runtime_pool_hits: snapshot.runtime_pool_hits,
            runtime_pool_misses: snapshot.runtime_pool_misses,
            warm_pool_hits: snapshot.warm_pool_hits,
            warm_pool_misses: snapshot.warm_pool_misses,
            queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
            execution_nanos_total: snapshot.execution_nanos_total,
            retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
        });
    }
}

fn pure_js_pool_modes_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pure_js");
    group.throughput(Throughput::Elements(1));

    for scenario_kind in [
        PureJsScenarioKind::RunToCompletionSingleTenant,
        PureJsScenarioKind::CooperativeLockerSingleTenant,
        PureJsScenarioKind::CooperativeLockerFourTenants,
    ] {
        let pool_modes: &[PoolMode] = if matches!(
            scenario_kind.execution_model(),
            RuntimeExecutionModel::CooperativeLocker
        ) {
            &[PoolMode::StartupSnapshotCache, PoolMode::WarmPool]
        } else {
            &[PoolMode::StartupSnapshotCache]
        };
        for &pool_mode in pool_modes {
            group.bench_with_input(
                BenchmarkId::new(scenario_kind.label(), pool_mode.label()),
                &(scenario_kind, pool_mode),
                |b, &(scenario_kind, pool_mode)| {
                    b.iter_custom(|iters| {
                        let mut scenario = SequentialScenario::new(pool_mode, scenario_kind);
                        scenario.prime();
                        let started_at = Instant::now();
                        for _ in 0..iters {
                            scenario.invoke_once();
                        }
                        let elapsed = started_at.elapsed();
                        scenario.assert_metrics(iters);
                        black_box(scenario.metrics_snapshot());
                        elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

fn pir0_profile_matrix_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pir0_profile_matrix");
    group.throughput(Throughput::Elements(1));

    for profile in [
        BenchmarkProfile::WebStandard,
        BenchmarkProfile::Node20,
        BenchmarkProfile::Node22,
        BenchmarkProfile::Node24,
        BenchmarkProfile::Node26,
    ] {
        for workload in [
            PureJsWorkloadKind::HostlessTrivial,
            PureJsWorkloadKind::ComputeBound,
            PureJsWorkloadKind::SetupHeavy,
        ] {
            for execution_model in [
                RuntimeExecutionModel::RunToCompletion,
                RuntimeExecutionModel::CooperativeLocker,
            ] {
                let pool_modes: &[PoolMode] =
                    if matches!(execution_model, RuntimeExecutionModel::CooperativeLocker) {
                        &[PoolMode::StartupSnapshotCache, PoolMode::WarmPool]
                    } else {
                        &[PoolMode::StartupSnapshotCache]
                    };
                for &pool_mode in pool_modes {
                    let scenario_label = execution_model_label(execution_model);
                    let benchmark_id = BenchmarkId::new(
                        format!(
                            "{}/{}/{}",
                            profile.label(),
                            workload.label(),
                            scenario_label
                        ),
                        pool_mode.label(),
                    );
                    group.bench_with_input(
                        benchmark_id,
                        &(profile, workload, execution_model, pool_mode),
                        |b, &(profile, workload, execution_model, pool_mode)| {
                            b.iter_custom(|iters| {
                                let mut scenario = SequentialScenario::new_profiled(
                                    profile,
                                    pool_mode,
                                    execution_model,
                                    &["tenant-a"],
                                    "runtime_pool_modes_pir0_profile_matrix",
                                    execution_model_label(execution_model),
                                    workload,
                                );
                                scenario.prime();
                                let started_at = Instant::now();
                                for _ in 0..iters {
                                    scenario.invoke_once();
                                }
                                let elapsed = started_at.elapsed();
                                scenario.assert_metrics(iters);
                                black_box(scenario.metrics_snapshot());
                                elapsed
                            });
                        },
                    );
                }
            }
        }
    }

    group.finish();
}

fn pir0_synthetic_await_matrix_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pir0_synthetic_await_matrix");
    group.throughput(Throughput::Elements(4));

    for profile in [BenchmarkProfile::WebStandard, BenchmarkProfile::Node22] {
        for synthetic_await in [
            Duration::from_millis(0),
            Duration::from_millis(1),
            Duration::from_millis(5),
            Duration::from_millis(50),
        ] {
            for scenario_kind in [
                AsyncHostBatchScenarioKind::RunToCompletionFourTenants,
                AsyncHostBatchScenarioKind::CooperativeLockerFourTenants,
            ] {
                let pool_modes: &[PoolMode] = match scenario_kind {
                    AsyncHostBatchScenarioKind::CooperativeLockerFourTenants => {
                        &[PoolMode::StartupSnapshotCache, PoolMode::WarmPool]
                    }
                    AsyncHostBatchScenarioKind::RunToCompletionFourTenants => {
                        &[PoolMode::StartupSnapshotCache]
                    }
                };
                for &pool_mode in pool_modes {
                    if is_blocked_pir0_await_row(profile, synthetic_await, scenario_kind, pool_mode)
                        && !include_blocked_pir0_await_rows()
                    {
                        continue;
                    }
                    let benchmark_id = BenchmarkId::new(
                        format!(
                            "{}/await_{}ms/{}",
                            profile.label(),
                            synthetic_await.as_millis(),
                            scenario_kind.label()
                        ),
                        pool_mode.label(),
                    );
                    group.bench_with_input(
                        benchmark_id,
                        &(profile, synthetic_await, scenario_kind, pool_mode),
                        |b, &(profile, synthetic_await, scenario_kind, pool_mode)| {
                            b.iter_custom(|iters| {
                                let mut scenario = AsyncHostBatchScenario::new_profiled(
                                    profile,
                                    pool_mode,
                                    scenario_kind,
                                    "runtime_pool_modes_pir0_synthetic_await_matrix",
                                    synthetic_await,
                                );
                                scenario.prime();
                                let started_at = Instant::now();
                                for _ in 0..iters {
                                    scenario.invoke_batch_once();
                                }
                                let elapsed = started_at.elapsed();
                                scenario.assert_metrics(iters);
                                black_box(scenario.metrics_snapshot());
                                elapsed
                            });
                        },
                    );
                }
            }
        }
    }

    group.finish();
}

fn pir2_context_recycle_impact_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pir2_context_recycle_impact");
    group.throughput(Throughput::Elements(1));

    for workload in [
        PureJsWorkloadKind::HostlessTrivial,
        PureJsWorkloadKind::SetupHeavy,
    ] {
        for pool_mode in [
            PoolMode::StartupSnapshotCache,
            PoolMode::WarmPool,
            PoolMode::WarmContextRecycle,
        ] {
            let benchmark_id = BenchmarkId::new(
                format!(
                    "{}/{}/{}",
                    BenchmarkProfile::WebStandard.label(),
                    workload.label(),
                    execution_model_label(RuntimeExecutionModel::CooperativeLocker)
                ),
                pool_mode.label(),
            );
            group.bench_with_input(
                benchmark_id,
                &(workload, pool_mode),
                |b, &(workload, pool_mode)| {
                    b.iter_custom(|iters| {
                        let mut scenario = SequentialScenario::new_profiled(
                            BenchmarkProfile::WebStandard,
                            pool_mode,
                            RuntimeExecutionModel::CooperativeLocker,
                            &["tenant-a"],
                            "runtime_pool_modes_pir2_context_recycle_impact",
                            execution_model_label(RuntimeExecutionModel::CooperativeLocker),
                            workload,
                        );
                        scenario.prime();
                        let started_at = Instant::now();
                        for _ in 0..iters {
                            scenario.invoke_once();
                        }
                        let elapsed = started_at.elapsed();
                        scenario.assert_metrics(iters);
                        black_box(scenario.metrics_snapshot());
                        elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

fn pir6_code_cache_impact_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pir6_code_cache_impact");
    group.throughput(Throughput::Elements(1));

    for profile in [BenchmarkProfile::WebStandard, BenchmarkProfile::Node22] {
        for cache_state in [
            CodeCacheState::FreshBundleEachInvocation,
            CodeCacheState::PrimedBundleCodeCache,
        ] {
            let benchmark_id = BenchmarkId::new(
                format!(
                    "{}/{}",
                    profile.label(),
                    PureJsWorkloadKind::SetupHeavy.label()
                ),
                cache_state.label(),
            );
            group.bench_with_input(
                benchmark_id,
                &(profile, cache_state),
                |b, &(profile, cache_state)| {
                    b.iter_custom(|iters| {
                        let scenario = CodeCacheImpactScenario::new(profile);
                        match cache_state {
                            CodeCacheState::FreshBundleEachInvocation => {
                                scenario.prime_runtime_only();
                                let started_at = Instant::now();
                                for _ in 0..iters {
                                    scenario.invoke_fresh_bundle_once();
                                }
                                started_at.elapsed()
                            }
                            CodeCacheState::PrimedBundleCodeCache => {
                                scenario.prime_cached_bundle();
                                let started_at = Instant::now();
                                for _ in 0..iters {
                                    scenario.invoke_cached_bundle_once();
                                }
                                started_at.elapsed()
                            }
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

fn pir5_retained_density_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_pir5_retained_density");
    group.throughput(Throughput::Elements(PIR5_RETAINED_DENSITY_COUNT as u64));

    for profile in [
        BenchmarkProfile::WebStandard,
        BenchmarkProfile::Node20,
        BenchmarkProfile::Node22,
        BenchmarkProfile::Node24,
        BenchmarkProfile::Node26,
    ] {
        if !include_pir5_retained_density_profile(profile) {
            continue;
        }
        let benchmark_id = BenchmarkId::new(
            profile.label(),
            format!("retained_{}", PIR5_RETAINED_DENSITY_COUNT),
        );
        group.bench_with_input(benchmark_id, &profile, |b, &profile| {
            b.iter_custom(|iters| {
                prime_profile_bootstrap(profile);
                let scenario = RetainedDensityScenario::new(profile, PIR5_RETAINED_DENSITY_COUNT);
                let rss_before_bytes = current_rss_bytes();
                let started_at = Instant::now();
                for _ in 0..iters {
                    scenario.retain_runtimes();
                }
                let elapsed = started_at.elapsed();
                let rss_after_bytes = current_rss_bytes();
                scenario.emit_trace(iters, rss_before_bytes, rss_after_bytes);
                black_box(scenario.metrics_snapshot());
                elapsed
            });
        });
    }

    group.finish();
}

fn nfr6_node_full_realm_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_nfr6_node_full_realm");
    group.throughput(Throughput::Elements(1));

    for profile in [
        BenchmarkProfile::Node20,
        BenchmarkProfile::Node22,
        BenchmarkProfile::Node24,
        BenchmarkProfile::Node26,
    ] {
        if !include_nfr6_profile(profile) {
            continue;
        }
        for workload in [
            NodeFullNfr6WorkloadKind::SetupHeavy,
            NodeFullNfr6WorkloadKind::LoaderHookDynamicBuiltin,
            NodeFullNfr6WorkloadKind::Node24CjsTranslatorBoundary,
        ] {
            if !include_nfr6_workload(workload) {
                continue;
            }
            for pool_mode in [
                PoolMode::StartupSnapshotCache,
                PoolMode::WarmPool,
                PoolMode::WarmContextRecycle,
            ] {
                if !include_nfr6_pool_mode(pool_mode) {
                    continue;
                }
                let benchmark_id = BenchmarkId::new(
                    format!(
                        "{}/{}/{}",
                        profile.label(),
                        workload.label(),
                        execution_model_label(RuntimeExecutionModel::CooperativeLocker)
                    ),
                    pool_mode.label(),
                );
                group.bench_with_input(
                    benchmark_id,
                    &(profile, workload, pool_mode),
                    |b, &(profile, workload, pool_mode)| {
                        b.iter_custom(|iters| {
                            let mut scenario =
                                Nfr6NodeFullScenario::new(profile, workload, pool_mode);
                            scenario.prime();
                            let trace_enabled =
                                std::env::var_os("NIMBUS_NFR6_TRACE_PATH").is_some();
                            let rss_before_bytes = current_rss_bytes();
                            let mut latency_nanos = if trace_enabled {
                                Vec::with_capacity(iters.min(100_000) as usize)
                            } else {
                                Vec::new()
                            };
                            let started_at = Instant::now();
                            for _ in 0..iters {
                                if trace_enabled {
                                    let invocation_started_at = Instant::now();
                                    scenario.invoke_once();
                                    latency_nanos
                                        .push(duration_nanos_u64(invocation_started_at.elapsed()));
                                } else {
                                    scenario.invoke_once();
                                }
                            }
                            let elapsed = started_at.elapsed();
                            let rss_after_bytes = current_rss_bytes();
                            scenario.assert_metrics(iters);
                            scenario.emit_trace(
                                iters,
                                elapsed,
                                &latency_nanos,
                                rss_before_bytes,
                                rss_after_bytes,
                            );
                            black_box(scenario.metrics_snapshot());
                            elapsed
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

fn async_host_batch_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_pool_modes_async_host_batch");
    group.throughput(Throughput::Elements(4));

    for scenario_kind in [
        AsyncHostBatchScenarioKind::RunToCompletionFourTenants,
        AsyncHostBatchScenarioKind::CooperativeLockerFourTenants,
    ] {
        let pool_modes: &[PoolMode] = match scenario_kind {
            AsyncHostBatchScenarioKind::CooperativeLockerFourTenants => {
                &[PoolMode::StartupSnapshotCache, PoolMode::WarmPool]
            }
            AsyncHostBatchScenarioKind::RunToCompletionFourTenants => {
                &[PoolMode::StartupSnapshotCache]
            }
        };
        for &pool_mode in pool_modes {
            group.bench_with_input(
                BenchmarkId::new(scenario_kind.label(), pool_mode.label()),
                &(scenario_kind, pool_mode),
                |b, &(scenario_kind, pool_mode)| {
                    b.iter_custom(|iters| {
                        let mut scenario = AsyncHostBatchScenario::new(pool_mode, scenario_kind);
                        scenario.prime();
                        let started_at = Instant::now();
                        for _ in 0..iters {
                            scenario.invoke_batch_once();
                        }
                        let elapsed = started_at.elapsed();
                        scenario.assert_metrics(iters);
                        black_box(scenario.metrics_snapshot());
                        elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    pure_js_pool_modes_benchmark,
    async_host_batch_benchmark,
    pir0_profile_matrix_benchmark,
    pir0_synthetic_await_matrix_benchmark,
    pir2_context_recycle_impact_benchmark,
    pir6_code_cache_impact_benchmark,
    pir5_retained_density_benchmark,
    nfr6_node_full_realm_benchmark,
    runtime_pool_modes_post_pir::post_pir_optimization_benchmark
);
criterion_main!(benches);
