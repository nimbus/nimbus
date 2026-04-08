use super::*;
use crate::limits::RuntimePoolKind;
use crate::runtime::bootstrap::RuntimeWorkerIsolatePool;

#[test]
#[should_panic(expected = "WarmPool requires CooperativeLocker")]
fn warm_pool_with_run_to_completion_fails_fast() {
    let _policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: crate::limits::RuntimePoolKind::WarmPool,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
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
        r#"globalThis.__neovexInvoke = function () { return {}; }; export {};"#,
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_cross_tenant_isolation_inner());
}

async fn warm_pool_cross_tenant_isolation_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
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

    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::WarmPool,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner = NeovexRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();

    // Step 1: Take a runtime for tenant-A (cold miss — pool is empty).
    let reusable_a = isolate_pool
        .take_runtime_with_options(&runtime_owner, &bundle_tenant_a, true)
        .expect("tenant-a cold take should succeed");
    let metrics_after_cold = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics_after_cold.warm_pool_misses, 1);
    assert_eq!(metrics_after_cold.warm_pool_hits, 0);

    // Return the runtime to the pool under tenant-A's identity.
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle_tenant_a, None, reusable_a);
    assert_eq!(isolate_pool.warm_pool_count_for_test(), 1);

    // Step 2: Attempt take for tenant-B → must be a cold miss because the
    // pooled entry belongs to tenant-A.
    let reusable_b = isolate_pool
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
    assert_eq!(isolate_pool.warm_pool_count_for_test(), 1);

    // Return tenant-B's runtime.
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle_tenant_b, None, reusable_b);
    assert_eq!(isolate_pool.warm_pool_count_for_test(), 2);

    // Step 3: Take for tenant-A again → must be a warm hit.
    let _reusable_a2 = isolate_pool
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

    // Pool should now have 1 entry (tenant-B's).
    assert_eq!(isolate_pool.warm_pool_count_for_test(), 1);
}
