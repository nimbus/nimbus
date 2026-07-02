use super::support::*;
use super::*;

fn cloudflare_worker_request(args: Value) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::CloudflareWorkerFetch,
        function_name: "worker:fetch".to_string(),
        args,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

#[tokio::test]
async fn cloudflare_worker_fetch_invokes_default_export_with_request_env_and_ctx() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(
        &bundle_path,
        r#"
export default {
  async fetch(request, env, ctx) {
    ctx.passThroughOnException();
    return new Response(JSON.stringify({
      method: request.method,
      url: request.url,
      message: env.MESSAGE,
    }), {
      status: 201,
      statusText: "Created",
      headers: { "x-worker": "nimbus" },
    });
  },
};
"#,
    )
    .expect("worker bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_request(serde_json::json!({
                "request": {
                    "url": "https://example.com/hello?x=1",
                    "method": "POST",
                    "headers": [["content-type", "text/plain"]],
                    "body": "payload",
                },
                "env": {
                    "MESSAGE": { "value": "from-env" },
                },
            })),
            "tenant-a",
        )
        .await
        .expect("Cloudflare Worker fetch should execute");

    assert_eq!(result["status"], serde_json::json!(201));
    assert_eq!(result["statusText"], serde_json::json!("Created"));
    assert_eq!(result["passThroughOnException"], serde_json::json!(true));
    let body: Value = serde_json::from_str(
        result["body"]
            .as_str()
            .expect("serialized response body should be text"),
    )
    .expect("response body should be JSON");
    assert_eq!(
        body,
        serde_json::json!({
            "method": "POST",
            "url": "https://example.com/hello?x=1",
            "message": "from-env",
        })
    );
    assert!(
        result["headers"]
            .as_array()
            .expect("headers should serialize as entries")
            .iter()
            .any(|entry| entry == &serde_json::json!(["x-worker", "nimbus"])),
        "expected x-worker response header, got {result}"
    );
}

#[tokio::test]
async fn cloudflare_worker_kv_binding_uses_session_bound_host_ops() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(
        &bundle_path,
        r#"
export default {
  async fetch(_request, env) {
    await env.CACHE.put("greeting", "hello", {
      metadata: { lang: "en" },
      expirationTtl: 120,
    });
    const listed = await env.CACHE.list({ prefix: "g", limit: 10 });
    return new Response(JSON.stringify(listed), {
      headers: { "content-type": "application/json" },
    });
  },
};
"#,
    )
    .expect("worker bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_request(serde_json::json!({
                "request": {
                    "url": "https://example.com/cache",
                    "method": "GET",
                },
                "env": {
                    "CACHE": {
                        "type": "kv_namespace",
                        "tenant_id": "tenant-a",
                        "namespace": "SESSION_CACHE",
                    },
                },
            })),
            "tenant-a",
        )
        .await
        .expect("Cloudflare Worker KV binding should execute");

    let body: Value = serde_json::from_str(
        result["body"]
            .as_str()
            .expect("serialized response body should be text"),
    )
    .expect("response body should be JSON");
    assert_eq!(body["operation"], serde_json::json!("cf_kv_list"));
    assert_eq!(
        body["payload"],
        serde_json::json!({
            "tenant_id": "tenant-a",
            "namespace": "SESSION_CACHE",
            "prefix": "g",
            "cursor": null,
            "limit": 10,
            "host_call_session_id": "cloudflare_worker_fetch:worker:fetch",
        })
    );

    let calls = host
        .calls
        .lock()
        .expect("recording host lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].operation, HostCallOperation::CfKvPut);
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "tenant_id": "tenant-a",
            "namespace": "SESSION_CACHE",
            "key": "greeting",
            "value_base64": "aGVsbG8=",
            "metadata": { "lang": "en" },
            "expiration": null,
            "expiration_ttl": 120,
            "host_call_session_id": "cloudflare_worker_fetch:worker:fetch",
        })
    );
    assert_eq!(calls[1].operation, HostCallOperation::CfKvList);
}

#[tokio::test]
async fn cloudflare_worker_unsupported_request_cf_is_named_error() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(
        &bundle_path,
        r#"
export default {
  async fetch(request) {
    return new Response(String(request.cf));
  },
};
"#,
    )
    .expect("worker bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let error = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_request(serde_json::json!({
                "request": {
                    "url": "https://example.com/cf",
                    "method": "GET",
                },
            })),
            "tenant-a",
        )
        .await
        .expect_err("unsupported request.cf should fail by name");

    assert!(
        error
            .to_string()
            .contains("Cloudflare Workers API request.cf is not supported by Nimbus CFA4"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn cloudflare_worker_wait_until_is_drained_after_response() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(
        &bundle_path,
        r#"
export default {
  async fetch(_request, _env, ctx) {
    ctx.waitUntil(Promise.reject(new Error("background failed")));
    return new Response("ok");
  },
};
"#,
    )
    .expect("worker bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let error = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_request(serde_json::json!({
                "request": {
                    "url": "https://example.com/wait",
                    "method": "GET",
                },
            })),
            "tenant-a",
        )
        .await
        .expect_err("rejected waitUntil should fail after response readiness");

    assert!(
        error
            .to_string()
            .contains("Nimbus waitUntil background drain rejected 1 promise"),
        "unexpected waitUntil error: {error}"
    );
}
