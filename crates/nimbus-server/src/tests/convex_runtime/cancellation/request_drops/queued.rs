use super::helpers::runtime_request_drop_registry;
use super::*;
use nimbus_core::Timestamp;

const QUEUED_REQUEST_DROP_CASE: DeterministicTestCase = DeterministicTestCase::new(
    "runtime-request-drop-queued",
    "run-to-completion-snapshot",
    "dropping queued runtime work cancels pressure cleanly and never starts the queued mutation",
);

const QUEUED_REQUEST_RECOVERY_CASE: DeterministicTestCase = DeterministicTestCase::new(
    "runtime-request-drop-queued-recovery",
    "run-to-completion-snapshot",
    "runtime recovers and serves new work after queued request-drop pressure clears",
);

fn queued_request_drop_runtime_limits() -> nimbus_runtime::RuntimeLimits {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_active_top_level_invocations_per_tenant = 1;
    limits.max_in_flight_top_level_invocations_per_tenant = 1;
    limits.max_queued_top_level_invocations_per_tenant = 1;
    limits
}

fn queued_request_drop_host_budget() -> nimbus_runtime::RuntimeHostResourceBudget {
    nimbus_runtime::RuntimeHostResourceBudget {
        host_millicpus: 2_000,
        system_reserved_millicpus: 0,
        nimbus_control_plane_reserved_millicpus: 0,
        runtime_hard_ceiling_millicpus: None,
        runtime_seat_millicpus: std::num::NonZeroU32::new(1_000)
            .expect("one runtime seat should be nonzero"),
    }
}

fn router_for_queued_request_drop(engine: Arc<Engine>, registry: ConvexRegistry) -> axum::Router {
    // #41: the application-convex team gate now guards every `/convex/<silo>`
    // route, so this specialized queued-drop router must install the same test
    // verifier + team tenancy as `router_for_convex_team_for` (so the team-bound
    // bearer is admitted) while keeping its own host budget + nominal pressure
    // source. Going through `build_router` instead binds the *production* convex
    // verifier, which rejects the test bearer and would silently refuse the
    // blocking query before it can start — so build the config directly.
    crate::router::RouterBuildConfig::core(engine)
        .with_application_auth_verifier(Arc::new(crate::tests::StaticConvexTeamVerifier))
        .with_convex(registry)
        .with_convex_tenancy(convex_team_tenancy_for("demo"))
        .with_runtime_host_resource_budget(queued_request_drop_host_budget())
        .with_runtime_host_pressure_source(Arc::new(
            nimbus_runtime::NominalRuntimeHostPressureSource,
        ))
        .build()
}

#[tokio::test]
async fn dropped_queued_runtime_request_never_starts_mutation() {
    let registry = runtime_request_drop_registry(json!([
        {
            "name": "messages:block",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => { while (true) {} }"
        },
        {
            "name": "messages:insertQueued",
            "kind": "mutation",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
        }
    ]))
    .with_runtime_limits(queued_request_drop_runtime_limits());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_queued_request_drop(
        service.clone(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_DROP_CASE,
        "blocking runtime query to start",
        |metrics| {
            metrics.active_runtime_instances == 1 && metrics.worker_dispatched_invocations == 1
        },
    )
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    let queued_snapshot = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_DROP_CASE,
        "queued runtime mutation to be admitted at the executor queue",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics.admission_decisions >= 2
                && metrics.worker_dispatched_invocations == 1
                && metrics.started_invocations == 1
                && metrics
                    .recent_request_correlations
                    .iter()
                    .any(|correlation| {
                        correlation.function_name == "messages:insertQueued"
                            && correlation
                                .server_request_id
                                .starts_with("convex-mutation-")
                    })
        },
    )
    .await;
    assert_eq!(queued_snapshot.active_runtime_instances, 1);
    assert_eq!(queued_snapshot.worker_dispatched_invocations, 1);
    assert_eq!(queued_snapshot.started_invocations, 1);
    assert_eq!(queued_snapshot.queued_canceled_invocations, 0);

    drop(queued_mutation);
    let queued_canceled = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_DROP_CASE,
        "queued runtime mutation cancellation before worker dispatch",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics.worker_dispatched_invocations == 1
                && metrics.queued_canceled_invocations == 1
                && metrics.in_flight_canceled_invocations == 0
        },
    )
    .await;
    assert_eq!(queued_canceled.canceled_invocations, 1);
    assert_eq!(queued_canceled.disconnect_canceled_invocations, 1);

    drop(blocker);

    let metrics = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_DROP_CASE,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_runtime_instances == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.queued_canceled_invocations, 1);
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.disconnect_canceled_invocations, 2);
    assert_eq!(metrics.explicit_canceled_invocations, 0);
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 0);
    assert_eq!(metrics.runtime_pool_replacements, 1);
    let tenant_metrics = metrics
        .tenants
        .get("demo")
        .expect("tenant runtime metrics should be present");
    assert_eq!(tenant_metrics.started_invocations, 1);
    assert_eq!(tenant_metrics.completed_invocations, 1);
    assert_eq!(tenant_metrics.queued_canceled_invocations, 1);
    assert_eq!(tenant_metrics.in_flight_canceled_invocations, 1);
    assert_eq!(tenant_metrics.disconnect_canceled_invocations, 2);
    assert_eq!(tenant_metrics.explicit_canceled_invocations, 0);
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:block"
                    && correlation.server_request_id.starts_with("convex-query-")
            })
    );
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:insertQueued"
                    && correlation
                        .server_request_id
                        .starts_with("convex-mutation-")
            })
    );

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing queued mutation table should succeed");
    assert!(documents.is_empty(), "queued mutation should never start");
}

#[tokio::test]
async fn dropped_queued_runtime_request_recovers_and_serves_new_work_after_pressure_clears() {
    let registry = runtime_request_drop_registry(json!([
        {
            "name": "messages:block",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => { while (true) {} }"
        },
        {
            "name": "messages:insertQueued",
            "kind": "mutation",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
        }
    ]))
    .with_runtime_limits(queued_request_drop_runtime_limits());
    let harness =
        DeterministicHarness::scenario("runtime-request-drop-recovery", 75, Timestamp(75_000));
    let fixture = EngineFixture::new_with_harness(harness.clone(), |path, harness| {
        Engine::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_queued_request_drop(
        service.clone(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_RECOVERY_CASE,
        "blocking runtime query to start",
        |metrics| {
            metrics.active_runtime_instances == 1 && metrics.worker_dispatched_invocations == 1
        },
    )
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    let queued_snapshot = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_RECOVERY_CASE,
        "queued runtime mutation recovery request to be admitted at the executor queue",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics.admission_decisions >= 2
                && metrics.worker_dispatched_invocations == 1
                && metrics.started_invocations == 1
                && metrics
                    .recent_request_correlations
                    .iter()
                    .any(|correlation| {
                        correlation.function_name == "messages:insertQueued"
                            && correlation
                                .server_request_id
                                .starts_with("convex-mutation-")
                    })
        },
    )
    .await;
    assert_eq!(queued_snapshot.active_runtime_instances, 1);
    assert_eq!(queued_snapshot.worker_dispatched_invocations, 1);
    assert_eq!(queued_snapshot.started_invocations, 1);
    assert_eq!(queued_snapshot.queued_canceled_invocations, 0);

    drop(queued_mutation);
    let queued_canceled = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_RECOVERY_CASE,
        "queued runtime mutation recovery cancellation before worker dispatch",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics.worker_dispatched_invocations == 1
                && metrics.queued_canceled_invocations == 1
                && metrics.in_flight_canceled_invocations == 0
        },
    )
    .await;
    assert_eq!(queued_canceled.canceled_invocations, 1);
    assert_eq!(queued_canceled.disconnect_canceled_invocations, 1);

    drop(blocker);

    let canceled = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_RECOVERY_CASE,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_runtime_instances == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(canceled.worker_dispatched_invocations, 1);
    assert_eq!(canceled.queued_canceled_invocations, 1);
    assert_eq!(canceled.in_flight_canceled_invocations, 1);

    let recovery_response = api
        .convex_named_mutation(
            "demo",
            "messages:insertQueued",
            json!({ "body": "after-heal" }),
        )
        .await;
    assert_eq!(recovery_response.status(), StatusCode::OK);

    let recovered = wait_for_runtime_metrics_case(
        &registry,
        QUEUED_REQUEST_RECOVERY_CASE,
        "runtime recovery after queued request drop",
        |metrics| {
            metrics.active_runtime_instances == 0
                && metrics.worker_dispatched_invocations == 2
                && metrics.started_invocations == 2
                && metrics.completed_invocations == 2
        },
    )
    .await;
    assert_eq!(
        recovered.runtime_pool_hits + recovered.runtime_pool_misses,
        2,
        "two started invocations should account for two pool outcomes"
    );
    assert_eq!(recovered.queued_canceled_invocations, 1);
    assert_eq!(recovered.in_flight_canceled_invocations, 1);
    assert_eq!(recovered.disconnect_canceled_invocations, 2);
    assert_eq!(recovered.runtime_pool_replacements, 1);

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing recovered mutation table should succeed");
    assert_eq!(
        documents.len(),
        1,
        "recovery mutation should persist exactly once"
    );
    assert_eq!(documents[0].fields.get("body"), Some(&json!("after-heal")));
}
