use super::*;
use crate::RuntimeLimits;
use std::path::{Path, PathBuf};
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

fn generated_node_modules_package_root(
    bundle_path: &std::path::Path,
    package_name: &str,
) -> std::path::PathBuf {
    bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules")
        .join(package_name)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent should resolve")
        .parent()
        .expect("repo root should resolve")
        .to_path_buf()
}

fn networking_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/networking-canaries")
}

fn tooling_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/tooling-canaries")
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("destination directory should build");
    for entry in std::fs::read_dir(source).expect("source directory should be readable") {
        let entry = entry.expect("directory entry should load");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("file type should load");
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path);
        } else if file_type.is_symlink() {
            let symlink_target =
                std::fs::read_link(&source_path).expect("symlink target should load");
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&symlink_target, &destination_path)
                    .expect("destination symlink should write");
            }
            #[cfg(windows)]
            {
                let metadata = std::fs::metadata(&source_path)
                    .expect("symlink metadata should load for windows fallback");
                if metadata.is_dir() {
                    std::os::windows::fs::symlink_dir(&symlink_target, &destination_path)
                        .expect("destination dir symlink should write");
                } else {
                    std::os::windows::fs::symlink_file(&symlink_target, &destination_path)
                        .expect("destination file symlink should write");
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                let metadata = std::fs::metadata(&source_path)
                    .expect("symlink metadata should load for copy fallback");
                if metadata.is_dir() {
                    copy_dir_recursive(&source_path, &destination_path);
                } else {
                    std::fs::copy(&source_path, &destination_path)
                        .expect("symlink file fallback copy should succeed");
                }
            }
        } else {
            std::fs::copy(&source_path, &destination_path).expect("file copy should succeed");
        }
    }
}

struct PreparedApplicationCanaryApp {
    _tempdir: tempfile::TempDir,
    bundle_path: PathBuf,
}

struct PreparedToolingCanaryApp {
    _tempdir: tempfile::TempDir,
    bundle_path: PathBuf,
}

fn prepare_application_networking_canary_app() -> PreparedApplicationCanaryApp {
    let canary_root = networking_canary_root();
    let canary_node_modules = canary_root.join("node_modules");
    assert!(
        canary_node_modules.is_dir(),
        "networking canary dependencies are missing at {}; run `npm ci --prefix {}` first",
        canary_node_modules.display(),
        canary_root.display(),
    );

    let tempdir = tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_dir = app_root.join(".neovex/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    // Application-preset package resolution is intentionally scoped to the
    // generated bundle root. Copy the pinned canary dependencies fully into
    // that tree so runtime reads never escape through a top-level symlink.
    copy_dir_recursive(&canary_node_modules, &bundle_dir.join("node_modules"));

    PreparedApplicationCanaryApp {
        _tempdir: tempdir,
        bundle_path: bundle_dir.join("bundle.mjs"),
    }
}

fn prepare_tooling_canary_app() -> PreparedToolingCanaryApp {
    let canary_root = tooling_canary_root();
    let canary_node_modules = canary_root.join("node_modules");
    assert!(
        canary_node_modules.is_dir(),
        "tooling canary dependencies are missing at {}; run `npm ci --prefix {}` first",
        canary_node_modules.display(),
        canary_root.display(),
    );

    let tempdir = tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    copy_dir_recursive(&canary_root, &app_root);
    let tooling_bin_root = app_root.join("node_modules/neovex-host-node/bin");
    std::fs::create_dir_all(&tooling_bin_root).expect("tooling bin root should build");
    #[cfg(unix)]
    write_test_executable(
        &tooling_bin_root.join("node"),
        "#!/bin/sh\nexec node \"$@\"\n",
    );
    #[cfg(windows)]
    write_test_executable(
        &tooling_bin_root.join("node.cmd"),
        "@echo off\r\nnode %*\r\n",
    );
    let bundle_dir = app_root.join(".neovex/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    std::fs::create_dir_all(app_root.join(".neovex/tmp")).expect("tooling tmp dir should build");

    PreparedToolingCanaryApp {
        _tempdir: tempdir,
        bundle_path: bundle_dir.join("bundle.mjs"),
    }
}

fn stage_networking_canary_bundle(app: &PreparedApplicationCanaryApp, bundle_fixture_name: &str) {
    let source = networking_canary_root()
        .join("bundles")
        .join(bundle_fixture_name);
    std::fs::copy(&source, &app.bundle_path).unwrap_or_else(|error| {
        panic!(
            "networking canary bundle {} should stage: {error}",
            source.display()
        )
    });
}

fn stage_tooling_canary_bundle(app: &PreparedToolingCanaryApp, bundle_fixture_name: &str) {
    let source = tooling_canary_root()
        .join("bundles")
        .join(bundle_fixture_name);
    std::fs::copy(&source, &app.bundle_path).unwrap_or_else(|error| {
        panic!(
            "tooling canary bundle {} should stage: {error}",
            source.display()
        )
    });
}

fn networking_canary_expected_result(bundle_fixture_name: &str) -> Value {
    match bundle_fixture_name {
        "express.mjs" => serde_json::json!({
            "okStatus": 200,
            "okBody": {
                "framework": "express",
                "ok": true,
            },
            "traceHeader": "middleware-hit",
            "errorStatus": 418,
            "errorBody": {
                "framework": "express",
                "ok": false,
                "message": "express-canary-boom",
            },
        }),
        "fastify.mjs" => serde_json::json!({
            "okStatus": 200,
            "okBody": {
                "framework": "fastify",
                "ok": true,
            },
            "traceHeader": "fastify-hook",
            "errorStatus": 418,
            "errorBody": {
                "framework": "fastify",
                "ok": false,
                "message": "fastify-canary-boom",
            },
        }),
        "axios.mjs" => serde_json::json!({
            "okStatus": 200,
            "okBody": {
                "client": "axios",
                "ok": true,
            },
            "errorStatus": 418,
            "errorBody": {
                "client": "axios",
                "ok": false,
            },
        }),
        "undici.mjs" => serde_json::json!({
            "okStatus": 200,
            "okBody": {
                "client": "undici",
                "ok": true,
            },
            "errorStatus": 418,
            "errorBody": {
                "client": "undici",
                "ok": false,
            },
        }),
        "socket-io.mjs" => serde_json::json!({
            "welcomeTransport": "websocket",
            "pongPayload": {
                "echoed": {
                    "message": "hello",
                },
                "clientCount": 1,
            },
        }),
        other => panic!("unexpected networking canary bundle fixture: {other}"),
    }
}

async fn run_application_networking_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
) -> Value {
    stage_networking_canary_bundle(app, bundle_fixture_name);
    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
    );
    runtime
        .invoke_bundle(
            &RuntimeBundle::new(&app.bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "networking:canary".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .unwrap_or_else(|error| {
            panic!("networking canary bundle {bundle_fixture_name} should execute: {error}")
        })
}

async fn run_tooling_canary_bundle(
    app: &PreparedToolingCanaryApp,
    bundle_fixture_name: &str,
) -> Value {
    stage_tooling_canary_bundle(app, bundle_fixture_name);
    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    runtime
        .invoke_bundle(
            &RuntimeBundle::new(&app.bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "tooling:canary".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .unwrap_or_else(|error| {
            panic!("tooling canary bundle {bundle_fixture_name} should execute: {error}")
        })
}

fn tooling_canary_status(actual: &Value, key: &str) -> std::result::Result<i64, String> {
    actual
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer field {key} in {actual}"))
}

fn tooling_canary_bool(actual: &Value, key: &str) -> std::result::Result<bool, String> {
    actual
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing bool field {key} in {actual}"))
}

fn tooling_canary_string<'a>(actual: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    actual
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {key} in {actual}"))
}

fn assert_tooling_canary_result(
    bundle_fixture_name: &str,
    actual: &Value,
) -> std::result::Result<(), String> {
    match bundle_fixture_name {
        "tsx.mjs" => {
            if tooling_canary_status(actual, "successStatus")? != 0 {
                return Err(format!("tsx success status was not zero: {actual}"));
            }
            if tooling_canary_string(actual, "successStdout")? != "tsx-ok:84" {
                return Err(format!("tsx success stdout mismatch: {actual}"));
            }
            if tooling_canary_status(actual, "failureStatus")? == 0 {
                return Err(format!(
                    "tsx failure status unexpectedly succeeded: {actual}"
                ));
            }
            if !tooling_canary_bool(actual, "failureHasToken")? {
                return Err(format!("tsx failure token missing: {actual}"));
            }
            Ok(())
        }
        "ts-node.mjs" => {
            if tooling_canary_status(actual, "successStatus")? != 0 {
                return Err(format!("ts-node success status was not zero: {actual}"));
            }
            if tooling_canary_string(actual, "successStdout")? != "ts-node-ok:42" {
                return Err(format!("ts-node success stdout mismatch: {actual}"));
            }
            if tooling_canary_status(actual, "failureStatus")? == 0 {
                return Err(format!(
                    "ts-node failure status unexpectedly succeeded: {actual}"
                ));
            }
            if !tooling_canary_bool(actual, "failureHasToken")? {
                return Err(format!("ts-node failure token missing: {actual}"));
            }
            Ok(())
        }
        "jest.mjs" => {
            if tooling_canary_status(actual, "successStatus")? != 0 {
                return Err(format!("jest success status was not zero: {actual}"));
            }
            if !tooling_canary_bool(actual, "successHasPassToken")? {
                return Err(format!("jest success output missed PASS token: {actual}"));
            }
            if !tooling_canary_bool(actual, "successHasTestName")? {
                return Err(format!("jest success output missed test name: {actual}"));
            }
            if tooling_canary_status(actual, "failureStatus")? == 0 {
                return Err(format!(
                    "jest failure status unexpectedly succeeded: {actual}"
                ));
            }
            if !tooling_canary_bool(actual, "failureHasFailToken")? {
                return Err(format!("jest failure output missed FAIL token: {actual}"));
            }
            if !tooling_canary_bool(actual, "failureHasTestName")? {
                return Err(format!("jest failure output missed test name: {actual}"));
            }
            Ok(())
        }
        "prisma.mjs" => {
            let mode = tooling_canary_string(actual, "mode")?;
            if mode == "success" {
                if tooling_canary_status(actual, "validateStatus")? != 0
                    || tooling_canary_status(actual, "generateStatus")? != 0
                    || tooling_canary_status(actual, "pushStatus")? != 0
                    || tooling_canary_status(actual, "smokeStatus")? != 0
                {
                    return Err(format!(
                        "prisma success statuses were not all zero: {actual}"
                    ));
                }
                let smoke = actual
                    .get("smokeResult")
                    .ok_or_else(|| format!("prisma smokeResult missing in {actual}"))?;
                if smoke
                    .get("createdEmail")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    != "ada@example.com"
                {
                    return Err(format!("prisma createdEmail mismatch: {actual}"));
                }
                if smoke
                    .get("count")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
                    != 1
                {
                    return Err(format!("prisma count mismatch: {actual}"));
                }
                if smoke
                    .get("foundName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    != "Ada"
                {
                    return Err(format!("prisma foundName mismatch: {actual}"));
                }
                return Ok(());
            }

            if mode != "documented-boundary" {
                return Err(format!("unexpected prisma mode {mode} in {actual}"));
            }
            if tooling_canary_status(actual, "status")? == 0 {
                return Err(format!(
                    "documented prisma boundary unexpectedly succeeded: {actual}"
                ));
            }
            let step = tooling_canary_string(actual, "step")?;
            if !matches!(step, "validate" | "generate" | "push" | "smoke") {
                return Err(format!(
                    "unexpected prisma boundary step {step} in {actual}"
                ));
            }
            let boundary_token = actual
                .get("boundaryToken")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("missing prisma boundary token in {actual}"))?;
            if !matches!(
                boundary_token,
                "Using engine type \"client\" requires either \"adapter\" or \"accelerateUrl\""
                    | "Prisma Client could not locate the Query Engine"
                    | "Query engine library for current platform"
                    | "Unable to require"
                    | "Node-API library"
                    | "native addon"
            ) {
                return Err(format!(
                    "unexpected prisma boundary token {boundary_token} in {actual}"
                ));
            }
            Ok(())
        }
        "next.mjs" => {
            if tooling_canary_status(actual, "buildStatus")? != 0 {
                return Err(format!("next build failed: {actual}"));
            }
            if tooling_canary_status(actual, "smokeStatus")? != 0 {
                return Err(format!("next smoke script failed: {actual}"));
            }
            let smoke = actual
                .get("smokeResult")
                .ok_or_else(|| format!("next smokeResult missing in {actual}"))?;
            if smoke
                .get("okStatus")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                != 200
            {
                return Err(format!("next ok status mismatch: {actual}"));
            }
            if !smoke
                .get("okBodyIncludes")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(format!(
                    "next ok body did not include sentinel text: {actual}"
                ));
            }
            if smoke
                .get("missingStatus")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                != 404
            {
                return Err(format!("next missing status mismatch: {actual}"));
            }
            if !smoke
                .get("missingBodyIncludes")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(format!(
                    "next missing body did not include sentinel text: {actual}"
                ));
            }
            Ok(())
        }
        other => Err(format!("unexpected tooling canary bundle fixture: {other}")),
    }
}

struct ScopedProcessEnvVar {
    key: &'static str,
    previous_value: Option<String>,
}

impl ScopedProcessEnvVar {
    fn set(key: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(key).ok();
        // SAFETY: basic_invocation suite execution is serialized under
        // acquire_basic_invocation_suite_lock(), so temporary process env
        // mutations in a focused runtime test do not race sibling tests.
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            previous_value,
        }
    }
}

impl Drop for ScopedProcessEnvVar {
    fn drop(&mut self) {
        // SAFETY: see ScopedProcessEnvVar::set; restoration happens within the
        // same serialized test scope.
        unsafe {
            if let Some(previous_value) = &self.previous_value {
                std::env::set_var(self.key, previous_value);
            } else {
                std::env::remove_var(self.key);
            }
        }
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
    bufferValue: typeof globalThis.Buffer,
    bufferRoundTrip: globalThis.Buffer?.from("hi").toString("utf8") ?? null,
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
    processExecPath: globalThis.process?.execPath ?? null,
    stdoutType: typeof globalThis.process?.stdout,
    stdoutWriteType: typeof globalThis.process?.stdout?.write,
    stderrType: typeof globalThis.process?.stderr,
    stderrWriteType: typeof globalThis.process?.stderr?.write,
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
            "bufferValue": "function",
            "bufferRoundTrip": "hi",
            "processVersion": "v22.0.0-neovex",
            "nodeVersion": "22.0.0-neovex",
            "processExecPath": std::env::current_exe().expect("current executable path should resolve").display().to_string(),
            "stdoutType": "object",
            "stdoutWriteType": "function",
            "stderrType": "object",
            "stderrWriteType": "function",
        })
    );
}

#[tokio::test]
async fn node22_target_delivers_manual_process_warning_events() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const warnings = [];
  process.on("warning", (warning) => {
    warnings.push({
      name: warning?.name ?? null,
      code: warning?.code ?? null,
      message: warning?.message ?? null,
    });
  });
  process.emitWarning("manual warning", "DeprecationWarning", "DEPTEST");
  await new Promise((resolve) => process.nextTick(resolve));
  return {
    warningCount: warnings.length,
    firstWarning: warnings[0] ?? null,
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
            "warningCount": 1,
            "firstWarning": {
                "name": "DeprecationWarning",
                "code": "DEPTEST",
                "message": "manual warning",
            },
        })
    );
}

#[tokio::test]
async fn node22_target_load_env_file_missing_file_surfaces_node_not_found_error() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

globalThis.__neovexInvoke = function () {
  const { fileURLToPath } = require("node:url");
  const missingPath = fileURLToPath(new URL("./missing.env", import.meta.url));
  try {
    process.loadEnvFile(missingPath);
    return {
      threw: false,
      path: missingPath,
    };
  } catch (error) {
    return {
      threw: true,
      type: typeof error,
      stringified: String(error),
      constructorName: error?.constructor?.name ?? null,
      ownKeys: error && typeof error === "object" ? Object.getOwnPropertyNames(error).sort() : [],
      name: error?.name ?? null,
      code: error?.code ?? null,
      syscall: error?.syscall ?? null,
      path: error?.path ?? null,
      message: error?.message ?? null,
      missingPath,
    };
  }
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

    let missing_path = result
        .get("missingPath")
        .and_then(Value::as_str)
        .expect("missingPath should be captured");
    assert_eq!(result.get("threw"), Some(&Value::Bool(true)));
    assert_eq!(
        result.get("type"),
        Some(&Value::String("object".to_string()))
    );
    assert_eq!(
        result.get("constructorName"),
        Some(&Value::String("Error".to_string()))
    );
    assert_eq!(
        result.get("name"),
        Some(&Value::String("Error".to_string()))
    );
    assert_eq!(
        result.get("code"),
        Some(&Value::String("ENOENT".to_string()))
    );
    assert_eq!(
        result.get("syscall"),
        Some(&Value::String("open".to_string()))
    );
    assert_eq!(
        result.get("path"),
        Some(&Value::String(missing_path.to_string()))
    );
    assert_eq!(
        result.get("message"),
        Some(&Value::String(format!(
            "ENOENT: no such file or directory, open '{missing_path}'"
        )))
    );
    assert_eq!(
        result.get("stringified"),
        Some(&Value::String(format!(
            "Error: ENOENT: no such file or directory, open '{missing_path}'"
        )))
    );
    assert_eq!(
        result.get("ownKeys"),
        Some(&serde_json::json!([
            "code", "errno", "message", "path", "stack", "syscall",
        ]))
    );
}

#[tokio::test]
async fn node22_target_delivers_process_warning_events_for_deprecated_modules() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

globalThis.__neovexInvoke = async function () {
  const warnings = [];
  const emitWarningCalls = [];
  const originalEmitWarning = process.emitWarning;
  process.on("warning", (warning) => {
    warnings.push({
      name: warning?.name ?? null,
      code: warning?.code ?? null,
      message: warning?.message ?? null,
    });
  });
  process.emitWarning = function (...args) {
    emitWarningCalls.push(args.map((value) => String(value)));
    return originalEmitWarning.apply(this, args);
  };
  require("punycode");
  await new Promise((resolve) => process.nextTick(resolve));
  return {
    emitWarningCallCount: emitWarningCalls.length,
    warningCount: warnings.length,
    firstWarning: warnings[0] ?? null,
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
            "emitWarningCallCount": 1,
            "warningCount": 1,
            "firstWarning": {
                "name": "DeprecationWarning",
                "code": "DEP0040",
                "message": "The `punycode` module is deprecated. Please use a userland alternative instead.",
            },
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
async fn node22_target_supports_core_semantics_builtins_and_subpaths() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import assert from "node:assert/strict";
import legacyAssert from "node:assert";
import { Buffer } from "node:buffer";
import { Console } from "node:console";
import { EventEmitter, once } from "node:events";
import posix from "node:path/posix";
import win32 from "node:path/win32";
import * as punycode from "node:punycode";
import { parse, stringify } from "node:querystring";
import { StringDecoder } from "node:string_decoder";
import { URL, URLSearchParams } from "node:url";

globalThis.__neovexInvoke = async function () {
  assert.equal(Buffer.from([0x68, 0x69]).toString("utf8"), "hi");
  legacyAssert.ok(typeof legacyAssert.ifError === "function");

  const emitter = new EventEmitter();
  const observed = once(emitter, "done");
  emitter.emit("done", "events-ok");
  const [eventValue] = await observed;

  const query = stringify({ a: "1", b: "two words" });
  const parsed = parse(query);
  const decoder = new StringDecoder("utf8");
  const decoded = decoder.write(Buffer.from([0x68, 0x69]));
  const runtimeUrl = new URL("https://example.com/demo?message=hi");
  runtimeUrl.searchParams.set("lang", "en");
  const params = new URLSearchParams("a=1&b=two+words");

  return {
    eventValue,
    query,
    parsedA: parsed.a,
    parsedB: parsed.b,
    decoded,
    ascii: punycode.toASCII("mañana.com"),
    unicode: punycode.toUnicode("xn--maana-pta.com"),
    posixJoin: posix.join("demo", "messages", "file.txt"),
    win32Join: win32.join("demo", "messages", "file.txt"),
    consoleCtor: typeof Console,
    assertIfError: typeof legacyAssert.ifError,
    urlHref: runtimeUrl.href,
    urlHost: runtimeUrl.host,
    urlParamB: params.get("b"),
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
        .expect("core semantics builtins should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "eventValue": "events-ok",
            "query": "a=1&b=two%20words",
            "parsedA": "1",
            "parsedB": "two words",
            "decoded": "hi",
            "ascii": "xn--maana-pta.com",
            "unicode": "mañana.com",
            "posixJoin": "demo/messages/file.txt",
            "win32Join": "demo\\messages\\file.txt",
            "consoleCtor": "function",
            "assertIfError": "function",
            "urlHref": "https://example.com/demo?message=hi&lang=en",
            "urlHost": "example.com",
            "urlParamB": "two words",
        })
    );
}

#[tokio::test]
async fn application_node22_commonjs_package_can_require_core_semantics_builtins() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import runCorePackage from "core-semantics-cjs";

globalThis.__neovexInvoke = async function () {
  return runCorePackage();
};

export {};
"#,
    );
    let package_root = generated_node_modules_package_root(&bundle_path, "core-semantics-cjs");
    std::fs::create_dir_all(&package_root).expect("package root should build");
    std::fs::write(
        package_root.join("package.json"),
        r#"{
  "name": "core-semantics-cjs",
  "main": "./index.cjs"
}"#,
    )
    .expect("package manifest should write");
    std::fs::write(
        package_root.join("index.cjs"),
        r#"
const assert = require("node:assert/strict");
const legacyAssert = require("node:assert");
const { Buffer } = require("node:buffer");
const { Console } = require("node:console");
const events = require("node:events");
const posix = require("node:path/posix");
const punycode = require("node:punycode");
const querystring = require("node:querystring");
const { StringDecoder } = require("node:string_decoder");
const { URL } = require("node:url");

module.exports = function runCorePackage() {
  assert.equal(Buffer.from("ok").toString("utf8"), "ok");
  legacyAssert.ok(typeof legacyAssert.ifError === "function");
  const emitter = new events.EventEmitter();
  let eventValue = null;
  emitter.once("ready", (value) => {
    eventValue = value;
  });
  emitter.emit("ready", "commonjs-ok");
  const decoded = new StringDecoder("utf8").write(Buffer.from([0x68, 0x69]));
  const runtimeUrl = new URL("https://example.com/demo?message=hi");
  runtimeUrl.searchParams.set("lang", "en");
  return {
    eventValue,
    query: querystring.stringify({ a: "1", b: "two words" }),
    ascii: punycode.toASCII("mañana.com"),
    posixJoin: posix.join("demo", "messages"),
    consoleCtor: typeof Console,
    decoded,
    assertIfError: typeof legacyAssert.ifError,
    urlHref: runtimeUrl.href,
  };
};
"#,
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
        .expect("commonjs package should require core builtins");

    assert_eq!(
        result,
        serde_json::json!({
            "eventValue": "commonjs-ok",
            "query": "a=1&b=two%20words",
            "ascii": "xn--maana-pta.com",
            "posixJoin": "demo/messages",
            "consoleCtor": "function",
            "decoded": "hi",
            "assertIfError": "function",
            "urlHref": "https://example.com/demo?message=hi&lang=en",
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
async fn application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
	import { readFile, stat, writeFile } from "node:fs/promises";

	globalThis.__neovexInvoke = async function () {
	  const config = await readFile("./config.txt", "utf8");
	  const nodeEnv = process.env.NODE_ENV ?? null;
	  let writeDenied = null;
	  let metadataDenied = null;
	  try {
	    await writeFile("../escape.txt", "should-fail");
	  } catch (error) {
	    writeDenied = error?.message ?? String(error);
	  }
	  try {
	    await stat("/");
	  } catch (error) {
	    metadataDenied = error?.message ?? String(error);
	  }
	  return {
	    cwd: process.cwd(),
	    config,
	    nodeEnv,
	    writeDenied,
	    metadataDenied,
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
    assert_eq!(result["nodeEnv"], serde_json::json!(null));
    let write_denied = result["writeDenied"]
        .as_str()
        .expect("write denial should be a string");
    assert!(
        write_denied.contains("runtime write capability denied")
            || write_denied.contains("Requires write access"),
        "unexpected write denial: {write_denied}"
    );
    let metadata_denied = result["metadataDenied"]
        .as_str()
        .expect("metadata denial should be a string");
    assert!(
        metadata_denied.contains("runtime read capability denied")
            || metadata_denied.contains("Requires read access"),
        "unexpected metadata denial: {metadata_denied}"
    );
}

#[tokio::test]
async fn application_node22_allows_tls_reject_unauthorized_env_lookup() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _tls_env = ScopedProcessEnvVar::set("NODE_TLS_REJECT_UNAUTHORIZED", "0");
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__neovexInvoke = async function () {
  return {
    tlsRejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED ?? null,
  };
};

export {};
"#,
    );

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

    assert_eq!(result["tlsRejectUnauthorized"], serde_json::json!("0"));
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
async fn application_node22_denies_child_process_spawn_even_for_process_exec_path() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { spawnSync } from "node:child_process";

globalThis.__neovexInvoke = function () {
  try {
    const child = spawnSync(process.execPath, ["-e", "console.log('child-ok')"], {
      encoding: "utf8",
    });
    return {
      denied: child.error?.message ?? null,
      deniedCode: child.error?.code ?? null,
      status: child.status ?? null,
      signal: child.signal ?? null,
      stdout: child.stdout ?? null,
      stderr: child.stderr ?? null,
      keys: Object.keys(child).sort(),
    };
  } catch (error) {
    return {
      denied: error?.message ?? String(error),
      deniedCode: error?.code ?? null,
      status: null,
      signal: null,
      stdout: null,
      stderr: null,
      keys: [],
    };
  }
};

export {};
"#,
    );

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

    let denied = result["denied"].as_str();
    let status_is_denied = result["status"] == serde_json::json!(null);
    let stdout_is_empty = result["stdout"].is_null() || result["stdout"] == serde_json::json!("");
    let stderr_is_empty = result["stderr"].is_null() || result["stderr"] == serde_json::json!("");
    assert!(
        denied.is_some_and(|message| {
            message.contains("runtime run capability denied")
                || message.contains("Requires run access")
        }) || (status_is_denied && stdout_is_empty && stderr_is_empty),
        "unexpected child_process denial payload: {result}"
    );
    assert_eq!(result["status"], serde_json::json!(null));
}

#[tokio::test]
async fn application_node22_worker_threads_require_worker_grant() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { Worker } from "node:worker_threads";

globalThis.__neovexInvoke = function () {
  try {
    new Worker("require('node:worker_threads').parentPort.postMessage('ok')", {
      eval: true,
    });
    return { denied: null };
  } catch (error) {
    return { denied: error?.message ?? String(error) };
  }
};

export {};
"#,
    );

    let mut limits = RuntimeLimits::application_node22();
    limits.grants.worker.clear();
    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
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
        .expect("bundle should execute far enough to prove worker denial");

    let denied = result["denied"]
        .as_str()
        .expect("worker creation should be denied by grants");
    assert!(
        denied.contains("runtime worker grant denied for `thread`"),
        "unexpected worker denial: {denied}"
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
    let package_root = generated_node_modules_package_root(&bundle_path, "demo-package");
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
    let package_root = generated_node_modules_package_root(&bundle_path, "demo-package");
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
#[ignore = "Pinned NLC6 networking canary batch: requires `npm ci --prefix tests/runtime/node/networking-canaries` before execution"]
async fn application_node22_networking_package_canary_batch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let app = prepare_application_networking_canary_app();
    let bundles = [
        "express.mjs",
        "fastify.mjs",
        "axios.mjs",
        "undici.mjs",
        "socket-io.mjs",
    ];
    run_application_networking_canary_batch(&app, &bundles).await;
}

#[tokio::test]
#[ignore = "Pinned NLC6 Node20 supported canary batch: requires `npm ci --prefix tests/runtime/node/networking-canaries` before execution"]
async fn application_node20_networking_supported_canary_batch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let app = prepare_application_networking_canary_app();
    let bundles = ["express.mjs", "fastify.mjs"];
    run_application_networking_canary_batch(&app, &bundles).await;
}

async fn run_application_networking_canary_batch(
    app: &PreparedApplicationCanaryApp,
    bundles: &[&str],
) {
    let mut failures = Vec::new();

    for bundle_fixture_name in bundles {
        let actual = run_application_networking_canary_bundle(app, bundle_fixture_name).await;
        let expected = networking_canary_expected_result(bundle_fixture_name);
        if actual != expected {
            failures.push(format!(
                "{bundle_fixture_name} mismatch\nexpected: {expected}\nactual: {actual}"
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "application networking package canary batch had {} failure(s):\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

#[tokio::test]
#[ignore = "Pinned NLC10 tooling canary batch: requires `npm ci --prefix tests/runtime/node/tooling-canaries` before execution"]
async fn tooling_node22_package_canary_batch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let app = prepare_tooling_canary_app();
    let bundles = [
        "tsx.mjs",
        "ts-node.mjs",
        "jest.mjs",
        "prisma.mjs",
        "next.mjs",
    ];
    let mut failures = Vec::new();

    for bundle_fixture_name in bundles {
        let actual = run_tooling_canary_bundle(&app, bundle_fixture_name).await;
        if let Err(error) = assert_tooling_canary_result(bundle_fixture_name, &actual) {
            failures.push(format!(
                "{bundle_fixture_name} mismatch\nerror: {error}\nactual: {actual}"
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "tooling package canary batch had {} failure(s):\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
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
    let package_root = generated_node_modules_package_root(&bundle_path, "demo-package");
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
    let package_root = generated_node_modules_package_root(&bundle_path, "demo-package");
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
        .expect("tooling preset should resolve app-root CommonJS packages");

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
        .expect("esbuild-style staged binary should execute in tooling preset");

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
