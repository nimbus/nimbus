#![cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]

use std::collections::BTreeMap;
use std::sync::Arc;

use nimbus_runtime::{
    HostBridge, HostCallRequest, InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle,
    RuntimeLimits, RuntimePolicy,
};
use serde_json::{Value, json};

#[derive(Debug)]
struct NoopHost;

impl HostBridge for NoopHost {
    fn call(&self, _request: HostCallRequest) -> nimbus_runtime::Result<Value> {
        Err(nimbus_runtime::NimbusRuntimeError::Contract(
            "Bun/JSC same-process proof must not reach the host bridge".to_string(),
        ))
    }
}

#[test]
fn bun_shared_adapter_coexists_with_v8_runtime_in_same_process() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let v8_bundle_path = temp_dir.path().join("v8-bundle.mjs");
    std::fs::write(
        &v8_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function(request) {
  return {
    engine: "v8",
    functionName: request.function_name,
    body: request.args.body,
  };
};

export {};
"#,
    )
    .expect("V8 bundle should be written");
    let bun_bundle_path = temp_dir.path().join("bun-program-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function(request) {
  return {
    status: "ok",
    value: {
      engine: "bun_jsc",
      functionName: request.function_name,
      body: request.args.body,
    },
  };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let v8_runtime = NimbusRuntime::with_policy(
        Arc::new(NoopHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_web_standard())),
    );
    let v8_request = |body: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:v8Proof".to_string(),
        args: json!({ "body": body }),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    assert_eq!(
        v8_runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&v8_bundle_path), &v8_request("before"))
            .expect("V8 invocation before Bun/JSC should run"),
        json!({
            "engine": "v8",
            "functionName": "messages:v8Proof",
            "body": "before",
        })
    );

    let bun_runtime = NimbusRuntime::with_policy(
        Arc::new(NoopHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let bun_request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:bunProof".to_string(),
        args: json!({ "body": "between" }),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };
    assert_eq!(
        bun_runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &bun_request)
            .expect("linked Bun/JSC invocation should run after V8"),
        json!({
            "status": "ok",
            "value": {
                "engine": "bun_jsc",
                "functionName": "messages:bunProof",
                "body": "between",
            },
        })
    );

    assert_eq!(
        v8_runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&v8_bundle_path), &v8_request("after"))
            .expect("V8 invocation after Bun/JSC should still run"),
        json!({
            "engine": "v8",
            "functionName": "messages:v8Proof",
            "body": "after",
        })
    );
}
