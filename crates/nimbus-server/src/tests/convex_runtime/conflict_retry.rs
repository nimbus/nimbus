use std::time::Duration;

use nimbus_core::{DocumentId, PrincipalContext, SequenceNumber, TableName, TenantId};
use nimbus_engine::{Engine, commit_fault_labels};
use nimbus_testing::{EngineFixture, HttpApiFixture, ServerFixture};
use reqwest::StatusCode;
use serde_json::json;

use crate::tests::{
    convex_registry_with_routes_and_bundle, convex_team_bearer, router_for_convex_team,
};

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

fn conflict_retry_registry() -> crate::ConvexRegistry {
    convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "counters:incrementA",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, args) => increment(ctx, args)"
            },
            {
                "name": "counters:incrementB",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, args) => increment(ctx, args)"
            }
        ]),
        json!([]),
        Some(
            r#"
async function increment(ctx, { id }) {
  const counter = await ctx.db.get(id);
  const count = counter.count + 1;
  await ctx.db.patch(id, { count });
  return count;
}

const handlers = new Map([
  ["counters:incrementA", increment],
  ["counters:incrementB", increment],
]);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const value = await handlers.get(request.function_name)(
      globalThis.__nimbusCreateContext({
        hostCallSessionId: `${request.kind}:${request.function_name}`,
        request,
      }),
      request.args ?? {},
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
}

fn pin_retry_policy(max_attempts: usize) {
    // SAFETY: nextest runs each test in a separate process, so these runtime
    // policy knobs cannot race another test's environment reads.
    unsafe {
        std::env::set_var("NIMBUS_MUTATION_OCC_MAX_RETRIES", max_attempts.to_string());
        std::env::set_var("NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS", "1");
        std::env::set_var("NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS", "1");
    }
}

async fn seed_counter(api: &HttpApiFixture<'_>) -> String {
    let response = api
        .insert_document("demo", "counters", json!({ "count": 0 }))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response
        .json::<serde_json::Value>()
        .await
        .expect("counter insert response should parse")["id"]
        .as_str()
        .expect("counter insert should return an id")
        .to_string()
}

async fn flush_seed_observers(engine: &Engine) {
    engine
        .flush_committed_mutation_observers_for_testing(
            &TenantId::new("demo").expect("tenant id should build"),
        )
        .await
        .expect("seed mutation observers should drain before fault injection");
}

async fn invoke_mutation(
    server: &ServerFixture,
    name: &'static str,
    document_id: String,
) -> reqwest::Response {
    let scoped_document_id = format!("counters:{document_id}");
    server
        .client()
        .post(server.http_url("/convex/demo/mutation"))
        .header("authorization", convex_team_bearer())
        .json(&json!({ "name": name, "args": { "id": scoped_document_id } }))
        .send()
        .await
        .expect("Convex mutation request should send")
}

#[tokio::test(flavor = "multi_thread")]
async fn forced_conflict_integration_test() {
    pin_retry_policy(4);
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(
        engine.clone(),
        conflict_retry_registry(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let document_id = seed_counter(&api).await;
    flush_seed_observers(&engine).await;
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(commit_fault_labels::PRE_ASSIGN);

    let first_client = server.client().clone();
    let first_url = server.http_url("/convex/demo/mutation");
    let first_id = format!("counters:{document_id}");
    let first = tokio::spawn(async move {
        first_client
            .post(first_url)
            .header("authorization", convex_team_bearer())
            .json(&json!({
                "name": "counters:incrementA",
                "args": { "id": first_id },
            }))
            .send()
            .await
            .expect("first conflicting mutation should send")
    });
    let wait_faults = faults.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_faults.wait_until_entered(commit_fault_labels::PRE_ASSIGN, WAIT_TIMEOUT)
        })
        .await
        .expect("commit pause waiter should join"),
        "first mutation should reach the pre-assign pause"
    );

    let second = invoke_mutation(&server, "counters:incrementB", document_id.clone()).await;
    let second_status = second.status();
    let second_body = second
        .json::<serde_json::Value>()
        .await
        .expect("second mutation response should parse");
    assert_eq!(second_status, StatusCode::OK, "{second_body}");
    faults.release(commit_fault_labels::PRE_ASSIGN);

    let first = first.await.expect("first mutation task should join");
    let first_status = first.status();
    let first_body = first
        .json::<serde_json::Value>()
        .await
        .expect("first mutation response should parse");
    assert_eq!(first_status, StatusCode::OK, "{first_body}");

    let stored = api.get_document("demo", "counters", &document_id).await;
    assert_eq!(stored.status(), StatusCode::OK);
    let stored = stored
        .json::<serde_json::Value>()
        .await
        .expect("stored counter should parse");
    assert_eq!(stored["document"]["count"], json!(2));
    let diagnostics = engine
        .tenant_engine_diagnostics(&TenantId::new("demo").expect("tenant id should build"))
        .expect("tenant diagnostics should load");
    assert!(diagnostics.commit_phases.mutation_conflict_retries_total > 0);
    assert_eq!(
        diagnostics.commit_phases.mutation_conflict_exhausted_total,
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_exhaustion_test() {
    pin_retry_policy(3);
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(
        engine.clone(),
        conflict_retry_registry(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let document_id = seed_counter(&api).await;
    flush_seed_observers(&engine).await;
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let applied_head = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("tenant diagnostics should load")
        .mutation_journal
        .applied_head;
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject_retryable_conflicts(commit_fault_labels::PRE_ASSIGN, 3, Some(applied_head));

    let response = invoke_mutation(&server, "counters:incrementA", document_id).await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("exhausted conflict response should parse");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], json!("OCC"));
    assert_eq!(
        body["error"]["detail"]["shortName"],
        json!("OptimisticConcurrencyControlFailure")
    );
    assert_eq!(body["error"]["detail"]["retryability"], json!("retryable"));
    assert_eq!(body["error"]["detail"]["attempts"], json!(3));
    assert_eq!(
        body["error"]["detail"]["conflictingSequence"],
        json!(applied_head.0)
    );
    assert_eq!(faults.hit_count(commit_fault_labels::PRE_ASSIGN), 3);
    let diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("tenant diagnostics should load");
    assert_eq!(diagnostics.commit_phases.mutation_conflict_retries_total, 2);
    assert_eq!(
        diagnostics.commit_phases.mutation_conflict_exhausted_total,
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wait_before_retry_test() {
    pin_retry_policy(3);
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(
        engine.clone(),
        conflict_retry_registry(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let document_id = seed_counter(&api).await;
    flush_seed_observers(&engine).await;
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("tenant diagnostics should load");
    let conflicting_sequence = SequenceNumber(before.mutation_journal.applied_head.0 + 1);

    let writer = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("writer execution unit should begin");
    writer
        .update_document(
            TableName::new("counters").expect("table should build"),
            DocumentId::from_key(document_id.clone()).expect("document id should parse"),
            serde_json::Map::from_iter([("writer".to_string(), json!(true))]),
        )
        .expect("writer update should stage");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(commit_fault_labels::DURABLE_BEFORE_PUBLISH);
    let writer_task = tokio::task::spawn_blocking(move || writer.commit());
    let wait_faults = faults.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_faults
                .wait_until_entered(commit_fault_labels::DURABLE_BEFORE_PUBLISH, WAIT_TIMEOUT)
        })
        .await
        .expect("durability pause waiter should join"),
        "writer should reach durable-before-publish"
    );
    let pre_assign_baseline = faults.hit_count(commit_fault_labels::PRE_ASSIGN);
    faults.inject_retryable_conflicts(
        commit_fault_labels::PRE_ASSIGN,
        1,
        Some(conflicting_sequence),
    );

    let mutation_client = server.client().clone();
    let mutation_url = server.http_url("/convex/demo/mutation");
    let mutation_id = format!("counters:{document_id}");
    let mutation_task = tokio::spawn(async move {
        mutation_client
            .post(mutation_url)
            .header("authorization", convex_team_bearer())
            .json(&json!({
                "name": "counters:incrementA",
                "args": { "id": mutation_id },
            }))
            .send()
            .await
            .expect("waiting mutation should send")
    });
    let wait_faults = faults.clone();
    let conflict_reached = tokio::task::spawn_blocking(move || {
        wait_faults.wait_until_hits(
            commit_fault_labels::PRE_ASSIGN,
            pre_assign_baseline + 1,
            WAIT_TIMEOUT,
        )
    })
    .await
    .expect("conflict hit waiter should join");
    if !conflict_reached {
        faults.release(commit_fault_labels::DURABLE_BEFORE_PUBLISH);
        writer_task
            .await
            .expect("writer task should join during failed setup")
            .expect("writer commit should succeed during failed setup");
        let response = mutation_task.await.expect("mutation task should join");
        panic!(
            "first mutation attempt did not consume the forced conflict; status={}",
            response.status()
        );
    }

    let blocked = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("blocked retry diagnostics should load");
    assert!(blocked.mutation_journal.applied_head < conflicting_sequence);
    assert_eq!(
        blocked.commit_phases.mutation_conflict_retries_total,
        before.commit_phases.mutation_conflict_retries_total
    );
    assert!(!mutation_task.is_finished(), "retry must still be waiting");

    faults.release(commit_fault_labels::DURABLE_BEFORE_PUBLISH);
    let writer_commit = writer_task
        .await
        .expect("writer task should join")
        .expect("writer commit should succeed")
        .expect("writer should produce a commit");
    assert_eq!(writer_commit.sequence, conflicting_sequence);
    let response = mutation_task.await.expect("mutation task should join");
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("waiting mutation response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("post-retry diagnostics should load");
    assert!(after.mutation_journal.applied_head >= conflicting_sequence);
    assert_eq!(
        faults.hit_count(commit_fault_labels::PRE_ASSIGN),
        pre_assign_baseline + 2
    );
    assert_eq!(
        after.commit_phases.mutation_conflict_retries_total,
        before.commit_phases.mutation_conflict_retries_total + 1
    );
}
