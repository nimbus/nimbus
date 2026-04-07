use super::*;

#[test]
fn retained_runtime_pool_tracks_snapshot_seeded_construction_mode_for_test() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy)
        .with_retained_runtime_construction_mode_for_test(RuntimeConstructionMode::StartupSnapshot);
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let context = RuntimeInvocationContext::top_level(&request);

    let runtime = isolate_pool
        .take_runtime_for_invocation(&runtime_owner, &bundle, Some(&context))
        .expect("snapshot-seeded retained runtime should build");
    assert_eq!(
        runtime.construction_mode,
        RuntimeConstructionMode::StartupSnapshot
    );
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&context), runtime);

    assert_eq!(
        isolate_pool.retained_runtime_construction_modes_for_test(),
        vec![RuntimeConstructionMode::StartupSnapshot]
    );
}

#[test]
fn retained_runtime_pool_prefers_exact_affinity_match_before_other_idle_entries() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let tenant_b = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-b");

    let runtime_a = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("tenant A runtime should build");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), runtime_a);

    let runtime_b = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_b), true)
        .expect("tenant B runtime should build");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_b), runtime_b);

    let reused_a = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("tenant A should reuse its own retained runtime");

    let remaining = isolate_pool.retained_runtime_affinity_keys_for_test();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(
        &remaining[0],
        Some(crate::affinity::RuntimeAffinityKey::Tenant(tenant)) if tenant == "tenant-b"
    ));

    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), reused_a);

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 2);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}

#[test]
fn retained_runtime_pool_evicts_idle_lru_when_worker_cap_is_full() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
        max_retained_runtimes_per_worker: 2,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let tenant_b = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-b");
    let tenant_c = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-c");

    let runtime_a = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("tenant A runtime should build");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), runtime_a);

    let runtime_b = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_b), true)
        .expect("tenant B runtime should build");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_b), runtime_b);

    let reused_a = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("tenant A should reuse its retained runtime");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), reused_a);

    let runtime_c = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_c), true)
        .expect("tenant C should reuse the idle LRU runtime once the pool is full");

    let remaining = isolate_pool.retained_runtime_affinity_keys_for_test();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(
        &remaining[0],
        Some(crate::affinity::RuntimeAffinityKey::Tenant(tenant)) if tenant == "tenant-a"
    ));

    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_c), runtime_c);

    let mut remaining = isolate_pool
        .retained_runtime_affinity_keys_for_test()
        .into_iter()
        .map(|key| match key {
            Some(crate::affinity::RuntimeAffinityKey::Tenant(tenant)) => tenant,
            other => panic!("unexpected retained affinity key: {other:?}"),
        })
        .collect::<Vec<_>>();
    remaining.sort();
    assert_eq!(
        remaining,
        vec!["tenant-a".to_string(), "tenant-c".to_string()]
    );
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 2);

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 2);
    assert_eq!(metrics.isolate_pool_hits, 2);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}

#[test]
fn retained_runtime_pool_retires_runtime_after_reuse_cap() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
        max_retained_runtime_reuses: 1,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let fresh_runtime = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("fresh runtime should build");
    isolate_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_a),
        fresh_runtime,
    );
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 1);

    let reused_runtime = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("first retained reuse should succeed");
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 0);

    isolate_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_a),
        reused_runtime,
    );
    assert_eq!(
        isolate_pool.retained_runtime_count_for_test(),
        0,
        "runtime should retire instead of remaining idle once it reaches the reuse cap"
    );

    let rebuilt_runtime = isolate_pool
        .take_runtime_with_options_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), true)
        .expect("retired runtime should be rebuilt on the next checkout");
    isolate_pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_a),
        rebuilt_runtime,
    );
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 1);

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 2);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
    assert_eq!(metrics.retained_runtime_pool_evictions, 0);
    assert_eq!(metrics.retained_runtime_pool_retirements, 1);
    assert_eq!(metrics.retained_runtime_pool_entries, 1);
}

#[test]
fn retained_runtime_pool_keeps_run_to_completion_mode_single_entry_per_worker() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
        max_retained_runtimes_per_worker: 4,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let tenant_b = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-b");

    let runtime_a = isolate_pool
        .take_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a))
        .expect("tenant A runtime should build");
    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_a), runtime_a);
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 1);

    let runtime_b = isolate_pool
        .take_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_b))
        .expect("tenant B should reuse the single run-to-completion retained runtime");
    assert_eq!(isolate_pool.retained_runtime_count_for_test(), 0);

    isolate_pool.return_runtime_for_invocation(&runtime_owner, &bundle, Some(&tenant_b), runtime_b);

    let remaining = isolate_pool.retained_runtime_affinity_keys_for_test();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(
        &remaining[0],
        Some(crate::affinity::RuntimeAffinityKey::Tenant(tenant)) if tenant == "tenant-b"
    ));

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}

#[tokio::test]
async fn retained_runtime_pool_mode_reuses_runtime_after_main_realm_reset() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  globalThis.__userCounter = (globalThis.__userCounter ?? 0) + 1;
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db.get("messages", "doc-1");
  return {
    counter: globalThis.__userCounter,
    deno: typeof globalThis.Deno,
    session: value.payload.session_id,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: Some(test_invocation_auth("token-1")),
    };

    let first = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("first retained invocation should succeed");

    let second = runtime
        .invoke_bundle(
            &bundle,
            &InvocationRequest {
                auth: Some(test_invocation_auth("token-2")),
                ..request
            },
        )
        .await
        .expect("second retained invocation should reuse a reset runtime");

    assert_eq!(
        first,
        serde_json::json!({
            "counter": 1,
            "deno": "undefined",
            "session": "session-1",
        })
    );
    assert_eq!(
        second,
        serde_json::json!({
            "counter": 1,
            "deno": "undefined",
            "session": "session-1",
        })
    );
    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}
