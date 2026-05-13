use super::support::*;
use super::*;

#[tokio::test]
async fn application_node22_resolves_local_esm_packages_from_scoped_node_modules() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { runtimeValue } from "demo-package";

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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
#[ignore = "Pinned networking package canary batch: requires `npm ci --prefix tests/runtime/node/networking-canaries` before execution"]
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
#[ignore = "Pinned networking Node20 supported canary batch: requires `npm ci --prefix tests/runtime/node/networking-canaries` before execution"]
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
#[ignore = "Pinned tooling package canary batch: requires `npm ci --prefix tests/runtime/node/tooling-canaries` before execution"]
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
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
