use super::*;
use crate::RuntimeLimits;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

pub(super) fn networking_canary_expected_result(bundle_fixture_name: &str) -> Value {
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

pub(super) async fn run_application_networking_canary_bundle(
    app: &PreparedApplicationCanaryApp,
    bundle_fixture_name: &str,
) -> Value {
    stage_networking_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
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

pub(super) async fn run_tooling_canary_bundle(
    app: &PreparedToolingCanaryApp,
    bundle_fixture_name: &str,
) -> Value {
    stage_tooling_canary_bundle(app, bundle_fixture_name);
    let runtime = NimbusRuntime::with_policy(
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
