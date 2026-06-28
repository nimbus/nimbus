use super::*;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
struct FixedRuntimeHostPressureSource(RuntimeHostPressureSample);

impl RuntimeHostPressureSource for FixedRuntimeHostPressureSource {
    fn sample(&self) -> RuntimeHostPressureSample {
        self.0
    }
}

#[test]
fn runtime_compatibility_target_parses_public_node_lts_aliases() {
    for (raw, expected) in [
        ("\"20\"", RuntimeCompatibilityTarget::Node20),
        ("\"node20\"", RuntimeCompatibilityTarget::Node20),
        ("\"Node20\"", RuntimeCompatibilityTarget::Node20),
        ("\"22\"", RuntimeCompatibilityTarget::Node22),
        ("\"node22\"", RuntimeCompatibilityTarget::Node22),
        ("\"Node22\"", RuntimeCompatibilityTarget::Node22),
        ("\"24\"", RuntimeCompatibilityTarget::Node24),
        ("\"node24\"", RuntimeCompatibilityTarget::Node24),
        ("\"Node24\"", RuntimeCompatibilityTarget::Node24),
        ("\"26\"", RuntimeCompatibilityTarget::Node26),
        ("\"node26\"", RuntimeCompatibilityTarget::Node26),
        ("\"Node26\"", RuntimeCompatibilityTarget::Node26),
    ] {
        let parsed: RuntimeCompatibilityTarget =
            serde_json::from_str(raw).expect("target alias should parse");
        assert_eq!(parsed, expected, "{raw} should parse to {expected:?}");
    }
}

#[test]
fn runtime_node_lts_metadata_is_derived_from_registry() {
    assert_eq!(
        RuntimeCompatibilityTarget::product_default_node_lts_target(),
        RuntimeCompatibilityTarget::Node24
    );
    assert_eq!(
        RuntimeCompatibilityTarget::configured_node_lts_targets(),
        vec![
            RuntimeCompatibilityTarget::Node20,
            RuntimeCompatibilityTarget::Node22,
            RuntimeCompatibilityTarget::Node24,
            RuntimeCompatibilityTarget::Node26,
        ]
    );
    assert_eq!(
        RuntimeCompatibilityTarget::supported_node_lts_targets(),
        vec![
            RuntimeCompatibilityTarget::Node22,
            RuntimeCompatibilityTarget::Node24,
        ],
        "Node20 is EOL legacy and Node26 is Current/non-LTS, so neither is supported LTS"
    );

    for (target, major, phase, version, tag, codename, module_version, product_default) in [
        (
            RuntimeCompatibilityTarget::Node20,
            20,
            RuntimeNodeSupportPhase::EolLegacy,
            "20.20.2",
            "v20.20.2",
            Some("Iron"),
            "115",
            false,
        ),
        (
            RuntimeCompatibilityTarget::Node22,
            22,
            RuntimeNodeSupportPhase::MaintenanceLts,
            "22.22.3",
            "v22.22.3",
            Some("Jod"),
            "127",
            false,
        ),
        (
            RuntimeCompatibilityTarget::Node24,
            24,
            RuntimeNodeSupportPhase::ActiveLts,
            "24.16.0",
            "v24.16.0",
            Some("Krypton"),
            "137",
            true,
        ),
        (
            RuntimeCompatibilityTarget::Node26,
            26,
            RuntimeNodeSupportPhase::CurrentNonLts,
            "26.2.0",
            "v26.2.0",
            None,
            "147",
            false,
        ),
    ] {
        let metadata = target
            .node_lts_metadata()
            .expect("node target should have registry metadata");
        assert_eq!(target.node_major_version(), Some(major));
        assert_eq!(target.node_support_phase(), Some(phase));
        assert_eq!(target.node_runtime_version_number(), Some(version));
        assert_eq!(target.node_runtime_version(), Some(tag));
        assert_eq!(target.node_release_name(), Some("node"));
        assert_eq!(target.node_release_lts_codename(), codename);
        assert_eq!(target.node_module_version(), Some(module_version));
        assert_eq!(target.is_supported_node_lts(), phase.is_supported_lts());
        assert_eq!(metadata.product_default, product_default);
        assert_eq!(
            metadata.runtime_compatibility_target,
            Some(target),
            "registry target should round-trip for {target:?}"
        );
    }

    assert!(
        !RuntimeCompatibilityTarget::Node20.is_supported_node_lts(),
        "Node20 must not be treated as active enterprise LTS after EOL"
    );
    assert!(
        !RuntimeCompatibilityTarget::Node26.is_supported_node_lts(),
        "Node26 is selectable as Current/non-LTS but must not be treated as enterprise LTS"
    );
    assert!(
        RuntimeCompatibilityTarget::WebStandardIsolate
            .node_lts_metadata()
            .is_none()
    );
    assert!(
        RuntimeCompatibilityTarget::BunJsc
            .node_lts_metadata()
            .is_none()
    );
}

#[test]
fn runtime_profile_is_derived_from_v8_javascript_surface_only() {
    let web_limits = RuntimeLimits::application_web_standard().normalized();
    assert_eq!(
        RuntimeProfile::for_limits(&web_limits),
        Some(RuntimeProfile::WebLean)
    );

    for limits in [
        RuntimeLimits::application_node20(),
        RuntimeLimits::application_node22(),
        RuntimeLimits::application_node24(),
        RuntimeLimits::application_node26(),
    ] {
        assert_eq!(
            RuntimeProfile::for_limits(&limits.normalized()),
            Some(RuntimeProfile::NodeFull)
        );
    }

    let bun_limits = RuntimeLimits::application_bun_jsc().normalized();
    assert_eq!(
        RuntimeProfile::for_limits(&bun_limits),
        None,
        "Bun/JSC is not silently collapsed into a V8 runtime efficiency profile"
    );
}

#[test]
fn runtime_density_plan_uses_measured_profile_rss_and_reserves_active_slots() {
    let mut limits = RuntimeLimits::application_node22();
    limits.max_concurrent_runtime_instances = 4;
    limits.worker_threads = 4;
    limits.max_warm_pool_entries_per_worker = 8;
    limits.max_heap_mb = 64;

    let measurement = RuntimeDensityMeasurement::from_total_rss_delta(
        RuntimeProfile::NodeFull,
        RuntimeCompatibilityTarget::Node22,
        RuntimeDensityMeasurementMethod::ProcessRssDelta,
        NonZeroUsize::new(2).expect("nonzero sample count"),
        mib(400),
    );
    let budget = RuntimeDensityBudget {
        host_runtime_budget_bytes: mib(2048),
        operator_reserved_headroom_bytes: mib(256),
    };

    let plan = RuntimeDensityPlan::for_limits_measurement_and_budget(&limits, measurement, budget);

    assert_eq!(plan.measured_per_runtime_rss_bytes, mib(200));
    assert_eq!(plan.heap_cap_bytes_per_runtime, mib(64));
    assert_eq!(plan.planning_bytes_per_runtime, mib(200));
    assert_eq!(plan.available_runtime_budget_bytes, mib(1792));
    assert_eq!(plan.active_runtime_slots_reserved, 4);
    assert_eq!(plan.active_runtime_reservation_bytes, mib(800));
    assert_eq!(plan.retained_pool_budget_bytes, mib(992));
    assert_eq!(plan.max_retained_runtimes_by_memory, 4);
    assert_eq!(plan.max_retained_runtimes_per_worker_by_memory, 1);
    assert_eq!(plan.effective_max_warm_pool_entries_per_worker, 1);
}

#[test]
fn runtime_density_plan_bounds_node_pool_lower_than_web_under_same_budget() {
    let budget = RuntimeDensityBudget {
        host_runtime_budget_bytes: mib(4096),
        operator_reserved_headroom_bytes: mib(512),
    };
    let web_limits = RuntimeLimits {
        max_concurrent_runtime_instances: 4,
        worker_threads: 4,
        max_warm_pool_entries_per_worker: 16,
        max_heap_mb: 128,
        ..RuntimeLimits::application_web_standard()
    };
    let node_limits = RuntimeLimits {
        max_concurrent_runtime_instances: 4,
        worker_threads: 4,
        max_warm_pool_entries_per_worker: 16,
        max_heap_mb: 128,
        ..RuntimeLimits::application_node26()
    };

    let web_plan = RuntimeDensityPlan::for_limits_measurement_and_budget(
        &web_limits,
        RuntimeDensityMeasurement::from_total_rss_delta(
            RuntimeProfile::WebLean,
            RuntimeCompatibilityTarget::WebStandardIsolate,
            RuntimeDensityMeasurementMethod::ProcessRssDelta,
            NonZeroUsize::new(1).expect("nonzero sample count"),
            mib(84),
        ),
        budget,
    );
    let node_plan = RuntimeDensityPlan::for_limits_measurement_and_budget(
        &node_limits,
        RuntimeDensityMeasurement::from_total_rss_delta(
            RuntimeProfile::NodeFull,
            RuntimeCompatibilityTarget::Node26,
            RuntimeDensityMeasurementMethod::ProcessRssDelta,
            NonZeroUsize::new(1).expect("nonzero sample count"),
            mib(189),
        ),
        budget,
    );

    assert_eq!(web_plan.planning_bytes_per_runtime, mib(128));
    assert_eq!(node_plan.planning_bytes_per_runtime, mib(189));
    assert!(
        node_plan.effective_max_warm_pool_entries_per_worker
            < web_plan.effective_max_warm_pool_entries_per_worker,
        "node density must be bounded by measured RSS instead of inheriting web-sized pool caps"
    );
    assert_eq!(web_plan.effective_max_warm_pool_entries_per_worker, 6);
    assert_eq!(node_plan.effective_max_warm_pool_entries_per_worker, 3);
}

#[test]
fn runtime_density_plan_keeps_isolate_group_ffi_deferred() {
    let limits = RuntimeLimits::application_node22();
    let plan = RuntimeDensityPlan::for_limits_measurement_and_budget(
        &limits,
        RuntimeDensityMeasurement::from_total_rss_delta(
            RuntimeProfile::NodeFull,
            RuntimeCompatibilityTarget::Node22,
            RuntimeDensityMeasurementMethod::ProcessRssDelta,
            NonZeroUsize::new(1).expect("nonzero sample count"),
            mib(153),
        ),
        RuntimeDensityBudget {
            host_runtime_budget_bytes: mib(4096),
            operator_reserved_headroom_bytes: mib(512),
        },
    );

    assert_eq!(
        plan.isolate_group_ffi_status,
        RuntimeIsolateGroupFfiStatus::DeferredPendingValidation
    );
    assert!(!plan.isolate_group_ffi_allowed());
}

#[test]
fn runtime_memory_pressure_sample_classifies_observed_watermarks() {
    let nominal = RuntimeMemoryPressureSample::observed(mib(512), mib(768), mib(960)).classify();
    assert_eq!(nominal.level, RuntimeMemoryPressureLevel::Nominal);
    assert_eq!(
        nominal.source_status,
        RuntimeMemoryPressureSourceStatus::Observed
    );
    assert!(!nominal.pause_prewarming);
    assert!(!nominal.evict_idle_retained_runtimes);

    let high = RuntimeMemoryPressureSample::observed(mib(800), mib(768), mib(960)).classify();
    assert_eq!(high.level, RuntimeMemoryPressureLevel::High);
    assert_eq!(
        high.source_status,
        RuntimeMemoryPressureSourceStatus::Observed
    );
    assert!(high.pause_prewarming);
    assert!(high.run_idle_low_memory_maintenance);
    assert!(high.evict_idle_retained_runtimes);

    let critical = RuntimeMemoryPressureSample::observed(mib(960), mib(768), mib(960)).classify();
    assert_eq!(critical.level, RuntimeMemoryPressureLevel::Critical);
    assert_eq!(
        critical.source_status,
        RuntimeMemoryPressureSourceStatus::Observed
    );
    assert!(critical.pause_prewarming);
    assert!(critical.run_idle_low_memory_maintenance);
    assert!(critical.evict_idle_retained_runtimes);
}

#[test]
fn runtime_memory_pressure_sample_degrades_conservatively_without_source() {
    let decision = RuntimeMemoryPressureSample::unavailable().classify();

    assert_eq!(decision.level, RuntimeMemoryPressureLevel::High);
    assert_eq!(
        decision.source_status,
        RuntimeMemoryPressureSourceStatus::Unavailable
    );
    assert!(
        decision.pause_prewarming,
        "missing host/cgroup memory source must stop speculative warm growth"
    );
    assert!(
        decision.evict_idle_retained_runtimes,
        "missing host/cgroup memory source must prefer shrinking idle retained runtimes"
    );
}

#[test]
fn runtime_memory_pressure_decision_pauses_prewarm_scheduler() {
    let nominal = RuntimeMemoryPressureSample::observed(mib(512), mib(768), mib(960))
        .classify()
        .schedule_prewarm_entries(3);
    assert_eq!(nominal.requested_entries, 3);
    assert_eq!(nominal.admitted_entries, 3);
    assert!(!nominal.paused_by_memory_pressure);
    assert_eq!(
        nominal.memory_pressure_level,
        RuntimeMemoryPressureLevel::Nominal
    );
    assert_eq!(
        nominal.memory_pressure_source_status,
        RuntimeMemoryPressureSourceStatus::Observed
    );

    let high = RuntimeMemoryPressureSample::observed(mib(800), mib(768), mib(960))
        .classify()
        .schedule_prewarm_entries(3);
    assert_eq!(high.requested_entries, 3);
    assert_eq!(high.admitted_entries, 0);
    assert!(high.paused_by_memory_pressure);
    assert_eq!(high.memory_pressure_level, RuntimeMemoryPressureLevel::High);
    assert_eq!(
        high.memory_pressure_source_status,
        RuntimeMemoryPressureSourceStatus::Observed
    );

    let critical = RuntimeMemoryPressureSample::observed(mib(960), mib(768), mib(960))
        .classify()
        .schedule_prewarm_entries(3);
    assert_eq!(critical.requested_entries, 3);
    assert_eq!(critical.admitted_entries, 0);
    assert!(critical.paused_by_memory_pressure);
    assert_eq!(
        critical.memory_pressure_level,
        RuntimeMemoryPressureLevel::Critical
    );

    let unavailable = RuntimeMemoryPressureSample::unavailable()
        .classify()
        .schedule_prewarm_entries(3);
    assert_eq!(unavailable.requested_entries, 3);
    assert_eq!(unavailable.admitted_entries, 0);
    assert!(unavailable.paused_by_memory_pressure);
    assert_eq!(
        unavailable.memory_pressure_source_status,
        RuntimeMemoryPressureSourceStatus::Unavailable
    );
}

#[test]
fn runtime_memory_pressure_decision_sizes_retained_evictions() {
    let nominal = RuntimeMemoryPressureDecision::for_level(
        RuntimeMemoryPressureLevel::Nominal,
        RuntimeMemoryPressureSourceStatus::Observed,
    );
    assert_eq!(nominal.retained_runtime_eviction_target(5), 0);

    let high = RuntimeMemoryPressureDecision::for_level(
        RuntimeMemoryPressureLevel::High,
        RuntimeMemoryPressureSourceStatus::Observed,
    );
    assert_eq!(
        high.retained_runtime_eviction_target(5),
        3,
        "high pressure evicts the oldest half, rounded up"
    );

    let critical = RuntimeMemoryPressureDecision::for_level(
        RuntimeMemoryPressureLevel::Critical,
        RuntimeMemoryPressureSourceStatus::Observed,
    );
    assert_eq!(
        critical.retained_runtime_eviction_target(5),
        5,
        "critical pressure evicts every idle retained runtime"
    );
}

#[test]
fn runtime_host_resource_budget_reserves_system_and_control_plane_capacity() {
    let budget =
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(8).unwrap());
    assert_eq!(budget.host_millicpus, 8000);
    assert_eq!(budget.system_reserved_millicpus, 1000);
    assert_eq!(budget.nimbus_control_plane_reserved_millicpus, 1000);
    assert_eq!(budget.runtime_allocatable_millicpus(), 6000);
    assert_eq!(budget.nominal_dispatch_seats(16), 6);

    let capped = RuntimeHostResourceBudget {
        runtime_hard_ceiling_millicpus: Some(2500),
        ..budget
    };
    assert_eq!(capped.runtime_allocatable_millicpus(), 2500);
    assert_eq!(capped.nominal_dispatch_seats(16), 2);

    let nominal: RuntimeHostResourceDecision =
        capped.decide(16, RuntimeHostPressureSample::nominal());
    assert_eq!(nominal.effective_dispatch_seats, 2);
}

#[test]
fn runtime_host_pressure_overrides_unused_tenant_quota_for_lower_qos_work() {
    let budget =
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(8).unwrap());
    let decision = budget.decide(
        8,
        RuntimeHostPressureSample::observed(
            RuntimeHostPressureLevel::High,
            RuntimeMemoryPressureSample::observed(mib(512), mib(768), mib(960)).classify(),
            false,
        ),
    );

    assert_eq!(decision.host_pressure_level, RuntimeHostPressureLevel::High);
    assert_eq!(decision.nominal_dispatch_seats, 6);
    assert_eq!(decision.effective_dispatch_seats, 3);
    assert!(decision.pause_prewarming);
    let guaranteed: RuntimeHostAdmissionDecision =
        decision.admission_for_in_flight(0, RuntimeHostWorkClass::Guaranteed, true);
    assert_eq!(guaranteed.action, RuntimeHostAdmissionAction::Admit);
    assert_eq!(
        decision
            .admission_for_in_flight(0, RuntimeHostWorkClass::Burstable, true)
            .action,
        RuntimeHostAdmissionAction::Admit,
        "host pressure should admit burstable work while a reduced host seat remains available"
    );
    assert_eq!(
        decision
            .admission_for_in_flight(3, RuntimeHostWorkClass::Burstable, true)
            .action,
        RuntimeHostAdmissionAction::Queue,
        "host pressure can queue burstable work when tenant quota remains but reduced host seats are full"
    );
    assert_eq!(
        decision
            .admission_for_in_flight(0, RuntimeHostWorkClass::BestEffort, true)
            .action,
        RuntimeHostAdmissionAction::Shed,
        "host pressure can shed best-effort work even when tenant quota remains and a reduced host seat is available"
    );
    assert_eq!(
        decision
            .admission_for_in_flight(0, RuntimeHostWorkClass::BestEffort, true)
            .over_capacity_action,
        RuntimeHostAdmissionAction::Shed
    );
}

#[test]
fn runtime_host_pressure_degrades_conservatively_without_cpu_source() {
    let budget =
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(4).unwrap());
    let decision = budget.decide(
        4,
        RuntimeHostPressureSample::unavailable(
            RuntimeMemoryPressureSample::unavailable().classify(),
        ),
    );

    assert_eq!(decision.host_pressure_level, RuntimeHostPressureLevel::High);
    assert_eq!(
        decision.cpu_source_status,
        RuntimeHostPressureSourceStatus::Unavailable
    );
    assert_eq!(
        decision.memory_source_status,
        RuntimeMemoryPressureSourceStatus::Unavailable
    );
    assert_eq!(decision.nominal_dispatch_seats, 2);
    assert_eq!(decision.effective_dispatch_seats, 1);
    assert!(decision.pause_prewarming);
    assert!(decision.run_idle_low_memory_maintenance);
    assert!(decision.evict_idle_retained_runtimes);
}

#[test]
fn runtime_policy_records_low_cardinality_host_pressure_metrics() {
    let policy = RuntimePolicy::with_host_resource_governor(
        RuntimeLimits {
            max_concurrent_runtime_instances: 4,
            ..RuntimeLimits::default()
        },
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(4).unwrap()),
        Arc::new(FixedRuntimeHostPressureSource(
            RuntimeHostPressureSample::unavailable(
                RuntimeMemoryPressureSample::unavailable().classify(),
            ),
        )),
    );

    let decision = policy.host_resource_decision();
    let metrics = policy.metrics_snapshot();

    assert_eq!(decision.host_pressure_level, RuntimeHostPressureLevel::High);
    assert_eq!(metrics.host_pressure.decisions, 1);
    assert_eq!(metrics.host_pressure.high_decisions, 1);
    assert_eq!(
        metrics.host_pressure.latest_host_pressure_level,
        RuntimeHostPressureLevel::High
    );
    assert_eq!(
        metrics.host_pressure.latest_cpu_source_status,
        RuntimeHostPressureSourceStatus::Unavailable
    );
    assert_eq!(
        metrics.host_pressure.latest_memory_source_status,
        RuntimeMemoryPressureSourceStatus::Unavailable
    );
    assert_eq!(metrics.host_pressure.latest_effective_dispatch_seats, 1);
    assert!(
        metrics.tenants.is_empty(),
        "host pressure telemetry must not add tenant-cardinality labels"
    );
}

#[test]
fn runtime_policy_carries_adaptive_controller_settings_without_enabling_defaults() {
    let policy = RuntimePolicy::new(RuntimeLimits::default());
    assert!(
        !policy
            .adaptive_controller_settings()
            .live_adaptive_defaults_enabled()
    );

    let adaptive = RuntimeAdaptiveControllerSettings::shadow(RuntimeControllerReplayConfig {
        stable_window_observations: NonZeroUsize::new(2).expect("nonzero stable window"),
        panic_window_observations: NonZeroUsize::new(1).expect("nonzero panic window"),
        ..RuntimeControllerReplayConfig::default()
    });
    let policy = policy.with_adaptive_controller_settings(adaptive);

    assert_eq!(
        policy.adaptive_controller_settings().mode(),
        RuntimeAdaptiveControllerMode::Shadow
    );
    assert!(
        !policy
            .adaptive_controller_settings()
            .live_adaptive_defaults_enabled()
    );
}

#[test]
fn runtime_policy_clone_with_effective_plan_preserves_operational_controls() {
    let budget =
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(4).unwrap());
    let adaptive = RuntimeAdaptiveControllerSettings::shadow(RuntimeControllerReplayConfig {
        stable_window_observations: NonZeroUsize::new(2).expect("nonzero stable window"),
        panic_window_observations: NonZeroUsize::new(1).expect("nonzero panic window"),
        ..RuntimeControllerReplayConfig::default()
    });
    let policy = RuntimePolicy::with_host_resource_governor(
        RuntimeLimits {
            max_concurrent_runtime_instances: 4,
            ..RuntimeLimits::default()
        },
        budget,
        Arc::new(FixedRuntimeHostPressureSource(
            RuntimeHostPressureSample::observed(
                RuntimeHostPressureLevel::High,
                RuntimeMemoryPressureSample::observed(mib(512), mib(768), mib(960)).classify(),
                false,
            ),
        )),
    )
    .with_adaptive_controller_settings(adaptive);
    let effective = RuntimeScalingTarget {
        min_warm: 0,
        max_warm: 2,
        scale_down_delay_secs: 120,
        autoscaling: true,
    };
    let plan = EffectiveRuntimeScalingPlan::baked_standard("messages:send", 6)
        .with_pressure_adjustment(effective, RuntimeScalingAdjustmentReason::HostPressure);

    let mut plans = RuntimeScalingPlanSet::single(EffectiveRuntimeScalingPlan::baked_standard(
        "__default__",
        4,
    ));
    plans.insert_function_override(plan.clone());

    let cloned = policy.clone_with_effective_scaling_plans(plans);

    assert_eq!(cloned.effective_scaling_plan().function, "__default__");
    assert_eq!(
        cloned.effective_scaling_plan_for_function("messages:send"),
        &plan
    );
    assert_eq!(
        cloned
            .effective_scaling_plan_for_function("messages:list")
            .function,
        "__default__"
    );
    assert_eq!(
        cloned.effective_scaling_plans().function_override_count(),
        1
    );
    assert_eq!(cloned.host_resource_budget(), budget);
    assert_eq!(
        cloned.adaptive_controller_settings().mode(),
        RuntimeAdaptiveControllerMode::Shadow
    );
    assert_eq!(
        cloned.host_resource_decision().host_pressure_level,
        RuntimeHostPressureLevel::High
    );
    assert_eq!(
        policy.metrics_snapshot().host_pressure.decisions,
        1,
        "policy overlays must keep reporting into the original lane metrics"
    );
}

#[test]
fn runtime_policy_clone_with_host_governor_preserves_lane_metrics() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        max_concurrent_runtime_instances: 2,
        ..RuntimeLimits::default()
    });
    policy.metrics().record_worker_dispatch();
    let governed = policy.clone_with_host_resource_governor(
        RuntimeHostResourceBudget::conservative_for_logical_cpus(NonZeroUsize::new(2).unwrap()),
        Arc::new(FixedRuntimeHostPressureSource(
            RuntimeHostPressureSample::unavailable(
                RuntimeMemoryPressureSample::unavailable().classify(),
            ),
        )),
    );

    assert_eq!(
        governed.metrics_snapshot().worker_dispatched_invocations,
        1,
        "host-governor overlays must not fork lane dispatch metrics"
    );
    governed.metrics().record_worker_dispatch();
    assert_eq!(
        policy.metrics_snapshot().worker_dispatched_invocations,
        2,
        "host-governor overlay metrics must remain visible through the source policy"
    );
}

#[test]
fn application_preset_supports_node_lts_targets() {
    let web_limits = RuntimeLimits::application_web_standard().normalized();
    assert_eq!(web_limits.mode, RuntimeMode::Standard);
    assert_eq!(web_limits.language, RuntimeLanguage::JavaScript);
    assert_eq!(web_limits.preset, RuntimePreset::Application);
    assert!(web_limits.grants.run.is_empty());
    assert_eq!(
        web_limits.compatibility_target,
        RuntimeCompatibilityTarget::WebStandardIsolate
    );

    let node20_limits = RuntimeLimits::application_node20().normalized();
    assert_eq!(node20_limits.mode, RuntimeMode::Standard);
    assert_eq!(node20_limits.preset, RuntimePreset::Application);
    assert!(node20_limits.grants.run.is_empty());
    assert!(node20_limits.grants.net_connect.is_empty());
    assert!(node20_limits.grants.net_listen.is_empty());
    assert!(node20_limits.grants.worker.is_empty());
    assert!(node20_limits.grants.sys.contains(&"osRelease".to_string()));
    assert!(!node20_limits.grants.sys.contains(&"homedir".to_string()));
    assert!(!node20_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node20_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node20
    );

    let node_limits = RuntimeLimits::application_node22().normalized();
    assert_eq!(node_limits.mode, RuntimeMode::Standard);
    assert_eq!(node_limits.preset, RuntimePreset::Application);
    assert!(node_limits.grants.run.is_empty());
    assert!(node_limits.grants.net_connect.is_empty());
    assert!(node_limits.grants.net_listen.is_empty());
    assert!(node_limits.grants.worker.is_empty());
    assert!(node_limits.grants.sys.contains(&"osRelease".to_string()));
    assert!(!node_limits.grants.sys.contains(&"homedir".to_string()));
    assert!(!node_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );

    let node24_limits = RuntimeLimits::application_node24().normalized();
    assert_eq!(node24_limits.mode, RuntimeMode::Standard);
    assert_eq!(node24_limits.preset, RuntimePreset::Application);
    assert!(node24_limits.grants.run.is_empty());
    assert!(node24_limits.grants.net_connect.is_empty());
    assert!(node24_limits.grants.net_listen.is_empty());
    assert!(node24_limits.grants.worker.is_empty());
    assert!(node24_limits.grants.sys.contains(&"osRelease".to_string()));
    assert!(!node24_limits.grants.sys.contains(&"homedir".to_string()));
    assert!(!node24_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node24_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );

    let node26_limits = RuntimeLimits::application_node26().normalized();
    assert_eq!(node26_limits.mode, RuntimeMode::Standard);
    assert_eq!(node26_limits.preset, RuntimePreset::Application);
    assert!(node26_limits.grants.run.is_empty());
    assert!(node26_limits.grants.net_connect.is_empty());
    assert!(node26_limits.grants.net_listen.is_empty());
    assert!(node26_limits.grants.worker.is_empty());
    assert!(node26_limits.grants.sys.contains(&"osRelease".to_string()));
    assert!(!node26_limits.grants.sys.contains(&"homedir".to_string()));
    assert!(!node26_limits.grants.sys.contains(&"inspector".to_string()));
    assert_eq!(
        node26_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node26
    );
}

fn mib(mebibytes: u64) -> u64 {
    mebibytes * 1024 * 1024
}

#[test]
fn node_permission_profiles_are_separated_by_deployment_intent() {
    let production = RuntimeLimits::application_node22().normalized();
    assert_eq!(
        production.grants,
        RuntimeGrants::application_node_production_in_process()
    );
    assert!(production.grants.net_connect.is_empty());
    assert!(production.grants.net_listen.is_empty());
    assert!(production.grants.worker.is_empty());
    assert!(production.grants.run.is_empty());
    assert!(production.grants.ffi.is_empty());
    assert!(production.grants.sys.contains(&"osRelease".to_string()));
    assert!(!production.grants.sys.contains(&"homedir".to_string()));
    assert!(!production.grants.sys.contains(&"inspector".to_string()));
    assert!(
        !production
            .grants
            .env_read
            .contains(&"NODE_TLS_REJECT_UNAUTHORIZED".to_string()),
        "production in-process Node must not inherit ambient TLS-disable env"
    );

    let local_dev = RuntimeLimits::application_node22_local_development().normalized();
    assert_eq!(
        local_dev.grants,
        RuntimeGrants::application_node_local_development()
    );
    assert!(
        local_dev
            .grants
            .net_connect
            .contains(&"localhost".to_string())
    );
    assert!(local_dev.grants.net_listen.contains(&"0.0.0.0".to_string()));
    assert!(local_dev.grants.worker.contains(&"thread".to_string()));
    assert!(local_dev.grants.sys.contains(&"inspector".to_string()));
    assert!(
        local_dev
            .grants
            .env_read
            .contains(&"NODE_TLS_REJECT_UNAUTHORIZED".to_string()),
        "local development keeps the compatibility env escape explicit"
    );

    let service = RuntimeLimits::application_node22_service_microvm().normalized();
    assert_eq!(
        service.grants,
        RuntimeGrants::application_node_service_microvm()
    );
    assert!(service.grants.net_listen.contains(&"[::]".to_string()));
    assert!(service.grants.worker.contains(&"thread".to_string()));
}

#[test]
fn tooling_preset_requires_node_target() {
    for valid in [
        RuntimeLimits::tooling_node20().normalized(),
        RuntimeLimits::tooling_node22().normalized(),
        RuntimeLimits::tooling_node24().normalized(),
        RuntimeLimits::tooling_node26().normalized(),
    ] {
        assert_eq!(valid.mode, RuntimeMode::Standard);
        assert_eq!(valid.preset, RuntimePreset::Tooling);
        assert_eq!(
            valid.grants.run,
            vec![
                "$discovered_tooling".to_string(),
                "$runtime_self_exec".to_string(),
                "$runtime_host_exec".to_string(),
            ]
        );
        assert!(valid.compatibility_target.is_node());
    }

    let err = std::panic::catch_unwind(|| {
        RuntimeLimits {
            preset: RuntimePreset::Tooling,
            grants: RuntimeGrants::tooling(),
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            ..RuntimeLimits::default()
        }
        .normalized()
    });
    assert!(err.is_err());
}

#[test]
fn runtime_self_exec_run_grant_requires_node_target() {
    let valid = RuntimeLimits {
        grants: RuntimeGrants {
            run: vec!["$runtime_self_exec".to_string()],
            ..RuntimeGrants::application_node()
        },
        ..RuntimeLimits::application_node24()
    }
    .normalized();
    assert_eq!(valid.grants.run, vec!["$runtime_self_exec".to_string()]);

    let err = std::panic::catch_unwind(|| {
        RuntimeLimits {
            grants: RuntimeGrants {
                run: vec!["$runtime_self_exec".to_string()],
                ..RuntimeGrants::application_node()
            },
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            ..RuntimeLimits::default()
        }
        .normalized()
    });
    assert!(err.is_err());
}

#[test]
fn runtime_modes_enforce_grant_ceilings() {
    let restricted = RuntimeLimits::restricted_code().normalized();
    assert_eq!(restricted.mode, RuntimeMode::Restricted);
    assert_eq!(restricted.language, RuntimeLanguage::JavaScript);
    assert_eq!(restricted.preset, RuntimePreset::Code);
    assert_eq!(restricted.grants, RuntimeGrants::restricted());

    let restricted_run = std::panic::catch_unwind(|| {
        RuntimeLimits {
            mode: RuntimeMode::Restricted,
            preset: RuntimePreset::Code,
            grants: RuntimeGrants {
                run: vec!["node".to_string()],
                ..RuntimeGrants::restricted()
            },
            ..RuntimeLimits::default()
        }
        .normalized()
    });
    assert!(restricted_run.is_err());

    let restricted_node_preset_rewrite = std::panic::catch_unwind(|| {
        RuntimeLimits {
            mode: RuntimeMode::Restricted,
            preset: RuntimePreset::Application,
            compatibility_target: RuntimeCompatibilityTarget::Node22,
            grants: RuntimeGrants::application_web_standard(),
            ..RuntimeLimits::default()
        }
        .normalized()
    });
    assert!(
        restricted_node_preset_rewrite.is_err(),
        "effective node grants must be checked against the final Restricted ceiling"
    );

    let standard_ffi = std::panic::catch_unwind(|| {
        RuntimeLimits {
            mode: RuntimeMode::Standard,
            grants: RuntimeGrants {
                ffi: vec!["/usr/lib/libexample.dylib".to_string()],
                ..RuntimeGrants::application_node()
            },
            ..RuntimeLimits::application_node22()
        }
        .normalized()
    });
    assert!(standard_ffi.is_err());

    let privileged = RuntimeLimits {
        grants: RuntimeGrants {
            ffi: vec!["/usr/lib/libexample.dylib".to_string()],
            ..RuntimeGrants::restricted()
        },
        ..RuntimeLimits::privileged_operator()
    }
    .normalized();
    assert_eq!(privileged.mode, RuntimeMode::Privileged);
    assert_eq!(privileged.preset, RuntimePreset::Operator);
    assert_eq!(privileged.grants.ffi, vec!["/usr/lib/libexample.dylib"]);
}

#[test]
fn runtime_preset_and_execution_model_are_independent_axes() {
    let run_to_completion = RuntimeLimits {
        preset: RuntimePreset::Application,
        compatibility_target: RuntimeCompatibilityTarget::Node22,
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }
    .normalized();
    assert_eq!(run_to_completion.preset, RuntimePreset::Application);
    assert_eq!(
        run_to_completion.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );
    assert_eq!(
        run_to_completion.execution_model,
        RuntimeExecutionModel::RunToCompletion
    );

    let cooperative = RuntimeLimits {
        preset: RuntimePreset::Application,
        compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        ..RuntimeLimits::default()
    }
    .normalized();
    assert_eq!(cooperative.preset, RuntimePreset::Application);
    assert_eq!(
        cooperative.compatibility_target,
        RuntimeCompatibilityTarget::WebStandardIsolate
    );
    assert_eq!(
        cooperative.execution_model,
        RuntimeExecutionModel::CooperativeLocker
    );
}

#[test]
fn runtime_policy_accepts_current_v8_javascript_axis_combinations() {
    for compatibility_target in [
        RuntimeCompatibilityTarget::WebStandardIsolate,
        RuntimeCompatibilityTarget::Node20,
        RuntimeCompatibilityTarget::Node22,
        RuntimeCompatibilityTarget::Node24,
    ] {
        let run_to_completion = RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target,
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        });
        assert_eq!(
            run_to_completion.limits().backend_kind,
            RuntimeBackendKind::V8
        );
        assert_eq!(
            run_to_completion.limits().backend_lifecycle_policy,
            RuntimeBackendLifecyclePolicy::V8DenoCorePool
        );
        assert_eq!(
            run_to_completion.limits().backend_trust_tier,
            RuntimeBackendTrustTier::InProcessUntrusted
        );
        assert_eq!(
            run_to_completion.limits().backend_lockdown_profile,
            RuntimeBackendLockdownProfile::V8DenoCore
        );
        assert_eq!(
            run_to_completion.limits().bundle_content_kind,
            RuntimeBundleContentKind::JavaScript
        );
        assert_eq!(
            run_to_completion.limits().javascript_evaluation_format,
            RuntimeJavaScriptEvaluationFormat::EsModule
        );

        let cooperative_snapshot = RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        });
        assert_eq!(
            cooperative_snapshot.limits().execution_model,
            RuntimeExecutionModel::CooperativeLocker
        );
    }

    let cooperative_warm_pool = RuntimePolicy::new(RuntimeLimits {
        backend_kind: RuntimeBackendKind::V8,
        bundle_content_kind: RuntimeBundleContentKind::JavaScript,
        compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        ..RuntimeLimits::default()
    });
    assert_eq!(
        cooperative_warm_pool.limits().runtime_pool_kind,
        RuntimePoolKind::WarmPool
    );

    let cooperative_context_recycle = RuntimePolicy::new(RuntimeLimits {
        backend_kind: RuntimeBackendKind::V8,
        bundle_content_kind: RuntimeBundleContentKind::JavaScript,
        compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
        ..RuntimeLimits::default()
    });
    assert_eq!(
        cooperative_context_recycle.limits().runtime_pool_kind,
        RuntimePoolKind::WarmContextRecycle
    );
    assert_eq!(
        cooperative_context_recycle
            .limits()
            .module_state_semantics(),
        RuntimeModuleStateSemantics::FreshPerInvocation
    );
}

#[test]
#[should_panic(expected = "WarmContextRecycle requires CooperativeLocker")]
fn warm_context_recycle_rejects_run_to_completion() {
    let _ = RuntimePolicy::new(RuntimeLimits {
        execution_model: RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
        ..RuntimeLimits::default()
    });
}

#[test]
#[should_panic(expected = "requires same-owner exact-authority realm reuse proof")]
fn warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof() {
    let _ = RuntimePolicy::new(RuntimeLimits {
        compatibility_target: RuntimeCompatibilityTarget::Node22,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
        ..RuntimeLimits::default()
    });
}

#[test]
fn warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        compatibility_target: RuntimeCompatibilityTarget::Node22,
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
        node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority,
        ..RuntimeLimits::default()
    });

    assert_eq!(
        policy.limits().node_full_realm_reuse_policy,
        RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority
    );
}

#[test]
fn runtime_policy_accepts_bun_jsc_only_with_proven_lockdown_profile() {
    let policy = RuntimePolicy::new(RuntimeLimits::application_bun_jsc());
    let limits = policy.limits();
    assert_eq!(limits.backend_kind, RuntimeBackendKind::BunJsc);
    assert_eq!(
        limits.backend_trust_tier,
        RuntimeBackendTrustTier::InProcessUntrusted
    );
    assert_eq!(
        limits.backend_lockdown_profile,
        RuntimeBackendLockdownProfile::BunJscInProcessUntrusted
    );
    assert_eq!(
        limits.backend_lifecycle_policy,
        RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired
    );
    assert_eq!(
        limits.javascript_evaluation_format,
        RuntimeJavaScriptEvaluationFormat::ProgramWrapper
    );
    assert_eq!(
        limits.compatibility_target,
        RuntimeCompatibilityTarget::BunJsc
    );
    assert_eq!(
        limits.execution_model,
        RuntimeExecutionModel::BackendOwnedEventLoop
    );
    assert_eq!(
        limits.runtime_pool_kind,
        RuntimePoolKind::BunJscFreshDiscard
    );
    assert_eq!(
        limits.memory_enforcement,
        RuntimeMemoryEnforcement::OuterQuotaRequired
    );
    assert!(limits.grants.run.is_empty());
    assert!(limits.grants.ffi.is_empty());
    assert!(limits.grants.net_connect.is_empty());
    assert!(limits.grants.net_listen.is_empty());
}

#[test]
fn runtime_pool_kind_exposes_engine_owned_diagnostics() {
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::StartupSnapshotCache).unwrap(),
        serde_json::json!("startup_snapshot_cache")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::WarmPool).unwrap(),
        serde_json::json!("warm_pool")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::WarmContextRecycle).unwrap(),
        serde_json::json!("warm_context_recycle")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::BunJscTrustedRetained).unwrap(),
        serde_json::json!("bun_jsc_trusted_retained")
    );
    assert_eq!(
        serde_json::to_value(RuntimePoolKind::BunJscFreshDiscard).unwrap(),
        serde_json::json!("bun_jsc_fresh_discard")
    );
    assert_eq!(
        serde_json::to_value(RuntimeMemoryEnforcement::V8IsolateHeapLimit).unwrap(),
        serde_json::json!("v8_isolate_heap_limit")
    );
    assert_eq!(
        serde_json::to_value(RuntimeMemoryEnforcement::OuterQuotaRequired).unwrap(),
        serde_json::json!("outer_quota_required")
    );

    let bun_trusted_retained = RuntimeLimits {
        runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        bun_trusted_retained.module_state_semantics(),
        RuntimeModuleStateSemantics::WarmPerBundle
    );
    assert_eq!(
        bun_trusted_retained.reset_capabilities(),
        RuntimeResetCapabilities {
            op_state_per_invocation: true,
            bootstrap_state_per_invocation: true,
            user_module_state_per_invocation: false,
        }
    );

    let bun_fresh_discard = RuntimeLimits {
        runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        bun_fresh_discard.module_state_semantics(),
        RuntimeModuleStateSemantics::FreshPerInvocation
    );
    assert_eq!(
        bun_fresh_discard.reset_capabilities(),
        RuntimeResetCapabilities {
            op_state_per_invocation: true,
            bootstrap_state_per_invocation: true,
            user_module_state_per_invocation: true,
        }
    );

    let v8_warm_context_recycle = RuntimeLimits {
        runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
        ..RuntimeLimits::default()
    };
    assert_eq!(
        v8_warm_context_recycle.module_state_semantics(),
        RuntimeModuleStateSemantics::FreshPerInvocation
    );
    assert_eq!(
        v8_warm_context_recycle.reset_capabilities(),
        RuntimeResetCapabilities {
            op_state_per_invocation: true,
            bootstrap_state_per_invocation: true,
            user_module_state_per_invocation: true,
        }
    );
}

#[test]
fn runtime_limits_expose_tenant_budget_from_normalized_limits() {
    let mut limits = RuntimeLimits::application_web_standard();
    limits.max_concurrent_runtime_instances = 8;
    limits.worker_threads = 16;
    limits.max_active_top_level_invocations_per_tenant = 3;
    limits.max_in_flight_top_level_invocations_per_tenant = 5;
    limits.max_queued_top_level_invocations_per_tenant = 7;
    limits.max_heap_mb = 256;
    limits.execution_timeout = Duration::from_secs(9);
    limits.system_timeout = Duration::from_secs(13);
    limits.max_nested_runtime_invocations = 11;

    let budget = limits.tenant_budget();

    assert_eq!(budget.max_active_runtime_slots, 3);
    assert_eq!(budget.max_in_flight_top_level_invocations, 5);
    assert_eq!(budget.max_queued_top_level_invocations, 7);
    assert_eq!(budget.max_worker_thread_slots, 3);
    assert_eq!(budget.max_heap_mb_per_runtime, 256);
    assert_eq!(
        budget.memory_enforcement,
        RuntimeMemoryEnforcement::V8IsolateHeapLimit
    );
    assert_eq!(budget.max_active_heap_mb, 768);
    assert_eq!(budget.execution_timeout, Duration::from_secs(9));
    assert_eq!(budget.system_timeout, Duration::from_secs(13));
    assert_eq!(budget.max_nested_runtime_invocations_per_top_level, 11);
    assert_eq!(RuntimePolicy::new(limits).tenant_budget(), budget);
}

#[test]
fn runtime_policy_rejects_unsupported_engine_axis_combinations() {
    let run_to_completion_warm_pool = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        run_to_completion_warm_pool.is_err(),
        "WarmPool must not be accepted with RunToCompletion"
    );

    let wasm_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::WasmComponent,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        wasm_on_v8.is_err(),
        "V8 must reject non-JavaScript bundle content"
    );

    let program_wrapper_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        program_wrapper_on_v8.is_err(),
        "V8 must reject Bun/JSC program-wrapper evaluation"
    );

    let bun_lifecycle_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_lifecycle_on_v8.is_err(),
        "V8 must reject Bun/JSC lifecycle policies"
    );

    for runtime_pool_kind in [
        RuntimePoolKind::BunJscTrustedRetained,
        RuntimePoolKind::BunJscFreshDiscard,
    ] {
        let bun_pool_on_v8 = std::panic::catch_unwind(|| {
            RuntimePolicy::new(RuntimeLimits {
                backend_kind: RuntimeBackendKind::V8,
                runtime_pool_kind,
                ..RuntimeLimits::default()
            })
        });
        assert!(
            bun_pool_on_v8.is_err(),
            "V8 must reject Bun/JSC pool kind {runtime_pool_kind:?}"
        );
    }

    let bun_target_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_target_on_v8.is_err(),
        "V8 must reject Bun/JSC compatibility target"
    );

    let outer_quota_on_v8 = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::V8,
            memory_enforcement: RuntimeMemoryEnforcement::OuterQuotaRequired,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        outer_quota_on_v8.is_err(),
        "V8 must reject outer-quota-only memory enforcement"
    );

    let bun_jsc_without_matching_profile = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_without_matching_profile.is_err(),
        "Bun/JSC must require an explicit matching lockdown profile"
    );

    let bun_jsc_proof_only = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::ProofOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscProofOnly,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_proof_only.is_err(),
        "Bun/JSC proof-only profile must remain non-selectable"
    );

    let bun_jsc_trusted_only = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessTrustedOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscTrustedRetained,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_trusted_only.is_err(),
        "Bun/JSC trusted-only generated-wrapper profile must not create a product route"
    );

    let bun_jsc_untrusted_wrong_target = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            backend_lifecycle_policy:
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::Node22,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_untrusted_wrong_target.is_err(),
        "Bun/JSC in-process-untrusted profile must not be labeled as a Node target"
    );

    let bun_jsc_untrusted_without_outer_quota_memory = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            memory_enforcement: RuntimeMemoryEnforcement::V8IsolateHeapLimit,
            ..RuntimeLimits::application_bun_jsc()
        })
    });
    assert!(
        bun_jsc_untrusted_without_outer_quota_memory.is_err(),
        "Bun/JSC in-process-untrusted profile must require outer-quota memory enforcement"
    );

    let bun_jsc_untrusted_missing_outer_quota_lifecycle = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_untrusted_missing_outer_quota_lifecycle.is_err(),
        "Bun/JSC in-process-untrusted profile must require the fresh-VM outer-quota lifecycle policy"
    );

    let bun_jsc_trusted_profile_with_v8_pool = std::panic::catch_unwind(|| {
        RuntimePolicy::new(RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessTrustedOnly,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
            ..RuntimeLimits::default()
        })
    });
    assert!(
        bun_jsc_trusted_profile_with_v8_pool.is_err(),
        "Bun/JSC must reject V8/Deno pool kinds"
    );
}

#[test]
fn runtime_policy_rejects_bundle_content_kind_mismatches() {
    let policy = RuntimePolicy::new(RuntimeLimits::default());
    assert!(
        policy
            .validate_bundle_content_kind(RuntimeBundleContentKind::JavaScript)
            .is_ok()
    );

    let error = policy
        .validate_bundle_content_kind(RuntimeBundleContentKind::WasmComponent)
        .expect_err("V8 JavaScript policy should reject Wasm content");
    assert!(
        error
            .to_string()
            .contains("runtime bundle content kind WasmComponent does not match"),
        "unexpected content-kind mismatch error: {error}"
    );
}
