use super::support::*;
use super::*;

#[tokio::test]
async fn node_dgram_default_lookup_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import dgram from "node:dgram";
import dns from "node:dns";

function bind(hostname) {
  return new Promise((resolve, reject) => {
    const socket = dgram.createSocket("udp4");
    socket.once("error", (error) => {
      try {
        socket.close();
      } catch (_closeError) {}
      reject(error);
    });
    socket.bind(0, hostname, () => socket.close(resolve));
  });
}

globalThis.__nimbusInvoke = async function () {
  const originalLookup = dns.lookup;
  const lookupHosts = [];
  dns.lookup = (hostname, _family, callback) => {
    lookupHosts.push(hostname);
    queueMicrotask(() => callback(null, "127.0.0.1", 4));
  };
  try {
    await bind("nimbus-dgram.invalid");
    await bind("127.0.0.1");
    return { node: process.versions.node, lookupHosts };
  } finally {
    dns.lookup = originalLookup;
  }
};

export {};
"#,
    );

    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            serde_json::json!(["nimbus-dgram.invalid", "127.0.0.1"]),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            serde_json::json!(["nimbus-dgram.invalid", "127.0.0.1"]),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            serde_json::json!(["nimbus-dgram.invalid"]),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            serde_json::json!(["nimbus-dgram.invalid"]),
        ),
    ];

    for (limits, expected_major, expected_lookup_hosts) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("dgram lookup bundle should execute");

        assert!(
            result["node"]
                .as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {result}"
        );
        assert_eq!(result["lookupHosts"], expected_lookup_hosts);
    }
}

// Expected values are real Node.js v20.20.2, v22.23.1, v24.20.0 and v26.8.1 output.
#[tokio::test]
async fn node_password_cipher_api_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import crypto from "node:crypto";

globalThis.__nimbusInvoke = async function () {
  const warnings = [];
  const onWarning = (warning) => {
    warnings.push({
      name: warning.name,
      code: warning.code ?? null,
      message: warning.message,
    });
  };
  process.on("warning", onWarning);
  try {
    const surface = {
      createCipher: typeof crypto.createCipher,
      createDecipher: typeof crypto.createDecipher,
      Cipher: typeof crypto.Cipher,
      Decipher: typeof crypto.Decipher,
      enumerable: Object.keys(crypto).filter((key) =>
        /^(create)?(De)?[Cc]ipher$/.test(key)
      ),
    };
    if (typeof crypto.createCipher !== "function") {
      return { node: process.versions.node, surface };
    }
    const errorOf = (fn) => {
      try {
        fn();
        return null;
      } catch (error) {
        return { name: error.name, code: error.code };
      }
    };
    const cbc = crypto.createCipher("aes-128-cbc", "pw");
    const cbcHex = cbc.update("hi", "utf8", "hex") + cbc.final("hex");
    const cbcPlain = crypto.createDecipher("aes-128-cbc", "pw");
    const roundTrip = cbcPlain.update(cbcHex, "hex", "utf8") +
      cbcPlain.final("utf8");
    const calledWithoutNew =
      crypto.Cipher("aes-128-cbc", "pw") instanceof crypto.Cipher;
    const gcm = crypto.createCipher("aes-256-gcm", "pw");
    const gcmHex = gcm.update("hello", "utf8", "hex") + gcm.final("hex");
    const gcmTag = gcm.getAuthTag().toString("hex");
    const gcmPlain = crypto.createDecipher("aes-256-gcm", "pw");
    gcmPlain.setAuthTag(Buffer.from(gcmTag, "hex"));
    const gcmRoundTrip = gcmPlain.update(gcmHex, "hex", "utf8") +
      gcmPlain.final("utf8");
    const errors = {
      cipherType: errorOf(() => crypto.createCipher(1, "pw")),
      passwordType: errorOf(() => crypto.createCipher("aes-128-cbc", 1)),
      unknownCipher: errorOf(() => crypto.createCipher("nope", "pw")),
      authTagLength: errorOf(() =>
        crypto.createCipher("aes-128-ccm", "pw", { authTagLength: -1 })
      ),
    };
    await new Promise((resolve) => setTimeout(resolve, 10));
    const realEmitWarning = process.emitWarning;
    const overrideCalls = [];
    process.emitWarning = (message) => {
      overrideCalls.push(String(message));
    };
    for (const [name, options] of [
      ["aes-128-ctr"],
      ["aes-128-gcm"],
      ["aes-128-ccm", { authTagLength: 16 }],
      ["aes-128-ocb", { authTagLength: 16 }],
      ["chacha20-poly1305", { authTagLength: 16 }],
      ["aes-128-cbc"],
    ]) {
      crypto.createCipher(name, "pw", options);
      crypto.createDecipher(name, "pw", options);
    }
    process.emitWarning = () => {
      throw new Error("foo");
    };
    let throwingOverride = null;
    try {
      crypto.createCipher("aes-256-gcm", "pw");
    } catch (error) {
      throwingOverride = String(error);
    }
    process.emitWarning = realEmitWarning;
    return {
      node: process.versions.node,
      surface,
      cbcHex,
      roundTrip,
      calledWithoutNew,
      gcmHex,
      gcmTag,
      gcmRoundTrip,
      errors,
      warnings,
      overrideCalls,
      throwingOverride,
    };
  } finally {
    process.off("warning", onWarning);
  }
};

export {};
"#,
    );

    let removed_surface = |enumerable: Value| {
        serde_json::json!({
            "createCipher": "undefined",
            "createDecipher": "undefined",
            "Cipher": "undefined",
            "Decipher": "undefined",
            "enumerable": enumerable,
        })
    };
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            serde_json::json!({
                "surface": {
                    "createCipher": "function",
                    "createDecipher": "function",
                    "Cipher": "function",
                    "Decipher": "function",
                    "enumerable": ["Cipher", "Decipher"],
                },
                "cbcHex": "3346aed96f62dae82a27d5639e78f573",
                "roundTrip": "hi",
                "calledWithoutNew": true,
                "gcmHex": "97cbed0c3c",
                "gcmTag": "242a1180041dd291e7f20d95a8faa93a",
                "gcmRoundTrip": "hello",
                "errors": {
                    "cipherType": { "name": "TypeError", "code": "ERR_INVALID_ARG_TYPE" },
                    "passwordType": { "name": "TypeError", "code": "ERR_INVALID_ARG_TYPE" },
                    "unknownCipher": { "name": "Error", "code": "ERR_CRYPTO_UNKNOWN_CIPHER" },
                    "authTagLength": { "name": "TypeError", "code": "ERR_INVALID_ARG_VALUE" },
                },
                "warnings": [
                    {
                        "name": "DeprecationWarning",
                        "code": "DEP0106",
                        "message": "crypto.createCipher is deprecated.",
                    },
                    {
                        "name": "Warning",
                        "code": null,
                        "message": "Use Cipheriv for counter mode of aes-256-gcm",
                    },
                ],
                "overrideCalls": [
                    "Use Cipheriv for counter mode of aes-128-ctr",
                    "Use Cipheriv for counter mode of aes-128-gcm",
                    "Use Cipheriv for counter mode of aes-128-ccm",
                ],
                "throwingOverride": "Error: foo",
            }),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            serde_json::json!({ "surface": removed_surface(serde_json::json!(["Cipher", "Decipher"])) }),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            serde_json::json!({ "surface": removed_surface(serde_json::json!([])) }),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            serde_json::json!({ "surface": removed_surface(serde_json::json!([])) }),
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("password cipher bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(
            result, expected,
            "Node {expected_major} password cipher API"
        );
    }
}

// Expected values come from official Node.js 20.20.2, 22.23.1, 24.20.0 and
// 26.8.1 running the same script.
#[tokio::test]
async fn node_events_once_and_on_options_track_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { EventEmitter, on, once } from "node:events";

globalThis.__nimbusInvoke = async function () {
  const errorOf = (error) => ({ name: error.name, code: error.code ?? null });
  const onceWith = async (options) => {
    const emitter = new EventEmitter();
    process.nextTick(() => emitter.emit("event", 42));
    try {
      return { value: await once(emitter, "event", options) };
    } catch (error) {
      return { error: errorOf(error) };
    }
  };
  const onWith = (options) => {
    try {
      on(new EventEmitter(), "event", options).return();
      return { iterator: true };
    } catch (error) {
      return { error: errorOf(error) };
    }
  };
  const reason = new Error("stop");
  const aborted = AbortSignal.abort(reason);
  let earlyOnce;
  let earlyOn;
  try {
    await once(new EventEmitter(), "event", { signal: aborted });
  } catch (error) {
    earlyOnce = error;
  }
  try {
    on(new EventEmitter(), "event", { signal: aborted });
  } catch (error) {
    earlyOn = error;
  }
  const controller = new AbortController();
  const emitter = new EventEmitter();
  const pending = once(emitter, "event", { signal: controller.signal })
    .catch((error) => error);
  const next = on(emitter, "event", { signal: controller.signal }).next()
    .catch((error) => error);
  controller.abort(reason);
  const [lateOnce, lateOn] = await Promise.all([pending, next]);
  return {
    node: process.versions.node,
    onceNull: await onceWith(null),
    onceString: await onceWith("hi"),
    onNull: onWith(null),
    onNumber: onWith(1),
    abortCause: [earlyOnce, earlyOn, lateOnce, lateOn].map((error) => ({
      name: error?.name,
      causeIsReason: error?.cause === reason,
    })),
  };
};

export {};
"#,
    );

    let aborts = serde_json::json!([
        { "name": "AbortError", "causeIsReason": true },
        { "name": "AbortError", "causeIsReason": true },
        { "name": "AbortError", "causeIsReason": true },
        { "name": "AbortError", "causeIsReason": true },
    ]);
    let invalid = serde_json::json!({
        "error": { "name": "TypeError", "code": "ERR_INVALID_ARG_TYPE" },
    });
    let validated = serde_json::json!({
        "onceNull": invalid,
        "onceString": invalid,
        "onNull": invalid,
        "onNumber": invalid,
        "abortCause": aborts,
    });
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            serde_json::json!({
                "onceNull": { "value": [42] },
                "onceString": { "value": [42] },
                "onNull": { "error": { "name": "TypeError", "code": null } },
                "onNumber": { "iterator": true },
                "abortCause": aborts,
            }),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            validated.clone(),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            validated.clone(),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            validated,
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("events options bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(result, expected, "Node {expected_major} events options");
    }
}

// Expected values come from official Node.js 20.20.2, 22.23.2, 24.21.0 and
// 26.9.0 running the same script.
#[tokio::test]
async fn node_buffer_max_length_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import buffer, { Buffer, constants, kMaxLength } from "node:buffer";

globalThis.__nimbusInvoke = async function () {
  let overLimit = null;
  try {
    Buffer.alloc(kMaxLength + 1);
  } catch (error) {
    overLimit = { name: error.name, code: error.code ?? null };
  }
  return {
    node: process.versions.node,
    kMaxLength,
    defaultKMaxLength: buffer.kMaxLength,
    maxLength: constants.MAX_LENGTH,
    overLimit,
  };
};

export {};
"#,
    );

    // The values exceed the int32 range, so they reach Rust as `f64`.
    let expected_for = |max_length: f64| {
        serde_json::json!({
            "kMaxLength": max_length,
            "defaultKMaxLength": max_length,
            "maxLength": max_length,
            "overLimit": { "name": "RangeError", "code": "ERR_OUT_OF_RANGE" },
        })
    };
    let safe_integer = 9_007_199_254_740_991_f64;
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            expected_for(4_294_967_296_f64),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            expected_for(safe_integer),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            expected_for(safe_integer),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            expected_for(safe_integer),
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("buffer max length bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(result, expected, "Node {expected_major} buffer max length");
    }
}

// Expected messages come from official Node.js 20.20.2, 22.23.2, 24.21.0 and
// 26.9.0 running the same script. Node 22, 24 and 26 agree.
#[tokio::test]
async fn node_assertion_error_diff_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import assert from "node:assert";

globalThis.__nimbusInvoke = async function () {
  const capture = (fn) => {
    try {
      fn();
      return null;
    } catch (error) {
      return error;
    }
  };
  const u8buf = capture(() =>
    assert.deepStrictEqual(
      new Uint8Array([120, 121, 122, 10]),
      Buffer.from([120, 121, 122, 10]),
    )
  );
  const big = capture(() =>
    assert.deepStrictEqual(
      Array.from({ length: 40 }, (_, i) => i),
      Array.from({ length: 40 }, (_, i) => (i === 20 ? 99 : i)),
    )
  );
  const cause = capture(() =>
    assert.deepStrictEqual(
      new Error("a", { cause: 1 }),
      new Error("a", { cause: 2 }),
    )
  );
  const custom = capture(() =>
    assert.deepStrictEqual({ a: 1 }, { a: 2 }, "custom")
  );
  return {
    node: process.versions.node,
    u8buf: u8buf.message,
    big: big.message,
    cause: cause?.message ?? null,
    custom: custom.message,
    ownDiff: Object.hasOwn(u8buf, "diff"),
  };
};

export {};
"#,
    );

    let node20: Value = serde_json::from_str(
        r##"{"u8buf":"Expected values to be strictly deep-equal:\n+ actual - expected ... Lines skipped\n\n+ Uint8Array(4) [\n- Buffer(4) [Uint8Array] [\n    120,\n...\n    122,\n    10\n  ]","big":"Expected values to be strictly deep-equal:\n+ actual - expected ... Lines skipped\n\n  [\n    0,\n...\n    18,\n    19,\n+   20,\n-   99,\n    21,\n...\n    38,\n    39\n  ]","cause":"Values have same structure but are not reference-equal:\n\n[Error: a]\n","custom":"custom\n+ actual - expected\n\n  {\n+   a: 1\n-   a: 2\n  }","ownDiff":false}"##,
    )
    .expect("Node 20 expectation should parse");
    let myers: Value = serde_json::from_str(
        r##"{"u8buf":"Expected values to be strictly deep-equal:\n+ actual - expected\n\n+ Uint8Array(4) [\n- Buffer(4) [Uint8Array] [\n    120,\n    121,\n    122,\n    10\n  ]\n","big":"Expected values to be strictly deep-equal:\n+ actual - expected\n... Skipped lines\n\n  [\n    0,\n    1,\n    2,\n    3,\n...\n    19,\n+   20,\n-   99,\n    21,\n    22,\n    23,\n    24,\n    25,\n","cause":"Expected values to be strictly deep-equal:\n+ actual - expected\n\n  [Error: a] {\n+   [cause]: 1\n-   [cause]: 2\n  }\n","custom":"custom\n+ actual - expected\n\n  {\n+   a: 1\n-   a: 2\n  }\n","ownDiff":true}"##,
    )
    .expect("Node 22 expectation should parse");
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            node20,
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            myers.clone(),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            myers.clone(),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            myers,
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("assertion error diff bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(
            result, expected,
            "Node {expected_major} assertion error diff"
        );
    }
}

// Node.js 20 has no `Assert` class and no `partialDeepStrictEqual` (added in
// 22.19.0 and 22.13.0). Node.js 25 removed `CallTracker` (DEP0173). Expected
// values come from official Node.js 20.20.2, 22.23.2, 24.21.0 and 26.9.0 running
// the same script.
#[tokio::test]
async fn node_assert_api_surface_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import assert from "node:assert";
import * as assertNs from "node:assert";
import * as strictNs from "node:assert/strict";

globalThis.__nimbusInvoke = async function () {
  const versioned = ["Assert", "CallTracker", "partialDeepStrictEqual"];
  const surface = (target) =>
    Object.fromEntries(
      versioned.map((name) => [
        name,
        Object.hasOwn(target, name) ? typeof target[name] : "absent",
      ]),
    );
  const bindings = (namespace) =>
    Object.fromEntries(versioned.map((name) => [name, typeof namespace[name]]));
  return {
    node: process.versions.node,
    default: surface(assert),
    strict: surface(assert.strict),
    esm: bindings(assertNs),
    strictEsm: bindings(strictNs),
    keys: Object.keys(assert).length,
  };
};

export {};
"#,
    );

    let node20: Value = serde_json::from_str(
        r#"{"default":{"Assert":"absent","CallTracker":"function","partialDeepStrictEqual":"absent"},"strict":{"Assert":"absent","CallTracker":"function","partialDeepStrictEqual":"absent"},"esm":{"Assert":"undefined","CallTracker":"function","partialDeepStrictEqual":"undefined"},"strictEsm":{"Assert":"undefined","CallTracker":"function","partialDeepStrictEqual":"undefined"},"keys":20}"#,
    )
    .expect("Node 20 expectation should parse");
    let node22: Value = serde_json::from_str(
        r#"{"default":{"Assert":"function","CallTracker":"function","partialDeepStrictEqual":"function"},"strict":{"Assert":"function","CallTracker":"function","partialDeepStrictEqual":"function"},"esm":{"Assert":"function","CallTracker":"function","partialDeepStrictEqual":"function"},"strictEsm":{"Assert":"function","CallTracker":"function","partialDeepStrictEqual":"function"},"keys":22}"#,
    )
    .expect("Node 22 expectation should parse");
    let node26: Value = serde_json::from_str(
        r#"{"default":{"Assert":"function","CallTracker":"absent","partialDeepStrictEqual":"function"},"strict":{"Assert":"function","CallTracker":"absent","partialDeepStrictEqual":"function"},"esm":{"Assert":"function","CallTracker":"undefined","partialDeepStrictEqual":"function"},"strictEsm":{"Assert":"function","CallTracker":"undefined","partialDeepStrictEqual":"function"},"keys":21}"#,
    )
    .expect("Node 26 expectation should parse");
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            node20,
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            node22.clone(),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            node22,
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            node26,
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("assert API surface bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(result, expected, "Node {expected_major} assert API surface");
    }
}

// Node.js 24 (nodejs/node#57622) stops the recursion when either side reaches a
// circular reference. Expected values come from official Node.js 20.20.2,
// 22.23.2, 24.21.0 and 26.9.0 running the same script.
#[tokio::test]
async fn node_deep_equal_cycle_stop_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import assert from "node:assert";
import util from "node:util";

globalThis.__nimbusInvoke = async function () {
  const a = {};
  a.a = a;
  const b = {};
  b.a = b;
  const c = {};
  c.a = a;
  const bothSides = util.isDeepStrictEqual(b, c);
  const reverse = util.isDeepStrictEqual(c, b);
  let assertDeepEqual = true;
  try {
    assert.deepEqual(b, c);
  } catch {
    assertDeepEqual = false;
  }
  return {
    node: process.versions.node,
    bothSides,
    reverse,
    assertDeepEqual,
  };
};

export {};
"#,
    );

    let both_sides = serde_json::json!({
        "bothSides": true,
        "reverse": true,
        "assertDeepEqual": true,
    });
    let either_side = serde_json::json!({
        "bothSides": false,
        "reverse": false,
        "assertDeepEqual": false,
    });
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            both_sides.clone(),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            both_sides,
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            either_side.clone(),
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            either_side,
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let mut result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("deep-equal cycle bundle should execute");

        let node = result
            .as_object_mut()
            .and_then(|result| result.remove("node"))
            .unwrap_or(Value::Null);
        assert!(
            node.as_str()
                .is_some_and(|version| version.starts_with(expected_major)),
            "unexpected Node version payload for Node {expected_major}: {node}"
        );
        assert_eq!(
            result, expected,
            "Node {expected_major} deep-equal cycle stop"
        );
    }
}

#[tokio::test]
async fn application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
	import { mkdirSync } from "node:fs";
	import { readFile, stat, writeFile } from "node:fs/promises";

	globalThis.__nimbusInvoke = async function () {
	  const config = await readFile("./config.txt", "utf8");
	  mkdirSync("./sync-created", { recursive: true });
	  await writeFile("./sync-created/file.txt", "sync-data");
	  const syncRoundTrip = await readFile("./sync-created/file.txt", "utf8");
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
	    syncRoundTrip,
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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

    let expected_cwd = tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("expected cwd should canonicalize");
    assert_eq!(
        result["cwd"],
        serde_json::json!(expected_cwd.display().to_string())
    );
    assert_eq!(result["config"], serde_json::json!("hello from bundle"));
    assert_eq!(result["syncRoundTrip"], serde_json::json!("sync-data"));
    assert_eq!(result["nodeEnv"], serde_json::json!(null));
    let write_denied = result["writeDenied"]
        .as_str()
        .expect("write denial should be a string");
    // node:fs surfaces a sandbox write denial as the Node-correct `EACCES`
    // errno (deno_node maps Deno's `NotCapable` permission error to EACCES so
    // fs consumers see libuv-style codes). Older Deno-native phrasing is kept
    // as an accepted alternative in case a denial is reported by the Nimbus
    // capability layer instead.
    assert!(
        write_denied.contains("EACCES")
            || write_denied.contains("runtime write capability denied")
            || write_denied.contains("Requires write access"),
        "unexpected write denial: {write_denied}"
    );
    // The hard security property: the escape write must never materialize a
    // file outside the write root, regardless of how the denial is phrased.
    assert!(
        !tempdir.path().join("app/.nimbus/escape.txt").exists(),
        "escape write must not create a file outside the write root"
    );
    let metadata_denied = result["metadataDenied"]
        .as_str()
        .expect("metadata denial should be a string");
    assert!(
        metadata_denied.contains("EACCES")
            || metadata_denied.contains("runtime read capability denied")
            || metadata_denied.contains("Requires read access"),
        "unexpected metadata denial: {metadata_denied}"
    );
}

#[tokio::test]
async fn application_node22_startup_snapshot_refreshes_policy_cwd_for_relative_fs_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (first_tempdir, first_bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  return { cwd: process.cwd() };
};

export {};
"#,
    );
    let (second_tempdir, second_bundle_path) = write_app_style_bundle(
        r#"
import { existsSync, mkdirSync, writeFileSync } from "node:fs";

globalThis.__nimbusInvoke = function () {
  mkdirSync("./node_modules/.prisma/client", { recursive: true });
  writeFileSync("./node_modules/.prisma/client/query_engine.node", "not a prisma engine");
  return {
    cwd: process.cwd(),
    wrote: existsSync("./node_modules/.prisma/client/query_engine.node"),
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&first_bundle_path),
            &request("snapshot:first"),
            "tenant-a",
        )
        .await
        .expect("first bundle should execute");
    let expected_first_cwd = first_tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("first cwd should canonicalize");
    assert_eq!(
        first["cwd"],
        serde_json::json!(expected_first_cwd.display().to_string())
    );

    let second = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&second_bundle_path),
            &request("snapshot:second"),
            "tenant-b",
        )
        .await
        .expect("second bundle should execute");
    let expected_second_cwd = second_tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("second cwd should canonicalize");
    assert_eq!(
        second["cwd"],
        serde_json::json!(expected_second_cwd.display().to_string())
    );
    assert_eq!(second["wrote"], serde_json::json!(true));
    assert!(
        second_bundle_path
            .parent()
            .expect("bundle parent should resolve")
            .join("node_modules/.prisma/client/query_engine.node")
            .exists(),
        "relative write should stay inside the bundle-generated root"
    );
}

#[tokio::test]
async fn application_node22_production_hides_tls_reject_unauthorized_env_lookup() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _tls_env = ScopedProcessEnvVar::set("NODE_TLS_REJECT_UNAUTHORIZED", "0");
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = async function () {
  return {
    tlsRejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED ?? null,
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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

    assert_eq!(result["tlsRejectUnauthorized"], serde_json::json!(null));
}

#[tokio::test]
async fn application_node22_local_development_allows_tls_reject_unauthorized_env_lookup() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _tls_env = ScopedProcessEnvVar::set("NODE_TLS_REJECT_UNAUTHORIZED", "0");
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = async function () {
  return {
    tlsRejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED ?? null,
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22_local_development()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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

    assert_eq!(result["tlsRejectUnauthorized"], serde_json::json!("0"));
}

#[tokio::test]
async fn application_node22_shared_worker_env_is_runtime_scoped_and_grant_gated() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { Worker, SHARE_ENV } from "node:worker_threads";

const NAME = "NIMBUS_C1_3_SHARED_ENV";

function sharedEnvOps() {
  return globalThis.__nimbusHiddenDenoGlobals.core.ops;
}

function capture(action) {
  try {
    return { ok: true, value: action() ?? null };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

function runSharedWorker(value) {
  const sharedEnv = globalThis.__nimbusInstallSharedWorkerEnvProxy();
  sharedEnv[NAME] = value;
  const workerSource = `
const { parentPort } = require("node:worker_threads");
const NAME = ${JSON.stringify(NAME)};
parentPort.postMessage({ before: process.env[NAME] ?? null });
process.env[NAME] = "worker-mutated-value";
parentPort.postMessage({ after: process.env[NAME] ?? null });
`;
  const worker = new Worker(workerSource, { eval: true, env: SHARE_ENV });
  const messages = [];
  return new Promise((resolve, reject) => {
    worker.once("error", reject);
    worker.on("message", (message) => {
      messages.push(message);
      if (messages.length === 2) {
        worker.terminate();
        resolve({
          messages,
          finalValue: sharedEnv[NAME] ?? null,
        });
      }
    });
  });
}

globalThis.__nimbusInvoke = async function (request) {
  const ops = sharedEnvOps();
  const mode = request.args?.mode;
  if (mode === "write") {
    ops.op_nimbus_runtime_shared_env_seed({});
    ops.op_nimbus_runtime_shared_env_set(NAME, request.args.value);
    return {
      value: ops.op_nimbus_runtime_shared_env_get(NAME) ?? null,
      snapshotValue: ops.op_nimbus_runtime_shared_env_snapshot()[NAME] ?? null,
    };
  }
  if (mode === "read") {
    return {
      value: ops.op_nimbus_runtime_shared_env_get(NAME) ?? null,
      snapshotValue: ops.op_nimbus_runtime_shared_env_snapshot()[NAME] ?? null,
    };
  }
  if (mode === "deniedWrite") {
    return capture(() => {
      ops.op_nimbus_runtime_shared_env_set(NAME, "denied");
      return ops.op_nimbus_runtime_shared_env_get(NAME);
    });
  }
  if (mode === "workerShare") {
    return await runSharedWorker(request.args.value);
  }
  return { error: `unexpected mode ${mode}` };
};

export {};
"#,
    );

    let mut read_write_limits = RuntimeLimits::application_node22();
    read_write_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    read_write_limits
        .grants
        .env_write
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());

    let writer_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_write_limits.clone()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let written = writer_runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({
                    "mode": "write",
                    "value": "writer-runtime-value",
                }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("writer runtime should execute");

    assert_eq!(
        written,
        serde_json::json!({
            "value": "writer-runtime-value",
            "snapshotValue": "writer-runtime-value",
        })
    );

    let mut worker_limits = RuntimeLimits::application_node22_local_development();
    worker_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    worker_limits
        .grants
        .env_write
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    let worker_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(worker_limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let worker_shared = worker_runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({
                    "mode": "workerShare",
                    "value": "parent-worker-value",
                }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("worker shared-env runtime should execute");

    assert_eq!(
        worker_shared,
        serde_json::json!({
            "messages": [
                { "before": "parent-worker-value" },
                { "after": "worker-mutated-value" },
            ],
            "finalValue": "worker-mutated-value",
        })
    );

    let reader_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_write_limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let read = reader_runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "mode": "read" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("reader runtime should execute");

    assert_eq!(
        read,
        serde_json::json!({
            "value": null,
            "snapshotValue": null,
        }),
        "a second runtime must not see the first runtime's shared env"
    );

    let mut read_only_limits = RuntimeLimits::application_node22();
    read_only_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    let read_only_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_only_limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let denied = read_only_runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "mode": "deniedWrite" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("read-only runtime should execute");

    assert_eq!(denied["ok"], serde_json::json!(false));
    let message = denied["message"]
        .as_str()
        .expect("env write denial should include a message");
    assert!(
        message.contains("runtime env write capability denied"),
        "unexpected shared env write denial: {message}"
    );
}

#[tokio::test]
async fn tooling_node22_allows_allowlisted_env_and_tmp_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { readFile, writeFile } from "node:fs/promises";

globalThis.__nimbusInvoke = async function () {
  await writeFile(".nimbus/tmp/tooling.txt", "tooling-data");
  const roundTrip = await readFile(".nimbus/tmp/tooling.txt", "utf8");
  return {
    cwd: process.cwd(),
    pathValue: process.env.PATH ?? null,
    roundTrip,
  };
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.nimbus/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::tooling_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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
        tempdir.path().join("app/.nimbus/tmp/tooling.txt").is_file(),
        "tooling write should materialize under the scoped tmp root"
    );
}

#[tokio::test]
async fn application_node22_denies_child_process_spawn_even_for_process_exec_path() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { spawnSync } from "node:child_process";

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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

globalThis.__nimbusInvoke = function () {
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

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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
async fn application_node22_denies_process_lifetime_near_heap_snapshot_callbacks() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { setHeapSnapshotNearHeapLimit } from "node:v8";

globalThis.__nimbusInvoke = function () {
  try {
    setHeapSnapshotNearHeapLimit(1);
    return { denied: null };
  } catch (error) {
    return {
      denied: error?.message ?? String(error),
      name: error?.name ?? null,
    };
  }
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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
        .expect("bundle should execute far enough to prove near-heap callback denial");

    assert_eq!(result["name"], serde_json::json!("Error"));
    assert_eq!(
        result["denied"],
        serde_json::json!("v8.setHeapSnapshotNearHeapLimit is disabled by the runtime embedder")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn application_node22_confines_symlink_stat_and_readlink_targets() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { stat, readlink } from "node:fs/promises";
import { statSync, readlinkSync } from "node:fs";

async function capture(action) {
  try {
    return { ok: true, value: await action() };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

function captureSync(action) {
  try {
    return { ok: true, value: action() };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

globalThis.__nimbusInvoke = async function () {
  return {
    insideStat: await capture(async () => (await stat("./inside-link.txt")).isFile()),
    insideReadlink: await capture(async () => await readlink("./inside-link.txt")),
    escapeStat: await capture(async () => (await stat("./escape-link.txt")).isFile()),
    escapeStatSync: captureSync(() => statSync("./escape-link.txt").isFile()),
    escapeReadlink: await capture(async () => await readlink("./escape-link.txt")),
    escapeReadlinkSync: captureSync(() => readlinkSync("./escape-link.txt")),
  };
};

export {};
"#,
    );
    let bundle_dir = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .to_path_buf();
    let inside_target = bundle_dir.join("inside-target.txt");
    let outside_target = tempdir.path().join("outside-secret.txt");
    std::fs::write(&inside_target, "inside").expect("inside target should write");
    std::fs::write(&outside_target, "outside").expect("outside target should write");
    std::os::unix::fs::symlink(&inside_target, bundle_dir.join("inside-link.txt"))
        .expect("inside symlink should write");
    std::os::unix::fs::symlink(&outside_target, bundle_dir.join("escape-link.txt"))
        .expect("escape symlink should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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
        result["insideStat"]["ok"],
        serde_json::json!(true),
        "inside symlink stat should resolve inside target: {result}"
    );
    assert_eq!(
        result["insideStat"]["value"],
        serde_json::json!(true),
        "inside symlink stat should report a file: {result}"
    );
    assert_eq!(
        result["insideReadlink"]["ok"],
        serde_json::json!(true),
        "inside symlink readlink should resolve inside target: {result}"
    );
    assert_eq!(
        result["insideReadlink"]["value"],
        serde_json::json!(inside_target.display().to_string())
    );

    for key in [
        "escapeStat",
        "escapeStatSync",
        "escapeReadlink",
        "escapeReadlinkSync",
    ] {
        assert_eq!(
            result[key]["ok"],
            serde_json::json!(false),
            "escaping symlink operation {key} should be denied: {result}"
        );
        let message = result[key]["message"]
            .as_str()
            .expect("symlink target denial should include a message");
        assert!(
            message.contains("runtime read capability denied")
                || message.contains("Requires read access"),
            "unexpected {key} denial: {message}"
        );
        assert!(
            message.contains("outside-secret"),
            "denial should identify the escaped target for {key}: {message}"
        );
    }
}

#[tokio::test]
async fn tooling_node22_write_file_requires_preexisting_parent_directory() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { writeFile } from "node:fs/promises";

globalThis.__nimbusInvoke = async function () {
  try {
    await writeFile(".nimbus/tmp/missing/tooling.txt", "tooling-data");
    return { ok: true };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      syscall: error?.syscall ?? null,
      message: error?.message ?? String(error),
    };
  }
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.nimbus/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::tooling_node22()),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
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
        result["ok"],
        serde_json::json!(false),
        "unexpected missing-parent write result: {result}"
    );
    assert_eq!(
        result["code"],
        serde_json::json!("ENOENT"),
        "unexpected missing-parent write result: {result}"
    );
    assert_eq!(
        result["syscall"],
        serde_json::json!("open"),
        "unexpected missing-parent write result: {result}"
    );
    let message = result["message"]
        .as_str()
        .expect("missing parent write failure should include a message");
    assert!(
        message.contains("no such file or directory"),
        "unexpected write failure: {message}"
    );
    assert!(
        !tempdir
            .path()
            .join("app/.nimbus/tmp/missing/tooling.txt")
            .exists(),
        "writeFile should not materialize missing parent directories"
    );
}
