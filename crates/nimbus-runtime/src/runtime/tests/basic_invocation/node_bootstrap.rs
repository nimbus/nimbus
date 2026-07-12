use super::support::*;
use super::*;

#[cfg(windows)]
fn synthetic_runtime_exec_path() -> &'static str {
    r"C:\nimbus\runtime\node.exe"
}

#[cfg(not(windows))]
fn synthetic_runtime_exec_path() -> &'static str {
    "/nimbus/runtime/node"
}

fn expected_self_exec_path(bundle_path: &std::path::Path) -> String {
    let current_exec = std::env::current_exe().expect("current executable path should resolve");
    let exec_name = current_exec
        .file_name()
        .expect("current executable should have a file name");
    bundle_path
        .parent()
        .expect("bundle should have a generated root")
        .canonicalize()
        .expect("generated root should canonicalize")
        .join("bin")
        .join(exec_name)
        .display()
        .to_string()
}

#[tokio::test]
async fn node22_target_exposes_minimal_node_globals() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    globalAliasIsSelf: globalThis.global === globalThis,
    bufferValue: typeof globalThis.Buffer,
    bufferRoundTrip: globalThis.Buffer?.from("hi").toString("utf8") ?? null,
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
    moduleVersion: globalThis.process?.versions?.modules ?? null,
    releaseName: globalThis.process?.release?.name ?? null,
    releaseLts: globalThis.process?.release?.lts ?? null,
    processExecPath: globalThis.process?.execPath ?? null,
    stdinType: typeof globalThis.process?.stdin,
    stdinFd: globalThis.process?.stdin?.fd ?? null,
    stdinIsTTY: globalThis.process?.stdin?.isTTY ?? null,
    stdoutType: typeof globalThis.process?.stdout,
    stdoutWriteType: typeof globalThis.process?.stdout?.write,
    stdoutCursorToType: typeof globalThis.process?.stdout?.cursorTo,
    stdoutMoveCursorType: typeof globalThis.process?.stdout?.moveCursor,
    stdoutClearLineType: typeof globalThis.process?.stdout?.clearLine,
    stdoutClearScreenDownType: typeof globalThis.process?.stdout?.clearScreenDown,
    stderrType: typeof globalThis.process?.stderr,
    stderrWriteType: typeof globalThis.process?.stderr?.write,
    stderrCursorToType: typeof globalThis.process?.stderr?.cursorTo,
    stderrMoveCursorType: typeof globalThis.process?.stderr?.moveCursor,
    stderrClearLineType: typeof globalThis.process?.stderr?.clearLine,
    stderrClearScreenDownType: typeof globalThis.process?.stderr?.clearScreenDown,
    refreshOpStateHelperType: typeof globalThis.__nimbusRefreshNodeRuntimeOpState,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "globalAliasIsSelf": true,
            "bufferValue": "function",
            "bufferRoundTrip": "hi",
            "processVersion": "v22.22.3",
            "nodeVersion": "22.22.3",
            "moduleVersion": "127",
            "releaseName": "node",
            "releaseLts": "Jod",
            "processExecPath": synthetic_runtime_exec_path(),
            "stdinType": "object",
            "stdinFd": -1,
            "stdinIsTTY": false,
            "stdoutType": "object",
            "stdoutWriteType": "function",
            "stdoutCursorToType": "undefined",
            "stdoutMoveCursorType": "undefined",
            "stdoutClearLineType": "undefined",
            "stdoutClearScreenDownType": "undefined",
            "stderrType": "object",
            "stderrWriteType": "function",
            "stderrCursorToType": "undefined",
            "stderrMoveCursorType": "undefined",
            "stderrClearLineType": "undefined",
            "stderrClearScreenDownType": "undefined",
            "refreshOpStateHelperType": "undefined",
        })
    );
}

#[tokio::test]
async fn node22_exec_path_uses_synthetic_path_without_run_grant() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    execPath: process.execPath,
    denoExecPath: Deno.execPath(),
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "execPath": synthetic_runtime_exec_path(),
            "denoExecPath": synthetic_runtime_exec_path(),
        })
    );
    assert_ne!(
        result["execPath"],
        serde_json::json!(
            std::env::current_exe()
                .expect("current executable path should resolve")
                .display()
                .to_string()
        )
    );
}

#[tokio::test]
async fn node22_self_exec_grant_exposes_staged_exec_path() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    execPath: process.execPath,
    denoExecPath: Deno.execPath(),
  };
};

export {};
"#,
    );
    let expected_exec_path = expected_self_exec_path(&bundle_path);

    let mut limits = RuntimeLimits::application_node22();
    limits.grants.run = vec!["$runtime_self_exec".to_string()];
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "execPath": expected_exec_path,
            "denoExecPath": expected_exec_path,
        })
    );
}

#[tokio::test]
async fn node22_host_exec_grant_is_required_to_expose_host_exec_path() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  return process.execPath;
};

export {};
"#,
    );

    let mut limits = RuntimeLimits::application_node22();
    limits.grants.run = vec!["$runtime_host_exec".to_string()];
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!(
            std::env::current_exe()
                .expect("current executable path should resolve")
                .display()
                .to_string()
        )
    );
}

#[tokio::test]
async fn node26_current_target_exposes_truthful_process_metadata() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
    moduleVersion: globalThis.process?.versions?.modules ?? null,
    releaseName: globalThis.process?.release?.name ?? null,
    releaseLts: globalThis.process?.release?.lts ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node26())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "processVersion": "v26.2.0",
            "nodeVersion": "26.2.0",
            "moduleVersion": "147",
            "releaseName": "node",
            "releaseLts": null,
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
globalThis.__nimbusInvoke = async function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

globalThis.__nimbusInvoke = async function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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
async fn node22_target_retains_managed_deno_for_lazy_node_polyfills() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    denoValue: typeof globalThis.Deno,
    hasOwnDeno: Object.prototype.hasOwnProperty.call(globalThis, "Deno"),
    denoStat: typeof globalThis.Deno?.stat,
    denoErrors: typeof globalThis.Deno?.errors,
    denoBadResource: typeof globalThis.Deno?.errors?.BadResource,
    denoKeys: Object.keys(globalThis.Deno ?? {}).filter((key) => key === "stat" || key === "statSync"),
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "denoValue": "object",
            "hasOwnDeno": true,
            "denoStat": "function",
            "denoErrors": "object",
            "denoBadResource": "function",
            "denoKeys": ["stat", "statSync"],
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

globalThis.__nimbusInvoke = async function () {
  return {
    dirname: path.dirname("/demo/messages/file.txt"),
    joined: path.join("demo", "messages", "file.txt"),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

globalThis.__nimbusInvoke = async function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

globalThis.__nimbusInvoke = async function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

globalThis.__nimbusInvoke = async function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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

/// HG7: `__nimbusRefreshNodeProcessCwd` (host/reset-called across warm-pool
/// invocations) and `__nimbusPerfHooksBuiltin` (read fresh by the trusted
/// builtin-module resolver, `module_loader/builtins/module_wiring.js`'s
/// `getBuiltinModule`, on every guest `require("perf_hooks")`) used to be
/// installed `{configurable: true, writable: false}`. `writable: false` alone
/// only blocks a PLAIN assignment; `Object.defineProperty` with
/// `configurable: true` still permits full property redefinition — the
/// bypass this test exercises. Post-fix both slots are `{configurable:
/// false, writable: false}`, so the redefinition attempt throws instead of
/// silently swapping in an impostor that would otherwise ride into a later
/// same-tenant invocation on a warm-pooled realm.
#[tokio::test]
async fn guest_cannot_bypass_hardened_node_hooks_via_configurable_defineproperty() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);

globalThis.__nimbusInvoke = function () {
  const originalCwdRefresh = globalThis.__nimbusRefreshNodeProcessCwd;
  const originalPerfHooks = globalThis.__nimbusPerfHooksBuiltin;

  // The bypass: configurable:true still permits Object.defineProperty to
  // fully redefine the property even though writable:false blocks a plain
  // assignment. Pre-fix both slots used exactly this vulnerable pattern.
  let cwdRefreshDefineThrew = false;
  try {
    Object.defineProperty(globalThis, "__nimbusRefreshNodeProcessCwd", {
      value: function () {
        globalThis.__cwdImpostorCalled = true;
      },
      configurable: true,
      writable: true,
    });
  } catch (_error) {
    cwdRefreshDefineThrew = true;
  }

  let perfHooksDefineThrew = false;
  try {
    Object.defineProperty(globalThis, "__nimbusPerfHooksBuiltin", {
      value: { performance: { now: () => -1 }, __impostor: true },
      configurable: true,
      writable: true,
    });
  } catch (_error) {
    perfHooksDefineThrew = true;
  }

  // Simulate the host's per-invocation reset script actually calling the
  // hook (what reset_bootstrap_invocation_state.js does), and a later
  // require("perf_hooks") consulting the resolver — the two real trusted
  // consumers of these slots.
  globalThis.__nimbusRefreshNodeProcessCwd();
  const perfHooksModule = require("node:perf_hooks");

  const cwdRefreshDescriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    "__nimbusRefreshNodeProcessCwd",
  );
  const perfHooksDescriptor = Object.getOwnPropertyDescriptor(
    globalThis,
    "__nimbusPerfHooksBuiltin",
  );

  return {
    cwdRefreshDefineThrew,
    perfHooksDefineThrew,
    cwdRefreshIdentityStable: globalThis.__nimbusRefreshNodeProcessCwd === originalCwdRefresh,
    perfHooksIdentityStable: globalThis.__nimbusPerfHooksBuiltin === originalPerfHooks,
    cwdImpostorCalled: globalThis.__cwdImpostorCalled === true,
    perfHooksRequireIsImpostor: perfHooksModule?.__impostor === true,
    cwdRefreshWritable: cwdRefreshDescriptor.writable,
    cwdRefreshConfigurable: cwdRefreshDescriptor.configurable,
    perfHooksWritable: perfHooksDescriptor.writable,
    perfHooksConfigurable: perfHooksDescriptor.configurable,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "cwdRefreshDefineThrew": true,
            "perfHooksDefineThrew": true,
            "cwdRefreshIdentityStable": true,
            "perfHooksIdentityStable": true,
            "cwdImpostorCalled": false,
            "perfHooksRequireIsImpostor": false,
            "cwdRefreshWritable": false,
            "cwdRefreshConfigurable": false,
            "perfHooksWritable": false,
            "perfHooksConfigurable": false,
        }),
        "the configurable:true redefinition bypass must be closed for both hooks: {result}"
    );
}

/// HG9: `__nimbusHiddenDenoGlobals`/`__nimbusHiddenNodeGlobals`'s SLOTS were
/// already hardened (`writable: false, configurable: false`), but that only
/// protects which object the slot points to — the object's OWN properties
/// (`deno.core`, `hiddenNodeGlobals.Buffer`, …) were themselves installed
/// `{configurable: true, writable: false}`, the same redefinition-bypass
/// pattern as HG7. The trusted extension-transpiler prelude
/// (`bootstrap/transpile.rs`'s injected `Deno` proxy) reads these properties
/// straight off the live object on every lazily-transpiled internal Node
/// extension script, on a warm-pooled realm, across invocations — so a guest
/// that redefined `Deno.core` in invocation N would poison invocation N+1's
/// trusted internal polyfill loading. Post-fix both objects are shallow-
/// frozen (`Object.freeze`), closing every own-property redefinition path at
/// once.
#[tokio::test]
async fn guest_cannot_poison_frozen_deno_and_node_globals_object_graphs_via_configurable_defineproperty()
 {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  const deno = globalThis.Deno;
  const hiddenNodeGlobals = globalThis.__nimbusHiddenNodeGlobals;

  const originalDenoCore = deno.core;
  const originalBuffer = hiddenNodeGlobals.Buffer;

  const denoFrozenBefore = Object.isFrozen(deno);
  const nodeGlobalsFrozenBefore = Object.isFrozen(hiddenNodeGlobals);

  let denoCoreDefineThrew = false;
  try {
    Object.defineProperty(deno, "core", {
      value: { ops: { __impostor: true } },
      configurable: true,
      writable: true,
    });
  } catch (_error) {
    denoCoreDefineThrew = true;
  }

  let nodeGlobalsBufferDefineThrew = false;
  try {
    Object.defineProperty(hiddenNodeGlobals, "Buffer", {
      value: function ImpostorBuffer() {},
      configurable: true,
      writable: true,
    });
  } catch (_error) {
    nodeGlobalsBufferDefineThrew = true;
  }

  return {
    denoFrozenBefore,
    nodeGlobalsFrozenBefore,
    denoCoreDefineThrew,
    nodeGlobalsBufferDefineThrew,
    denoCoreIdentityStable: deno.core === originalDenoCore,
    nodeGlobalsBufferIdentityStable: hiddenNodeGlobals.Buffer === originalBuffer,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result,
        serde_json::json!({
            "denoFrozenBefore": true,
            "nodeGlobalsFrozenBefore": true,
            "denoCoreDefineThrew": true,
            "nodeGlobalsBufferDefineThrew": true,
            "denoCoreIdentityStable": true,
            "nodeGlobalsBufferIdentityStable": true,
        }),
        "the configurable:true redefinition bypass on the value graphs behind \
         both hardened slots must be closed: {result}"
    );
}
