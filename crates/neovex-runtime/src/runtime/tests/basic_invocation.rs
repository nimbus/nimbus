use super::*;
use crate::RuntimeLimits;
use std::sync::OnceLock;

fn basic_invocation_suite_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn acquire_basic_invocation_suite_lock() -> tokio::sync::MutexGuard<'static, ()> {
    // These end-to-end runtime tests intentionally mix snapshot-backed
    // WebStandard runs with live Node22 bootstrap runs. The current Deno-family
    // Node bootstrap path still shares enough process-global V8 state that
    // libtest's default high parallelism can trip native assertions even though
    // the dedicated concurrency lanes remain healthy. Serialize this suite and
    // keep true runtime concurrency covered in the executor/verification
    // harnesses instead of letting unrelated test interleavings make the lane
    // non-deterministic.
    basic_invocation_suite_lock().lock().await
}

fn write_app_style_bundle(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_dir = tempdir.path().join("app/.neovex/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    let bundle_path = bundle_dir.join("bundle.mjs");
    std::fs::write(&bundle_path, source).expect("bundle should write");
    (tempdir, bundle_path)
}

fn write_test_executable(path: &std::path::Path, source: &str) {
    std::fs::write(path, source).expect("test executable should write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .expect("test executable metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .expect("test executable permissions should update");
    }
}

#[tokio::test]
async fn runtime_new_uses_product_default_runtime_policy() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
    assert_eq!(
        runtime.policy().limits(),
        &product_default_runtime_test_limits()
    );
}

#[tokio::test]
async fn runtime_loads_bundle_and_invokes_host_bridge() {
    let _guard = acquire_basic_invocation_suite_lock().await;
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
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "author": "Ada" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle invocation should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "host": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );

    let calls = host
        .calls
        .lock()
        .expect("recording host lock should not be poisoned")
        .clone();
    assert_eq!(
        calls,
        vec![HostCallRequest::new(
            HostCallOperation::DocumentGet,
            serde_json::json!({
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:list",
            }),
        )]
    );
}

#[tokio::test]
async fn runtime_requires_bundle_contract() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export const noop = 1;").expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "messages:missing".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect_err("missing global invoke contract should fail");

    assert!(
        error.to_string().contains("__neovexInvoke"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn runtime_awaits_async_bundle_handlers() {
    let _guard = acquire_basic_invocation_suite_lock().await;
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
  const value = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    awaited: await Promise.resolve({
      operation: value.operation,
      payload: value.payload,
    }),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime =
        NeovexRuntime::with_policy(host, run_to_completion_snapshot_runtime_test_policy());
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("async bundle invocation should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "awaited": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );
}

#[tokio::test]
async fn runtime_does_not_expose_legacy_host_globals() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return {
    rawHostCall: typeof globalThis.__neovexRawHostCall,
    hostValue: typeof globalThis.__neovexHostValue,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should observe runtime globals");

    assert_eq!(
        result,
        serde_json::json!({
            "rawHostCall": "undefined",
            "hostValue": "undefined",
        })
    );
}

#[tokio::test]
async fn runtime_removes_deno_global_from_bundle_execution() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
if (typeof Deno !== "undefined") {
  throw new Error("Deno should not be exposed to runtime bundles");
}

globalThis.__neovexInvoke = function () {
  return {
    denoValue: typeof globalThis.Deno,
    bootstrapValue: typeof globalThis.__bootstrap,
    legacyBootstrapValue: typeof globalThis.bootstrap,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute without exposing bootstrap globals");

    assert_eq!(
        result,
        serde_json::json!({
            "denoValue": "undefined",
            "bootstrapValue": "undefined",
            "legacyBootstrapValue": "undefined",
        })
    );
}

#[tokio::test]
async fn convenience_runtime_invocations_reuse_runtime_owned_executor() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__neovexInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("first convenience invocation should succeed");
    let second = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("second convenience invocation should succeed");

    assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
    assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
    assert_eq!(metrics.bundle_loads, 2);
    assert!(metrics.bundle_load_nanos_total > 0);
    assert_eq!(metrics.bundle_module_loads, 2);
    assert!(metrics.bundle_module_load_nanos_total > 0);
    assert_eq!(metrics.bundle_evaluations, 2);
    assert!(metrics.bundle_evaluation_nanos_total > 0);
}

#[tokio::test]
async fn web_standard_target_does_not_expose_node_globals() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return {
    globalAlias: typeof globalThis.global,
    processValue: typeof globalThis.process,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_web_standard())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "globalAlias": "undefined",
            "processValue": "undefined",
        })
    );
}

#[tokio::test]
async fn node22_target_exposes_minimal_node_globals() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return {
    globalAliasIsSelf: globalThis.global === globalThis,
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "globalAliasIsSelf": true,
            "processVersion": "v22.0.0-neovex",
            "nodeVersion": "22.0.0-neovex",
        })
    );
}

#[tokio::test]
async fn node22_target_hides_deno_bootstrap_globals() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return {
    denoValue: typeof globalThis.Deno,
    hasOwnDeno: Object.prototype.hasOwnProperty.call(globalThis, "Deno"),
    bootstrapValue: typeof globalThis.__bootstrap,
    hasOwnBootstrap: Object.prototype.hasOwnProperty.call(globalThis, "__bootstrap"),
    legacyBootstrapValue: typeof globalThis.bootstrap,
    hasOwnLegacyBootstrap: Object.prototype.hasOwnProperty.call(globalThis, "bootstrap"),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "denoValue": "undefined",
            "hasOwnDeno": false,
            "bootstrapValue": "undefined",
            "hasOwnBootstrap": false,
            "legacyBootstrapValue": "undefined",
            "hasOwnLegacyBootstrap": false,
        })
    );
}

#[tokio::test]
async fn node22_target_supports_node_path_builtin_imports() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import path from "node:path";

globalThis.__neovexInvoke = async function () {
  return {
    dirname: path.dirname("/demo/messages/file.txt"),
    joined: path.join("demo", "messages", "file.txt"),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "dirname": "/demo/messages",
            "joined": "demo/messages/file.txt",
        })
    );
}

#[tokio::test]
async fn node22_target_reports_platform_metadata_for_node_packages() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import os from "node:os";

globalThis.__neovexInvoke = async function () {
  return {
    platform: process.platform,
    arch: process.arch,
    osArch: os.arch(),
    endianness: os.endianness(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let expected_platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let expected_arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" | "i686" => "ia32",
        "riscv64gc" => "riscv64",
        other => other,
    };
    let expected_endianness = if cfg!(target_endian = "little") {
        "LE"
    } else {
        "BE"
    };

    assert_eq!(
        result,
        serde_json::json!({
            "platform": expected_platform,
            "arch": expected_arch,
            "osArch": expected_arch,
            "endianness": expected_endianness,
        })
    );
}

#[tokio::test]
async fn application_node22_reads_local_files_and_denies_env_and_escape_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { readFile, writeFile } from "node:fs/promises";

globalThis.__neovexInvoke = async function () {
  const config = await readFile("./config.txt", "utf8");
  let envDenied = null;
  try {
    void process.env.NODE_ENV;
  } catch (error) {
    envDenied = error?.message ?? String(error);
  }
  let writeDenied = null;
  try {
    await writeFile("../escape.txt", "should-fail");
  } catch (error) {
    writeDenied = error?.message ?? String(error);
  }
  return {
    cwd: process.cwd(),
    config,
    envDenied,
    writeDenied,
  };
};

export {};
"#,
    );
    std::fs::write(
        bundle_path
            .parent()
            .expect("bundle parent should resolve")
            .join("config.txt"),
        "hello from bundle",
    )
    .expect("config should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let expected_cwd = tempdir
        .path()
        .join("app/.neovex/convex")
        .canonicalize()
        .expect("expected cwd should canonicalize");
    assert_eq!(
        result["cwd"],
        serde_json::json!(expected_cwd.display().to_string())
    );
    assert_eq!(result["config"], serde_json::json!("hello from bundle"));
    let env_denied = result["envDenied"]
        .as_str()
        .expect("env denial should be a string");
    assert!(
        env_denied.contains("runtime env capability denied"),
        "unexpected env denial: {env_denied}"
    );
    let write_denied = result["writeDenied"]
        .as_str()
        .expect("write denial should be a string");
    assert!(
        write_denied.contains("runtime write capability denied"),
        "unexpected write denial: {write_denied}"
    );
}

#[tokio::test]
async fn tooling_node22_allows_allowlisted_env_and_tmp_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { readFile, writeFile } from "node:fs/promises";

globalThis.__neovexInvoke = async function () {
  await writeFile(".neovex/tmp/tooling.txt", "tooling-data");
  const roundTrip = await readFile(".neovex/tmp/tooling.txt", "utf8");
  return {
    cwd: process.cwd(),
    pathValue: process.env.PATH ?? null,
    roundTrip,
  };
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.neovex/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let expected_cwd = tempdir
        .path()
        .join("app")
        .canonicalize()
        .expect("expected cwd should canonicalize");
    assert_eq!(
        result["cwd"],
        serde_json::json!(expected_cwd.display().to_string())
    );
    assert_eq!(
        result["pathValue"],
        serde_json::json!(std::env::var("PATH").expect("PATH should be present in tests"))
    );
    assert_eq!(result["roundTrip"], serde_json::json!("tooling-data"));
    assert!(
        tempdir.path().join("app/.neovex/tmp/tooling.txt").is_file(),
        "tooling write should materialize under the scoped tmp root"
    );
}

#[tokio::test]
async fn tooling_node22_write_file_requires_preexisting_parent_directory() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { writeFile } from "node:fs/promises";

globalThis.__neovexInvoke = async function () {
  try {
    await writeFile(".neovex/tmp/missing/tooling.txt", "tooling-data");
    return { ok: true };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.neovex/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(result["ok"], serde_json::json!(false));
    assert_eq!(result["code"], serde_json::json!("ENOENT"));
    let message = result["message"]
        .as_str()
        .expect("missing parent write failure should include a message");
    assert!(
        message.contains("writeFile"),
        "unexpected write failure: {message}"
    );
    assert!(
        !tempdir
            .path()
            .join("app/.neovex/tmp/missing/tooling.txt")
            .exists(),
        "writeFile should not materialize missing parent directories"
    );
}

#[tokio::test]
async fn application_node22_resolves_local_esm_packages_from_scoped_node_modules() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { runtimeValue } from "demo-package";

globalThis.__neovexInvoke = function () {
  return { runtimeValue };
};

export {};
"#,
    );
    let package_root = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules/demo-package");
    std::fs::create_dir_all(&package_root).expect("package root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{"name":"demo-package","type":"module","main":"./index.js"}"#,
    )
    .expect("package.json should write");
    std::fs::write(
        package_root.join("index.js"),
        r#"export const runtimeValue = "demo-package-ok";"#,
    )
    .expect("package entry should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({ "runtimeValue": "demo-package-ok" })
    );
}

#[tokio::test]
async fn application_node22_resolves_package_exports_from_scoped_node_modules() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { exportedValue } from "demo-package/feature";

globalThis.__neovexInvoke = function () {
  return { exportedValue };
};

export {};
"#,
    );
    let package_root = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules/demo-package");
    let source_root = package_root.join("src");
    std::fs::create_dir_all(&source_root).expect("package source root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{
  "name": "demo-package",
  "type": "module",
  "exports": {
    ".": "./src/index.js",
    "./feature": "./src/feature.js"
  }
}"#,
    )
    .expect("package.json should write");
    std::fs::write(
        source_root.join("index.js"),
        r#"export const ignored = "root";"#,
    )
    .expect("package root entry should write");
    std::fs::write(
        source_root.join("feature.js"),
        r#"export const exportedValue = "exports-ok";"#,
    )
    .expect("package exports entry should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(result, serde_json::json!({ "exportedValue": "exports-ok" }));
}

#[tokio::test]
async fn application_node22_loads_commonjs_package_entries_via_esm_import() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import pkg, { namedValue } from "demo-package";

globalThis.__neovexInvoke = function () {
  return {
    defaultValue: pkg.defaultValue,
    namedValue,
  };
};

export {};
"#,
    );
    let package_root = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules/demo-package");
    std::fs::create_dir_all(&package_root).expect("package root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{"name":"demo-package","main":"./index.cjs"}"#,
    )
    .expect("package.json should write");
    std::fs::write(
        package_root.join("index.cjs"),
        r#"
module.exports.defaultValue = "commonjs-ok";
module.exports.namedValue = 42;
"#,
    )
    .expect("commonjs entry should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("commonjs package entry should load");

    assert_eq!(
        result,
        serde_json::json!({
            "defaultValue": "commonjs-ok",
            "namedValue": 42,
        })
    );
}

#[tokio::test]
async fn application_node22_loads_implicit_commonjs_packages_with_nested_require() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import pkg from "demo-package";

globalThis.__neovexInvoke = function () {
  return pkg;
};

export {};
"#,
    );
    let package_root = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules/demo-package");
    std::fs::create_dir_all(&package_root).expect("package root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{"name":"demo-package","main":"./index.js"}"#,
    )
    .expect("package.json should write");
    std::fs::write(
        package_root.join("index.js"),
        r#"
const child = require("./child.cjs");
const payload = require("./data.json");

module.exports = {
  child,
  answer: payload.answer,
};
"#,
    )
    .expect("commonjs entry should write");
    std::fs::write(
        package_root.join("child.cjs"),
        r#"module.exports = "nested-commonjs";"#,
    )
    .expect("child commonjs should write");
    std::fs::write(package_root.join("data.json"), r#"{"answer": 42}"#)
        .expect("json payload should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("implicit CommonJS package entry should load");

    assert_eq!(
        result,
        serde_json::json!({
            "child": "nested-commonjs",
            "answer": 42,
        })
    );
}

#[tokio::test]
async fn tooling_node22_resolves_commonjs_packages_from_app_root_node_modules() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import pkg, { exportedValue } from "demo-package";

globalThis.__neovexInvoke = function () {
  return {
    defaultValue: pkg.defaultValue,
    exportedValue,
  };
};

export {};
"#,
    );
    let package_root = tempdir.path().join("app/node_modules/demo-package");
    std::fs::create_dir_all(&package_root).expect("package root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{"name":"demo-package","exports":"./index.cjs"}"#,
    )
    .expect("package.json should write");
    std::fs::write(
        package_root.join("index.cjs"),
        r#"
module.exports.defaultValue = "tooling-commonjs-ok";
module.exports.exportedValue = "exports-from-app-root";
"#,
    )
    .expect("commonjs entry should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("tooling profile should resolve app-root CommonJS packages");

    assert_eq!(
        result,
        serde_json::json!({
            "defaultValue": "tooling-commonjs-ok",
            "exportedValue": "exports-from-app-root",
        })
    );
}

#[tokio::test]
async fn tooling_node22_executes_esbuild_style_staged_binary() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { build } from "esbuild";

globalThis.__neovexInvoke = function () {
  return build();
};

export {};
"#,
    );
    let package_root = tempdir.path().join("app/node_modules/esbuild");
    let lib_root = package_root.join("lib");
    let bin_root = package_root.join("bin");
    std::fs::create_dir_all(&lib_root).expect("esbuild lib root should build");
    std::fs::create_dir_all(&bin_root).expect("esbuild bin root should build");
    #[cfg(unix)]
    let executable_name = "esbuild";
    #[cfg(windows)]
    let executable_name = "esbuild.cmd";
    let executable_path = bin_root.join(executable_name);
    #[cfg(unix)]
    write_test_executable(&executable_path, "#!/bin/sh\necho esbuild-ok\n");
    #[cfg(windows)]
    write_test_executable(&executable_path, "@echo off\r\necho esbuild-ok\r\n");
    std::fs::write(
        package_root.join("package.json"),
        r#"{
  "name": "esbuild",
  "main": "./lib/main.js"
}"#,
    )
    .expect("esbuild package manifest should write");
    std::fs::write(
        lib_root.join("main.js"),
        r#"
var child_process = require("child_process");
var Buffer = require("buffer").Buffer;
var crypto = require("crypto");
var path = require("path");
var fs = require("fs");
var os = require("os");
var tty = require("tty");
var worker_threads;
try {
  worker_threads = require("worker_threads");
} catch {
}

exports.build = function build() {
  if (typeof tty.isatty !== "function") {
    throw new Error("expected tty.isatty to be available");
  }
  const command = path.join(__dirname, "..", "bin", process.platform === "win32" ? "esbuild.cmd" : "esbuild");
  const digest = crypto.createHash("sha256").update(os.tmpdir()).digest("hex");
  const result = child_process.spawnSync(command, [], { encoding: "utf8" });
  return {
    keys: Object.keys(result).sort(),
    status: result.status,
    signal: result.signal ?? null,
    errorCode: result.error?.code ?? null,
    stdoutType: typeof result.stdout,
    stdout: typeof result.stdout === "string" ? result.stdout.trim() : result.stdout ?? null,
    stderr: result.stderr ?? null,
    bufferRoundTrip: Buffer.from("esbuild-ok").toString("utf8"),
    digestLength: digest.length,
  };
};
"#,
    )
    .expect("esbuild main entry should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("esbuild-style staged binary should execute in tooling profile");

    assert_eq!(
        result,
        serde_json::json!({
            "keys": ["output", "pid", "signal", "status", "stderr", "stdout"],
            "status": 0,
            "signal": serde_json::Value::Null,
            "errorCode": serde_json::Value::Null,
            "stdoutType": "string",
            "stdout": "esbuild-ok",
            "stderr": "",
            "bufferRoundTrip": "esbuild-ok",
            "digestLength": 64,
        })
    );
}
