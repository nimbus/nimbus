use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput};
use nimbus_runtime::{
    HostBridge, InvocationRequest, RuntimeAdaptiveActuationResult, RuntimeAdaptiveActuator,
    RuntimeAdaptiveClock, RuntimeAdaptiveControllerMetricsSnapshot, RuntimeAdaptiveControllerMode,
    RuntimeAdaptiveControllerSettings, RuntimeAdaptiveObservationSource,
    RuntimeAdaptivePressureAdapter, RuntimeAdaptiveWarmPoolActuationKind,
    RuntimeAdaptiveWarmPoolAuthorityInput, RuntimeAdaptiveWarmPoolController,
    RuntimeAdaptiveWarmPoolDecision, RuntimeAdaptiveWarmPoolEvaluation, RuntimeAdaptiveWarmPoolRun,
    RuntimeAdaptiveWarmPoolSnapshot, RuntimeControllerReplayAuthorityInput,
    RuntimeControllerReplayAuthorityKey, RuntimeControllerReplayConfig,
    RuntimeControllerReplayDecision, RuntimeControllerReplayObservation,
    RuntimeControllerReplayState, RuntimeExecutionModel, RuntimeHostPressureLevel,
    RuntimeHostPressureSample, RuntimeHostResourceBudget, RuntimeHostResourceDecision,
    RuntimeInvocationContext, RuntimeMemoryPressureDecision, RuntimeMemoryPressureLevel,
    RuntimeMemoryPressureSample, RuntimeMemoryPressureSourceStatus, RuntimeMetrics,
    RuntimeMetricsSnapshot, RuntimeProfile, RuntimeRoutingAffinity, replay_runtime_controller,
};
use serde::Serialize;

use super::{
    BenchmarkProfile, CodeCacheState, DelayedAsyncHost, NodeFullNfr6WorkloadKind, NoopHost,
    PoolMode, PureJsWorkloadKind, build_runtime_with_config, current_rss_bytes,
    current_rss_source_label, duration_nanos_u64, execution_model_label, percentile_nanos,
    write_bundle, write_nfr6_workload_bundle,
};

const POST_PIR_TRACE_SCHEMA: &str = "nimbus.profile_aware_isolate_runtime.post_pir.optimization.v1";
const POST_PIR_FANOUT_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.fanout.v1";
const POST_PIR_HOT_TAIL_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.hot_tail_prewarm.v1";
const POST_PIR_POOL_SIZING_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.pool_sizing.v1";
const POST_PIR_COOPERATIVE_MIXED_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.cooperative_mixed.v1";
const POST_PIR_FRAGMENTATION_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.fragmentation.v1";
const POST_PIR_CODE_CACHE_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.code_cache.v1";
const POST_PIR_NODE_LAZY_INIT_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.node_lazy_init.v1";
const POST_PIR_CONTROLLER_REPLAY_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.controller_replay.v1";
const POST_PIR_LIVE_ADAPTIVE_TRACE_SCHEMA: &str =
    "nimbus.profile_aware_isolate_runtime.post_pir.live_adaptive_controller.v1";
const POST_PIR_GROUP: &str = "runtime_pool_modes_post_pir_optimization";
const POST_PIR_FANOUT_GROUP: &str = "runtime_pool_modes_post_pir_fanout";
const POST_PIR_HOT_TAIL_GROUP: &str = "runtime_pool_modes_post_pir_hot_tail_prewarm";
const POST_PIR_POOL_SIZING_GROUP: &str = "runtime_pool_modes_post_pir_pool_sizing";
const POST_PIR_COOPERATIVE_MIXED_GROUP: &str = "runtime_pool_modes_post_pir_cooperative_mixed";
const POST_PIR_FRAGMENTATION_GROUP: &str = "runtime_pool_modes_post_pir_fragmentation";
const POST_PIR_CODE_CACHE_GROUP: &str = "runtime_pool_modes_post_pir_code_cache";
const POST_PIR_NODE_LAZY_INIT_GROUP: &str = "runtime_pool_modes_post_pir_node_lazy_init";
const POST_PIR_CONTROLLER_REPLAY_GROUP: &str = "runtime_pool_modes_post_pir_controller_replay";
const POST_PIR_LIVE_ADAPTIVE_GROUP: &str = "runtime_pool_modes_post_pir_live_adaptive_controller";
const POST_PIR_HOT_TAIL_AUTHORITY_FANOUT: usize = 64;
const POST_PIR_HOT_TAIL_HOT_AUTHORITIES: usize = 8;
const POST_PIR_TRACE_MIN_HOT_TAIL_ITERATIONS: u64 = 128;
const POST_PIR_TRACE_MIN_COOPERATIVE_MIXED_WAVES: u64 = 32;
const POST_PIR_TRACE_MIN_FRAGMENTATION_ITERATIONS: u64 = 128;
const POST_PIR_TRACE_MIN_CODE_CACHE_ITERATIONS: u64 = 64;
const POST_PIR_TRACE_MIN_NODE_LAZY_INIT_ITERATIONS: u64 = 32;
const POST_PIR_TRACE_MIN_CONTROLLER_REPLAY_ITERATIONS: u64 = 512;
const POST_PIR_TRACE_MIN_LIVE_ADAPTIVE_ITERATIONS: u64 = 512;

pub(crate) fn post_pir_optimization_benchmark(c: &mut Criterion) {
    let include_matrix_rows = include_post_pir_optimization_rows();
    let include_fanout_rows = include_post_pir_fanout_rows();
    let include_hot_tail_rows = include_post_pir_hot_tail_rows();
    let include_pool_sizing_rows = include_post_pir_pool_sizing_rows();
    let include_cooperative_mixed_rows = include_post_pir_cooperative_mixed_rows();
    let include_fragmentation_rows = include_post_pir_fragmentation_rows();
    let include_code_cache_rows = include_post_pir_code_cache_rows();
    let include_node_lazy_init_rows = include_post_pir_node_lazy_init_rows();
    let include_controller_replay_rows = include_post_pir_controller_replay_rows();
    let include_live_adaptive_rows = include_post_pir_live_adaptive_rows();
    if !include_matrix_rows
        && !include_fanout_rows
        && !include_hot_tail_rows
        && !include_pool_sizing_rows
        && !include_cooperative_mixed_rows
        && !include_fragmentation_rows
        && !include_code_cache_rows
        && !include_node_lazy_init_rows
        && !include_controller_replay_rows
        && !include_live_adaptive_rows
    {
        return;
    }

    if include_matrix_rows {
        run_post_pir_optimization_matrix(c);
    }
    if include_fanout_rows {
        run_post_pir_fanout_matrix(c);
    }
    if include_hot_tail_rows {
        run_post_pir_hot_tail_prewarm_matrix(c);
    }
    if include_pool_sizing_rows {
        run_post_pir_pool_sizing_matrix(c);
    }
    if include_cooperative_mixed_rows {
        run_post_pir_cooperative_mixed_matrix(c);
    }
    if include_fragmentation_rows {
        run_post_pir_fragmentation_matrix(c);
    }
    if include_code_cache_rows {
        run_post_pir_code_cache_matrix(c);
    }
    if include_node_lazy_init_rows {
        run_post_pir_node_lazy_init_matrix(c);
    }
    if include_controller_replay_rows {
        run_post_pir_controller_replay_matrix(c);
    }
    if include_live_adaptive_rows {
        run_post_pir_live_adaptive_matrix(c);
    }
}

fn run_post_pir_optimization_matrix(c: &mut Criterion) {
    if !include_post_pir_optimization_rows() {
        return;
    }

    let mut group = c.benchmark_group(POST_PIR_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in PostPirWorkload::all() {
        for &tenant_distribution in PostPirTenantDistribution::all() {
            for &pool_path in PostPirPoolPath::all() {
                let benchmark_id = BenchmarkId::new(
                    format!("{}/{}", workload.label(), tenant_distribution.label()),
                    pool_path.label_for_profile(BenchmarkProfile::WebStandard),
                );
                group.bench_with_input(
                    benchmark_id,
                    &(workload, tenant_distribution, pool_path),
                    |b, &(workload, tenant_distribution, pool_path)| {
                        b.iter_custom(|iters| {
                            let mut scenario =
                                PostPirScenario::new(workload, tenant_distribution, pool_path);
                            scenario.prime();
                            let trace_enabled =
                                std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
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
                            scenario.emit_trace(
                                iters,
                                elapsed,
                                &latency_nanos,
                                rss_before_bytes,
                                rss_after_bytes,
                            );
                            std::hint::black_box(scenario.metrics_snapshot());
                            elapsed
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

fn include_post_pir_optimization_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_OPTIMIZATION_BENCH").is_some()
        || std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some()
}

fn include_post_pir_fanout_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_FANOUT_BENCH").is_some()
}

fn include_post_pir_hot_tail_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_HOT_TAIL_BENCH").is_some()
}

fn include_post_pir_pool_sizing_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_POOL_SIZING_BENCH").is_some()
}

fn include_post_pir_cooperative_mixed_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_COOPERATIVE_MIXED_BENCH").is_some()
}

fn include_post_pir_fragmentation_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_FRAGMENTATION_BENCH").is_some()
}

fn include_post_pir_code_cache_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_CODE_CACHE_BENCH").is_some()
}

fn include_post_pir_node_lazy_init_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_NODE_LAZY_INIT_BENCH").is_some()
}

fn include_post_pir_controller_replay_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_CONTROLLER_REPLAY_BENCH").is_some()
}

fn include_post_pir_live_adaptive_rows() -> bool {
    std::env::var_os("NIMBUS_POST_PIR_LIVE_ADAPTIVE_BENCH").is_some()
}

#[derive(Clone, Copy)]
enum PostPirPoolPath {
    ExactKeyWarmPool,
    OpenWorkersOwnerKeyedDiagnostic,
    StartupSnapshotCache,
}

impl PostPirPoolPath {
    fn all() -> &'static [Self] {
        &[
            Self::ExactKeyWarmPool,
            Self::OpenWorkersOwnerKeyedDiagnostic,
            Self::StartupSnapshotCache,
        ]
    }

    fn fanout_all() -> &'static [Self] {
        &[
            Self::ExactKeyWarmPool,
            Self::OpenWorkersOwnerKeyedDiagnostic,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::ExactKeyWarmPool => "webstandard_exact_key_warm_pool",
            Self::OpenWorkersOwnerKeyedDiagnostic => "openworkers_owner_keyed_diagnostic",
            Self::StartupSnapshotCache => "startup_snapshot_cache",
        }
    }

    fn label_for_profile(self, profile: BenchmarkProfile) -> &'static str {
        match (self, profile.is_node_full()) {
            (Self::StartupSnapshotCache, false) => "unsnapshotted_runtime_cache",
            _ => self.label(),
        }
    }

    fn pool_mode(self) -> PoolMode {
        match self {
            Self::ExactKeyWarmPool | Self::OpenWorkersOwnerKeyedDiagnostic => PoolMode::WarmPool,
            Self::StartupSnapshotCache => PoolMode::StartupSnapshotCache,
        }
    }

    fn routing_affinity(
        self,
        tenant_distribution: PostPirTenantDistribution,
    ) -> RuntimeRoutingAffinity {
        match self {
            Self::OpenWorkersOwnerKeyedDiagnostic => RuntimeRoutingAffinity::None,
            Self::ExactKeyWarmPool | Self::StartupSnapshotCache => {
                tenant_distribution.routing_affinity()
            }
        }
    }

    fn is_authority_relaxed_diagnostic(self) -> bool {
        matches!(self, Self::OpenWorkersOwnerKeyedDiagnostic)
    }
}

#[derive(Clone, Copy)]
struct PostPirFanoutShape {
    authority_fanout: usize,
    retained_cap: usize,
}

impl PostPirFanoutShape {
    fn all() -> &'static [Self] {
        &[
            Self {
                authority_fanout: 8,
                retained_cap: 1,
            },
            Self {
                authority_fanout: 8,
                retained_cap: 8,
            },
            Self {
                authority_fanout: 32,
                retained_cap: 8,
            },
            Self {
                authority_fanout: 32,
                retained_cap: 32,
            },
            Self {
                authority_fanout: 64,
                retained_cap: 16,
            },
            Self {
                authority_fanout: 64,
                retained_cap: 64,
            },
        ]
    }

    fn label(self) -> String {
        format!(
            "fanout_{}_retained_cap_{}",
            self.authority_fanout, self.retained_cap
        )
    }
}

#[derive(Clone, Copy)]
struct PostPirHotTailPrewarmShape {
    requested_prewarm_entries: usize,
    retained_cap: usize,
    pressure: PostPirPrewarmPressure,
}

impl PostPirHotTailPrewarmShape {
    fn all() -> &'static [Self] {
        &[
            Self {
                requested_prewarm_entries: 0,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::Nominal,
            },
            Self {
                requested_prewarm_entries: 4,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::Nominal,
            },
            Self {
                requested_prewarm_entries: 8,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::Nominal,
            },
            Self {
                requested_prewarm_entries: 16,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::Nominal,
            },
            Self {
                requested_prewarm_entries: 8,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::HighMemory,
            },
            Self {
                requested_prewarm_entries: 8,
                retained_cap: 16,
                pressure: PostPirPrewarmPressure::CriticalMemory,
            },
        ]
    }

    fn label(self) -> String {
        format!(
            "prewarm_{}_cap_{}_pressure_{}",
            self.requested_prewarm_entries,
            self.retained_cap,
            self.pressure.label()
        )
    }

    fn schedule_decision(self) -> nimbus_runtime::RuntimePrewarmScheduleDecision {
        self.pressure
            .memory_decision()
            .schedule_prewarm_entries(self.requested_prewarm_entries)
    }
}

#[derive(Clone, Copy)]
struct PostPirPoolSizingShape {
    retained_cap: usize,
}

impl PostPirPoolSizingShape {
    fn all() -> &'static [Self] {
        &[
            Self { retained_cap: 4 },
            Self { retained_cap: 8 },
            Self { retained_cap: 12 },
            Self { retained_cap: 16 },
            Self { retained_cap: 24 },
            Self { retained_cap: 32 },
            Self { retained_cap: 48 },
            Self { retained_cap: 64 },
        ]
    }

    fn label(self) -> String {
        format!("hot_tail_fanout_64_retained_cap_{}", self.retained_cap)
    }
}

#[derive(Clone, Copy)]
struct PostPirCooperativeMixedShape {
    label: &'static str,
    async_host_invocations_per_wave: usize,
    compute_invocations_per_wave: usize,
    synthetic_await_ms: u64,
    submit_order: PostPirCooperativeSubmitOrder,
}

impl PostPirCooperativeMixedShape {
    fn all() -> &'static [Self] {
        &[
            Self {
                label: "io_only_4x1ms",
                async_host_invocations_per_wave: 4,
                compute_invocations_per_wave: 0,
                synthetic_await_ms: 1,
                submit_order: PostPirCooperativeSubmitOrder::IoFirst,
            },
            Self {
                label: "balanced_io_first_2io_2cpu",
                async_host_invocations_per_wave: 2,
                compute_invocations_per_wave: 2,
                synthetic_await_ms: 1,
                submit_order: PostPirCooperativeSubmitOrder::IoFirst,
            },
            Self {
                label: "balanced_cpu_first_2cpu_2io",
                async_host_invocations_per_wave: 2,
                compute_invocations_per_wave: 2,
                synthetic_await_ms: 1,
                submit_order: PostPirCooperativeSubmitOrder::CpuFirst,
            },
            Self {
                label: "cpu_heavy_cpu_first_1io_3cpu",
                async_host_invocations_per_wave: 1,
                compute_invocations_per_wave: 3,
                synthetic_await_ms: 1,
                submit_order: PostPirCooperativeSubmitOrder::CpuFirst,
            },
        ]
    }

    fn total_invocations_per_wave(self) -> usize {
        self.async_host_invocations_per_wave
            .saturating_add(self.compute_invocations_per_wave)
    }
}

#[derive(Clone, Copy)]
enum PostPirCooperativeSubmitOrder {
    IoFirst,
    CpuFirst,
}

impl PostPirCooperativeSubmitOrder {
    fn label(self) -> &'static str {
        match self {
            Self::IoFirst => "io_first",
            Self::CpuFirst => "cpu_first",
        }
    }
}

#[derive(Clone, Copy)]
struct PostPirFragmentationShape {
    dimension: PostPirFragmentationDimension,
    authority_fanout: usize,
    retained_cap: usize,
}

impl PostPirFragmentationShape {
    fn all() -> &'static [Self] {
        &[
            Self {
                dimension: PostPirFragmentationDimension::Tenant,
                authority_fanout: 32,
                retained_cap: 16,
            },
            Self {
                dimension: PostPirFragmentationDimension::Tenant,
                authority_fanout: 32,
                retained_cap: 32,
            },
            Self {
                dimension: PostPirFragmentationDimension::Function,
                authority_fanout: 32,
                retained_cap: 16,
            },
            Self {
                dimension: PostPirFragmentationDimension::Function,
                authority_fanout: 32,
                retained_cap: 32,
            },
            Self {
                dimension: PostPirFragmentationDimension::Script,
                authority_fanout: 32,
                retained_cap: 16,
            },
            Self {
                dimension: PostPirFragmentationDimension::Script,
                authority_fanout: 32,
                retained_cap: 32,
            },
        ]
    }

    fn label(self) -> String {
        format!(
            "{}_fanout_{}_retained_cap_{}",
            self.dimension.label(),
            self.authority_fanout,
            self.retained_cap
        )
    }
}

#[derive(Clone, Copy)]
enum PostPirFragmentationDimension {
    Tenant,
    Function,
    Script,
}

impl PostPirFragmentationDimension {
    fn label(self) -> &'static str {
        match self {
            Self::Tenant => "tenant",
            Self::Function => "function",
            Self::Script => "script",
        }
    }

    fn routing_affinity(self) -> RuntimeRoutingAffinity {
        match self {
            Self::Tenant => RuntimeRoutingAffinity::Tenant,
            Self::Function => RuntimeRoutingAffinity::Function,
            Self::Script => RuntimeRoutingAffinity::Script,
        }
    }
}

#[derive(Clone, Copy)]
enum PostPirControllerReplayShape {
    SteadyNominal,
    BurstSpillover,
    MemoryPressurePanic,
    ZipfTenantCap,
    PeriodicDecay,
}

#[derive(Clone, Copy)]
enum PostPirLiveAdaptiveShape {
    DisabledStatic,
    ShadowBurst,
    CanaryAdmittedBurst,
    CanaryExcludedBurst,
    LiveMemoryPressure,
    RollbackPeriodic,
    LiveZipfTenantCap,
}

impl PostPirLiveAdaptiveShape {
    fn all() -> &'static [Self] {
        &[
            Self::DisabledStatic,
            Self::ShadowBurst,
            Self::CanaryAdmittedBurst,
            Self::CanaryExcludedBurst,
            Self::LiveMemoryPressure,
            Self::RollbackPeriodic,
            Self::LiveZipfTenantCap,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::DisabledStatic => "disabled_static",
            Self::ShadowBurst => "shadow_burst",
            Self::CanaryAdmittedBurst => "canary_admitted_burst",
            Self::CanaryExcludedBurst => "canary_excluded_burst",
            Self::LiveMemoryPressure => "live_memory_pressure",
            Self::RollbackPeriodic => "rollback_periodic",
            Self::LiveZipfTenantCap => "live_zipf_tenant_cap",
        }
    }
}

impl PostPirControllerReplayShape {
    fn all() -> &'static [Self] {
        &[
            Self::SteadyNominal,
            Self::BurstSpillover,
            Self::MemoryPressurePanic,
            Self::ZipfTenantCap,
            Self::PeriodicDecay,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::SteadyNominal => "steady_nominal",
            Self::BurstSpillover => "burst_spillover",
            Self::MemoryPressurePanic => "memory_pressure_panic",
            Self::ZipfTenantCap => "zipf_tenant_cap",
            Self::PeriodicDecay => "periodic_decay",
        }
    }
}

#[derive(Clone, Copy)]
enum PostPirPrewarmPressure {
    Nominal,
    HighMemory,
    CriticalMemory,
}

impl PostPirPrewarmPressure {
    fn label(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::HighMemory => "high_memory",
            Self::CriticalMemory => "critical_memory",
        }
    }

    fn memory_decision(self) -> RuntimeMemoryPressureDecision {
        match self {
            Self::Nominal => RuntimeMemoryPressureDecision::for_level(
                RuntimeMemoryPressureLevel::Nominal,
                RuntimeMemoryPressureSourceStatus::Observed,
            ),
            Self::HighMemory => RuntimeMemoryPressureDecision::for_level(
                RuntimeMemoryPressureLevel::High,
                RuntimeMemoryPressureSourceStatus::Observed,
            ),
            Self::CriticalMemory => RuntimeMemoryPressureDecision::for_level(
                RuntimeMemoryPressureLevel::Critical,
                RuntimeMemoryPressureSourceStatus::Observed,
            ),
        }
    }
}

#[derive(Clone, Copy)]
enum PostPirTenantDistribution {
    SingleTenant,
    ZipfHotTenant,
    HighAuthorityFragmentation,
}

impl PostPirTenantDistribution {
    fn all() -> &'static [Self] {
        &[
            Self::SingleTenant,
            Self::ZipfHotTenant,
            Self::HighAuthorityFragmentation,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::SingleTenant => "single_tenant",
            Self::ZipfHotTenant => "zipf_hot_tenant",
            Self::HighAuthorityFragmentation => "high_authority_fragmentation",
        }
    }

    fn routing_affinity(self) -> RuntimeRoutingAffinity {
        match self {
            Self::SingleTenant | Self::ZipfHotTenant => RuntimeRoutingAffinity::Tenant,
            Self::HighAuthorityFragmentation => RuntimeRoutingAffinity::Function,
        }
    }

    fn tenant_at(self, index: usize) -> &'static str {
        match self {
            Self::SingleTenant => "tenant-a",
            Self::ZipfHotTenant => {
                const ZIPF: &[&str] = &[
                    "tenant-a", "tenant-a", "tenant-a", "tenant-a", "tenant-a", "tenant-b",
                    "tenant-b", "tenant-c",
                ];
                ZIPF[index % ZIPF.len()]
            }
            Self::HighAuthorityFragmentation => {
                const TENANTS: &[&str] = &[
                    "tenant-a", "tenant-b", "tenant-c", "tenant-d", "tenant-e", "tenant-f",
                    "tenant-g", "tenant-h",
                ];
                TENANTS[index % TENANTS.len()]
            }
        }
    }

    fn function_at(self, index: usize) -> &'static str {
        match self {
            Self::SingleTenant | Self::ZipfHotTenant => "messages:list",
            Self::HighAuthorityFragmentation => {
                const FUNCTIONS: &[&str] = &[
                    "messages:list",
                    "messages:search",
                    "messages:recent",
                    "messages:stats",
                    "notifications:list",
                    "notifications:summary",
                    "users:profile",
                    "users:settings",
                ];
                FUNCTIONS[index % FUNCTIONS.len()]
            }
        }
    }

    fn prime_count(self) -> usize {
        match self {
            Self::SingleTenant => 1,
            Self::ZipfHotTenant | Self::HighAuthorityFragmentation => 8,
        }
    }
}

#[derive(Clone, Copy)]
enum PostPirWorkload {
    Pure(PureJsWorkloadKind),
    AsyncHostCall,
}

impl PostPirWorkload {
    fn all() -> &'static [Self] {
        &[
            Self::Pure(PureJsWorkloadKind::HostlessTrivial),
            Self::Pure(PureJsWorkloadKind::SetupHeavy),
            Self::Pure(PureJsWorkloadKind::ComputeBound),
            Self::AsyncHostCall,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pure(workload) => workload.label(),
            Self::AsyncHostCall => "async_host_call",
        }
    }

    fn synthetic_await_ms(self) -> Option<u64> {
        match self {
            Self::Pure(_) => None,
            Self::AsyncHostCall => Some(1),
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Pure(workload) => workload.source(),
            Self::AsyncHostCall => {
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
    functionName: request.function_name,
  };
};

export {};
"#
            }
        }
    }

    fn host(self) -> Arc<dyn HostBridge> {
        match self {
            Self::Pure(_) => Arc::new(NoopHost),
            Self::AsyncHostCall => Arc::new(DelayedAsyncHost::new(Duration::from_millis(1))),
        }
    }
}

struct PostPirScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    workload: PostPirWorkload,
    tenant_distribution: PostPirTenantDistribution,
    pool_path: PostPirPoolPath,
    next_invocation_index: usize,
}

impl PostPirScenario {
    fn new(
        workload: PostPirWorkload,
        tenant_distribution: PostPirTenantDistribution,
        pool_path: PostPirPoolPath,
    ) -> Self {
        let tempdir = tempfile::tempdir().expect("post-PIR benchmark tempdir should build");
        let bundle = write_bundle(&tempdir, workload.source());
        let (runtime, executor) = build_runtime_with_config(
            BenchmarkProfile::WebStandard,
            workload.host(),
            pool_path.pool_mode(),
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = pool_path.routing_affinity(tenant_distribution);
                limits.max_warm_pool_entries_per_worker = 32;
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: super::benchmark_request(),
            workload,
            tenant_distribution,
            pool_path,
            next_invocation_index: 0,
        }
    }

    fn prime(&mut self) {
        for _ in 0..self.tenant_distribution.prime_count() {
            self.invoke_once();
        }
    }

    fn invoke_once(&mut self) {
        let index = self.next_invocation_index;
        self.next_invocation_index = self.next_invocation_index.saturating_add(1);
        let tenant_label = self.tenant_distribution.tenant_at(index);
        let mut request = self.request.clone();
        request.function_name = self.tenant_distribution.function_at(index).to_string();
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            self.bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, tenant_label),
        );
        let result = result.expect("post-PIR benchmark invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_bytes: Option<u64>,
        rss_after_bytes: Option<u64>,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let total_invocations = self
            .tenant_distribution
            .prime_count()
            .saturating_add(measured_iterations as usize) as u64;
        let benchmark_id = format!(
            "{}/{}/{}",
            self.workload.label(),
            self.tenant_distribution.label(),
            self.pool_path
                .label_for_profile(BenchmarkProfile::WebStandard)
        );
        maybe_emit_post_pir_trace_record(
            path,
            PostPirTraceRecord {
                schema: POST_PIR_TRACE_SCHEMA,
                benchmark_group: POST_PIR_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.workload.label(),
                pool_path: self
                    .pool_path
                    .label_for_profile(BenchmarkProfile::WebStandard),
                pool_kind: self.pool_path.pool_mode().label(),
                routing_affinity: routing_affinity_label(
                    self.pool_path.routing_affinity(self.tenant_distribution),
                ),
                authority_relaxed_diagnostic: self.pool_path.is_authority_relaxed_diagnostic(),
                tenant_distribution: self.tenant_distribution.label(),
                execution_model: execution_model_label(RuntimeExecutionModel::CooperativeLocker),
                synthetic_await_ms: self.workload.synthetic_await_ms(),
                measured_iterations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                rss_source: current_rss_source_label(),
                rss_before_bytes,
                rss_after_bytes,
                rss_delta_bytes: rss_before_bytes
                    .zip(rss_after_bytes)
                    .map(|(before, after)| after.saturating_sub(before)),
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
                host_pressure_decisions: snapshot.host_pressure.decisions,
                host_pressure_high_decisions: snapshot.host_pressure.high_decisions,
                host_pressure_critical_decisions: snapshot.host_pressure.critical_decisions,
                latest_effective_dispatch_seats: snapshot
                    .host_pressure
                    .latest_effective_dispatch_seats,
            },
        );
    }
}

fn run_post_pir_fanout_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_FANOUT_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in post_pir_fanout_workloads() {
        for &shape in PostPirFanoutShape::all() {
            for &pool_path in PostPirPoolPath::fanout_all() {
                let benchmark_id = BenchmarkId::new(
                    format!("{}/{}", workload.label(), shape.label()),
                    pool_path.label(),
                );
                group.bench_with_input(
                    benchmark_id,
                    &(workload, shape, pool_path),
                    |b, &(workload, shape, pool_path)| {
                        b.iter_custom(|iters| {
                            let mut scenario =
                                PostPirFanoutScenario::new(workload, shape, pool_path);
                            let rss_before_bytes = current_rss_bytes();
                            scenario.prime();
                            let rss_after_prime_bytes = current_rss_bytes();
                            let trace_enabled =
                                std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
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
                            let rss_after_measurement_bytes = current_rss_bytes();
                            scenario.emit_trace(
                                iters,
                                elapsed,
                                &latency_nanos,
                                rss_before_bytes,
                                rss_after_prime_bytes,
                                rss_after_measurement_bytes,
                            );
                            std::hint::black_box(scenario.metrics_snapshot());
                            elapsed
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

fn post_pir_fanout_workloads() -> &'static [PureJsWorkloadKind] {
    &[
        PureJsWorkloadKind::HostlessTrivial,
        PureJsWorkloadKind::SetupHeavy,
    ]
}

struct PostPirFanoutScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    workload: PureJsWorkloadKind,
    shape: PostPirFanoutShape,
    pool_path: PostPirPoolPath,
    next_invocation_index: usize,
}

impl PostPirFanoutScenario {
    fn new(
        workload: PureJsWorkloadKind,
        shape: PostPirFanoutShape,
        pool_path: PostPirPoolPath,
    ) -> Self {
        let tempdir = tempfile::tempdir().expect("post-PIR fanout benchmark tempdir should build");
        let bundle = write_bundle(&tempdir, workload.source());
        let (runtime, executor) = build_runtime_with_config(
            BenchmarkProfile::WebStandard,
            Arc::new(NoopHost),
            pool_path.pool_mode(),
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = match pool_path {
                    PostPirPoolPath::OpenWorkersOwnerKeyedDiagnostic => {
                        RuntimeRoutingAffinity::None
                    }
                    _ => RuntimeRoutingAffinity::Function,
                };
                limits.routing_affinity_max_entries = shape.authority_fanout.max(1);
                limits.max_warm_pool_entries_per_worker = shape.retained_cap.max(1);
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: super::benchmark_request(),
            workload,
            shape,
            pool_path,
            next_invocation_index: 0,
        }
    }

    fn prime(&mut self) {
        for index in 0..self.shape.authority_fanout {
            self.invoke_index(index);
        }
    }

    fn invoke_once(&mut self) {
        let index = self.next_invocation_index % self.shape.authority_fanout;
        self.next_invocation_index = self.next_invocation_index.saturating_add(1);
        self.invoke_index(index);
    }

    fn invoke_index(&self, index: usize) {
        let authority_index = index % self.shape.authority_fanout;
        let mut request = self.request.clone();
        request.function_name = format!("messages:fanout_{authority_index:04}");
        let tenant_label = format!("tenant-{authority_index:04}");
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            self.bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, tenant_label),
        );
        let result = result.expect("post-PIR fanout benchmark invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_bytes: Option<u64>,
        rss_after_prime_bytes: Option<u64>,
        rss_after_measurement_bytes: Option<u64>,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let prime_invocations = self.shape.authority_fanout as u64;
        let total_invocations = prime_invocations.saturating_add(measured_iterations);
        let benchmark_id = format!(
            "{}/{}/{}",
            self.workload.label(),
            self.shape.label(),
            self.pool_path
                .label_for_profile(BenchmarkProfile::WebStandard)
        );
        let rss_prime_delta_bytes = rss_before_bytes
            .zip(rss_after_prime_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let rss_measurement_delta_bytes = rss_before_bytes
            .zip(rss_after_measurement_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let rss_per_retained_entry_bytes =
            rss_prime_delta_bytes.and_then(|delta| match snapshot.retained_runtime_pool_entries {
                0 => None,
                retained_entries => Some(delta / retained_entries as u64),
            });
        maybe_emit_post_pir_fanout_trace_record(
            path,
            PostPirFanoutTraceRecord {
                schema: POST_PIR_FANOUT_TRACE_SCHEMA,
                benchmark_group: POST_PIR_FANOUT_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.workload.label(),
                pool_path: self
                    .pool_path
                    .label_for_profile(BenchmarkProfile::WebStandard),
                pool_kind: self.pool_path.pool_mode().label(),
                routing_affinity: routing_affinity_label(match self.pool_path {
                    PostPirPoolPath::OpenWorkersOwnerKeyedDiagnostic => {
                        RuntimeRoutingAffinity::None
                    }
                    _ => RuntimeRoutingAffinity::Function,
                }),
                authority_relaxed_diagnostic: self.pool_path.is_authority_relaxed_diagnostic(),
                authority_fanout: self.shape.authority_fanout,
                retained_cap: self.shape.retained_cap,
                prime_invocations,
                measured_iterations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                rss_source: current_rss_source_label(),
                rss_before_bytes,
                rss_after_prime_bytes,
                rss_after_measurement_bytes,
                rss_prime_delta_bytes,
                rss_measurement_delta_bytes,
                rss_per_retained_entry_bytes,
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
                host_pressure_decisions: snapshot.host_pressure.decisions,
                host_pressure_high_decisions: snapshot.host_pressure.high_decisions,
                host_pressure_critical_decisions: snapshot.host_pressure.critical_decisions,
                latest_effective_dispatch_seats: snapshot
                    .host_pressure
                    .latest_effective_dispatch_seats,
            },
        );
    }
}

fn run_post_pir_hot_tail_prewarm_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_HOT_TAIL_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in post_pir_hot_tail_workloads() {
        for &shape in PostPirHotTailPrewarmShape::all() {
            let benchmark_id = BenchmarkId::new(
                format!("{}/{}", workload.label(), shape.label()),
                "webstandard_exact_key_warm_pool",
            );
            group.bench_with_input(benchmark_id, &(workload, shape), |b, &(workload, shape)| {
                b.iter_custom(|iters| {
                    let mut scenario = PostPirHotTailScenario::new(workload, shape);
                    let rss_before_prewarm_bytes = current_rss_bytes();
                    scenario.prewarm();
                    let rss_after_prewarm_bytes = current_rss_bytes();
                    let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                    let measured_iterations = post_pir_trace_iterations(iters, trace_enabled);
                    let mut latency_nanos = if trace_enabled {
                        Vec::with_capacity(measured_iterations.min(100_000) as usize)
                    } else {
                        Vec::new()
                    };
                    let started_at = Instant::now();
                    for _ in 0..measured_iterations {
                        if trace_enabled {
                            let invocation_started_at = Instant::now();
                            scenario.invoke_once();
                            latency_nanos.push(duration_nanos_u64(invocation_started_at.elapsed()));
                        } else {
                            scenario.invoke_once();
                        }
                    }
                    let elapsed = started_at.elapsed();
                    let rss_after_measurement_bytes = current_rss_bytes();
                    scenario.emit_hot_tail_trace(
                        measured_iterations,
                        elapsed,
                        &latency_nanos,
                        rss_before_prewarm_bytes,
                        rss_after_prewarm_bytes,
                        rss_after_measurement_bytes,
                    );
                    std::hint::black_box(scenario.metrics_snapshot());
                    elapsed
                });
            });
        }
    }

    group.finish();
}

fn run_post_pir_pool_sizing_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_POOL_SIZING_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in post_pir_hot_tail_workloads() {
        for &shape in PostPirPoolSizingShape::all() {
            let benchmark_id = BenchmarkId::new(
                format!("{}/{}", workload.label(), shape.label()),
                "webstandard_exact_key_warm_pool",
            );
            group.bench_with_input(benchmark_id, &(workload, shape), |b, &(workload, shape)| {
                b.iter_custom(|iters| {
                    let mut scenario = PostPirPoolSizingScenario::new(workload, shape);
                    let rss_before_prewarm_bytes = current_rss_bytes();
                    scenario.prewarm_hot_set();
                    let rss_after_prewarm_bytes = current_rss_bytes();
                    let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                    let measured_iterations = post_pir_trace_iterations(iters, trace_enabled);
                    let mut latency_nanos = if trace_enabled {
                        Vec::with_capacity(measured_iterations.min(100_000) as usize)
                    } else {
                        Vec::new()
                    };
                    let started_at = Instant::now();
                    for _ in 0..measured_iterations {
                        if trace_enabled {
                            let invocation_started_at = Instant::now();
                            scenario.invoke_once();
                            latency_nanos.push(duration_nanos_u64(invocation_started_at.elapsed()));
                        } else {
                            scenario.invoke_once();
                        }
                    }
                    let elapsed = started_at.elapsed();
                    let rss_after_measurement_bytes = current_rss_bytes();
                    scenario.emit_pool_sizing_trace(
                        measured_iterations,
                        elapsed,
                        &latency_nanos,
                        rss_before_prewarm_bytes,
                        rss_after_prewarm_bytes,
                        rss_after_measurement_bytes,
                    );
                    std::hint::black_box(scenario.metrics_snapshot());
                    elapsed
                });
            });
        }
    }

    group.finish();
}

fn run_post_pir_cooperative_mixed_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_COOPERATIVE_MIXED_GROUP);
    group.throughput(Throughput::Elements(1));

    for &shape in PostPirCooperativeMixedShape::all() {
        let benchmark_id =
            BenchmarkId::new(shape.label, "webstandard_cooperative_exact_key_warm_pool");
        group.bench_with_input(benchmark_id, &shape, |b, &shape| {
            b.iter_custom(|iters| {
                let mut scenario = PostPirCooperativeMixedScenario::new(shape);
                scenario.prime();
                let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                let measured_waves = post_pir_cooperative_mixed_trace_waves(iters, trace_enabled);
                let mut latency = PostPirCooperativeMixedLatency::default();
                let started_at = Instant::now();
                for _ in 0..measured_waves {
                    let wave_latency = scenario.invoke_wave();
                    if trace_enabled {
                        latency.extend(wave_latency);
                    }
                }
                let elapsed = started_at.elapsed();
                scenario.emit_trace(measured_waves, elapsed, latency);
                std::hint::black_box(scenario.metrics_snapshot());
                elapsed
            });
        });
    }

    group.finish();
}

fn run_post_pir_fragmentation_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_FRAGMENTATION_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in post_pir_fragmentation_workloads() {
        for &shape in PostPirFragmentationShape::all() {
            let benchmark_id = BenchmarkId::new(
                format!("{}/{}", workload.label(), shape.label()),
                "webstandard_exact_key_warm_pool",
            );
            group.bench_with_input(benchmark_id, &(workload, shape), |b, &(workload, shape)| {
                b.iter_custom(|iters| {
                    let mut scenario = PostPirFragmentationScenario::new(workload, shape);
                    let rss_before_prime_bytes = current_rss_bytes();
                    scenario.prime();
                    let rss_after_prime_bytes = current_rss_bytes();
                    let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                    let measured_iterations =
                        post_pir_fragmentation_trace_iterations(iters, trace_enabled);
                    let mut latency_nanos = if trace_enabled {
                        Vec::with_capacity(measured_iterations.min(100_000) as usize)
                    } else {
                        Vec::new()
                    };
                    let started_at = Instant::now();
                    for _ in 0..measured_iterations {
                        if trace_enabled {
                            let invocation_started_at = Instant::now();
                            scenario.invoke_once();
                            latency_nanos.push(duration_nanos_u64(invocation_started_at.elapsed()));
                        } else {
                            scenario.invoke_once();
                        }
                    }
                    let elapsed = started_at.elapsed();
                    let rss_after_measurement_bytes = current_rss_bytes();
                    scenario.emit_trace(
                        measured_iterations,
                        elapsed,
                        &latency_nanos,
                        rss_before_prime_bytes,
                        rss_after_prime_bytes,
                        rss_after_measurement_bytes,
                    );
                    std::hint::black_box(scenario.metrics_snapshot());
                    elapsed
                });
            });
        }
    }

    group.finish();
}

fn run_post_pir_code_cache_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_CODE_CACHE_GROUP);
    group.throughput(Throughput::Elements(1));

    for &workload in post_pir_code_cache_workloads() {
        for &cache_state in [
            CodeCacheState::FreshBundleEachInvocation,
            CodeCacheState::PrimedBundleCodeCache,
        ]
        .iter()
        {
            let benchmark_id = BenchmarkId::new(
                format!("{}/unsnapshotted_runtime_cache", workload.label()),
                cache_state.label(),
            );
            group.bench_with_input(
                benchmark_id,
                &(workload, cache_state),
                |b, &(workload, cache_state)| {
                    b.iter_custom(|iters| {
                        let scenario = PostPirCodeCacheScenario::new(workload);
                        scenario.prime(cache_state);
                        let trace_enabled =
                            std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                        let measured_iterations =
                            post_pir_code_cache_trace_iterations(iters, trace_enabled);
                        let mut latency_nanos = if trace_enabled {
                            Vec::with_capacity(measured_iterations.min(100_000) as usize)
                        } else {
                            Vec::new()
                        };
                        let started_at = Instant::now();
                        for _ in 0..measured_iterations {
                            if trace_enabled {
                                let invocation_started_at = Instant::now();
                                scenario.invoke_once(cache_state);
                                latency_nanos
                                    .push(duration_nanos_u64(invocation_started_at.elapsed()));
                            } else {
                                scenario.invoke_once(cache_state);
                            }
                        }
                        let elapsed = started_at.elapsed();
                        scenario.emit_trace(
                            cache_state,
                            measured_iterations,
                            elapsed,
                            &latency_nanos,
                        );
                        std::hint::black_box(scenario.metrics_snapshot());
                        elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

fn run_post_pir_node_lazy_init_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_NODE_LAZY_INIT_GROUP);
    group.throughput(Throughput::Elements(1));

    for &profile in post_pir_node_lazy_init_profiles() {
        for &workload in post_pir_node_lazy_init_workloads() {
            let benchmark_id = BenchmarkId::new(
                format!("{}/{}", profile.label(), workload.label()),
                "startup_snapshot_cache",
            );
            group.bench_with_input(
                benchmark_id,
                &(profile, workload),
                |b, &(profile, workload)| {
                    b.iter_custom(|iters| {
                        let mut scenario = PostPirNodeLazyInitScenario::new(profile, workload);
                        scenario.prime();
                        let trace_enabled =
                            std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                        let measured_iterations =
                            post_pir_node_lazy_init_trace_iterations(iters, trace_enabled);
                        let mut latency_nanos = if trace_enabled {
                            Vec::with_capacity(measured_iterations.min(100_000) as usize)
                        } else {
                            Vec::new()
                        };
                        let started_at = Instant::now();
                        for _ in 0..measured_iterations {
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
                        scenario.emit_trace(measured_iterations, elapsed, &latency_nanos);
                        std::hint::black_box(scenario.metrics_snapshot());
                        elapsed
                    });
                },
            );
        }
    }

    group.finish();
}

fn run_post_pir_controller_replay_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_CONTROLLER_REPLAY_GROUP);
    group.throughput(Throughput::Elements(1));

    for &shape in PostPirControllerReplayShape::all() {
        let benchmark_id = BenchmarkId::from_parameter(shape.label());
        group.bench_with_input(benchmark_id, &shape, |b, &shape| {
            b.iter_custom(|iters| {
                let scenario = PostPirControllerReplayScenario::new(shape);
                let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                let measured_iterations =
                    post_pir_controller_replay_trace_iterations(iters, trace_enabled);
                let started_at = Instant::now();
                let mut decisions = Vec::new();
                for _ in 0..measured_iterations {
                    decisions = scenario.replay_once();
                    std::hint::black_box(&decisions);
                }
                let elapsed = started_at.elapsed();
                scenario.emit_trace(measured_iterations, elapsed, &decisions);
                elapsed
            });
        });
    }

    group.finish();
}

fn run_post_pir_live_adaptive_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group(POST_PIR_LIVE_ADAPTIVE_GROUP);
    group.throughput(Throughput::Elements(1));

    for &shape in PostPirLiveAdaptiveShape::all() {
        let benchmark_id = BenchmarkId::from_parameter(shape.label());
        group.bench_with_input(benchmark_id, &shape, |b, &shape| {
            b.iter_custom(|iters| {
                let scenario = PostPirLiveAdaptiveScenario::new(shape);
                let trace_enabled = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH").is_some();
                let measured_iterations =
                    post_pir_live_adaptive_trace_iterations(iters, trace_enabled);
                let started_at = Instant::now();
                let mut run = scenario.run_once();
                for _ in 1..measured_iterations {
                    run = scenario.run_once();
                    std::hint::black_box(&run);
                }
                let elapsed = started_at.elapsed();
                scenario.emit_trace(measured_iterations, elapsed, &run);
                elapsed
            });
        });
    }

    group.finish();
}

fn post_pir_hot_tail_workloads() -> &'static [PureJsWorkloadKind] {
    &[
        PureJsWorkloadKind::HostlessTrivial,
        PureJsWorkloadKind::SetupHeavy,
    ]
}

fn post_pir_fragmentation_workloads() -> &'static [PureJsWorkloadKind] {
    &[
        PureJsWorkloadKind::HostlessTrivial,
        PureJsWorkloadKind::SetupHeavy,
    ]
}

fn post_pir_code_cache_workloads() -> &'static [PureJsWorkloadKind] {
    &[
        PureJsWorkloadKind::HostlessTrivial,
        PureJsWorkloadKind::SetupHeavy,
    ]
}

fn post_pir_node_lazy_init_profiles() -> &'static [BenchmarkProfile] {
    &[BenchmarkProfile::Node22, BenchmarkProfile::Node24]
}

fn post_pir_node_lazy_init_workloads() -> &'static [NodeFullNfr6WorkloadKind] {
    &[
        NodeFullNfr6WorkloadKind::SetupHeavy,
        NodeFullNfr6WorkloadKind::LoaderHookDynamicBuiltin,
        NodeFullNfr6WorkloadKind::Node24CjsTranslatorBoundary,
    ]
}

fn post_pir_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    // Trace artifacts are the proof source for tail latency. Keep them on a
    // complete hot-tail window even when Criterion asks for very small samples.
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_HOT_TAIL_ITERATIONS)
    } else {
        iters
    }
}

fn post_pir_cooperative_mixed_trace_waves(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_COOPERATIVE_MIXED_WAVES)
    } else {
        iters
    }
}

fn post_pir_fragmentation_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_FRAGMENTATION_ITERATIONS)
    } else {
        iters
    }
}

fn post_pir_code_cache_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_CODE_CACHE_ITERATIONS)
    } else {
        iters
    }
}

fn post_pir_node_lazy_init_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_NODE_LAZY_INIT_ITERATIONS)
    } else {
        iters
    }
}

fn post_pir_controller_replay_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_CONTROLLER_REPLAY_ITERATIONS)
    } else {
        iters
    }
}

fn post_pir_live_adaptive_trace_iterations(iters: u64, trace_enabled: bool) -> u64 {
    if trace_enabled {
        iters.max(POST_PIR_TRACE_MIN_LIVE_ADAPTIVE_ITERATIONS)
    } else {
        iters
    }
}

fn hot_tail_authority_index(measured_index: usize) -> usize {
    let slot = measured_index % 16;
    if slot < 12 {
        return slot % POST_PIR_HOT_TAIL_HOT_AUTHORITIES;
    }
    let tail_width = POST_PIR_HOT_TAIL_AUTHORITY_FANOUT - POST_PIR_HOT_TAIL_HOT_AUTHORITIES;
    POST_PIR_HOT_TAIL_HOT_AUTHORITIES + (((measured_index / 16) * 4 + (slot - 12)) % tail_width)
}

struct PostPirHotTailScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    workload: PureJsWorkloadKind,
    shape: PostPirHotTailPrewarmShape,
    prewarm_decision: nimbus_runtime::RuntimePrewarmScheduleDecision,
    next_invocation_index: usize,
    measured_hot_invocations: u64,
    measured_tail_invocations: u64,
}

impl PostPirHotTailScenario {
    fn new(workload: PureJsWorkloadKind, shape: PostPirHotTailPrewarmShape) -> Self {
        let tempdir =
            tempfile::tempdir().expect("post-PIR hot-tail benchmark tempdir should build");
        let bundle = write_bundle(&tempdir, workload.source());
        let (runtime, executor) = build_runtime_with_config(
            BenchmarkProfile::WebStandard,
            Arc::new(NoopHost),
            PoolMode::WarmPool,
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = RuntimeRoutingAffinity::Function;
                limits.routing_affinity_max_entries = POST_PIR_HOT_TAIL_AUTHORITY_FANOUT;
                limits.max_warm_pool_entries_per_worker = shape.retained_cap.max(1);
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: super::benchmark_request(),
            workload,
            shape,
            prewarm_decision: shape.schedule_decision(),
            next_invocation_index: 0,
            measured_hot_invocations: 0,
            measured_tail_invocations: 0,
        }
    }

    fn prewarm(&mut self) {
        for authority_index in 0..self.prewarm_decision.admitted_entries {
            self.invoke_authority(authority_index % POST_PIR_HOT_TAIL_AUTHORITY_FANOUT);
        }
    }

    fn invoke_once(&mut self) {
        let authority_index = hot_tail_authority_index(self.next_invocation_index);
        self.next_invocation_index = self.next_invocation_index.saturating_add(1);
        if authority_index < POST_PIR_HOT_TAIL_HOT_AUTHORITIES {
            self.measured_hot_invocations = self.measured_hot_invocations.saturating_add(1);
        } else {
            self.measured_tail_invocations = self.measured_tail_invocations.saturating_add(1);
        }
        self.invoke_authority(authority_index);
    }

    fn invoke_authority(&self, authority_index: usize) {
        let mut request = self.request.clone();
        request.function_name = format!("messages:hot_tail_{authority_index:04}");
        let tenant_label = format!("tenant-hot-tail-{authority_index:04}");
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            self.bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, tenant_label),
        );
        let result = result.expect("post-PIR hot-tail benchmark invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_hot_tail_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_prewarm_bytes: Option<u64>,
        rss_after_prewarm_bytes: Option<u64>,
        rss_after_measurement_bytes: Option<u64>,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let total_invocations = self
            .prewarm_decision
            .admitted_entries
            .saturating_add(measured_iterations as usize) as u64;
        let prewarm_rss_delta_bytes = rss_before_prewarm_bytes
            .zip(rss_after_prewarm_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let measurement_rss_delta_bytes = rss_before_prewarm_bytes
            .zip(rss_after_measurement_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let rss_per_retained_entry_bytes = prewarm_rss_delta_bytes.and_then(|delta| match snapshot
            .retained_runtime_pool_entries
        {
            0 => None,
            retained_entries => Some(delta / retained_entries as u64),
        });
        let benchmark_id = format!(
            "{}/{}/webstandard_exact_key_warm_pool",
            self.workload.label(),
            self.shape.label()
        );
        maybe_emit_post_pir_hot_tail_trace_record(
            path,
            PostPirHotTailTraceRecord {
                schema: POST_PIR_HOT_TAIL_TRACE_SCHEMA,
                benchmark_group: POST_PIR_HOT_TAIL_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.workload.label(),
                pool_path: PostPirPoolPath::ExactKeyWarmPool.label(),
                pool_kind: PoolMode::WarmPool.label(),
                routing_affinity: routing_affinity_label(RuntimeRoutingAffinity::Function),
                authority_fanout: POST_PIR_HOT_TAIL_AUTHORITY_FANOUT,
                hot_authority_count: POST_PIR_HOT_TAIL_HOT_AUTHORITIES,
                hot_traffic_percent: 75,
                retained_cap: self.shape.retained_cap,
                requested_prewarm_entries: self.prewarm_decision.requested_entries,
                admitted_prewarm_entries: self.prewarm_decision.admitted_entries,
                prewarm_paused_by_memory_pressure: self.prewarm_decision.paused_by_memory_pressure,
                prewarm_memory_pressure_level: memory_pressure_level_label(
                    self.prewarm_decision.memory_pressure_level,
                ),
                prewarm_memory_pressure_source_status: memory_pressure_source_status_label(
                    self.prewarm_decision.memory_pressure_source_status,
                ),
                measured_iterations,
                measured_hot_invocations: self.measured_hot_invocations,
                measured_tail_invocations: self.measured_tail_invocations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                rss_source: current_rss_source_label(),
                rss_before_prewarm_bytes,
                rss_after_prewarm_bytes,
                rss_after_measurement_bytes,
                prewarm_rss_delta_bytes,
                measurement_rss_delta_bytes,
                rss_per_retained_entry_bytes,
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
            },
        );
    }
}

struct PostPirPoolSizingScenario {
    inner: PostPirHotTailScenario,
    shape: PostPirPoolSizingShape,
}

impl PostPirPoolSizingScenario {
    fn new(workload: PureJsWorkloadKind, shape: PostPirPoolSizingShape) -> Self {
        Self {
            inner: PostPirHotTailScenario::new(
                workload,
                PostPirHotTailPrewarmShape {
                    requested_prewarm_entries: POST_PIR_HOT_TAIL_HOT_AUTHORITIES,
                    retained_cap: shape.retained_cap,
                    pressure: PostPirPrewarmPressure::Nominal,
                },
            ),
            shape,
        }
    }

    fn prewarm_hot_set(&mut self) {
        self.inner.prewarm();
    }

    fn invoke_once(&mut self) {
        self.inner.invoke_once();
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.inner.metrics_snapshot()
    }

    fn emit_pool_sizing_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_prewarm_bytes: Option<u64>,
        rss_after_prewarm_bytes: Option<u64>,
        rss_after_measurement_bytes: Option<u64>,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let total_invocations = self
            .inner
            .prewarm_decision
            .admitted_entries
            .saturating_add(measured_iterations as usize) as u64;
        let prewarm_rss_delta_bytes = rss_before_prewarm_bytes
            .zip(rss_after_prewarm_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let measurement_rss_delta_bytes = rss_before_prewarm_bytes
            .zip(rss_after_measurement_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let rss_per_retained_entry_bytes = prewarm_rss_delta_bytes.and_then(|delta| match snapshot
            .retained_runtime_pool_entries
        {
            0 => None,
            retained_entries => Some(delta / retained_entries as u64),
        });
        let high_pressure_eviction_target = RuntimeMemoryPressureDecision::for_level(
            RuntimeMemoryPressureLevel::High,
            RuntimeMemoryPressureSourceStatus::Observed,
        )
        .retained_runtime_eviction_target(snapshot.retained_runtime_pool_entries);
        let critical_pressure_eviction_target = RuntimeMemoryPressureDecision::for_level(
            RuntimeMemoryPressureLevel::Critical,
            RuntimeMemoryPressureSourceStatus::Observed,
        )
        .retained_runtime_eviction_target(snapshot.retained_runtime_pool_entries);
        let warm_pool_lookups = snapshot
            .warm_pool_hits
            .saturating_add(snapshot.warm_pool_misses);
        let warm_hit_ratio = if warm_pool_lookups == 0 {
            0.0
        } else {
            snapshot.warm_pool_hits as f64 / warm_pool_lookups as f64
        };
        let benchmark_id = format!(
            "{}/{}/webstandard_exact_key_warm_pool",
            self.inner.workload.label(),
            self.shape.label()
        );
        maybe_emit_post_pir_pool_sizing_trace_record(
            path,
            PostPirPoolSizingTraceRecord {
                schema: POST_PIR_POOL_SIZING_TRACE_SCHEMA,
                benchmark_group: POST_PIR_POOL_SIZING_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.inner.workload.label(),
                pool_path: PostPirPoolPath::ExactKeyWarmPool.label(),
                pool_kind: PoolMode::WarmPool.label(),
                routing_affinity: routing_affinity_label(RuntimeRoutingAffinity::Function),
                traffic_shape: "zipf_hot_tail_64_8_75",
                authority_fanout: POST_PIR_HOT_TAIL_AUTHORITY_FANOUT,
                hot_authority_count: POST_PIR_HOT_TAIL_HOT_AUTHORITIES,
                hot_traffic_percent: 75,
                retained_cap: self.shape.retained_cap,
                requested_prewarm_entries: self.inner.prewarm_decision.requested_entries,
                admitted_prewarm_entries: self.inner.prewarm_decision.admitted_entries,
                measured_iterations,
                measured_hot_invocations: self.inner.measured_hot_invocations,
                measured_tail_invocations: self.inner.measured_tail_invocations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                rss_source: current_rss_source_label(),
                rss_before_prewarm_bytes,
                rss_after_prewarm_bytes,
                rss_after_measurement_bytes,
                prewarm_rss_delta_bytes,
                measurement_rss_delta_bytes,
                rss_per_retained_entry_bytes,
                warm_hit_ratio,
                high_pressure_eviction_target,
                critical_pressure_eviction_target,
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
            },
        );
    }
}

struct PostPirCooperativeMixedScenario {
    _io_tempdir: tempfile::TempDir,
    _compute_tempdir: tempfile::TempDir,
    io_runtime: nimbus_runtime::NimbusRuntime,
    compute_runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    io_bundle: nimbus_runtime::RuntimeBundle,
    compute_bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    shape: PostPirCooperativeMixedShape,
    tokio_runtime: tokio::runtime::Runtime,
    next_wave_index: usize,
}

impl PostPirCooperativeMixedScenario {
    fn new(shape: PostPirCooperativeMixedShape) -> Self {
        let io_tempdir =
            tempfile::tempdir().expect("post-PIR cooperative mixed I/O tempdir should build");
        let compute_tempdir =
            tempfile::tempdir().expect("post-PIR cooperative mixed compute tempdir should build");
        let io_bundle = write_bundle(&io_tempdir, PostPirWorkload::AsyncHostCall.source());
        let compute_bundle =
            write_bundle(&compute_tempdir, PureJsWorkloadKind::ComputeBound.source());

        let mut limits = BenchmarkProfile::WebStandard.limits();
        limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
        limits.runtime_pool_kind = PoolMode::WarmPool.runtime_pool_kind();
        limits.routing_affinity = RuntimeRoutingAffinity::Function;
        limits.routing_affinity_max_entries = shape.total_invocations_per_wave().max(1);
        limits.max_warm_pool_entries_per_worker = 16;
        limits.max_warm_reuses = 1_000_000;
        limits.max_heap_mb = 256;
        limits.max_concurrent_runtime_instances = 1;
        limits.worker_threads = 1;
        limits.max_active_top_level_invocations_per_tenant = 1;
        limits.max_in_flight_top_level_invocations_per_tenant = 1;
        limits.max_queued_top_level_invocations_per_tenant =
            shape.total_invocations_per_wave().max(1);

        let policy = Arc::new(nimbus_runtime::RuntimePolicy::new(limits));
        let executor = nimbus_runtime::RuntimeExecutor::new(policy.clone());
        let io_runtime = nimbus_runtime::NimbusRuntime::with_policy(
            Arc::new(DelayedAsyncHost::new(Duration::from_millis(
                shape.synthetic_await_ms,
            ))),
            policy.clone(),
            nimbus_runtime::RuntimeEgressPosture::CoarsePermissions,
        );
        let compute_runtime = nimbus_runtime::NimbusRuntime::with_policy(
            Arc::new(NoopHost),
            policy,
            nimbus_runtime::RuntimeEgressPosture::CoarsePermissions,
        );
        let tokio_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("post-PIR cooperative mixed Tokio runtime should build");

        Self {
            _io_tempdir: io_tempdir,
            _compute_tempdir: compute_tempdir,
            io_runtime,
            compute_runtime,
            executor,
            io_bundle,
            compute_bundle,
            request: super::benchmark_request(),
            shape,
            tokio_runtime,
            next_wave_index: 0,
        }
    }

    fn prime(&mut self) {
        let _ = self.invoke_wave();
    }

    fn invoke_wave(&mut self) -> PostPirCooperativeMixedLatency {
        let wave_index = self.next_wave_index;
        self.next_wave_index = self.next_wave_index.saturating_add(1);
        let specs = self.wave_specs(wave_index);
        let executor = self.executor.clone();
        let io_runtime = self.io_runtime.clone();
        let compute_runtime = self.compute_runtime.clone();
        let io_bundle = self.io_bundle.clone();
        let compute_bundle = self.compute_bundle.clone();
        self.tokio_runtime.block_on(async move {
            let mut handles = Vec::with_capacity(specs.len());
            for spec in specs {
                let executor = executor.clone();
                let runtime = match spec.class {
                    PostPirCooperativeInvocationClass::AsyncHost => io_runtime.clone(),
                    PostPirCooperativeInvocationClass::Compute => compute_runtime.clone(),
                };
                let bundle = match spec.class {
                    PostPirCooperativeInvocationClass::AsyncHost => io_bundle.clone(),
                    PostPirCooperativeInvocationClass::Compute => compute_bundle.clone(),
                };
                handles.push(tokio::spawn(async move {
                    let started_at = Instant::now();
                    let result = executor
                        .invoke_on_worker(runtime, bundle, spec.request, spec.context, None)
                        .await;
                    let result =
                        result.expect("post-PIR cooperative mixed invocation should succeed");
                    std::hint::black_box(result);
                    PostPirCooperativeInvocationLatency {
                        class: spec.class,
                        nanos: duration_nanos_u64(started_at.elapsed()),
                    }
                }));
            }

            let mut latency = PostPirCooperativeMixedLatency::default();
            for handle in handles {
                latency.record(
                    handle
                        .await
                        .expect("post-PIR cooperative mixed invocation task should join"),
                );
            }
            latency
        })
    }

    fn wave_specs(&self, wave_index: usize) -> Vec<PostPirCooperativeInvocationSpec> {
        let mut io_specs = Vec::with_capacity(self.shape.async_host_invocations_per_wave);
        for slot in 0..self.shape.async_host_invocations_per_wave {
            io_specs.push(self.invocation_spec(
                PostPirCooperativeInvocationClass::AsyncHost,
                wave_index,
                slot,
            ));
        }

        let mut compute_specs = Vec::with_capacity(self.shape.compute_invocations_per_wave);
        for slot in 0..self.shape.compute_invocations_per_wave {
            compute_specs.push(self.invocation_spec(
                PostPirCooperativeInvocationClass::Compute,
                wave_index,
                slot,
            ));
        }

        match self.shape.submit_order {
            PostPirCooperativeSubmitOrder::IoFirst => {
                io_specs.extend(compute_specs);
                io_specs
            }
            PostPirCooperativeSubmitOrder::CpuFirst => {
                compute_specs.extend(io_specs);
                compute_specs
            }
        }
    }

    fn invocation_spec(
        &self,
        class: PostPirCooperativeInvocationClass,
        wave_index: usize,
        slot: usize,
    ) -> PostPirCooperativeInvocationSpec {
        let mut request = self.request.clone();
        request.function_name = match class {
            PostPirCooperativeInvocationClass::AsyncHost => {
                format!("messages:cooperative_io_{slot:02}")
            }
            PostPirCooperativeInvocationClass::Compute => {
                format!("messages:cooperative_cpu_{slot:02}")
            }
        };
        let tenant = match class {
            PostPirCooperativeInvocationClass::AsyncHost => format!("tenant-io-{slot:02}"),
            PostPirCooperativeInvocationClass::Compute => format!("tenant-cpu-{slot:02}"),
        };
        let request_id = format!(
            "post-pir-cooperative-{}-{}-{wave_index}-{slot}",
            self.shape.label,
            class.label()
        );
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request, tenant, request_id,
        );
        PostPirCooperativeInvocationSpec {
            class,
            request,
            context,
        }
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        measured_waves: u64,
        elapsed: Duration,
        latency: PostPirCooperativeMixedLatency,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let all_invocations = latency.sorted_all_invocations();
        let total_measured_invocations =
            measured_waves.saturating_mul(self.shape.total_invocations_per_wave() as u64);
        let prime_invocations = self.shape.total_invocations_per_wave() as u64;
        let benchmark_id = format!(
            "{}/webstandard_cooperative_exact_key_warm_pool",
            self.shape.label
        );
        maybe_emit_post_pir_cooperative_mixed_trace_record(
            path,
            PostPirCooperativeMixedTraceRecord {
                schema: POST_PIR_COOPERATIVE_MIXED_TRACE_SCHEMA,
                benchmark_group: POST_PIR_COOPERATIVE_MIXED_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                pool_path: PostPirPoolPath::ExactKeyWarmPool.label(),
                pool_kind: PoolMode::WarmPool.label(),
                routing_affinity: routing_affinity_label(RuntimeRoutingAffinity::Function),
                execution_model: execution_model_label(RuntimeExecutionModel::CooperativeLocker),
                traffic_shape: self.shape.label,
                submit_order: self.shape.submit_order.label(),
                synthetic_await_ms: self.shape.synthetic_await_ms,
                worker_threads: 1,
                max_concurrent_runtime_instances: 1,
                async_host_invocations_per_wave: self.shape.async_host_invocations_per_wave,
                compute_invocations_per_wave: self.shape.compute_invocations_per_wave,
                prime_invocations,
                measured_waves,
                measured_invocations: total_measured_invocations,
                measured_async_host_invocations: latency.async_host_count,
                measured_compute_invocations: latency.compute_count,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    total_measured_invocations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&all_invocations, 0.50),
                latency_p95_nanos: percentile_nanos(&all_invocations, 0.95),
                latency_p99_nanos: percentile_nanos(&all_invocations, 0.99),
                async_host_latency_p50_nanos: latency.async_host_percentile(0.50),
                async_host_latency_p95_nanos: latency.async_host_percentile(0.95),
                async_host_latency_p99_nanos: latency.async_host_percentile(0.99),
                compute_latency_p50_nanos: latency.compute_percentile(0.50),
                compute_latency_p95_nanos: latency.compute_percentile(0.95),
                compute_latency_p99_nanos: latency.compute_percentile(0.99),
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
                active_runtime_instances: snapshot.active_runtime_instances,
                queued_invocations: snapshot.queued_invocations,
                worker_dispatched_invocations: snapshot.worker_dispatched_invocations,
                started_invocations: snapshot.started_invocations,
                completed_invocations: snapshot.completed_invocations,
            },
        );
    }
}

struct PostPirCooperativeInvocationSpec {
    class: PostPirCooperativeInvocationClass,
    request: InvocationRequest,
    context: RuntimeInvocationContext,
}

#[derive(Clone, Copy)]
enum PostPirCooperativeInvocationClass {
    AsyncHost,
    Compute,
}

impl PostPirCooperativeInvocationClass {
    fn label(self) -> &'static str {
        match self {
            Self::AsyncHost => "async_host",
            Self::Compute => "compute",
        }
    }
}

struct PostPirCooperativeInvocationLatency {
    class: PostPirCooperativeInvocationClass,
    nanos: u64,
}

#[derive(Default)]
struct PostPirCooperativeMixedLatency {
    all: Vec<u64>,
    async_host: Vec<u64>,
    compute: Vec<u64>,
    async_host_count: u64,
    compute_count: u64,
}

impl PostPirCooperativeMixedLatency {
    fn record(&mut self, invocation: PostPirCooperativeInvocationLatency) {
        self.all.push(invocation.nanos);
        match invocation.class {
            PostPirCooperativeInvocationClass::AsyncHost => {
                self.async_host.push(invocation.nanos);
                self.async_host_count = self.async_host_count.saturating_add(1);
            }
            PostPirCooperativeInvocationClass::Compute => {
                self.compute.push(invocation.nanos);
                self.compute_count = self.compute_count.saturating_add(1);
            }
        }
    }

    fn extend(&mut self, mut other: Self) {
        self.all.append(&mut other.all);
        self.async_host.append(&mut other.async_host);
        self.compute.append(&mut other.compute);
        self.async_host_count = self.async_host_count.saturating_add(other.async_host_count);
        self.compute_count = self.compute_count.saturating_add(other.compute_count);
    }

    fn sorted_all_invocations(&self) -> Vec<u64> {
        let mut sorted = self.all.clone();
        sorted.sort_unstable();
        sorted
    }

    fn async_host_percentile(&self, percentile: f64) -> Option<u64> {
        let mut sorted = self.async_host.clone();
        sorted.sort_unstable();
        percentile_nanos(&sorted, percentile)
    }

    fn compute_percentile(&self, percentile: f64) -> Option<u64> {
        let mut sorted = self.compute.clone();
        sorted.sort_unstable();
        percentile_nanos(&sorted, percentile)
    }
}

struct PostPirFragmentationScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundles: Vec<nimbus_runtime::RuntimeBundle>,
    request: InvocationRequest,
    workload: PureJsWorkloadKind,
    shape: PostPirFragmentationShape,
    next_invocation_index: usize,
}

impl PostPirFragmentationScenario {
    fn new(workload: PureJsWorkloadKind, shape: PostPirFragmentationShape) -> Self {
        let tempdir =
            tempfile::tempdir().expect("post-PIR fragmentation benchmark tempdir should build");
        let bundles = match shape.dimension {
            PostPirFragmentationDimension::Tenant | PostPirFragmentationDimension::Function => {
                vec![write_bundle(&tempdir, workload.source())]
            }
            PostPirFragmentationDimension::Script => (0..shape.authority_fanout)
                .map(|index| {
                    write_named_bundle(
                        &tempdir,
                        &format!("script-fragment-{index:04}.mjs"),
                        workload.source(),
                    )
                })
                .collect(),
        };
        let (runtime, executor) = build_runtime_with_config(
            BenchmarkProfile::WebStandard,
            Arc::new(NoopHost),
            PoolMode::WarmPool,
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = shape.dimension.routing_affinity();
                limits.routing_affinity_max_entries = shape.authority_fanout.max(1);
                limits.max_warm_pool_entries_per_worker = shape.retained_cap.max(1);
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundles,
            request: super::benchmark_request(),
            workload,
            shape,
            next_invocation_index: 0,
        }
    }

    fn prime(&mut self) {
        for index in 0..self.shape.authority_fanout {
            self.invoke_authority(index);
        }
    }

    fn invoke_once(&mut self) {
        let index = self.next_invocation_index % self.shape.authority_fanout;
        self.next_invocation_index = self.next_invocation_index.saturating_add(1);
        self.invoke_authority(index);
    }

    fn invoke_authority(&self, authority_index: usize) {
        let mut request = self.request.clone();
        let tenant_label = match self.shape.dimension {
            PostPirFragmentationDimension::Tenant => {
                request.function_name = "messages:fragmentation".to_string();
                format!("tenant-fragment-{authority_index:04}")
            }
            PostPirFragmentationDimension::Function => {
                request.function_name = format!("messages:fragmentation_{authority_index:04}");
                "tenant-fragment-function".to_string()
            }
            PostPirFragmentationDimension::Script => {
                request.function_name = "messages:fragmentation".to_string();
                "tenant-fragment-script".to_string()
            }
        };
        let bundle = match self.shape.dimension {
            PostPirFragmentationDimension::Script => {
                self.bundles[authority_index % self.bundles.len()].clone()
            }
            PostPirFragmentationDimension::Tenant | PostPirFragmentationDimension::Function => {
                self.bundles[0].clone()
            }
        };
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            bundle,
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, tenant_label),
        );
        let result = result.expect("post-PIR fragmentation benchmark invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
        rss_before_prime_bytes: Option<u64>,
        rss_after_prime_bytes: Option<u64>,
        rss_after_measurement_bytes: Option<u64>,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let prime_invocations = self.shape.authority_fanout as u64;
        let total_invocations = prime_invocations.saturating_add(measured_iterations);
        let rss_prime_delta_bytes = rss_before_prime_bytes
            .zip(rss_after_prime_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let rss_measurement_delta_bytes = rss_before_prime_bytes
            .zip(rss_after_measurement_bytes)
            .map(|(before, after)| after.saturating_sub(before));
        let warm_pool_lookups = snapshot
            .warm_pool_hits
            .saturating_add(snapshot.warm_pool_misses);
        let warm_hit_ratio = if warm_pool_lookups == 0 {
            0.0
        } else {
            snapshot.warm_pool_hits as f64 / warm_pool_lookups as f64
        };
        let benchmark_id = format!(
            "{}/{}/webstandard_exact_key_warm_pool",
            self.workload.label(),
            self.shape.label()
        );
        maybe_emit_post_pir_fragmentation_trace_record(
            path,
            PostPirFragmentationTraceRecord {
                schema: POST_PIR_FRAGMENTATION_TRACE_SCHEMA,
                benchmark_group: POST_PIR_FRAGMENTATION_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.workload.label(),
                pool_path: PostPirPoolPath::ExactKeyWarmPool.label(),
                pool_kind: PoolMode::WarmPool.label(),
                routing_affinity: routing_affinity_label(self.shape.dimension.routing_affinity()),
                fragmentation_dimension: self.shape.dimension.label(),
                authority_fanout: self.shape.authority_fanout,
                retained_cap: self.shape.retained_cap,
                script_bundle_count: match self.shape.dimension {
                    PostPirFragmentationDimension::Script => self.bundles.len(),
                    PostPirFragmentationDimension::Tenant
                    | PostPirFragmentationDimension::Function => 1,
                },
                exact_key_partition_dimensions: "bundle_identity,affinity_key,runtime_limits,permission_profile,construction_mode,exact_service_grants",
                prime_invocations,
                measured_iterations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                rss_source: current_rss_source_label(),
                rss_before_prime_bytes,
                rss_after_prime_bytes,
                rss_after_measurement_bytes,
                rss_prime_delta_bytes,
                rss_measurement_delta_bytes,
                warm_hit_ratio,
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
            },
        );
    }
}

fn write_named_bundle(
    tempdir: &tempfile::TempDir,
    relative_path: &str,
    source: &str,
) -> nimbus_runtime::RuntimeBundle {
    let bundle_path = tempdir.path().join(relative_path);
    if let Some(parent) = bundle_path.parent() {
        std::fs::create_dir_all(parent).expect("post-PIR named bundle parent should exist");
    }
    std::fs::write(&bundle_path, source).expect("post-PIR named bundle should write");
    nimbus_runtime::RuntimeBundle::new(&bundle_path)
}

struct PostPirCodeCacheScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundle_path: std::path::PathBuf,
    cached_bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    workload: PureJsWorkloadKind,
}

impl PostPirCodeCacheScenario {
    fn new(workload: PureJsWorkloadKind) -> Self {
        let tempdir =
            tempfile::tempdir().expect("post-PIR code-cache benchmark tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, workload.source())
            .expect("post-PIR code-cache bundle should write");
        let cached_bundle = nimbus_runtime::RuntimeBundle::new(&bundle_path);
        let (runtime, executor) = build_runtime_with_config(
            BenchmarkProfile::WebStandard,
            Arc::new(NoopHost),
            PoolMode::StartupSnapshotCache,
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = RuntimeRoutingAffinity::Tenant;
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle_path,
            cached_bundle,
            request: super::benchmark_request(),
            workload,
        }
    }

    fn prime(&self, cache_state: CodeCacheState) {
        self.invoke_once(cache_state);
    }

    fn invoke_once(&self, cache_state: CodeCacheState) {
        let bundle = match cache_state {
            CodeCacheState::FreshBundleEachInvocation => {
                nimbus_runtime::RuntimeBundle::new(&self.bundle_path)
            }
            CodeCacheState::PrimedBundleCodeCache => self.cached_bundle.clone(),
        };
        let result = self.executor.invoke_blocking(
            self.runtime.clone(),
            bundle,
            self.request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&self.request, "tenant-code-cache"),
        );
        let result = result.expect("post-PIR code-cache benchmark invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(
        &self,
        cache_state: CodeCacheState,
        measured_iterations: u64,
        elapsed: Duration,
        latency_nanos: &[u64],
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let total_invocations = measured_iterations.saturating_add(1);
        let benchmark_id = format!(
            "{}/unsnapshotted_runtime_cache/{}",
            self.workload.label(),
            cache_state.label()
        );
        maybe_emit_post_pir_code_cache_trace_record(
            path,
            PostPirCodeCacheTraceRecord {
                schema: POST_PIR_CODE_CACHE_TRACE_SCHEMA,
                benchmark_group: POST_PIR_CODE_CACHE_GROUP,
                benchmark_id: &benchmark_id,
                profile: BenchmarkProfile::WebStandard.label(),
                workload: self.workload.label(),
                pool_path: PostPirPoolPath::StartupSnapshotCache
                    .label_for_profile(BenchmarkProfile::WebStandard),
                pool_kind: PoolMode::StartupSnapshotCache.label(),
                routing_affinity: routing_affinity_label(RuntimeRoutingAffinity::Tenant),
                execution_model: execution_model_label(RuntimeExecutionModel::CooperativeLocker),
                code_cache_state: cache_state.label(),
                prime_invocations: 1,
                measured_iterations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                bundle_loads: snapshot.bundle_loads,
                request_correlation_records: snapshot.request_correlation_records,
                request_correlation_nanos_total: snapshot.request_correlation_nanos_total,
                execution_plan_builds: snapshot.execution_plan_builds,
                execution_plan_build_nanos_total: snapshot.execution_plan_build_nanos_total,
                admission_decisions: snapshot.admission_decisions,
                admission_decision_nanos_total: snapshot.admission_decision_nanos_total,
                worker_router_dispatches: snapshot.worker_router_dispatches,
                worker_router_dispatch_nanos_total: snapshot.worker_router_dispatch_nanos_total,
                bundle_integrity_verifications: snapshot.bundle_integrity_verifications,
                bundle_integrity_verify_nanos_total: snapshot.bundle_integrity_verify_nanos_total,
                bundle_module_loads: snapshot.bundle_module_loads,
                bundle_evaluations: snapshot.bundle_evaluations,
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
            },
        );
    }
}

struct PostPirNodeLazyInitScenario {
    _tempdir: tempfile::TempDir,
    runtime: nimbus_runtime::NimbusRuntime,
    executor: nimbus_runtime::RuntimeExecutor,
    bundle: nimbus_runtime::RuntimeBundle,
    request: InvocationRequest,
    profile: BenchmarkProfile,
    workload: NodeFullNfr6WorkloadKind,
}

impl PostPirNodeLazyInitScenario {
    fn new(profile: BenchmarkProfile, workload: NodeFullNfr6WorkloadKind) -> Self {
        debug_assert!(profile.is_node_full());
        let tempdir =
            tempfile::tempdir().expect("post-PIR NodeFull lazy-init tempdir should build");
        let bundle = write_nfr6_workload_bundle(&tempdir, workload);
        let (runtime, executor) = build_runtime_with_config(
            profile,
            Arc::new(NoopHost),
            PoolMode::StartupSnapshotCache,
            RuntimeExecutionModel::CooperativeLocker,
            |limits| {
                limits.routing_affinity = RuntimeRoutingAffinity::Tenant;
            },
        );
        Self {
            _tempdir: tempdir,
            runtime,
            executor,
            bundle,
            request: super::benchmark_request(),
            profile,
            workload,
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
            RuntimeInvocationContext::top_level_for_tenant(&self.request, "tenant-node-lazy"),
        );
        let result = result.expect("post-PIR NodeFull lazy-init invocation should succeed");
        std::hint::black_box(result);
    }

    fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.executor.policy().metrics_snapshot()
    }

    fn emit_trace(&self, measured_iterations: u64, elapsed: Duration, latency_nanos: &[u64]) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let snapshot = self.metrics_snapshot();
        let mut sorted_latency_nanos = latency_nanos.to_vec();
        sorted_latency_nanos.sort_unstable();
        let total_invocations = measured_iterations.saturating_add(1);
        let benchmark_id = format!(
            "{}/{}/startup_snapshot_cache",
            self.profile.label(),
            self.workload.label()
        );
        maybe_emit_post_pir_node_lazy_init_trace_record(
            path,
            PostPirNodeLazyInitTraceRecord {
                schema: POST_PIR_NODE_LAZY_INIT_TRACE_SCHEMA,
                benchmark_group: POST_PIR_NODE_LAZY_INIT_GROUP,
                benchmark_id: &benchmark_id,
                profile: self.profile.label(),
                workload: self.workload.label(),
                pool_path: PostPirPoolPath::StartupSnapshotCache.label_for_profile(self.profile),
                pool_kind: PoolMode::StartupSnapshotCache.label(),
                routing_affinity: routing_affinity_label(RuntimeRoutingAffinity::Tenant),
                execution_model: execution_model_label(RuntimeExecutionModel::CooperativeLocker),
                snapshot_extension_init_mode: "lazy_init",
                execution_extension_init_mode: "init",
                node_lazy_contract: "snapshot_extensions_lazy_init_execution_extensions_init",
                prime_invocations: 1,
                measured_iterations,
                total_invocations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_invocations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                latency_p50_nanos: percentile_nanos(&sorted_latency_nanos, 0.50),
                latency_p95_nanos: percentile_nanos(&sorted_latency_nanos, 0.95),
                latency_p99_nanos: percentile_nanos(&sorted_latency_nanos, 0.99),
                latency_p999_nanos: percentile_nanos(&sorted_latency_nanos, 0.999),
                runtime_pool_hits: snapshot.runtime_pool_hits,
                runtime_pool_misses: snapshot.runtime_pool_misses,
                runtime_pool_replacements: snapshot.runtime_pool_replacements,
                warm_pool_hits: snapshot.warm_pool_hits,
                warm_pool_misses: snapshot.warm_pool_misses,
                warm_pool_retirements: snapshot.warm_pool_retirements,
                retained_runtime_pool_entries: snapshot.retained_runtime_pool_entries,
                retained_runtime_pool_evictions: snapshot.retained_runtime_pool_evictions,
                retained_runtime_pool_retirements: snapshot.retained_runtime_pool_retirements,
                queue_wait_nanos_total: snapshot.queue_wait_nanos_total,
                execution_nanos_total: snapshot.execution_nanos_total,
                host_bridge_calls: snapshot.host_bridge_calls,
                host_bridge_call_nanos_total: snapshot.host_bridge_call_nanos_total,
            },
        );
    }
}

struct PostPirControllerReplayScenario {
    shape: PostPirControllerReplayShape,
    config: RuntimeControllerReplayConfig,
    inputs: Vec<RuntimeControllerReplayAuthorityInput>,
}

impl PostPirControllerReplayScenario {
    fn new(shape: PostPirControllerReplayShape) -> Self {
        let mut config = post_pir_controller_replay_config();
        let inputs = match shape {
            PostPirControllerReplayShape::SteadyNominal => vec![post_pir_controller_input(
                1,
                1,
                0,
                vec![
                    RuntimeControllerReplayObservation::nominal(4, 1_000_000, 200_000),
                    RuntimeControllerReplayObservation::nominal(4, 1_000_000, 200_000),
                    RuntimeControllerReplayObservation::nominal(4, 1_000_000, 200_000),
                    RuntimeControllerReplayObservation::nominal(4, 1_000_000, 200_000),
                ],
            )],
            PostPirControllerReplayShape::BurstSpillover => {
                let mut burst = RuntimeControllerReplayObservation::nominal(8, 4_000_000, 500_000);
                burst.spillover_requests = 3;
                burst.isolate_stall_micros_total = 250_000;
                vec![post_pir_controller_input(
                    1,
                    0,
                    0,
                    vec![
                        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
                        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
                        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
                        burst,
                    ],
                )]
            }
            PostPirControllerReplayShape::MemoryPressurePanic => {
                let mut critical =
                    RuntimeControllerReplayObservation::nominal(10, 5_000_000, 250_000);
                critical.memory_pressure_level = RuntimeMemoryPressureLevel::Critical;
                critical.host_pressure_level = RuntimeHostPressureLevel::Critical;
                vec![post_pir_controller_input(1, 6, 0, vec![critical])]
            }
            PostPirControllerReplayShape::ZipfTenantCap => {
                config.max_warm_runtimes_per_tenant = 2;
                vec![
                    post_pir_controller_input(
                        1,
                        0,
                        0,
                        vec![RuntimeControllerReplayObservation::nominal(
                            100, 10_000_000, 1_000_000,
                        )],
                    ),
                    post_pir_controller_input(
                        2,
                        0,
                        0,
                        vec![RuntimeControllerReplayObservation::nominal(
                            1, 100_000, 10_000,
                        )],
                    ),
                ]
            }
            PostPirControllerReplayShape::PeriodicDecay => {
                config.max_scale_down_step = controller_nonzero(2);
                vec![post_pir_controller_input(
                    1,
                    5,
                    0,
                    vec![RuntimeControllerReplayObservation::idle()],
                )]
            }
        };
        Self {
            shape,
            config,
            inputs,
        }
    }

    fn replay_once(&self) -> Vec<RuntimeControllerReplayDecision> {
        replay_runtime_controller(self.config, &self.inputs)
    }

    fn emit_trace(
        &self,
        measured_replays: u64,
        elapsed: Duration,
        decisions: &[RuntimeControllerReplayDecision],
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        let tenant_cap_limited_decisions = decisions
            .iter()
            .filter(|decision| decision.tenant_cap_limited)
            .count();
        let paused_decisions = decisions
            .iter()
            .filter(|decision| decision.prewarming_paused)
            .count();
        let evicting_decisions = decisions
            .iter()
            .filter(|decision| decision.evict_idle_retained_runtimes)
            .count();
        let rate_limited_decisions = decisions
            .iter()
            .filter(|decision| decision.rate_limited)
            .count();
        let hysteresis_held_decisions = decisions
            .iter()
            .filter(|decision| decision.hysteresis_held)
            .count();
        maybe_emit_post_pir_controller_replay_trace_record(
            path,
            PostPirControllerReplayTraceRecord {
                schema: POST_PIR_CONTROLLER_REPLAY_TRACE_SCHEMA,
                benchmark_group: POST_PIR_CONTROLLER_REPLAY_GROUP,
                benchmark_id: self.shape.label(),
                live_adaptive_defaults_enabled: RuntimeAdaptiveControllerSettings::default()
                    .live_adaptive_defaults_enabled(),
                measured_replays,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_replays_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_replays as f64 / elapsed.as_secs_f64()
                },
                input_authorities: self.inputs.len(),
                decisions,
                stable_window_observations: self.config.stable_window_observations.get(),
                panic_window_observations: self.config.panic_window_observations.get(),
                headroom_entries: self.config.headroom_entries,
                max_scale_up_step: self.config.max_scale_up_step.get(),
                max_scale_down_step: self.config.max_scale_down_step.get(),
                scale_down_hysteresis_observations: self.config.scale_down_hysteresis_observations,
                max_warm_runtimes_per_authority: self.config.max_warm_runtimes_per_authority,
                max_warm_runtimes_per_tenant: self.config.max_warm_runtimes_per_tenant,
                tenant_cap_limited_decisions,
                paused_decisions,
                evicting_decisions,
                rate_limited_decisions,
                hysteresis_held_decisions,
            },
        );
    }
}

fn post_pir_controller_replay_config() -> RuntimeControllerReplayConfig {
    RuntimeControllerReplayConfig {
        stable_window_observations: controller_nonzero(4),
        panic_window_observations: controller_nonzero(1),
        max_scale_up_step: controller_nonzero(16),
        max_scale_down_step: controller_nonzero(16),
        scale_down_hysteresis_observations: 2,
        max_warm_runtimes_per_authority: 16,
        max_warm_runtimes_per_tenant: 16,
        ..RuntimeControllerReplayConfig::default()
    }
}

fn post_pir_controller_input(
    authority_hash: u64,
    current_warm_target: usize,
    scale_down_observations_remaining: usize,
    observations: Vec<RuntimeControllerReplayObservation>,
) -> RuntimeControllerReplayAuthorityInput {
    RuntimeControllerReplayAuthorityInput {
        key: RuntimeControllerReplayAuthorityKey {
            tenant_hash: 7,
            authority_hash,
            profile: RuntimeProfile::NodeFull,
        },
        previous_state: RuntimeControllerReplayState {
            current_warm_target,
            scale_down_observations_remaining,
        },
        observations,
    }
}

struct PostPirLiveAdaptiveScenario {
    shape: PostPirLiveAdaptiveShape,
    controller: RuntimeAdaptiveWarmPoolController,
    snapshot: RuntimeAdaptiveWarmPoolSnapshot,
    clock: PostPirLiveAdaptiveClock,
    metrics: RuntimeMetrics,
    actuator: PostPirLiveAdaptiveActuator,
}

impl PostPirLiveAdaptiveScenario {
    fn new(shape: PostPirLiveAdaptiveShape) -> Self {
        let config = post_pir_controller_replay_config();
        let (settings, host_resource_decision, authorities) = match shape {
            PostPirLiveAdaptiveShape::DisabledStatic => (
                RuntimeAdaptiveControllerSettings::disabled(),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Nominal,
                    RuntimeMemoryPressureLevel::Nominal,
                ),
                vec![post_pir_live_adaptive_authority(
                    1,
                    1,
                    1,
                    0,
                    96 * 1024 * 1024,
                    vec![RuntimeControllerReplayObservation::nominal(
                        4, 1_000_000, 200_000,
                    )],
                )],
            ),
            PostPirLiveAdaptiveShape::ShadowBurst => (
                RuntimeAdaptiveControllerSettings::shadow(config),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Nominal,
                    RuntimeMemoryPressureLevel::Nominal,
                ),
                vec![post_pir_live_adaptive_authority(
                    1,
                    1,
                    1,
                    0,
                    96 * 1024 * 1024,
                    post_pir_live_adaptive_burst_observations(),
                )],
            ),
            PostPirLiveAdaptiveShape::CanaryAdmittedBurst => (
                RuntimeAdaptiveControllerSettings::canary(config, 10),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Nominal,
                    RuntimeMemoryPressureLevel::Nominal,
                ),
                vec![post_pir_live_adaptive_authority(
                    7,
                    1,
                    1,
                    0,
                    96 * 1024 * 1024,
                    post_pir_live_adaptive_burst_observations(),
                )],
            ),
            PostPirLiveAdaptiveShape::CanaryExcludedBurst => (
                RuntimeAdaptiveControllerSettings::canary(config, 10),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Nominal,
                    RuntimeMemoryPressureLevel::Nominal,
                ),
                vec![post_pir_live_adaptive_authority(
                    17,
                    1,
                    1,
                    0,
                    96 * 1024 * 1024,
                    post_pir_live_adaptive_burst_observations(),
                )],
            ),
            PostPirLiveAdaptiveShape::LiveMemoryPressure => (
                RuntimeAdaptiveControllerSettings::live(config),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Critical,
                    RuntimeMemoryPressureLevel::Critical,
                ),
                vec![post_pir_live_adaptive_authority(
                    1,
                    2,
                    6,
                    0,
                    128 * 1024 * 1024,
                    vec![RuntimeControllerReplayObservation::nominal(
                        10, 5_000_000, 250_000,
                    )],
                )],
            ),
            PostPirLiveAdaptiveShape::RollbackPeriodic => (
                RuntimeAdaptiveControllerSettings::live(config)
                    .with_rollback_to_static_defaults(true),
                post_pir_live_adaptive_host_decision(
                    RuntimeHostPressureLevel::Nominal,
                    RuntimeMemoryPressureLevel::Nominal,
                ),
                vec![post_pir_live_adaptive_authority(
                    1,
                    5,
                    5,
                    0,
                    96 * 1024 * 1024,
                    vec![RuntimeControllerReplayObservation::idle()],
                )],
            ),
            PostPirLiveAdaptiveShape::LiveZipfTenantCap => {
                let mut capped_config = config;
                capped_config.max_warm_runtimes_per_tenant = 2;
                (
                    RuntimeAdaptiveControllerSettings::live(capped_config),
                    post_pir_live_adaptive_host_decision(
                        RuntimeHostPressureLevel::Nominal,
                        RuntimeMemoryPressureLevel::Nominal,
                    ),
                    vec![
                        post_pir_live_adaptive_authority(
                            1,
                            0,
                            0,
                            0,
                            96 * 1024 * 1024,
                            vec![RuntimeControllerReplayObservation::nominal(
                                100, 10_000_000, 1_000_000,
                            )],
                        ),
                        post_pir_live_adaptive_authority(
                            2,
                            0,
                            0,
                            0,
                            96 * 1024 * 1024,
                            vec![RuntimeControllerReplayObservation::nominal(
                                1, 100_000, 10_000,
                            )],
                        ),
                    ],
                )
            }
        };

        Self {
            shape,
            controller: RuntimeAdaptiveWarmPoolController::new(settings),
            snapshot: RuntimeAdaptiveWarmPoolSnapshot {
                observed_at_millis: 1_781_972_800_000,
                host_resource_decision,
                authorities,
            },
            clock: PostPirLiveAdaptiveClock(1_781_972_800_000),
            metrics: RuntimeMetrics::default(),
            actuator: PostPirLiveAdaptiveActuator,
        }
    }

    fn run_once(&self) -> RuntimeAdaptiveWarmPoolRun {
        let observations = PostPirLiveAdaptiveObservationSource {
            snapshot: self.snapshot.clone(),
        };
        let pressure = PostPirLiveAdaptivePressure {
            decision: self.snapshot.host_resource_decision,
        };
        self.controller.run_with_adapters(
            &observations,
            &self.clock,
            &pressure,
            &self.metrics,
            &self.actuator,
        )
    }

    fn emit_trace(
        &self,
        measured_iterations: u64,
        elapsed: Duration,
        run: &RuntimeAdaptiveWarmPoolRun,
    ) {
        let Some(path) = std::env::var_os("NIMBUS_POST_PIR_TRACE_PATH") else {
            return;
        };
        maybe_emit_post_pir_live_adaptive_trace_record(
            path,
            PostPirLiveAdaptiveTraceRecord {
                schema: POST_PIR_LIVE_ADAPTIVE_TRACE_SCHEMA,
                benchmark_group: POST_PIR_LIVE_ADAPTIVE_GROUP,
                benchmark_id: self.shape.label(),
                measured_iterations,
                elapsed_nanos: duration_nanos_u64(elapsed),
                throughput_evaluations_per_sec: if elapsed.is_zero() {
                    0.0
                } else {
                    measured_iterations as f64 / elapsed.as_secs_f64()
                },
                controller_mode: run.evaluation.mode,
                live_adaptive_defaults_enabled: run.evaluation.live_adaptive_defaults_enabled,
                rollback_to_static_defaults: run.evaluation.rollback_to_static_defaults,
                host_pressure_level: run.evaluation.host_pressure_level,
                memory_pressure_level: run.evaluation.memory_pressure_level,
                evaluation: &run.evaluation,
                actuation_results: &run.actuation_results,
                metrics: self.metrics.snapshot().adaptive_controller,
                input_authorities: self.snapshot.authorities.len(),
                static_warm_target_total: self
                    .snapshot
                    .authorities
                    .iter()
                    .map(|authority| authority.static_warm_target)
                    .sum(),
                current_retained_runtime_total: self
                    .snapshot
                    .authorities
                    .iter()
                    .map(|authority| authority.current_retained_runtimes)
                    .sum(),
                recommended_warm_target_total: run
                    .evaluation
                    .decisions
                    .iter()
                    .map(|decision| decision.recommended_warm_target)
                    .sum(),
                effective_warm_target_total: run
                    .evaluation
                    .decisions
                    .iter()
                    .map(|decision| decision.effective_warm_target)
                    .sum(),
                projected_runtime_rss_bytes_total: run
                    .evaluation
                    .decisions
                    .iter()
                    .map(|decision| decision.projected_runtime_rss_bytes)
                    .sum(),
                attempted_actuations: run
                    .actuation_results
                    .iter()
                    .filter(|result| result.attempted)
                    .count(),
                applied_actuations: run
                    .actuation_results
                    .iter()
                    .filter(|result| result.applied)
                    .count(),
                shadow_only_decisions: count_live_adaptive_decisions(
                    &run.evaluation,
                    RuntimeAdaptiveWarmPoolActuationKind::ShadowOnly,
                ),
                canary_skipped_decisions: count_live_adaptive_decisions(
                    &run.evaluation,
                    RuntimeAdaptiveWarmPoolActuationKind::CanarySkipped,
                ),
                rollback_to_static_decisions: count_live_adaptive_decisions(
                    &run.evaluation,
                    RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic,
                ),
                tenant_cap_limited_decisions: run
                    .evaluation
                    .decisions
                    .iter()
                    .filter(|decision| decision.replay.tenant_cap_limited)
                    .count(),
                prewarming_paused_decisions: run
                    .evaluation
                    .decisions
                    .iter()
                    .filter(|decision| decision.replay.prewarming_paused)
                    .count(),
                evict_idle_decisions: run
                    .evaluation
                    .decisions
                    .iter()
                    .filter(|decision| decision.replay.evict_idle_retained_runtimes)
                    .count(),
            },
        );
    }
}

fn post_pir_live_adaptive_burst_observations() -> Vec<RuntimeControllerReplayObservation> {
    let mut burst = RuntimeControllerReplayObservation::nominal(8, 4_000_000, 500_000);
    burst.spillover_requests = 3;
    burst.isolate_stall_micros_total = 250_000;
    vec![
        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
        RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
        burst,
    ]
}

fn post_pir_live_adaptive_authority(
    authority_hash: u64,
    static_warm_target: usize,
    current_warm_target: usize,
    scale_down_observations_remaining: usize,
    projected_bytes_per_runtime: u64,
    observations: Vec<RuntimeControllerReplayObservation>,
) -> RuntimeAdaptiveWarmPoolAuthorityInput {
    RuntimeAdaptiveWarmPoolAuthorityInput {
        replay_input: post_pir_controller_input(
            authority_hash,
            current_warm_target,
            scale_down_observations_remaining,
            observations,
        ),
        static_warm_target,
        current_retained_runtimes: current_warm_target,
        projected_bytes_per_runtime,
    }
}

fn post_pir_live_adaptive_host_decision(
    host_level: RuntimeHostPressureLevel,
    memory_level: RuntimeMemoryPressureLevel,
) -> RuntimeHostResourceDecision {
    let memory_sample = match memory_level {
        RuntimeMemoryPressureLevel::Nominal => RuntimeMemoryPressureSample::observed(128, 256, 512),
        RuntimeMemoryPressureLevel::High => RuntimeMemoryPressureSample::observed(256, 256, 512),
        RuntimeMemoryPressureLevel::Critical => {
            RuntimeMemoryPressureSample::observed(512, 256, 512)
        }
    };
    RuntimeHostResourceBudget {
        host_millicpus: 4_000,
        system_reserved_millicpus: 500,
        nimbus_control_plane_reserved_millicpus: 500,
        runtime_hard_ceiling_millicpus: None,
        runtime_seat_millicpus: std::num::NonZeroU32::new(1_000).unwrap(),
    }
    .decide(
        4,
        RuntimeHostPressureSample::observed(host_level, memory_sample.classify(), false),
    )
}

fn count_live_adaptive_decisions(
    evaluation: &RuntimeAdaptiveWarmPoolEvaluation,
    kind: RuntimeAdaptiveWarmPoolActuationKind,
) -> usize {
    evaluation
        .decisions
        .iter()
        .filter(|decision| decision.actuation.kind == kind)
        .count()
}

#[derive(Clone, Copy)]
struct PostPirLiveAdaptiveClock(u64);

impl RuntimeAdaptiveClock for PostPirLiveAdaptiveClock {
    fn now_millis(&self) -> u64 {
        self.0
    }
}

struct PostPirLiveAdaptivePressure {
    decision: RuntimeHostResourceDecision,
}

impl RuntimeAdaptivePressureAdapter for PostPirLiveAdaptivePressure {
    fn host_resource_decision(&self) -> RuntimeHostResourceDecision {
        self.decision
    }
}

struct PostPirLiveAdaptiveObservationSource {
    snapshot: RuntimeAdaptiveWarmPoolSnapshot,
}

impl RuntimeAdaptiveObservationSource for PostPirLiveAdaptiveObservationSource {
    fn snapshot(
        &self,
        observed_at_millis: u64,
        host_resource_decision: RuntimeHostResourceDecision,
    ) -> RuntimeAdaptiveWarmPoolSnapshot {
        RuntimeAdaptiveWarmPoolSnapshot {
            observed_at_millis,
            host_resource_decision,
            authorities: self.snapshot.authorities.clone(),
        }
    }
}

struct PostPirLiveAdaptiveActuator;

impl RuntimeAdaptiveActuator for PostPirLiveAdaptiveActuator {
    fn apply_warm_pool_target(
        &self,
        decision: &RuntimeAdaptiveWarmPoolDecision,
    ) -> RuntimeAdaptiveActuationResult {
        RuntimeAdaptiveActuationResult {
            key_authority_hash: decision.replay.key.authority_hash,
            attempted: true,
            applied: true,
            target_warm_runtimes: decision.effective_warm_target,
            kind: decision.actuation.kind,
        }
    }
}

fn controller_nonzero(value: usize) -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(value)
        .expect("post-PIR controller replay config uses nonzero constants")
}

#[derive(Serialize)]
struct PostPirTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    authority_relaxed_diagnostic: bool,
    tenant_distribution: &'static str,
    execution_model: &'static str,
    synthetic_await_ms: Option<u64>,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_bytes: Option<u64>,
    rss_after_bytes: Option<u64>,
    rss_delta_bytes: Option<u64>,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
    host_pressure_decisions: u64,
    host_pressure_high_decisions: u64,
    host_pressure_critical_decisions: u64,
    latest_effective_dispatch_seats: usize,
}

#[derive(Serialize)]
struct PostPirFanoutTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    authority_relaxed_diagnostic: bool,
    authority_fanout: usize,
    retained_cap: usize,
    prime_invocations: u64,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_bytes: Option<u64>,
    rss_after_prime_bytes: Option<u64>,
    rss_after_measurement_bytes: Option<u64>,
    rss_prime_delta_bytes: Option<u64>,
    rss_measurement_delta_bytes: Option<u64>,
    rss_per_retained_entry_bytes: Option<u64>,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
    host_pressure_decisions: u64,
    host_pressure_high_decisions: u64,
    host_pressure_critical_decisions: u64,
    latest_effective_dispatch_seats: usize,
}

#[derive(Serialize)]
struct PostPirHotTailTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    authority_fanout: usize,
    hot_authority_count: usize,
    hot_traffic_percent: usize,
    retained_cap: usize,
    requested_prewarm_entries: usize,
    admitted_prewarm_entries: usize,
    prewarm_paused_by_memory_pressure: bool,
    prewarm_memory_pressure_level: &'static str,
    prewarm_memory_pressure_source_status: &'static str,
    measured_iterations: u64,
    measured_hot_invocations: u64,
    measured_tail_invocations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_prewarm_bytes: Option<u64>,
    rss_after_prewarm_bytes: Option<u64>,
    rss_after_measurement_bytes: Option<u64>,
    prewarm_rss_delta_bytes: Option<u64>,
    measurement_rss_delta_bytes: Option<u64>,
    rss_per_retained_entry_bytes: Option<u64>,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
}

#[derive(Serialize)]
struct PostPirPoolSizingTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    traffic_shape: &'static str,
    authority_fanout: usize,
    hot_authority_count: usize,
    hot_traffic_percent: usize,
    retained_cap: usize,
    requested_prewarm_entries: usize,
    admitted_prewarm_entries: usize,
    measured_iterations: u64,
    measured_hot_invocations: u64,
    measured_tail_invocations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_prewarm_bytes: Option<u64>,
    rss_after_prewarm_bytes: Option<u64>,
    rss_after_measurement_bytes: Option<u64>,
    prewarm_rss_delta_bytes: Option<u64>,
    measurement_rss_delta_bytes: Option<u64>,
    rss_per_retained_entry_bytes: Option<u64>,
    warm_hit_ratio: f64,
    high_pressure_eviction_target: usize,
    critical_pressure_eviction_target: usize,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
}

#[derive(Serialize)]
struct PostPirCooperativeMixedTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    execution_model: &'static str,
    traffic_shape: &'static str,
    submit_order: &'static str,
    synthetic_await_ms: u64,
    worker_threads: usize,
    max_concurrent_runtime_instances: usize,
    async_host_invocations_per_wave: usize,
    compute_invocations_per_wave: usize,
    prime_invocations: u64,
    measured_waves: u64,
    measured_invocations: u64,
    measured_async_host_invocations: u64,
    measured_compute_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    async_host_latency_p50_nanos: Option<u64>,
    async_host_latency_p95_nanos: Option<u64>,
    async_host_latency_p99_nanos: Option<u64>,
    compute_latency_p50_nanos: Option<u64>,
    compute_latency_p95_nanos: Option<u64>,
    compute_latency_p99_nanos: Option<u64>,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
    active_runtime_instances: usize,
    queued_invocations: usize,
    worker_dispatched_invocations: u64,
    started_invocations: u64,
    completed_invocations: u64,
}

#[derive(Serialize)]
struct PostPirFragmentationTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    fragmentation_dimension: &'static str,
    authority_fanout: usize,
    retained_cap: usize,
    script_bundle_count: usize,
    exact_key_partition_dimensions: &'static str,
    prime_invocations: u64,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    rss_source: &'static str,
    rss_before_prime_bytes: Option<u64>,
    rss_after_prime_bytes: Option<u64>,
    rss_after_measurement_bytes: Option<u64>,
    rss_prime_delta_bytes: Option<u64>,
    rss_measurement_delta_bytes: Option<u64>,
    warm_hit_ratio: f64,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
}

#[derive(Serialize)]
struct PostPirCodeCacheTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    execution_model: &'static str,
    code_cache_state: &'static str,
    prime_invocations: u64,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    bundle_loads: u64,
    request_correlation_records: u64,
    request_correlation_nanos_total: u64,
    execution_plan_builds: u64,
    execution_plan_build_nanos_total: u64,
    admission_decisions: u64,
    admission_decision_nanos_total: u64,
    worker_router_dispatches: u64,
    worker_router_dispatch_nanos_total: u64,
    bundle_integrity_verifications: u64,
    bundle_integrity_verify_nanos_total: u64,
    bundle_module_loads: u64,
    bundle_evaluations: u64,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
}

#[derive(Serialize)]
struct PostPirNodeLazyInitTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    profile: &'static str,
    workload: &'static str,
    pool_path: &'static str,
    pool_kind: &'static str,
    routing_affinity: &'static str,
    execution_model: &'static str,
    snapshot_extension_init_mode: &'static str,
    execution_extension_init_mode: &'static str,
    node_lazy_contract: &'static str,
    prime_invocations: u64,
    measured_iterations: u64,
    total_invocations: u64,
    elapsed_nanos: u64,
    throughput_invocations_per_sec: f64,
    latency_p50_nanos: Option<u64>,
    latency_p95_nanos: Option<u64>,
    latency_p99_nanos: Option<u64>,
    latency_p999_nanos: Option<u64>,
    runtime_pool_hits: u64,
    runtime_pool_misses: u64,
    runtime_pool_replacements: u64,
    warm_pool_hits: u64,
    warm_pool_misses: u64,
    warm_pool_retirements: u64,
    retained_runtime_pool_entries: usize,
    retained_runtime_pool_evictions: u64,
    retained_runtime_pool_retirements: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    host_bridge_calls: u64,
    host_bridge_call_nanos_total: u64,
}

#[derive(Serialize)]
struct PostPirControllerReplayTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    live_adaptive_defaults_enabled: bool,
    measured_replays: u64,
    elapsed_nanos: u64,
    throughput_replays_per_sec: f64,
    input_authorities: usize,
    decisions: &'a [RuntimeControllerReplayDecision],
    stable_window_observations: usize,
    panic_window_observations: usize,
    headroom_entries: usize,
    max_scale_up_step: usize,
    max_scale_down_step: usize,
    scale_down_hysteresis_observations: usize,
    max_warm_runtimes_per_authority: usize,
    max_warm_runtimes_per_tenant: usize,
    tenant_cap_limited_decisions: usize,
    paused_decisions: usize,
    evicting_decisions: usize,
    rate_limited_decisions: usize,
    hysteresis_held_decisions: usize,
}

#[derive(Serialize)]
struct PostPirLiveAdaptiveTraceRecord<'a> {
    schema: &'static str,
    benchmark_group: &'static str,
    benchmark_id: &'a str,
    measured_iterations: u64,
    elapsed_nanos: u64,
    throughput_evaluations_per_sec: f64,
    controller_mode: RuntimeAdaptiveControllerMode,
    live_adaptive_defaults_enabled: bool,
    rollback_to_static_defaults: bool,
    host_pressure_level: RuntimeHostPressureLevel,
    memory_pressure_level: RuntimeMemoryPressureLevel,
    evaluation: &'a RuntimeAdaptiveWarmPoolEvaluation,
    actuation_results: &'a [RuntimeAdaptiveActuationResult],
    metrics: RuntimeAdaptiveControllerMetricsSnapshot,
    input_authorities: usize,
    static_warm_target_total: usize,
    current_retained_runtime_total: usize,
    recommended_warm_target_total: usize,
    effective_warm_target_total: usize,
    projected_runtime_rss_bytes_total: u64,
    attempted_actuations: usize,
    applied_actuations: usize,
    shadow_only_decisions: usize,
    canary_skipped_decisions: usize,
    rollback_to_static_decisions: usize,
    tenant_cap_limited_decisions: usize,
    prewarming_paused_decisions: usize,
    evict_idle_decisions: usize,
}

fn maybe_emit_post_pir_trace_record(path: std::ffi::OsString, record: PostPirTraceRecord<'_>) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent).expect("post-PIR trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR trace file should open");
    serde_json::to_writer(&mut file, &record).expect("post-PIR trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR trace record should end with newline");
}

fn maybe_emit_post_pir_fanout_trace_record(
    path: std::ffi::OsString,
    record: PostPirFanoutTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR fanout trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR fanout trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR fanout trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR fanout trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR fanout trace record should end with newline");
}

fn maybe_emit_post_pir_hot_tail_trace_record(
    path: std::ffi::OsString,
    record: PostPirHotTailTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR hot-tail trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR hot-tail trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR hot-tail trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR hot-tail trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR hot-tail trace record should end with newline");
}

fn maybe_emit_post_pir_pool_sizing_trace_record(
    path: std::ffi::OsString,
    record: PostPirPoolSizingTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR pool-sizing trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR pool-sizing trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR pool-sizing trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR pool-sizing trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR pool-sizing trace record should end with newline");
}

fn maybe_emit_post_pir_cooperative_mixed_trace_record(
    path: std::ffi::OsString,
    record: PostPirCooperativeMixedTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR cooperative mixed trace emission lock should not be poisoned");
    if emitted
        .get(&key)
        .is_some_and(|previous| *previous >= record.measured_waves)
    {
        return;
    }
    emitted.insert(key, record.measured_waves);
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
            .expect("post-PIR cooperative mixed trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR cooperative mixed trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR cooperative mixed trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR cooperative mixed trace record should end with newline");
}

fn maybe_emit_post_pir_fragmentation_trace_record(
    path: std::ffi::OsString,
    record: PostPirFragmentationTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR fragmentation trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR fragmentation trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR fragmentation trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR fragmentation trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR fragmentation trace record should end with newline");
}

fn maybe_emit_post_pir_code_cache_trace_record(
    path: std::ffi::OsString,
    record: PostPirCodeCacheTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR code-cache trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR code-cache trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR code-cache trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR code-cache trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR code-cache trace record should end with newline");
}

fn maybe_emit_post_pir_node_lazy_init_trace_record(
    path: std::ffi::OsString,
    record: PostPirNodeLazyInitTraceRecord<'_>,
) {
    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR NodeFull lazy-init trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR NodeFull lazy-init trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR NodeFull lazy-init trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR NodeFull lazy-init trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR NodeFull lazy-init trace record should end with newline");
}

fn maybe_emit_post_pir_controller_replay_trace_record(
    path: std::ffi::OsString,
    record: PostPirControllerReplayTraceRecord<'_>,
) {
    if record.measured_replays != POST_PIR_TRACE_MIN_CONTROLLER_REPLAY_ITERATIONS {
        return;
    }

    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR controller replay trace emission lock should not be poisoned");
    if emitted
        .get(&key)
        .is_some_and(|previous| *previous >= record.measured_replays)
    {
        return;
    }
    emitted.insert(key, record.measured_replays);
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
            .expect("post-PIR controller replay trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR controller replay trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR controller replay trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR controller replay trace record should end with newline");
}

fn maybe_emit_post_pir_live_adaptive_trace_record(
    path: std::ffi::OsString,
    record: PostPirLiveAdaptiveTraceRecord<'_>,
) {
    if record.measured_iterations != POST_PIR_TRACE_MIN_LIVE_ADAPTIVE_ITERATIONS {
        return;
    }

    static EMITTED_MAX_ITERS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    let key = format!("{}/{}", record.benchmark_group, record.benchmark_id);
    let emitted = EMITTED_MAX_ITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut emitted = emitted
        .lock()
        .expect("post-PIR live adaptive trace emission lock should not be poisoned");
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
        std::fs::create_dir_all(parent)
            .expect("post-PIR live adaptive trace parent directory should exist");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("post-PIR live adaptive trace file should open");
    serde_json::to_writer(&mut file, &record)
        .expect("post-PIR live adaptive trace record should serialize");
    file.write_all(b"\n")
        .expect("post-PIR live adaptive trace record should end with newline");
}

fn routing_affinity_label(routing_affinity: RuntimeRoutingAffinity) -> &'static str {
    match routing_affinity {
        RuntimeRoutingAffinity::None => "none",
        RuntimeRoutingAffinity::Tenant => "tenant",
        RuntimeRoutingAffinity::Function => "function",
        RuntimeRoutingAffinity::Script => "script",
    }
}

fn memory_pressure_level_label(level: RuntimeMemoryPressureLevel) -> &'static str {
    match level {
        RuntimeMemoryPressureLevel::Nominal => "nominal",
        RuntimeMemoryPressureLevel::High => "high",
        RuntimeMemoryPressureLevel::Critical => "critical",
    }
}

fn memory_pressure_source_status_label(status: RuntimeMemoryPressureSourceStatus) -> &'static str {
    match status {
        RuntimeMemoryPressureSourceStatus::Observed => "observed",
        RuntimeMemoryPressureSourceStatus::Unavailable => "unavailable",
    }
}
