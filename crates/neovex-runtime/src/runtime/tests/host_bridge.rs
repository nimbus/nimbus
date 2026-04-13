use super::*;

#[tokio::test]
async fn runtime_async_ops_use_async_host_bridge_path() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db.get("messages", "doc-1");
  return { value };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(AsyncOnlyHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:get".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("async host bridge should satisfy async op");

    assert_eq!(result, serde_json::json!({ "value": "async-host" }));
}

#[tokio::test]
async fn runtime_exposes_verified_identity_extension_separately_from_convex_identity() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const request = arguments[0];
  const ctx = globalThis.__neovexCreateContext({ request });
  return {
    user: await ctx.auth.getUserIdentity(),
    verified: await ctx.auth.getVerifiedIdentity(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "auth:whoami".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: Some(InvocationAuth::with_identities(
                    RuntimeUserIdentity {
                        token_identifier: "https://issuer.example.com|user-123".to_string(),
                        subject: "user-123".to_string(),
                        issuer: "https://issuer.example.com".to_string(),
                        name: None,
                        given_name: None,
                        family_name: None,
                        nickname: None,
                        preferred_username: None,
                        profile_url: None,
                        picture_url: None,
                        email: None,
                        email_verified: None,
                        gender: None,
                        birthday: None,
                        timezone: None,
                        language: None,
                        phone_number: None,
                        phone_number_verified: None,
                        address: None,
                        updated_at: None,
                        custom_claims: serde_json::from_value(serde_json::json!({
                            "email": "ada@example.com",
                            "given_name": "Ada",
                            "updated_at": 1710000000,
                            "address.formatted": "123 Analytical Engine Way",
                            "role": "admin"
                        }))
                        .expect("custom jwt compat claims should parse"),
                    },
                    VerifiedUserIdentity {
                        kind: VerifiedUserIdentityKind::CustomJwt,
                        token_identifier: "https://issuer.example.com|user-123".to_string(),
                        subject: "user-123".to_string(),
                        issuer: "https://issuer.example.com".to_string(),
                        name: Some("Ada Lovelace".to_string()),
                        given_name: Some("Ada".to_string()),
                        family_name: None,
                        nickname: None,
                        preferred_username: None,
                        profile_url: None,
                        picture_url: None,
                        email: Some("ada@example.com".to_string()),
                        email_verified: None,
                        gender: None,
                        birthday: None,
                        timezone: None,
                        language: None,
                        phone_number: None,
                        phone_number_verified: None,
                        address: Some("123 Analytical Engine Way".to_string()),
                        updated_at: Some("1710000000".to_string()),
                        custom_claims: serde_json::from_value(serde_json::json!({
                            "role": "admin"
                        }))
                        .expect("verified custom claims should parse"),
                    },
                    false,
                )),
                services: Default::default(),
            },
        )
        .await
        .expect("runtime should expose both auth views");

    assert_eq!(
        result,
        serde_json::json!({
            "user": {
                "tokenIdentifier": "https://issuer.example.com|user-123",
                "subject": "user-123",
                "issuer": "https://issuer.example.com",
                "email": "ada@example.com",
                "given_name": "Ada",
                "updated_at": 1710000000,
                "address.formatted": "123 Analytical Engine Way",
                "role": "admin"
            },
            "verified": {
                "kind": "custom_jwt",
                "tokenIdentifier": "https://issuer.example.com|user-123",
                "subject": "user-123",
                "issuer": "https://issuer.example.com",
                "name": "Ada Lovelace",
                "givenName": "Ada",
                "email": "ada@example.com",
                "address": "123 Analytical Engine Way",
                "updatedAt": "1710000000",
                "role": "admin"
            }
        })
    );
}

#[tokio::test]
async fn runtime_exposes_service_bindings_from_invocation_request() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({ request });
  return {
    db: ctx.services.db,
    metrics: ctx.services.metrics,
    names: Object.keys(ctx.services).sort(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:describe".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: serde_json::from_value(serde_json::json!({
                    "db": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp",
                        "endpoints": {
                            "postgres": {
                                "host": "127.0.0.1",
                                "port": 15432,
                                "protocol": "tcp"
                            },
                            "health": {
                                "host": "127.0.0.1",
                                "port": 18080,
                                "protocol": "http"
                            }
                        }
                    },
                    "metrics": {
                        "host": "127.0.0.1",
                        "port": 19090,
                        "protocol": "http"
                    }
                }))
                .expect("service bindings should deserialize"),
            },
        )
        .await
        .expect("runtime should expose request service bindings");

    assert_eq!(
        result,
        serde_json::json!({
            "db": {
                "host": "127.0.0.1",
                "port": 15432,
                "protocol": "tcp",
                "endpoints": {
                    "health": {
                        "host": "127.0.0.1",
                        "port": 18080,
                        "protocol": "http"
                    },
                    "postgres": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp"
                    }
                }
            },
            "metrics": {
                "host": "127.0.0.1",
                "port": 19090,
                "protocol": "http",
                "endpoints": {}
            },
            "names": ["db", "metrics"],
        })
    );
}

#[tokio::test]
async fn runtime_lazily_looks_up_missing_service_bindings_and_caches_them() {
    #[derive(Default)]
    struct LazyServiceLookupHost {
        calls: std::sync::Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for LazyServiceLookupHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.calls
                .lock()
                .expect("lazy service lookup host lock should not be poisoned")
                .push(request.clone());
            match request.operation {
                HostCallOperation::CtxServiceLookup => Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp",
                        "endpoints": {
                            "postgres": {
                                "host": "127.0.0.1",
                                "port": 15432,
                                "protocol": "tcp"
                            }
                        }
                    },
                })),
                other => Err(NeovexRuntimeError::Contract(format!(
                    "unexpected sync host op during lazy service lookup test: {other}"
                ))),
            }
        }
    }

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({ request });
  const namesBefore = Object.keys(ctx.services).sort();
  const first = ctx.services.db;
  const second = ctx.services.db;
  return {
    namesBefore,
    namesAfter: Object.keys(ctx.services).sort(),
    sameReference: first === second,
    db: second,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(LazyServiceLookupHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:lazy".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("runtime should lazily resolve missing service bindings");

    assert_eq!(
        result,
        serde_json::json!({
            "namesBefore": [],
            "namesAfter": ["db"],
            "sameReference": true,
            "db": {
                "host": "127.0.0.1",
                "port": 15432,
                "protocol": "tcp",
                "endpoints": {
                    "postgres": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp"
                    }
                }
            }
        })
    );

    let calls = host
        .calls
        .lock()
        .expect("lazy service lookup host lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 1, "missing service should be resolved once");
    assert_eq!(calls[0].operation, HostCallOperation::CtxServiceLookup);
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "service_name": "db",
            "session_id": "session-1",
        })
    );
}

#[tokio::test]
async fn runtime_query_builder_setup_uses_sync_host_bridge_path() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const builder = ctx
    .db
    .query("messages")
    .withIndex("by_author", (q) => q.eq(q.field("author"), "Ada"))
    .filter((q) => q.eq(q.field("channel"), "general"))
    .order("desc");
  return { builderId: builder.__builderId };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(SyncOnlyHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
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
        .expect("sync host bridge should satisfy query builder setup");

    assert_eq!(result, serde_json::json!({ "builderId": "builder-1" }));
    let calls = host
        .calls
        .lock()
        .expect("sync-only host lock should not be poisoned")
        .clone();
    assert_eq!(
        calls
            .into_iter()
            .map(|call| call.operation)
            .collect::<Vec<_>>(),
        vec![
            HostCallOperation::CtxDbQueryStart,
            HostCallOperation::CtxDbQueryWithIndex,
            HostCallOperation::CtxDbQueryFilter,
            HostCallOperation::CtxDbQueryOrder,
        ]
    );
}

#[tokio::test]
async fn runtime_async_write_and_scheduler_ops_use_async_host_bridge_path() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const insert = await ctx.db.insert("messages", { body: "hello" });
  const patch = await ctx.db.patch("messages", "doc-1", { body: "updated" });
  const deletion = await ctx.db.delete("messages", "doc-1");
  const runAfter = await ctx.scheduler.runAfter(
    100,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled" },
  );
  const runAt = await ctx.scheduler.runAt(
    500,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled-at" },
  );
  const cancel = await ctx.scheduler.cancel("job-1");
  return { insert, patch, deletion, runAfter, runAt, cancel };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: "messages:write".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("async host bridge should satisfy write and scheduler ops");

    assert_eq!(
        result,
        serde_json::json!({
            "insert": {
                "operation": "ctx_db_insert",
                "payload": {
                    "table": "messages",
                    "fields": { "body": "hello" },
                    "session_id": "session-1",
                }
            },
            "patch": {
                "operation": "ctx_db_patch",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "patch": { "body": "updated" },
                    "session_id": "session-1",
                }
            },
            "deletion": {
                "operation": "ctx_db_delete",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "session-1",
                }
            },
            "runAfter": {
                "operation": "ctx_scheduler_run_after",
                "payload": {
                    "delay_ms": 100,
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "scheduled" },
                    "session_id": "session-1",
                }
            },
            "runAt": {
                "operation": "ctx_scheduler_run_at",
                "payload": {
                    "timestamp_ms": 500,
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "scheduled-at" },
                    "session_id": "session-1",
                }
            },
            "cancel": {
                "operation": "ctx_scheduler_cancel",
                "payload": {
                    "job_id": "job-1",
                    "session_id": "session-1",
                }
            }
        })
    );
}

#[tokio::test]
async fn runtime_query_paginate_uses_async_host_bridge_and_returns_official_shape() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 2,
    cursor: null,
    maximumRowsRead: 32,
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(PaginateHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:listPage".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("paginate query should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "page": [
                { "body": "hello" }
            ],
            "isDone": true,
            "continueCursor": "",
            "splitCursor": null,
            "pageStatus": null,
        })
    );

    let sync_calls = host
        .sync_calls
        .lock()
        .expect("paginate host sync lock should not be poisoned")
        .clone();
    assert_eq!(sync_calls.len(), 1);
    assert_eq!(sync_calls[0].operation, HostCallOperation::CtxDbQueryStart);

    let async_calls = host
        .async_calls
        .lock()
        .expect("paginate host async lock should not be poisoned")
        .clone();
    assert_eq!(async_calls.len(), 1);
    assert_eq!(
        async_calls[0].operation,
        HostCallOperation::CtxDbQueryPaginate
    );
    assert_eq!(
        async_calls[0].payload,
        serde_json::json!({
            "builder_id": "builder-1",
            "page_size": 2,
            "cursor": Value::Null,
            "session_id": "session-1",
        })
    );
}

#[tokio::test]
async fn runtime_query_paginate_treats_full_page_with_cursor_as_not_done() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 1,
    cursor: "after-alpha",
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(PaginateContinuationHost);
    let runtime =
        NeovexRuntime::with_policy(host, run_to_completion_snapshot_runtime_test_policy());
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:listPage".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("paginate query should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "page": [
                { "body": "beta" }
            ],
            "isDone": false,
            "continueCursor": "after-beta",
            "splitCursor": null,
            "pageStatus": null,
        })
    );
}

#[tokio::test]
async fn runtime_same_isolate_nested_entry_uses_sync_host_bridge_path() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvokeNamedLocal = async function () {
  return "local-ok";
};

globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(SyncOnlyHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:outer".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("same-isolate nested entry should succeed");

    assert_eq!(result, serde_json::json!("local-ok"));
    let calls = host
        .calls
        .lock()
        .expect("sync-only host lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].operation,
        HostCallOperation::CtxRuntimeEnterNestedCall
    );
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "name": "messages:list",
            "visibility": "public",
            "session_id": "session-1",
        })
    );
}

#[tokio::test]
async fn runtime_async_ctx_run_ops_use_async_host_bridge_path() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const query = await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
  const mutation = await ctx.runMutation(
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "hello" },
  );
  const action = await ctx.runAction(
    { name: "messages:sendViaAction", visibility: "public" },
    { body: "wave" },
  );
  return { query, mutation, action };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "messages:outer".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("async host bridge should satisfy ctx.run* fallback ops");

    assert_eq!(
        result,
        serde_json::json!({
            "query": {
                "operation": "ctx_run_query",
                "payload": {
                    "name": "messages:list",
                    "visibility": "public",
                    "args": { "author": "Ada" },
                    "session_id": "session-1",
                }
            },
            "mutation": {
                "operation": "ctx_run_mutation",
                "payload": {
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "hello" },
                    "session_id": "session-1",
                }
            },
            "action": {
                "operation": "ctx_run_action",
                "payload": {
                    "name": "messages:sendViaAction",
                    "visibility": "public",
                    "args": { "body": "wave" },
                    "session_id": "session-1",
                }
            }
        })
    );
}
