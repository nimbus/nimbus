use std::collections::HashSet;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use neovex_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationKind,
    InvocationRequest, NeovexRuntime, NeovexRuntimeError, Result, RuntimeExecutionModel,
    RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits, RuntimeMetricsSnapshot,
    RuntimePolicy, RuntimePoolKind, RuntimeRoutingAffinity,
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[derive(Clone, Copy)]
enum PoolMode {
    StartupSnapshotCache,
    WarmPool,
}

impl PoolMode {
    fn label(self) -> &'static str {
        match self {
            Self::StartupSnapshotCache => "startup_snapshot_cache",
            Self::WarmPool => "warm_pool",
        }
    }

    fn runtime_pool_kind(self) -> RuntimePoolKind {
        match self {
            Self::StartupSnapshotCache => RuntimePoolKind::StartupSnapshotCache,
            Self::WarmPool => RuntimePoolKind::WarmPool,
        }
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
        Err(NeovexRuntimeError::Contract(format!(
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
        Err(NeovexRuntimeError::Contract(format!(
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
    }
}

fn write_bundle(tempdir: &TempDir, source: &str) -> neovex_runtime::RuntimeBundle {
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, source).expect("benchmark bundle should write");
    neovex_runtime::RuntimeBundle::new(&bundle_path)
}

fn build_runtime(
    host: Arc<dyn HostBridge>,
    pool_mode: PoolMode,
    execution_model: RuntimeExecutionModel,
) -> (NeovexRuntime, RuntimeExecutor) {
    let limits = RuntimeLimits {
        execution_model,
        runtime_pool_kind: pool_mode.runtime_pool_kind(),
        routing_affinity: RuntimeRoutingAffinity::Tenant,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        max_heap_mb: 256,
        // Criterion may run 16k+ iterations per closure call. Set the warm
        // reuse cap high enough that the benchmark doesn't hit retirement.
        max_warm_reuses: 1_000_000,
        ..RuntimeLimits::default()
    };
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime = NeovexRuntime::with_policy(host, policy.clone());
    let executor = RuntimeExecutor::new(policy);
    (runtime, executor)
}

fn maybe_report_phase_metrics_once(
    scenario_label: &str,
    pool_label: &str,
    snapshot: &RuntimeMetricsSnapshot,
    total_invocations: u64,
) {
    if std::env::var_os("NEOVEX_BENCH_REPORT_METRICS").is_none() || total_invocations == 0 {
        return;
    }

    // Keep reporting inside Neovex's own metrics surface. Criterion scenarios
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
        "phase-metrics {key}: module_load={:.3}ms evaluation={:.3}ms bundle_total={:.3}ms",
        per_invocation(snapshot.bundle_module_load_nanos_total),
        per_invocation(snapshot.bundle_evaluation_nanos_total),
        per_invocation(snapshot.bundle_load_nanos_total),
    );
}

struct SequentialScenario {
    _tempdir: TempDir,
    runtime: NeovexRuntime,
    executor: RuntimeExecutor,
    bundle: neovex_runtime::RuntimeBundle,
    request: InvocationRequest,
    tenant_labels: &'static [&'static str],
    next_tenant_index: usize,
    pool_mode: PoolMode,
    scenario_kind: PureJsScenarioKind,
}

impl SequentialScenario {
    fn new(pool_mode: PoolMode, scenario_kind: PureJsScenarioKind) -> Self {
        let tempdir = tempfile::tempdir().expect("benchmark tempdir should build");
        let bundle = write_bundle(
            &tempdir,
            r#"
globalThis.__neovexInvoke = function (request) {
  return {
    ok: true,
    functionName: request.function_name,
    kind: request.kind,
  };
};

export {};
"#,
        );
        let (runtime, executor) = build_runtime(
            Arc::new(NoopHost),
            pool_mode,
            scenario_kind.execution_model(),
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: benchmark_request(),
            tenant_labels: scenario_kind.tenant_labels(),
            next_tenant_index: 0,
            pool_mode,
            scenario_kind,
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
            PoolMode::StartupSnapshotCache => {
                assert_eq!(snapshot.bundle_loads, total_invocations);
                assert_eq!(snapshot.bundle_module_loads, total_invocations);
                assert_eq!(snapshot.bundle_evaluations, total_invocations);
            }
        }
        match self.pool_mode {
            PoolMode::StartupSnapshotCache => {
                assert_eq!(snapshot.isolate_pool_misses, 1);
                assert_eq!(
                    snapshot.isolate_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.retained_runtime_pool_entries, 0);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmPool => {
                // Already asserted above
            }
        }
        maybe_report_phase_metrics_once(
            self.scenario_kind.label(),
            self.pool_mode.label(),
            &snapshot,
            total_invocations,
        );
    }
}

struct AsyncHostBatchScenario {
    _tempdir: TempDir,
    runtime: NeovexRuntime,
    executor: RuntimeExecutor,
    bundle: neovex_runtime::RuntimeBundle,
    request: InvocationRequest,
    tenant_labels: &'static [&'static str],
    next_tenant_index: usize,
    pool_mode: PoolMode,
    scenario_kind: AsyncHostBatchScenarioKind,
}

impl AsyncHostBatchScenario {
    fn new(pool_mode: PoolMode, scenario_kind: AsyncHostBatchScenarioKind) -> Self {
        let tempdir = tempfile::tempdir().expect("benchmark tempdir should build");
        let bundle = write_bundle(
            &tempdir,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
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
            Arc::new(DelayedAsyncHost::new(Duration::from_millis(1))),
            pool_mode,
            scenario_kind.execution_model(),
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: benchmark_request(),
            tenant_labels: scenario_kind.tenant_labels(),
            next_tenant_index: 0,
            pool_mode,
            scenario_kind,
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
                assert_eq!(snapshot.isolate_pool_misses, 1);
                assert_eq!(
                    snapshot.isolate_pool_hits,
                    total_invocations.saturating_sub(1)
                );
                assert_eq!(snapshot.retained_runtime_pool_entries, 0);
                assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
                assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
            }
            PoolMode::WarmPool => {
                // Warm pool metrics are validated at the top-level match
            }
        }
        maybe_report_phase_metrics_once(
            self.scenario_kind.label(),
            self.pool_mode.label(),
            &snapshot,
            total_invocations,
        );
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
    async_host_batch_benchmark
);
criterion_main!(benches);
