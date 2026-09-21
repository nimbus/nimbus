use super::support::*;
use super::*;
use serde_json::json;
use std::path::Path;

// Node.js 26 (nodejs/node#61077) shows the target of a proxy in `Proxy(...)`
// when `showProxy` is off. Expected values come from official Node.js 20.20.2,
// 22.23.2, 24.21.0 and 26.9.0 running the same script. On Node.js 20 to 24, an
// inspection of a proxy around a revoked proxy throws a TypeError, so the
// script records only the error name.
#[tokio::test]
async fn node_util_inspect_proxy_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import util from "node:util";

globalThis.__nimbusInvoke = async function () {
  const revoked = () => {
    const { proxy, revoke } = Proxy.revocable({}, {});
    revoke();
    return proxy;
  };
  const errorName = (inspect) => {
    try {
      return inspect();
    } catch (error) {
      return error.name;
    }
  };
  const proxy = new Proxy([1, 2], {});
  return {
    node: process.versions.node,
    array: util.inspect(proxy),
    property: util.inspect({ a: proxy }),
    nested: util.inspect(new Proxy(proxy, {})),
    revoked: util.inspect(revoked()),
    nestedRevoked: errorName(() => util.inspect(new Proxy(revoked(), {}))),
    colors: util.inspect(proxy, { colors: true }),
    showProxy: util.inspect(proxy, { showProxy: true }),
    formatS: util.format("%s", proxy),
    formatO: util.format("%o", proxy),
  };
};

export {};
"#,
    );

    let target_only = json!({
        "array": "[ 1, 2 ]",
        "property": "{ a: [ 1, 2 ] }",
        "nested": "[ 1, 2 ]",
        "revoked": "<Revoked Proxy>",
        "nestedRevoked": "TypeError",
        "colors": "[ \u{1b}[33m1\u{1b}[39m, \u{1b}[33m2\u{1b}[39m ]",
        "showProxy": "Proxy [ [ 1, 2 ], {} ]",
        "formatS": "[ 1, 2 ]",
        "formatO": "Proxy [ [ 1, 2, [length]: 2 ], {} ]",
    });
    let annotate_target = json!({
        "array": "Proxy([ 1, 2 ])",
        "property": "{ a: Proxy([ 1, 2 ]) }",
        "nested": "Proxy(Proxy([ 1, 2 ]))",
        "revoked": "<Revoked Proxy>",
        "nestedRevoked": "Proxy(<Revoked Proxy>)",
        "colors": "\u{1b}[36mProxy(\u{1b}[39m[ \u{1b}[33m1\u{1b}[39m, \u{1b}[33m2\u{1b}[39m ]\u{1b}[36m)\u{1b}[39m",
        "showProxy": "Proxy [ [ 1, 2 ], {} ]",
        "formatS": "Proxy([ 1, 2 ])",
        "formatO": "Proxy [ [ 1, 2, [length]: 2 ], {} ]",
    });
    let cases = [
        (
            RuntimeLimits::application_node20_local_development(),
            "20",
            target_only.clone(),
        ),
        (
            RuntimeLimits::application_node22_local_development(),
            "22",
            target_only.clone(),
        ),
        (
            RuntimeLimits::application_node24_local_development(),
            "24",
            target_only,
        ),
        (
            RuntimeLimits::application_node26_local_development(),
            "26",
            annotate_target,
        ),
    ];

    for (limits, expected_major, expected) in cases {
        let result = invoke_on_lane(&bundle_path, limits, expected_major).await;
        assert_eq!(result, expected, "Node {expected_major} proxy inspection");
    }
}

// Deep equality must not reuse the memo entries of a finished nested
// comparison (nodejs/node#62509), and must compare a primitive Map key with
// object keys (nodejs/node#64441). Expected values come from official Node.js
// 20.20.2, 22.23.2, 24.21.0 and 26.9.0 running the same script. Node.js
// 22.23.2 throws a TypeError for `nullKeyStrict` (nodejs/node#64433). Nimbus
// fixes that defect on every lane and reports the assertion failure that the
// other Node.js lines report.
#[tokio::test]
async fn node_deep_equal_shared_values_and_primitive_map_keys_match_node() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import assert from "node:assert";
import util from "node:util";

globalThis.__nimbusInvoke = async function () {
  const outcome = (compare) => {
    try {
      compare();
      return "equal";
    } catch (error) {
      return `${error.name}:${error.code}`;
    }
  };
  const nullKeyMap = () => new Map([[null, { v: 1 }], [{ a: 1 }, 1]]);
  const objectKeyMap = () => new Map([[{ b: 2 }, { v: 1 }], [{ a: 1 }, 1]]);
  const shared = { outer: { inner: 0 } };
  const separate = () => [{ outer: { inner: 0 } }, { outer: { inner: 0 } }];
  const sharedBeforeCycle = util.isDeepStrictEqual(separate(), [shared, shared]);
  // A circular comparison switches the comparison to its memo mode.
  const a = {};
  a.self = a;
  const b = {};
  b.self = b;
  assert.deepStrictEqual(a, b);
  return {
    node: process.versions.node,
    sharedBeforeCycle,
    sharedAfterCycle: util.isDeepStrictEqual(separate(), [shared, shared]),
    nullKeyStrict: outcome(() => assert.deepStrictEqual(nullKeyMap(), objectKeyMap())),
    nullKeyLoose: outcome(() => assert.deepEqual(nullKeyMap(), objectKeyMap())),
    nullKeyEqual: outcome(() => assert.deepStrictEqual(nullKeyMap(), nullKeyMap())),
  };
};

export {};
"#,
    );

    let expected = json!({
        "sharedBeforeCycle": true,
        "sharedAfterCycle": true,
        "nullKeyStrict": "AssertionError:ERR_ASSERTION",
        "nullKeyLoose": "AssertionError:ERR_ASSERTION",
        "nullKeyEqual": "equal",
    });
    let cases = [
        (RuntimeLimits::application_node20_local_development(), "20"),
        (RuntimeLimits::application_node22_local_development(), "22"),
        (RuntimeLimits::application_node24_local_development(), "24"),
        (RuntimeLimits::application_node26_local_development(), "26"),
    ];

    for (limits, expected_major) in cases {
        let result = invoke_on_lane(&bundle_path, limits, expected_major).await;
        assert_eq!(result, expected, "Node {expected_major} deep equality");
    }
}

/// Runs the bundle on one Node.js lane, checks the reported Node.js major
/// version, and returns the result without the `node` field.
async fn invoke_on_lane(bundle_path: &Path, limits: RuntimeLimits, expected_major: &str) -> Value {
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(limits),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut result = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(bundle_path),
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
        .unwrap_or_else(|error| panic!("Node {expected_major} bundle should execute: {error:?}"));

    let node = result
        .as_object_mut()
        .and_then(|result| result.remove("node"))
        .unwrap_or(Value::Null);
    assert!(
        node.as_str()
            .is_some_and(|version| version.starts_with(expected_major)),
        "unexpected Node version payload for Node {expected_major}: {node}"
    );
    result
}
