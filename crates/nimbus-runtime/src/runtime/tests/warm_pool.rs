use super::*;
use crate::backends::v8::{
    RuntimeReuseLifecycleState, V8RuntimeConstructionMode, V8WorkerRuntimePool,
    heap_carryover_limit_bytes_for_test, retained_entry_eviction_count_for_pressure_for_test,
    v8_bootstrap_snapshot_build_count_for_test,
};
use crate::limits::{RuntimeCompatibilityTarget, RuntimeMemoryPressureLevel};

pub(super) const CROSS_TENANT_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-cross-tenant-isolation",
        "cooperative-warm-pool",
        "warm-pool entries stay isolated by tenant label even when bundle bytes match",
        "runtime::tests::warm_pool::warm_pool_cross_tenant_isolation_subprocess",
    );

pub(super) const SERVICE_GRANT_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-service-grant-partition",
        "cooperative-warm-pool",
        "warm-pool entries stay isolated by service-op state and exact service grants",
        "runtime::tests::warm_pool::warm_pool_partitions_by_exact_service_grants_subprocess",
    );

pub(super) const INVOCATION_KIND_REUSE_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-invocation-kind-reuse",
        "cooperative-warm-pool",
        "warm-pool entries are reused across invocation kinds: the configured grants are the \
         authority surface, so kind is not a partition dimension",
        "runtime::tests::warm_pool::warm_pool_reuses_across_invocation_kinds_subprocess",
    );

pub(super) const NODE_SNAPSHOT_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-node-startup-snapshot",
        "cooperative-warm-pool",
        "node-compatible warm-pool cold misses consume the V8 startup snapshot path",
        "runtime::tests::warm_pool::warm_pool_uses_startup_snapshot_for_node_targets_subprocess",
    );

pub(super) const BOUNDARY_GC_WARM_POOL_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-warm-pool-boundary-gc",
    "cooperative-warm-pool",
    "warm-pool return runs boundary memory maintenance before retention",
    "runtime::tests::warm_pool::warm_pool_runs_boundary_memory_maintenance_subprocess",
);

pub(super) const MAX_REUSE_CONDEMNATION_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-max-reuse-condemnation",
        "cooperative-warm-pool",
        "warm-pool return condemns runtimes that reached max_warm_reuses",
        "runtime::tests::warm_pool::warm_pool_condemns_after_max_warm_reuses_subprocess",
    );

pub(super) const MEMORY_PRESSURE_EVICTION_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-memory-pressure-eviction",
        "cooperative-warm-pool",
        "warm-pool memory pressure response evicts idle retained runtimes before OOM",
        "runtime::tests::warm_pool::warm_pool_memory_pressure_evicts_idle_entries_subprocess",
    );

pub(super) const NEAR_HEAP_LIMIT_WARM_POOL_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-warm-pool-near-heap-limit-condemnation",
        "cooperative-warm-pool",
        "near-heap-limit callbacks condemn checked-out warm-pool runtimes instead of retaining them",
        "runtime::tests::warm_pool::warm_pool_condemns_checked_out_runtime_after_near_heap_limit_subprocess",
    );

#[test]
#[should_panic(expected = "WarmPool requires CooperativeLocker")]
fn warm_pool_with_run_to_completion_fails_fast() {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.runtime_pool_kind = crate::limits::RuntimePoolKind::WarmPool;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let _policy = Arc::new(RuntimePolicy::new(limits));
}

/// Proves that `RuntimeBundleIdentity` includes the tenant dimension:
/// two bundles with identical entrypoint and SHA-256 but different tenant
/// labels produce different identities.
#[test]
fn bundle_identity_includes_tenant_label() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"globalThis.__nimbusInvoke = function () { return {}; }; export {};"#,
    )
    .expect("bundle should write");

    let sha =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");

    let bundle_a = RuntimeBundle::for_tenant(&bundle_path, &sha, "tenant-a")
        .expect("tenant-a bundle should build");
    let bundle_b = RuntimeBundle::for_tenant(&bundle_path, &sha, "tenant-b")
        .expect("tenant-b bundle should build");
    let bundle_no_tenant = RuntimeBundle::with_expected_sha256(&bundle_path, &sha)
        .expect("no-tenant bundle should build");

    // Same content identity.
    assert_eq!(
        bundle_a.identity().entrypoint(),
        bundle_b.identity().entrypoint()
    );
    assert_eq!(
        bundle_a.identity().expected_sha256(),
        bundle_b.identity().expected_sha256()
    );

    // Tenant label differs.
    assert_eq!(bundle_a.identity().tenant_label(), Some("tenant-a"));
    assert_eq!(bundle_b.identity().tenant_label(), Some("tenant-b"));
    assert_eq!(bundle_no_tenant.identity().tenant_label(), None);

    // Full identity differs due to tenant dimension.
    assert_ne!(bundle_a.identity(), bundle_b.identity());
    assert_ne!(bundle_a.identity(), bundle_no_tenant.identity());
    assert_ne!(bundle_b.identity(), bundle_no_tenant.identity());

    // Same tenant + same content = same identity.
    let bundle_a2 = RuntimeBundle::for_tenant(&bundle_path, &sha, "tenant-a")
        .expect("second tenant-a bundle should build");
    assert_eq!(bundle_a.identity(), bundle_a2.identity());
}

/// Proves that warm pool entries cannot be shared across tenants even when
/// bundles have identical entrypoint and SHA-256 content hash.
///
/// 1. Invoke tenant-A's bundle, return the warm runtime to the pool.
/// 2. Attempt to take a warm runtime for tenant-B → assert cold miss.
/// 3. Take again for tenant-A → assert warm hit.
#[test]
fn warm_pool_cross_tenant_isolation() {
    run_v8_sensitive_runtime_test_in_subprocess(CROSS_TENANT_WARM_POOL_CASE);
}

#[test]
fn warm_pool_partitions_by_exact_service_grants() {
    run_v8_sensitive_runtime_test_in_subprocess(SERVICE_GRANT_WARM_POOL_CASE);
}

#[test]
fn warm_pool_reuses_across_invocation_kinds() {
    run_v8_sensitive_runtime_test_in_subprocess(INVOCATION_KIND_REUSE_WARM_POOL_CASE);
}

#[test]
fn warm_pool_uses_startup_snapshot_for_node_targets() {
    run_v8_sensitive_runtime_test_in_subprocess(NODE_SNAPSHOT_WARM_POOL_CASE);
}

#[test]
fn warm_pool_runs_boundary_memory_maintenance() {
    run_v8_sensitive_runtime_test_in_subprocess(BOUNDARY_GC_WARM_POOL_CASE);
}

#[test]
fn warm_pool_condemns_after_max_warm_reuses() {
    run_v8_sensitive_runtime_test_in_subprocess(MAX_REUSE_CONDEMNATION_WARM_POOL_CASE);
}

#[test]
fn warm_pool_memory_pressure_evicts_idle_entries() {
    run_v8_sensitive_runtime_test_in_subprocess(MEMORY_PRESSURE_EVICTION_WARM_POOL_CASE);
}

#[test]
fn warm_pool_condemns_checked_out_runtime_after_near_heap_limit() {
    run_v8_sensitive_runtime_test_in_subprocess(NEAR_HEAP_LIMIT_WARM_POOL_CASE);
}

#[test]
fn warm_pool_heap_carryover_limit_is_internal_fraction_of_heap_cap() {
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_heap_mb = 128;

    assert_eq!(
        heap_carryover_limit_bytes_for_test(&limits),
        96 * 1024 * 1024
    );
}

#[test]
fn warm_pool_pressure_eviction_count_is_conservative() {
    assert_eq!(
        retained_entry_eviction_count_for_pressure_for_test(RuntimeMemoryPressureLevel::Nominal, 5),
        0
    );
    assert_eq!(
        retained_entry_eviction_count_for_pressure_for_test(RuntimeMemoryPressureLevel::High, 5),
        3
    );
    assert_eq!(
        retained_entry_eviction_count_for_pressure_for_test(
            RuntimeMemoryPressureLevel::Critical,
            5
        ),
        5
    );
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_cross_tenant_isolation_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_cross_tenant_isolation_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_partitions_by_exact_service_grants_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_partitions_by_exact_service_grants_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_reuses_across_invocation_kinds_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_reuses_across_invocation_kinds_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_uses_startup_snapshot_for_node_targets_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_uses_startup_snapshot_for_node_targets_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_runs_boundary_memory_maintenance_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_runs_boundary_memory_maintenance_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_condemns_after_max_warm_reuses_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_condemns_after_max_warm_reuses_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_memory_pressure_evicts_idle_entries_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_memory_pressure_evicts_idle_entries_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate warm-pool locker V8 state"]
fn warm_pool_condemns_checked_out_runtime_after_near_heap_limit_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_condemns_checked_out_runtime_after_near_heap_limit_inner());
}

async fn warm_pool_cross_tenant_isolation_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");

    let bundle_tenant_a = RuntimeBundle::for_tenant(&bundle_path, &expected_sha256, "tenant-a")
        .expect("tenant-a bundle should build");
    let bundle_tenant_b = RuntimeBundle::for_tenant(&bundle_path, &expected_sha256, "tenant-b")
        .expect("tenant-b bundle should build");

    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    // Step 1: Take a runtime for tenant-A (cold miss — pool is empty).
    let reusable_a = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle_tenant_a, true)
        .expect("tenant-a cold take should succeed");
    let metrics_after_cold = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics_after_cold.warm_pool_misses, 1);
    assert_eq!(metrics_after_cold.warm_pool_hits, 0);

    // Return the runtime to the pool under tenant-A's identity.
    v8_runtime_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle_tenant_a,
        None,
        reusable_a,
    );
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);

    // Step 2: Attempt take for tenant-B → must be a cold miss because the
    // pooled entry belongs to tenant-A.
    let reusable_b = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle_tenant_b, true)
        .expect("tenant-b cold take should succeed");
    let metrics_after_cross = runtime_owner.policy.metrics_snapshot();
    assert_eq!(
        metrics_after_cross.warm_pool_misses, 2,
        "cross-tenant take must be a cold miss"
    );
    assert_eq!(
        metrics_after_cross.warm_pool_hits, 0,
        "cross-tenant take must not produce a warm hit"
    );

    // The tenant-A entry should still be in the pool (tenant-B got a fresh one).
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);

    // Return tenant-B's runtime.
    v8_runtime_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle_tenant_b,
        None,
        reusable_b,
    );
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 2);

    // Step 3: Take for tenant-A again → must be a warm hit.
    let _reusable_a2 = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle_tenant_a, true)
        .expect("tenant-a warm take should succeed");
    let metrics_after_warm = runtime_owner.policy.metrics_snapshot();
    assert_eq!(
        metrics_after_warm.warm_pool_hits, 1,
        "same-tenant take must be a warm hit"
    );
    assert_eq!(
        metrics_after_warm.warm_pool_misses, 2,
        "same-tenant take must not increment misses"
    );
    assert_eq!(
        metrics_after_warm.retained_runtime_pool_entries, 1,
        "taking a retained entry must decrement the retained-entry gauge"
    );

    // Pool should now have 1 entry (tenant-B's).
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);
}

async fn warm_pool_partitions_by_exact_service_grants_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return {};
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);

    let mut native_limits = cooperative_warm_pool_runtime_test_limits();
    native_limits.max_concurrent_runtime_instances = 1;
    native_limits.worker_threads = 1;
    native_limits.service_capability_enabled = true;
    native_limits.grants.service = vec!["db".to_string()];
    let native_policy = Arc::new(RuntimePolicy::new(native_limits));
    let native_runtime = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), native_policy);

    let mut adapter_granted_limits = cooperative_warm_pool_runtime_test_limits();
    adapter_granted_limits.max_concurrent_runtime_instances = 1;
    adapter_granted_limits.worker_threads = 1;
    adapter_granted_limits.grants.service = vec!["db".to_string()];
    let adapter_granted_policy = Arc::new(RuntimePolicy::new(adapter_granted_limits));
    let adapter_granted_runtime =
        NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), adapter_granted_policy);

    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    let native = v8_runtime_pool
        .take_runtime_with_options(&native_runtime, &bundle, true)
        .expect("native service runtime cold take should succeed");
    let native_metrics_after_cold = native_runtime.policy.metrics_snapshot();
    assert_eq!(native_metrics_after_cold.warm_pool_misses, 1);
    assert_eq!(native_metrics_after_cold.warm_pool_hits, 0);

    v8_runtime_pool.return_runtime_for_invocation(&native_runtime, &bundle, None, native);
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);

    let adapter_granted = v8_runtime_pool
        .take_runtime_with_options(&adapter_granted_runtime, &bundle, true)
        .expect("adapter-granted runtime cold take should succeed");
    let adapter_granted_metrics = adapter_granted_runtime.policy.metrics_snapshot();
    assert_eq!(
        adapter_granted_metrics.warm_pool_misses, 1,
        "adapter-created invocation must not reuse a runtime built with native service ops"
    );
    assert_eq!(
        adapter_granted_metrics.warm_pool_hits, 0,
        "matching exact grants without native service-op state must still be partitioned"
    );
    assert_eq!(
        v8_runtime_pool.warm_pool_count_for_test(),
        1,
        "native service entry should remain retained after the adapter-granted cold miss"
    );

    v8_runtime_pool.return_runtime_for_invocation(
        &adapter_granted_runtime,
        &bundle,
        None,
        adapter_granted,
    );
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 2);

    let _native_again = v8_runtime_pool
        .take_runtime_with_options(&native_runtime, &bundle, true)
        .expect("same service-op state and exact service grants should reuse the native runtime");
    let native_metrics_after_reuse = native_runtime.policy.metrics_snapshot();
    assert_eq!(
        native_metrics_after_reuse.warm_pool_hits, 1,
        "matching service-op state and exact service grants should produce a warm hit"
    );
    assert_eq!(native_metrics_after_reuse.warm_pool_misses, 1);
}

async fn warm_pool_reuses_across_invocation_kinds_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return {};
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);

    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    let query_request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let query_context = RuntimeInvocationContext::top_level(&query_request);
    let action_request = InvocationRequest {
        kind: InvocationKind::Action,
        function_name: "messages:warm".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let action_context = RuntimeInvocationContext::top_level(&action_request);

    let query = v8_runtime_pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&query_context),
            true,
        )
        .expect("query-profile runtime cold take should succeed");
    let metrics_after_query_cold = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics_after_query_cold.warm_pool_misses, 1);
    assert_eq!(metrics_after_query_cold.warm_pool_hits, 0);

    v8_runtime_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&query_context),
        query,
    );
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);

    let action = v8_runtime_pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&action_context),
            true,
        )
        .expect("action take should succeed");
    let metrics_after_action_take = runtime_owner.policy.metrics_snapshot();
    assert_eq!(
        metrics_after_action_take.warm_pool_hits, 1,
        "an action invocation reuses the query-warmed runtime: the configured grants are \
         the authority surface, so invocation kind is not a pool partition dimension"
    );
    assert_eq!(metrics_after_action_take.warm_pool_misses, 1);
    assert_eq!(
        v8_runtime_pool.warm_pool_count_for_test(),
        0,
        "the reused entry leaves the pool while the action holds it"
    );
    assert_eq!(
        metrics_after_action_take.retained_runtime_pool_entries, 0,
        "taking a retained entry must decrement the retained-entry gauge"
    );

    v8_runtime_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&action_context),
        action,
    );
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);

    let _query_again = v8_runtime_pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&query_context),
            true,
        )
        .expect("query take should reuse the returned runtime");
    let metrics_after_query_reuse = runtime_owner.policy.metrics_snapshot();
    assert_eq!(
        metrics_after_query_reuse.warm_pool_hits, 2,
        "the same runtime keeps cycling across invocation kinds"
    );
    assert_eq!(metrics_after_query_reuse.warm_pool_misses, 1);
}

async fn warm_pool_uses_startup_snapshot_for_node_targets_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { node: globalThis.process?.release?.name ?? null };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.compatibility_target = RuntimeCompatibilityTarget::Node22;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    let snapshot_builds_before = v8_bootstrap_snapshot_build_count_for_test();
    let reusable = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("node-compatible warm-pool cold take should succeed");
    let snapshot_builds_after = v8_bootstrap_snapshot_build_count_for_test();

    assert_eq!(
        reusable.construction_mode,
        V8RuntimeConstructionMode::StartupSnapshot,
        "node-compatible V8 runtimes should consume the startup snapshot path"
    );
    assert_eq!(
        snapshot_builds_after,
        snapshot_builds_before + 1,
        "first node-compatible warm-pool cold miss should build the node bootstrap snapshot"
    );
}

async fn warm_pool_runs_boundary_memory_maintenance_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {};
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_reuses = 10;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    let reusable = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("runtime cold take should succeed");

    v8_runtime_pool.return_runtime_for_invocation(&runtime_owner, &bundle, None, reusable);

    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);
    let maintenance = v8_runtime_pool
        .last_boundary_maintenance_for_test()
        .expect("retained entry should record boundary maintenance");
    assert_eq!(
        v8_runtime_pool.last_lifecycle_state_for_test(),
        Some(RuntimeReuseLifecycleState::Ready),
        "retained warm-pool entries should be ready after clean return"
    );
    assert_eq!(
        v8_runtime_pool.last_lifecycle_history_for_test(),
        Some(
            &[
                RuntimeReuseLifecycleState::Cold,
                RuntimeReuseLifecycleState::Bootstrapping,
                RuntimeReuseLifecycleState::Ready,
                RuntimeReuseLifecycleState::Leased,
                RuntimeReuseLifecycleState::Draining,
                RuntimeReuseLifecycleState::CleanReturn,
                RuntimeReuseLifecycleState::Ready,
            ][..]
        ),
        "warm-pool retention should expose the PIR2 lifecycle transition history"
    );
    assert_eq!(maintenance.moderate_memory_pressure_notifications, 1);
    assert_eq!(maintenance.low_memory_notifications, 1);
    assert!(
        maintenance.cleanliness.warm_reuse_safe_before_reset,
        "retention should only continue after Deno reports the runtime event loop is quiescent"
    );
    assert!(
        maintenance.cleanliness.request_state_reset_succeeded,
        "retention should record that Deno request-state reset succeeded"
    );
    assert_eq!(
        maintenance.cleanliness.heap_after_maintenance, maintenance.heap_after,
        "cleanliness accounting should be captured after boundary memory maintenance"
    );
    assert_eq!(
        maintenance.cleanliness.retained_memory_bytes,
        maintenance.heap_after.retained_memory_bytes(),
        "cleanliness report should use the same retained-memory accounting as the pool"
    );
    assert_eq!(
        maintenance.cleanliness.carryover_limit_bytes,
        heap_carryover_limit_bytes_for_test(runtime_owner.policy.limits()),
        "warm-pool carryover threshold should come from RuntimeLimits, not V8 defaults"
    );
    assert_eq!(
        maintenance
            .cleanliness
            .heap_after_maintenance
            .detached_context_count,
        0,
        "retained runtimes must not carry detached V8 contexts"
    );
    assert!(
        maintenance.heap_after.used_heap_size_bytes <= maintenance.heap_after.heap_size_limit_bytes,
        "retained runtime heap usage should remain inside its V8 heap limit"
    );
    assert_eq!(
        maintenance.heap_after.retained_memory_bytes(),
        maintenance
            .heap_after
            .used_heap_size_bytes
            .saturating_add(maintenance.heap_after.external_memory_bytes),
        "retained runtime accounting should include V8-reported external memory"
    );
    assert_eq!(
        runtime_owner
            .policy
            .metrics_snapshot()
            .retained_runtime_pool_entries,
        1
    );
}

async fn warm_pool_condemns_after_max_warm_reuses_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {};
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_reuses = 1;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut reusable = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("runtime cold take should succeed");
    reusable.warm_reuse_count = 1;

    v8_runtime_pool.return_runtime_for_invocation(&runtime_owner, &bundle, None, reusable);

    assert_eq!(
        v8_runtime_pool.warm_pool_count_for_test(),
        0,
        "runtime at max_warm_reuses must be condemned instead of retained"
    );
    let metrics = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics.warm_pool_retirements, 1);
    assert_eq!(metrics.retained_runtime_pool_retirements, 1);
    assert_eq!(metrics.retained_runtime_pool_entries, 0);
}

async fn warm_pool_memory_pressure_evicts_idle_entries_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {};
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_pool_entries_per_worker = 8;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    for tenant in ["tenant-a", "tenant-b", "tenant-c"] {
        let bundle = RuntimeBundle::for_tenant(&bundle_path, &expected_sha256, tenant)
            .expect("tenant bundle should build");
        let reusable = v8_runtime_pool
            .take_runtime_with_options(&runtime_owner, &bundle, true)
            .expect("tenant runtime cold take should succeed");
        v8_runtime_pool.return_runtime_for_invocation(&runtime_owner, &bundle, None, reusable);
    }

    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 3);
    assert_eq!(
        runtime_owner
            .policy
            .metrics_snapshot()
            .retained_runtime_pool_entries,
        3
    );

    let high =
        v8_runtime_pool.apply_memory_pressure(&runtime_owner, RuntimeMemoryPressureLevel::High);
    assert_eq!(high.pressure, RuntimeMemoryPressureLevel::High);
    assert_eq!(high.evicted_entries, 2);
    assert_eq!(high.retained_entries, 1);
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 1);
    let metrics_after_high = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics_after_high.retained_runtime_pool_evictions, 2);
    assert_eq!(metrics_after_high.retained_runtime_pool_entries, 1);

    let critical =
        v8_runtime_pool.apply_memory_pressure(&runtime_owner, RuntimeMemoryPressureLevel::Critical);
    assert_eq!(critical.pressure, RuntimeMemoryPressureLevel::Critical);
    assert_eq!(critical.evicted_entries, 1);
    assert_eq!(critical.retained_entries, 0);
    assert_eq!(v8_runtime_pool.warm_pool_count_for_test(), 0);
    let metrics_after_critical = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics_after_critical.retained_runtime_pool_evictions, 3);
    assert_eq!(metrics_after_critical.retained_runtime_pool_entries, 0);
}

async fn warm_pool_condemns_checked_out_runtime_after_near_heap_limit_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  const retained = [];
  while (true) {
    retained.push("x".repeat(1024 * 1024));
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:heap".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.initial_heap_mb = 4;
    limits.max_heap_mb = 8;
    limits.execution_timeout = std::time::Duration::from_secs(5);
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level(&request);

    let error = runtime_owner
        .invoke_bundle_unmanaged(
            Some(&mut v8_runtime_pool),
            RuntimeInvocationExecution {
                watchdog: watchdog.clone(),
                bundle: bundle.clone(),
                request: request.clone(),
                context: context.clone(),
                execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                    runtime_owner.policy().as_ref(),
                    &request,
                    &context,
                ),
                external_cancellation: None,
                response_ready_tx: None,
                permit: permit.clone(),
            },
        )
        .await
        .expect_err("heap growth should trip the near-heap-limit callback");

    match error {
        NimbusRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 8),
        other => panic!("unexpected near-heap-limit error: {other}"),
    }

    let ready_jobs = permit.finish_invocation().await;
    assert!(ready_jobs.is_empty());
    watchdog.shutdown();

    assert_eq!(
        v8_runtime_pool.warm_pool_count_for_test(),
        0,
        "near-heap-limit runtime must not be retained after finalization"
    );
    let metrics = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics.warm_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_replacements, 1);
    assert_eq!(
        metrics.warm_pool_retirements, 1,
        "checked-out warm-pool runtime should be retired when near-heap-limit fires"
    );
    assert_eq!(metrics.retained_runtime_pool_entries, 0);
}
