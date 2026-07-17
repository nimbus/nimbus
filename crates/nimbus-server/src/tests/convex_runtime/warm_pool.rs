use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_testing::{
    EngineFixture, HttpApiFixture, ServerFixture, cooperative_warm_pool_runtime_test_limits,
};
use reqwest::StatusCode;
use serde_json::json;
use tokio::sync::Barrier;

use crate::tests::{
    assert_convex_anonymous_query_refused, convex_registry_with_routes_and_bundle,
    convex_team_bearer, router_for_convex_team, wait_for_runtime_metrics,
};

const CONCURRENT_MUTATIONS: usize = 4;

fn same_tenant_warm_pool_registry() -> crate::ConvexRegistry {
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = CONCURRENT_MUTATIONS;
    limits.worker_threads = CONCURRENT_MUTATIONS;
    limits.max_active_top_level_invocations_per_tenant = CONCURRENT_MUTATIONS;
    limits.max_in_flight_top_level_invocations_per_tenant = CONCURRENT_MUTATIONS;
    limits.max_queued_top_level_invocations_per_tenant = CONCURRENT_MUTATIONS;
    limits.execution_timeout = std::time::Duration::from_secs(30);
    limits.system_timeout = std::time::Duration::from_secs(30);

    convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:concurrentInsert",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { ordinal }) => { await new Promise((resolve) => setTimeout(resolve, 750)); return await ctx.db.insert(\"messages\", { ordinal }); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:concurrentInsert", {
    name: "messages:concurrentInsert",
    kind: "mutation",
    visibility: "public",
    runtime_handler: async (ctx, { ordinal }) => {
      await new Promise((resolve) => setTimeout(resolve, 750));
      return await ctx.db.insert("messages", { ordinal });
    },
  }],
]);

globalThis.__nimbusInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }
  try {
    const value = await definition.runtime_handler(
      globalThis.__nimbusCreateContext({
        hostCallSessionId: `${request.kind}:${request.function_name}`,
        request,
      }),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    )
    .with_runtime_limits(limits)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_tenant_concurrent_mutations_use_warm_pool_without_isolate_lifecycle_crash() {
    let registry = same_tenant_warm_pool_registry();
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server =
        ServerFixture::start(router_for_convex_team(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_convex_anonymous_query_refused(&server, "demo").await;
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let start = Arc::new(Barrier::new(CONCURRENT_MUTATIONS + 1));
    let mut requests = Vec::new();
    for ordinal in 0..CONCURRENT_MUTATIONS {
        let start = start.clone();
        let client = server.client().clone();
        let url = server.http_url("/convex/demo/mutation");
        let bearer = convex_team_bearer();
        requests.push(tokio::spawn(async move {
            start.wait().await;
            client
                .post(url)
                .header(reqwest::header::AUTHORIZATION, bearer)
                .json(&json!({
                    "name": "messages:concurrentInsert",
                    "args": { "ordinal": ordinal },
                }))
                .send()
                .await
                .expect("concurrent mutation request should send")
        }));
    }
    start.wait().await;

    let overlapping = wait_for_runtime_metrics(
        &registry,
        "same-tenant warm-pool mutations to overlap",
        |metrics| {
            metrics.active_runtime_instances >= 2
                && metrics
                    .tenants
                    .get("demo")
                    .is_some_and(|tenant| tenant.active_runtime_instances >= 2)
        },
    )
    .await;
    assert!(overlapping.active_runtime_instances >= 2);
    assert!(
        overlapping
            .tenants
            .get("demo")
            .is_some_and(|tenant| tenant.active_runtime_instances >= 2)
    );

    for request in requests {
        let response = request.await.expect("concurrent mutation task should join");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .json::<serde_json::Value>()
                .await
                .expect("concurrent mutation response should parse")
                .as_str()
                .is_some(),
            "runtime-backed insert should return a document id"
        );
    }

    let after_burst = registry.runtime_metrics_snapshot();
    assert!(
        after_burst.warm_pool_misses >= 2,
        "overlapping empty-pool checkouts must create multiple runtimes: {after_burst:?}"
    );
    assert!(
        after_burst.retained_runtime_pool_entries >= 2,
        "concurrent runtimes should return to the warm pool: {after_burst:?}"
    );

    let reuse = api
        .convex_named_mutation(
            "demo",
            "messages:concurrentInsert",
            json!({ "ordinal": CONCURRENT_MUTATIONS }),
        )
        .await;
    assert_eq!(reuse.status(), StatusCode::OK);
    let final_metrics = registry.runtime_metrics_snapshot();
    assert!(
        final_metrics.warm_pool_hits >= 1,
        "post-burst mutation should reuse a retained warm isolate: {final_metrics:?}"
    );
    assert_eq!(
        final_metrics.runtime_pool_hits,
        final_metrics.warm_pool_hits
    );
    assert_eq!(
        final_metrics.runtime_pool_misses,
        final_metrics.warm_pool_misses
    );

    let diagnostics = fixture
        .engine()
        .tenant_engine_diagnostics(&TenantId::new("demo").expect("tenant id"))
        .expect("tenant diagnostics should load");
    assert_eq!(diagnostics.mutation_isolate_admission.concurrent_count, 0);
    assert!(diagnostics.mutation_isolate_admission.ceiling >= CONCURRENT_MUTATIONS);
    assert!(diagnostics.mutation_isolate_admission.max_concurrent_count >= 2);
    assert!(
        diagnostics.mutation_isolate_admission.admitted_count >= (CONCURRENT_MUTATIONS + 1) as u64,
        "OCC retries may acquire additional mutation-isolate seats: {diagnostics:?}"
    );
    assert_eq!(diagnostics.mutation_isolate_admission.shed_count, 0);

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = listed
        .json::<serde_json::Value>()
        .await
        .expect("inserted documents should parse");
    assert_eq!(
        listed["data"].as_array().map(Vec::len),
        Some(CONCURRENT_MUTATIONS + 1)
    );
}
