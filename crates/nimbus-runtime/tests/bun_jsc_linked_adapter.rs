#![cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_runtime::{
    HostBridge, HostCallCancellation, HostCallOperation, HostCallRequest, InvocationKind,
    InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeLimits, RuntimePolicy,
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

#[derive(Debug, Clone, Copy)]
enum RecordingHostPolicy {
    AllowDocumentInsert,
    CancelDuringCall,
    DenyAll,
    RejectForgedTenantContext,
}

#[derive(Debug)]
struct RecordingHost {
    policy: RecordingHostPolicy,
    calls: Mutex<Vec<HostCallRequest>>,
}

impl RecordingHost {
    fn new(policy: RecordingHostPolicy) -> Self {
        Self {
            policy,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<HostCallRequest> {
        self.calls
            .lock()
            .expect("calls lock should not poison")
            .clone()
    }
}

impl HostBridge for RecordingHost {
    fn call(&self, request: HostCallRequest) -> nimbus_runtime::Result<Value> {
        self.record_call_and_respond(request, None)
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> nimbus_runtime::Result<Value> {
        self.record_call_and_respond(request, Some(cancellation))
    }
}

impl RecordingHost {
    fn record_call_and_respond(
        &self,
        request: HostCallRequest,
        cancellation: Option<&HostCallCancellation>,
    ) -> nimbus_runtime::Result<Value> {
        self.calls
            .lock()
            .expect("calls lock should not poison")
            .push(request.clone());

        match self.policy {
            RecordingHostPolicy::CancelDuringCall => {
                if let Some(cancellation) = cancellation {
                    cancellation.cancel();
                }
                Err(nimbus_runtime::NimbusRuntimeError::Cancelled)
            }
            RecordingHostPolicy::DenyAll => Err(nimbus_runtime::NimbusRuntimeError::Contract(
                "host policy denied Bun/JSC host call".to_string(),
            )),
            RecordingHostPolicy::RejectForgedTenantContext
                if request.payload.get("tenant_id").is_some() =>
            {
                Err(nimbus_runtime::NimbusRuntimeError::Contract(
                    "guest supplied tenant identity is not trusted".to_string(),
                ))
            }
            RecordingHostPolicy::AllowDocumentInsert
            | RecordingHostPolicy::RejectForgedTenantContext
                if request.operation == HostCallOperation::DocumentInsert =>
            {
                Ok(json!("message-id-from-host"))
            }
            _ => Err(nimbus_runtime::NimbusRuntimeError::Contract(format!(
                "unexpected host operation {}",
                request.operation
            ))),
        }
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

#[test]
fn bun_shared_adapter_routes_generated_context_db_insert_through_host_bridge() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-host-bridge-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({ request, sessionId: "host-session" });
  const id = await ctx.db.insert("messages", { body: request.args.body });
  return {
    status: "ok",
    value: {
      id,
      rawTokenPresent: typeof globalThis.__nimbusHostToken !== "undefined",
    },
  };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let host = Arc::new(RecordingHost::new(RecordingHostPolicy::AllowDocumentInsert));
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name: "messages:send".to_string(),
        args: json!({ "body": "hello from bun" }),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    assert_eq!(
        runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
            .expect("linked Bun/JSC invocation should call HostBridge"),
        json!({
            "status": "ok",
            "value": {
                "id": "message-id-from-host",
                "rawTokenPresent": false,
            },
        })
    );

    let calls = host.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].abi_version, 1);
    assert_eq!(calls[0].operation, HostCallOperation::DocumentInsert);
    assert_eq!(
        calls[0].payload,
        json!({
            "session_id": "host-session",
            "table": "messages",
            "fields": { "body": "hello from bun" },
        })
    );
}

#[test]
fn bun_shared_adapter_host_bridge_denials_are_guest_visible_without_tokens() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-host-denied-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function() {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_document_insert", {
      table: "messages",
      fields: { body: "denied" },
    });
    return { status: "unexpected-success" };
  } catch (error) {
    return {
      status: "error",
      value: {
        code: error.nimbusHostError && error.nimbusHostError.code,
        message: String(error.message),
        rawTokenPresent: typeof globalThis.__nimbusHostToken !== "undefined",
      },
    };
  }
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::new(RecordingHostPolicy::DenyAll)),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name: "messages:send".to_string(),
        args: json!({}),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    let response = runtime
        .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
        .expect("linked Bun/JSC invocation should report host denial to guest");
    assert_eq!(response["status"], "error");
    assert_eq!(response["value"]["code"], "host_bridge_denied");
    assert_eq!(response["value"]["rawTokenPresent"], false);
    assert!(
        response["value"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("host policy denied Bun/JSC host call")
    );
}

#[test]
fn bun_shared_adapter_host_bridge_cancellation_is_guest_visible_without_tokens() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-host-cancelled-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function() {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_document_insert", {
      table: "messages",
      fields: { body: "cancelled" },
    });
    return { status: "unexpected-success" };
  } catch (error) {
    return {
      status: "error",
      value: {
        code: error.nimbusHostError && error.nimbusHostError.code,
        message: String(error.message),
        rawTokenPresent: typeof globalThis.__nimbusHostToken !== "undefined",
      },
    };
  }
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let host = Arc::new(RecordingHost::new(RecordingHostPolicy::CancelDuringCall));
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name: "messages:send".to_string(),
        args: json!({}),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    let response = runtime
        .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
        .expect("linked Bun/JSC invocation should report host cancellation to guest");
    assert_eq!(response["status"], "error");
    assert_eq!(response["value"]["code"], "cancelled");
    assert_eq!(response["value"]["rawTokenPresent"], false);
    assert!(
        response["value"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("Bun/JSC host call was cancelled")
    );

    let calls = host.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].operation, HostCallOperation::DocumentInsert);
}

#[test]
fn bun_shared_adapter_forged_tenant_context_does_not_create_authority() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-forged-context-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function() {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_document_insert", {
      table: "messages",
      fields: { body: "forged" },
      tenant_id: "attacker-controlled",
    });
    return { status: "unexpected-success" };
  } catch (error) {
    return {
      status: "error",
      value: {
        code: error.nimbusHostError && error.nimbusHostError.code,
        message: String(error.message),
        rawTokenPresent: typeof globalThis.__nimbusHostToken !== "undefined",
      },
    };
  }
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let host = Arc::new(RecordingHost::new(
        RecordingHostPolicy::RejectForgedTenantContext,
    ));
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name: "messages:send".to_string(),
        args: json!({}),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    let response = runtime
        .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
        .expect("linked Bun/JSC invocation should report forged context denial");
    assert_eq!(response["status"], "error");
    assert_eq!(response["value"]["code"], "host_bridge_denied");
    assert_eq!(response["value"]["rawTokenPresent"], false);
    assert!(
        response["value"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("guest supplied tenant identity is not trusted")
    );

    let calls = host.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].operation, HostCallOperation::DocumentInsert);
    assert_eq!(calls[0].payload["tenant_id"], "attacker-controlled");
}

#[test]
fn bun_shared_adapter_makes_microtask_progress() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-microtask-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvoke = async function(request) {
  const value = await Promise.resolve(request.args.value).then((number) => number + 1);
  return { status: "ok", value };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(NoopHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:microtask".to_string(),
        args: json!({ "value": 41 }),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    assert_eq!(
        runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
            .expect("linked Bun/JSC invocation should complete microtasks"),
        json!({ "status": "ok", "value": 42 })
    );
}

#[test]
fn bun_shared_adapter_discards_guest_state_between_untrusted_invocations() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bun_bundle_path = temp_dir.path().join("bun-fresh-discard-wrapper.js");
    std::fs::write(
        &bun_bundle_path,
        r#"
globalThis.__nimbusInvocationCount = (globalThis.__nimbusInvocationCount || 0) + 1;
globalThis.__nimbusInvoke = async function() {
  return {
    status: "ok",
    value: globalThis.__nimbusInvocationCount,
  };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(NoopHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_bun_jsc())),
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:freshDiscard".to_string(),
        args: json!({}),
        page_size: None,
        cursor: None,
        auth: None,
        services: BTreeMap::new(),
    };

    for _ in 0..2 {
        assert_eq!(
            runtime
                .invoke_bundle_blocking(&RuntimeBundle::new(&bun_bundle_path), &request)
                .expect("linked Bun/JSC invocation should use a fresh VM"),
            json!({ "status": "ok", "value": 1 })
        );
    }
}
