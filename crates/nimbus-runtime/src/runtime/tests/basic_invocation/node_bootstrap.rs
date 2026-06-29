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
    stdoutType: typeof globalThis.process?.stdout,
    stdoutWriteType: typeof globalThis.process?.stdout?.write,
    stderrType: typeof globalThis.process?.stderr,
    stderrWriteType: typeof globalThis.process?.stderr?.write,
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
            "processVersion": "v22.22.3",
            "nodeVersion": "22.22.3",
            "moduleVersion": "127",
            "releaseName": "node",
            "releaseLts": "Jod",
            "processExecPath": synthetic_runtime_exec_path(),
            "stdoutType": "object",
            "stdoutWriteType": "function",
            "stderrType": "object",
            "stderrWriteType": "function",
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
