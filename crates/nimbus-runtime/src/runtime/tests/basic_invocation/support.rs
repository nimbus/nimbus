use super::*;
use crate::{RuntimeCompatibilityTarget, RuntimeLimits};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub(super) fn basic_invocation_suite_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(super) async fn acquire_basic_invocation_suite_lock() -> tokio::sync::MutexGuard<'static, ()> {
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

pub(super) fn write_app_style_bundle(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_dir = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    let bundle_path = bundle_dir.join("bundle.mjs");
    std::fs::write(&bundle_path, source).expect("bundle should write");
    (tempdir, bundle_path)
}

pub(super) fn write_test_executable(path: &std::path::Path, source: &str) {
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

pub(super) fn generated_node_modules_package_root(
    bundle_path: &std::path::Path,
    package_name: &str,
) -> std::path::PathBuf {
    bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .join("node_modules")
        .join(package_name)
}

pub(super) fn repo_root() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-runtime tests")
        .parent()
        .expect("crate parent should resolve")
        .parent()
        .expect("repo root should resolve")
        .to_path_buf()
}

pub(super) fn networking_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/networking-canaries")
}

pub(super) fn tooling_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/tooling-canaries")
}

pub(super) fn sdk_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/sdk-canaries")
}

pub(super) fn host_heavy_canary_root() -> PathBuf {
    repo_root().join("tests/runtime/node/host-heavy-canaries")
}

pub(super) fn copy_dir_recursive(source: &Path, destination: &Path) {
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

pub(super) struct PreparedApplicationCanaryApp {
    _tempdir: tempfile::TempDir,
    bundle_path: PathBuf,
}

pub(super) struct PreparedToolingCanaryApp {
    _tempdir: tempfile::TempDir,
    bundle_path: PathBuf,
}

pub(super) fn prepare_application_networking_canary_app() -> PreparedApplicationCanaryApp {
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
    let bundle_dir = app_root.join(".nimbus/convex");
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

pub(super) fn prepare_tooling_canary_app() -> PreparedToolingCanaryApp {
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
    let tooling_bin_root = app_root.join("node_modules/nimbus-host-node/bin");
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
    let bundle_dir = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    std::fs::create_dir_all(app_root.join(".nimbus/tmp")).expect("tooling tmp dir should build");

    PreparedToolingCanaryApp {
        _tempdir: tempdir,
        bundle_path: bundle_dir.join("bundle.mjs"),
    }
}

pub(super) fn prepare_application_sdk_canary_app() -> PreparedApplicationCanaryApp {
    let canary_root = sdk_canary_root();
    let canary_node_modules = canary_root.join("node_modules");
    assert!(
        canary_node_modules.is_dir(),
        "SDK canary dependencies are missing at {}; run `npm ci --prefix {}` first",
        canary_node_modules.display(),
        canary_root.display(),
    );

    let tempdir = tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_dir = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    copy_dir_recursive(&canary_node_modules, &bundle_dir.join("node_modules"));

    PreparedApplicationCanaryApp {
        _tempdir: tempdir,
        bundle_path: bundle_dir.join("bundle.mjs"),
    }
}

pub(super) fn prepare_application_host_heavy_canary_app() -> PreparedApplicationCanaryApp {
    let canary_root = host_heavy_canary_root();
    let canary_node_modules = canary_root.join("node_modules");
    assert!(
        canary_node_modules.is_dir(),
        "host-heavy canary dependencies are missing at {}; run `npm ci --prefix {}` first",
        canary_node_modules.display(),
        canary_root.display(),
    );

    let tempdir = tempdir().expect("tempdir should build");
    let app_root = tempdir.path().join("app");
    let bundle_dir = app_root.join(".nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should build");
    copy_dir_recursive(&canary_node_modules, &bundle_dir.join("node_modules"));

    PreparedApplicationCanaryApp {
        _tempdir: tempdir,
        bundle_path: bundle_dir.join("bundle.mjs"),
    }
}

pub(super) fn stage_networking_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
) {
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

pub(super) fn stage_sdk_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
) {
    let source = sdk_canary_root().join("bundles").join(bundle_fixture_name);
    std::fs::copy(&source, &app.bundle_path).unwrap_or_else(|error| {
        panic!(
            "SDK canary bundle {} should stage: {error}",
            source.display()
        )
    });
}

pub(super) fn stage_host_heavy_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
) {
    let source = host_heavy_canary_root()
        .join("bundles")
        .join(bundle_fixture_name);
    std::fs::copy(&source, &app.bundle_path).unwrap_or_else(|error| {
        panic!(
            "host-heavy canary bundle {} should stage: {error}",
            source.display()
        )
    });
}

pub(super) fn stage_tooling_canary_bundle(
    app: &PreparedToolingCanaryApp,
    bundle_fixture_name: &str,
) {
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

pub(super) fn networking_canary_expected_result(
    bundle_fixture_name: &str,
    target: RuntimeCompatibilityTarget,
) -> Value {
    match bundle_fixture_name {
        "platform.mjs" => {
            let metadata = target
                .node_lts_metadata()
                .expect("platform canary target should be a Node lane");
            serde_json::json!({
                "nodeMajor": metadata.major,
                "releaseLts": metadata.codename,
                "esmValue": "esm-ok",
                "cjsValue": "cjs-ok",
                "fileRoundtrip": "platform-canary",
                "pathBasename": "platform-canary.txt",
                "cryptoHash": "d4db462df901",
                "streamText": "stream-ok",
                "timerValue": "timer-ok",
                "fetchStatus": 200,
                "fetchBody": {
                    "ok": true,
                    "source": "platform",
                },
            })
        }
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
        "ws-echo.mjs" => serde_json::json!({
            "protocol": "ws",
            "sent": "hello-ws",
            "echoed": "echo:hello-ws",
        }),
        other => panic!("unexpected networking canary bundle fixture: {other}"),
    }
}

pub(super) async fn run_application_networking_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
    limits: RuntimeLimits,
) -> Value {
    stage_networking_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    runtime
        .invoke_bundle_for_tenant_for_test(
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
            "tenant-a",
        )
        .await
        .unwrap_or_else(|error| {
            panic!("networking canary bundle {bundle_fixture_name} should execute: {error}")
        })
}

pub(super) async fn run_application_sdk_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
    limits: RuntimeLimits,
) -> std::result::Result<Value, String> {
    stage_sdk_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&app.bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "sdk:canary".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .map_err(|error| format!("SDK canary bundle {bundle_fixture_name} should execute: {error}"))
}

pub(super) async fn run_application_host_heavy_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
    limits: RuntimeLimits,
) -> std::result::Result<Value, String> {
    stage_host_heavy_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&app.bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "host-heavy:canary".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .map_err(|error| {
            format!("host-heavy canary bundle {bundle_fixture_name} should execute: {error}")
        })
}

pub(super) async fn run_tooling_canary_bundle(
    app: &PreparedToolingCanaryApp,
    bundle_fixture_name: &str,
    limits: RuntimeLimits,
) -> Value {
    stage_tooling_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    runtime
        .invoke_bundle_for_tenant_for_test(
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
            "tenant-a",
        )
        .await
        .unwrap_or_else(|error| {
            panic!("tooling canary bundle {bundle_fixture_name} should execute: {error}")
        })
}

pub(super) fn tooling_canary_status(actual: &Value, key: &str) -> std::result::Result<i64, String> {
    actual
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer field {key} in {actual}"))
}

pub(super) fn tooling_canary_bool(actual: &Value, key: &str) -> std::result::Result<bool, String> {
    actual
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing bool field {key} in {actual}"))
}

pub(super) fn tooling_canary_string<'a>(
    actual: &'a Value,
    key: &str,
) -> std::result::Result<&'a str, String> {
    actual
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {key} in {actual}"))
}

pub(super) fn assert_tooling_canary_result(
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

fn sdk_canary_bool(actual: &Value, key: &str) -> std::result::Result<bool, String> {
    actual
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing bool field {key} in {actual}"))
}

fn sdk_canary_i64(actual: &Value, key: &str) -> std::result::Result<i64, String> {
    actual
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer field {key} in {actual}"))
}

fn sdk_canary_string<'a>(actual: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    actual
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {key} in {actual}"))
}

fn require_sdk_string(
    actual: &Value,
    key: &str,
    expected: &str,
) -> std::result::Result<(), String> {
    let actual_value = sdk_canary_string(actual, key)?;
    if actual_value == expected {
        Ok(())
    } else {
        Err(format!(
            "field {key} expected {expected:?}, got {actual_value:?} in {actual}"
        ))
    }
}

pub(super) fn assert_sdk_canary_result(
    bundle_fixture_name: &str,
    actual: &Value,
) -> std::result::Result<(), String> {
    match bundle_fixture_name {
        "openai.mjs" => {
            require_sdk_string(actual, "content", "openai-ok")?;
            require_sdk_string(actual, "requestPath", "/v1/chat/completions")?;
            require_sdk_string(actual, "requestModel", "nimbus-test-model")?;
            require_sdk_string(actual, "authHeader", "Bearer sk-nimbus")
        }
        "anthropic.mjs" => {
            require_sdk_string(actual, "text", "anthropic-ok")?;
            require_sdk_string(actual, "requestPath", "/v1/messages")?;
            require_sdk_string(actual, "requestModel", "claude-nimbus")?;
            require_sdk_string(actual, "apiKeyHeader", "sk-ant-nimbus")
        }
        "ai.mjs" => {
            require_sdk_string(actual, "schemaType", "object")?;
            require_sdk_string(actual, "toolResult", "ai:ok")?;
            if !sdk_canary_bool(actual, "hasStopPredicate")? {
                return Err(format!("AI SDK stop predicate missing: {actual}"));
            }
            Ok(())
        }
        "stripe.mjs" => {
            require_sdk_string(actual, "id", "cus_nimbus")?;
            require_sdk_string(actual, "email", "ada@example.com")?;
            require_sdk_string(actual, "requestPath", "/v1/customers")?;
            require_sdk_string(actual, "authHeader", "Bearer sk_test_nimbus")?;
            let body = sdk_canary_string(actual, "requestBody")?;
            if !body.contains("email=ada%40example.com") {
                return Err(format!("stripe request body missing email: {actual}"));
            }
            Ok(())
        }
        "resend.mjs" => {
            require_sdk_string(actual, "id", "email_nimbus")?;
            require_sdk_string(actual, "requestPath", "/emails")?;
            require_sdk_string(actual, "requestSubject", "Nimbus canary")?;
            require_sdk_string(actual, "authHeader", "Bearer re_nimbus")
        }
        "aws-s3.mjs" => {
            require_sdk_string(actual, "bucketName", "nimbus-canary")?;
            require_sdk_string(actual, "requestPath", "/?x-id=ListBuckets")?;
            if !sdk_canary_bool(actual, "authHeaderPresent")? {
                return Err(format!("AWS SDK auth header was missing: {actual}"));
            }
            Ok(())
        }
        "slack.mjs" => {
            if !sdk_canary_bool(actual, "ok")? {
                return Err(format!("slack auth.test did not report ok: {actual}"));
            }
            require_sdk_string(actual, "team", "Nimbus")?;
            require_sdk_string(actual, "user", "ada")?;
            require_sdk_string(actual, "requestPath", "/api/auth.test")?;
            require_sdk_string(actual, "authHeader", "Bearer xoxb-nimbus")
        }
        "octokit.mjs" => {
            require_sdk_string(actual, "login", "nimbus-bot")?;
            if sdk_canary_i64(actual, "id")? != 42 {
                return Err(format!("octokit id mismatch: {actual}"));
            }
            require_sdk_string(actual, "requestPath", "/user")?;
            require_sdk_string(actual, "authHeader", "token ghp_nimbus")
        }
        "jose.mjs" => {
            require_sdk_string(actual, "subject", "user_123")?;
            require_sdk_string(actual, "role", "admin")?;
            require_sdk_string(actual, "headerAlg", "HS256")?;
            if sdk_canary_i64(actual, "tokenParts")? != 3 {
                return Err(format!("jose token part count mismatch: {actual}"));
            }
            Ok(())
        }
        "zod.mjs" => {
            if sdk_canary_i64(actual, "count")? != 3 {
                return Err(format!("zod count mismatch: {actual}"));
            }
            require_sdk_string(actual, "firstTag", "nimbus")?;
            if sdk_canary_bool(actual, "failure")? {
                return Err(format!("zod invalid parse unexpectedly passed: {actual}"));
            }
            if sdk_canary_i64(actual, "issueCount")? < 1 {
                return Err(format!("zod issue count missing: {actual}"));
            }
            Ok(())
        }
        "uuid.mjs" => {
            require_sdk_string(actual, "id", "bc0a1831-8c89-5ac7-b2cb-a52eb2bf8222")?;
            if !sdk_canary_bool(actual, "valid")? {
                return Err(format!("uuid validation failed: {actual}"));
            }
            Ok(())
        }
        "nanoid.mjs" => {
            require_sdk_string(actual, "id", "nnnnnnnn")?;
            if sdk_canary_i64(actual, "length")? != 8 {
                return Err(format!("nanoid length mismatch: {actual}"));
            }
            Ok(())
        }
        "upstash-redis.mjs" => {
            require_sdk_string(actual, "value", "redis-ok")?;
            let calls = actual
                .get("calls")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("missing upstash calls in {actual}"))?;
            if calls.len() != 2 {
                return Err(format!("unexpected upstash call count in {actual}"));
            }
            Ok(())
        }
        other => Err(format!("unexpected SDK canary bundle fixture: {other}")),
    }
}

fn host_heavy_string<'a>(actual: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    actual
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {key} in {actual}"))
}

fn assert_denial_contains_any(
    actual: &Value,
    bundle_fixture_name: &str,
    tokens: &[&str],
) -> std::result::Result<(), String> {
    let denied = host_heavy_string(actual, "denied")?;
    if tokens.iter().any(|token| denied.contains(token)) {
        Ok(())
    } else {
        Err(format!(
            "{bundle_fixture_name} denial did not contain any expected token {tokens:?}: {actual}"
        ))
    }
}

pub(super) fn assert_host_heavy_canary_result(
    bundle_fixture_name: &str,
    actual: &Value,
) -> std::result::Result<(), String> {
    if host_heavy_string(actual, "supportStatus")? != "service_microvm_required" {
        return Err(format!(
            "{bundle_fixture_name} did not report service/microVM boundary: {actual}"
        ));
    }
    if host_heavy_string(actual, "diagnostic")? != "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED" {
        return Err(format!(
            "{bundle_fixture_name} did not report the host-heavy diagnostic token: {actual}"
        ));
    }

    match bundle_fixture_name {
        "child-process.mjs" => {
            if host_heavy_string(actual, "surface")? != "child_process" {
                return Err(format!("child_process surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["runtime run capability denied", "Requires run access"],
            )
        }
        "worker-threads.mjs" => {
            if host_heavy_string(actual, "surface")? != "worker_threads" {
                return Err(format!("worker_threads surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["runtime worker grant denied for `thread`"],
            )
        }
        "inspector.mjs" => {
            if host_heavy_string(actual, "surface")? != "inspector" {
                return Err(format!("inspector surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &[
                    "node:inspector requires a service/microVM route",
                    "inspector authority",
                ],
            )
        }
        "repl.mjs" => {
            if host_heavy_string(actual, "surface")? != "repl" {
                return Err(format!("repl surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &[
                    "node:repl requires an interactive host process",
                    "REPL authority",
                ],
            )
        }
        "node-test-runner.mjs" => {
            if host_heavy_string(actual, "surface")? != "node_test_runner" {
                return Err(format!("node_test_runner surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["runtime run capability denied", "Requires run access"],
            )
        }
        "native-addon.mjs" => {
            if host_heavy_string(actual, "surface")? != "native_addon" {
                return Err(format!("native_addon surface mismatch: {actual}"));
            }
            if host_heavy_string(actual, "deniedCode")? != "ERR_DLOPEN_DISABLED" {
                return Err(format!("native_addon denial code mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["native addon module", "ffi/native-addon authority", ".node"],
            )
        }
        "persistent-fs.mjs" => {
            if host_heavy_string(actual, "surface")? != "persistent_filesystem" {
                return Err(format!("persistent_filesystem surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &[
                    "runtime write capability denied",
                    "runtime read capability denied",
                    "Requires write access",
                    "Requires read access",
                ],
            )
        }
        "raw-server-listen.mjs" => {
            if host_heavy_string(actual, "surface")? != "raw_server_listen" {
                return Err(format!("raw_server_listen surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["Requires net access", "network access", "listen"],
            )
        }
        "ws-server-listen.mjs" => {
            if host_heavy_string(actual, "surface")? != "ws_server_listen" {
                return Err(format!("ws_server_listen surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["Requires net access", "network access", "listen"],
            )
        }
        "prisma-engine.mjs" => {
            if host_heavy_string(actual, "surface")? != "prisma_engine" {
                return Err(format!("prisma_engine surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &["native addon module", "ffi/native-addon authority", ".node"],
            )
        }
        "sharp-native.mjs" => {
            if host_heavy_string(actual, "surface")? != "sharp_native" {
                return Err(format!("sharp_native surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &[
                    "native addon module",
                    "ffi/native-addon authority",
                    "Could not load the \"sharp\" module",
                    "Requires sys access to \"cpus\"",
                    ".node",
                ],
            )
        }
        "esbuild-binary.mjs" => {
            if host_heavy_string(actual, "surface")? != "esbuild_binary" {
                return Err(format!("esbuild_binary surface mismatch: {actual}"));
            }
            assert_denial_contains_any(
                actual,
                bundle_fixture_name,
                &[
                    "runtime run capability denied",
                    "Requires run access",
                    "spawn",
                    "unref",
                ],
            )
        }
        other => Err(format!(
            "unexpected host-heavy canary bundle fixture: {other}"
        )),
    }
}

pub(super) fn assert_host_heavy_canary_error(
    bundle_fixture_name: &str,
    error: &str,
) -> std::result::Result<(), String> {
    match bundle_fixture_name {
        "raw-server-listen.mjs"
            if error.contains("Requires net access") && error.contains("127.0.0.1:0") =>
        {
            Ok(())
        }
        "raw-server-listen.mjs" => Err(format!(
            "raw-server-listen.mjs did not fail with net-listen denial: {error}"
        )),
        "ws-server-listen.mjs"
            if error.contains("Requires net access") && error.contains("127.0.0.1:0") =>
        {
            Ok(())
        }
        "ws-server-listen.mjs" => Err(format!(
            "ws-server-listen.mjs did not fail with net-listen denial: {error}"
        )),
        other => Err(format!(
            "{other} failed during execution instead of returning a diagnostic payload: {error}"
        )),
    }
}

#[test]
fn host_heavy_diagnostic_rejects_fake_success_payloads() {
    let supported_side_effect = serde_json::json!({
        "surface": "child_process",
        "supportStatus": "supported",
        "diagnostic": "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
        "denied": "spawned process successfully"
    });
    let supported_error =
        assert_host_heavy_canary_result("child-process.mjs", &supported_side_effect)
            .expect_err("supported fake-success payload must be rejected");
    assert!(
        supported_error.contains("service/microVM boundary"),
        "unexpected fake-success rejection: {supported_error}"
    );

    let misleading_diagnostic = serde_json::json!({
        "surface": "child_process",
        "supportStatus": "service_microvm_required",
        "diagnostic": "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
        "denied": "spawned process successfully"
    });
    let denial_error = assert_host_heavy_canary_result("child-process.mjs", &misleading_diagnostic)
        .expect_err("diagnostic payload that claims the side effect happened must be rejected");
    assert!(
        denial_error.contains("expected token"),
        "unexpected fake-success denial rejection: {denial_error}"
    );
}

pub(super) struct ScopedProcessEnvVar {
    key: &'static str,
    previous_value: Option<String>,
}

impl ScopedProcessEnvVar {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
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
