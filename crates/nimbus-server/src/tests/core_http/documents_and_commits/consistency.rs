use super::*;

#[tokio::test]
async fn tenant_consistency_route_returns_green_report_for_live_state() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for rank in [1, 2, 3] {
        let response = api
            .insert_document("demo", "tasks", serde_json::json!({ "rank": rank }))
            .await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let report = api
        .tenant_consistency_report("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("consistency report should parse");
    assert_eq!(report["ok"], serde_json::json!(true));
    assert_eq!(report["mismatches"], serde_json::json!([]));
    assert_eq!(
        report["authoritative"]["document_count"],
        serde_json::json!(3)
    );
    assert_eq!(
        report["authoritative"]["digest"],
        report["shadow"]["digest"]
    );
    assert_eq!(
        report["authoritative"]["digest"],
        report["embedded_replica"]["digest"]
    );
    assert_eq!(
        report["bootstrap"]["bootstrap_cut_sequence"],
        report["authoritative"]["durable_head"]
    );
}

#[tokio::test]
async fn embedded_replica_matches_server_results_and_catches_up_after_http_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for (title, rank) in [("alpha", 1), ("beta", 2)] {
        assert_eq!(
            api.insert_document(
                "demo",
                "tasks",
                serde_json::json!({ "title": title, "rank": rank }),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");

    let ordered_query = nimbus_core::Query {
        table: nimbus_core::TableName::new("tasks").expect("table name should build"),
        filters: Vec::new(),
        order: Some(nimbus_core::OrderBy {
            field: "rank".to_string(),
            direction: nimbus_core::OrderDirection::Asc,
        }),
        limit: None,
    };
    let server_query = api
        .query_documents(
            "demo",
            serde_json::to_value(&ordered_query).expect("query should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("server query should parse");
    let replica_query = replica
        .query_documents(&ordered_query)
        .expect("replica query should succeed")
        .into_iter()
        .map(|document| document.to_json())
        .collect::<Vec<_>>();
    assert_eq!(server_query["data"], serde_json::json!(replica_query));

    let paginated = nimbus_core::PaginatedQuery {
        query: ordered_query.clone(),
        page_size: 1,
        after: None,
    };
    let server_page = api
        .query_documents_paginated(
            "demo",
            serde_json::to_value(&paginated).expect("page should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("server page should parse");
    let replica_page = replica
        .paginate_documents(&paginated)
        .expect("replica page should succeed");
    assert_eq!(
        server_page,
        serde_json::to_value(&replica_page).expect("replica page should serialize")
    );

    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            serde_json::json!({ "title": "gamma", "rank": 3 }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should succeed");

    let updated_server_query = api
        .query_documents(
            "demo",
            serde_json::to_value(&ordered_query).expect("query should serialize"),
        )
        .await
        .json::<serde_json::Value>()
        .await
        .expect("updated server query should parse");
    let updated_replica_query = replica
        .query_documents(&ordered_query)
        .expect("updated replica query should succeed")
        .into_iter()
        .map(|document| document.to_json())
        .collect::<Vec<_>>();
    assert_eq!(
        updated_server_query["data"],
        serde_json::json!(updated_replica_query)
    );
}
