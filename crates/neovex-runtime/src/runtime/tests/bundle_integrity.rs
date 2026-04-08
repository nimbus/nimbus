use super::*;

#[tokio::test]
async fn runtime_reports_heap_limit_exceeded() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  let value = "";
  while (true) {
    value += "hello world";
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_limits(
        Arc::new(RecordingHost::default()),
        RuntimeLimits {
            max_heap_mb: 8,
            initial_heap_mb: 4,
            execution_timeout: std::time::Duration::from_secs(2),
            max_concurrent_isolates: 1,
            ..RuntimeLimits::default()
        },
    );
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
            },
        )
        .await
        .expect_err("heap growth should trip the runtime heap limit");

    match error {
        NeovexRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 8),
        other => panic!("unexpected heap-limit error: {other}"),
    }
}

#[tokio::test]
async fn runtime_rejects_module_imports_outside_bundle_root() {
    let tempdir = tempdir().expect("tempdir should build");
    let outside_path = tempdir.path().join("outside.mjs");
    let bundle_dir = tempdir.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should exist");
    let bundle_path = bundle_dir.join("bundle.mjs");

    std::fs::write(&outside_path, "export const secret = 'outside';")
        .expect("outside module should write");
    std::fs::write(
        &bundle_path,
        r#"
import "../outside.mjs";

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
            },
        )
        .await
        .expect_err("outside import should be rejected");

    assert!(
        error.to_string().contains("outside the bundle root"),
        "unexpected loader sandbox error: {error}"
    );
}

#[tokio::test]
async fn runtime_rejects_bundle_integrity_mismatch() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");
    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: false };
};

export {};
"#,
    )
    .expect("tampered bundle should write");

    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: crate::limits::RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }));
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
        .expect("bundle integrity metadata should build");
    let error = runtime
        .invoke_bundle(
            &bundle,
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
            },
        )
        .await
        .expect_err("tampered bundle should fail integrity verification");

    match error {
        NeovexRuntimeError::BundleIntegrityMismatch(message) => {
            assert!(message.contains("bundle.mjs"));
        }
        other => panic!("unexpected integrity error: {other}"),
    }
}

#[tokio::test]
async fn startup_snapshot_runtime_populates_and_reuses_bundle_module_code_cache() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "cached";
}
"#,
    )
    .expect("dependency should write");
    std::fs::write(
        &bundle_path,
        r#"
import { value } from "./dep.mjs";

globalThis.__neovexInvoke = async function () {
  return { value: value() };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };

    assert_eq!(bundle.module_code_cache_entry_count(), 0);
    assert_eq!(bundle.module_code_cache_write_count(), 0);

    let first = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("first invocation should succeed");
    assert_eq!(first, serde_json::json!({ "value": "cached" }));

    let first_entry_count = bundle.module_code_cache_entry_count();
    let first_write_count = bundle.module_code_cache_write_count();
    assert!(
        first_entry_count >= 2,
        "expected main module and dependency to populate cache"
    );
    assert!(
        first_write_count >= first_entry_count,
        "expected at least one cache write per populated module"
    );

    let second = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("second invocation should succeed");
    assert_eq!(second, serde_json::json!({ "value": "cached" }));
    assert_eq!(bundle.module_code_cache_entry_count(), first_entry_count);
    assert_eq!(bundle.module_code_cache_write_count(), first_write_count);
    let metrics = runtime.policy.metrics_snapshot();
    assert_eq!(metrics.bundle_loads, 2);
    assert!(metrics.bundle_load_nanos_total > 0);
    assert_eq!(metrics.bundle_module_loads, 2);
    assert!(metrics.bundle_module_load_nanos_total > 0);
    assert_eq!(metrics.bundle_evaluations, 2);
    assert!(metrics.bundle_evaluation_nanos_total > 0);
}

#[test]
fn runtime_bundle_clones_share_normalized_identity() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let bundle = RuntimeBundle::with_expected_sha256(
        bundle_path
            .parent()
            .expect("bundle parent should exist")
            .join(".")
            .join("bundle.mjs"),
        expected_sha256.to_ascii_uppercase(),
    )
    .expect("bundle identity metadata should build");
    let cloned = bundle.clone();
    let canonical_bundle_path = bundle_path
        .canonicalize()
        .expect("bundle path should canonicalize");

    assert!(bundle.shares_storage_with(&cloned));
    assert_eq!(bundle.identity(), cloned.identity());
    assert_eq!(bundle.identity().entrypoint(), canonical_bundle_path);
    assert_eq!(
        bundle.identity().expected_sha256(),
        Some(expected_sha256.as_str())
    );
    assert_eq!(
        bundle.canonical_entrypoint(),
        Some(canonical_bundle_path.as_path())
    );
    assert_eq!(
        bundle
            .module_root()
            .expect("bundle root should resolve from cached metadata"),
        canonical_bundle_path
            .parent()
            .expect("bundle root should exist")
            .to_path_buf()
    );
    assert_eq!(
        bundle
            .module_specifier()
            .expect("bundle specifier should resolve from cached metadata")
            .as_str(),
        deno_core::ModuleSpecifier::from_file_path(&canonical_bundle_path)
            .expect("canonical bundle path should convert to a file url")
            .as_str()
    );
    assert_eq!(
        cloned
            .module_root()
            .expect("cloned bundle should share cached root metadata"),
        canonical_bundle_path
            .parent()
            .expect("bundle root should exist")
            .to_path_buf()
    );
}

#[tokio::test]
async fn runtime_bundle_rechecks_integrity_after_prior_success() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
        .expect("bundle integrity metadata should build");
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: crate::limits::RuntimeExecutionModel::RunToCompletion,
        runtime_pool_kind: crate::limits::RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }));
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };

    let first_result = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("first bundle invocation should succeed");
    assert_eq!(first_result, serde_json::json!({ "ok": true }));

    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: false };
};

export {};
"#,
    )
    .expect("tampered bundle should write");

    let error = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect_err("tampered bundle should fail integrity verification");
    assert!(matches!(
        error,
        NeovexRuntimeError::BundleIntegrityMismatch(_)
    ));
}

#[tokio::test]
async fn runtime_bundle_identity_canonicalizes_paths_without_changing_integrity_results() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let canonical_bundle = RuntimeBundle::with_expected_sha256(&bundle_path, &expected_sha256)
        .expect("canonical bundle should build");
    let dot_path_bundle = RuntimeBundle::with_expected_sha256(
        bundle_path
            .parent()
            .expect("bundle parent should exist")
            .join(".")
            .join("bundle.mjs"),
        format!("{expected_sha256}\n"),
    )
    .expect("dot path bundle should build");
    let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };

    assert_eq!(canonical_bundle.identity(), dot_path_bundle.identity());

    let canonical_result = runtime
        .invoke_bundle(&canonical_bundle, &request)
        .await
        .expect("canonical bundle invocation should succeed");
    let dot_path_result = runtime
        .invoke_bundle(&dot_path_bundle, &request)
        .await
        .expect("dot path bundle invocation should succeed");

    assert_eq!(canonical_result, serde_json::json!({ "ok": true }));
    assert_eq!(dot_path_result, canonical_result);
}
