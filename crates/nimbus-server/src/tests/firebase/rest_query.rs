use super::*;

#[tokio::test]
async fn firebase_batch_write_reports_partial_success_and_rejects_duplicates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchWrite"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {
                                "name": { "stringValue": "San Francisco" }
                            }
                        }
                    },
                    {
                        "delete": "projects/demo/databases/(default)/documents/cities/LA",
                        "currentDocument": { "exists": true }
                    }
                ],
                "labels": {
                    "sdk": "rest"
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("BatchWrite should send");
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("BatchWrite response should deserialize");
    assert_eq!(body["writeResults"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["status"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["status"][0]["code"], json!(0));
    assert!(body["writeResults"][0]["updateTime"].as_str().is_some());
    assert_eq!(body["status"][1]["code"], json!(Code::NotFound as i32));
    assert!(body["writeResults"][1]["updateTime"].is_null());

    let locator = crate::adapters::firebase::locator_for_document_path(
        &DocumentPath::from_segments(["cities", "SF"]).expect("document path should parse"),
    )
    .expect("firebase locator should derive");
    let stored = service
        .get_document(&tenant_id, &locator.table, locator.id.clone())
        .expect("successful BatchWrite entries should persist");
    assert_eq!(stored.get_field("name"), Some(&json!("San Francisco")));

    let duplicate = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:batchWrite"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "database": "projects/demo/databases/(default)",
                "writes": [
                    {
                        "update": {
                            "name": "projects/demo/databases/(default)/documents/cities/SF",
                            "fields": {}
                        }
                    },
                    {
                        "delete": "projects/demo/databases/(default)/documents/cities/SF"
                    }
                ]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("duplicate BatchWrite should send");
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    let duplicate: serde_json::Value = duplicate
        .json()
        .await
        .expect("duplicate BatchWrite error should deserialize");
    assert_eq!(duplicate["error"]["status"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn firebase_run_query_executes_supported_subset_with_where_order_cursor_offset_limit_and_projection()
 {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let cities_table = crate::adapters::firebase::storage_table_for_collection_path(
        &CollectionPath::root(CollectionName::new("cities").expect("collection name should parse")),
    )
    .expect("cities collection table should derive");
    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: cities_table,
                fields: vec![
                    FieldSchema {
                        name: "name".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "state".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: vec![IndexDefinition {
                    name: "by_state_rank".to_string(),
                    fields: vec!["state".to_string(), "rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("cities schema should install");

    for (document_id, name, state, rank) in [
        ("alpha", "Alpha", "CA", 1),
        ("bravo", "Bravo", "CA", 2),
        ("charlie", "Charlie", "CA", 3),
        ("delta", "Delta", "CA", 4),
        ("echo", "Echo", "NV", 5),
    ] {
        let locator = crate::adapters::firebase::locator_for_document_path(
            &DocumentPath::from_segments(["cities", document_id])
                .expect("document path should parse"),
        )
        .expect("firebase locator should derive");
        service
            .insert_document_with_id(
                &tenant_id,
                locator.table.clone(),
                locator.id.clone(),
                serde_json::Map::from_iter([
                    ("name".to_string(), json!(name)),
                    ("state".to_string(), json!(state)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("seed document should insert");
    }

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "state" },
                            "op": "EQUAL",
                            "value": { "stringValue": "CA" }
                        }
                    },
                    "orderBy": [{
                        "field": { "fieldPath": "rank" },
                        "direction": "ASCENDING"
                    }],
                    "startAt": {
                        "values": [{ "integerValue": "2" }],
                        "before": true
                    },
                    "offset": 1,
                    "limit": 2,
                    "select": {
                        "fields": [
                            { "fieldPath": "__name__" },
                            { "fieldPath": "name" }
                        ]
                    }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase run query should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["skippedResults"], json!(1));
    assert_eq!(
        entries[0]["document"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/charlie")
    );
    assert_eq!(
        entries[1]["document"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/delta")
    );
    assert_eq!(
        entries[0]["document"]["fields"]["name"],
        json!({ "stringValue": "Charlie" })
    );
    assert!(
        entries[0]["document"]["fields"].get("state").is_none(),
        "projection should omit non-selected fields: {entries:?}"
    );
}

#[tokio::test]
async fn firebase_run_query_reports_missing_index_for_compound_query_without_matching_schema_index()
{
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    let cities_table = crate::adapters::firebase::storage_table_for_collection_path(
        &CollectionPath::root(CollectionName::new("cities").expect("collection name should parse")),
    )
    .expect("cities collection table should derive");
    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: cities_table,
                fields: vec![
                    FieldSchema {
                        name: "state".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: vec![
                    IndexDefinition {
                        name: "by_state".to_string(),
                        fields: vec!["state".to_string()],
                    },
                    IndexDefinition {
                        name: "by_rank".to_string(),
                        fields: vec!["rank".to_string()],
                    },
                ],
                access_policy: None,
            },
        )
        .expect("single-field cities schema should install");

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "state" },
                            "op": "EQUAL",
                            "value": { "stringValue": "CA" }
                        }
                    },
                    "orderBy": [{
                        "field": { "fieldPath": "rank" },
                        "direction": "ASCENDING"
                    }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase missing-index run query should send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("missing-index run query response should deserialize");
    assert_eq!(body["error"]["status"], json!("FAILED_PRECONDITION"));
    assert_eq!(
        body["error"]["details"][0]["@type"],
        json!("type.googleapis.com/google.rpc.PreconditionFailure")
    );
    assert!(
        body["error"]["message"].as_str().is_some_and(|message| {
            message.contains("requires an index")
                && message.contains("state")
                && message.contains("rank")
        }),
        "missing-index response should mention the required fields: {body:?}"
    );
}

#[tokio::test]
async fn firebase_run_query_returns_read_time_only_when_no_documents_match() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "state" },
                            "op": "EQUAL",
                            "value": { "stringValue": "WA" }
                        }
                    }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase empty run query should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(entries.len(), 1);
    assert!(entries[0]["document"].is_null());
    assert!(entries[0]["readTime"].as_str().is_some());
}

#[tokio::test]
async fn firebase_run_aggregation_query_counts_filtered_and_empty_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF"],
        [("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA"],
        [("state", json!("CA"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SEA"],
        [("state", json!("WA"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(
            server.http_url("/v1/projects/demo/databases/(default)/documents:runAggregationQuery"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredAggregationQuery": {
                    "structuredQuery": {
                        "from": [{ "collectionId": "cities" }],
                        "where": {
                            "fieldFilter": {
                                "field": { "fieldPath": "state" },
                                "op": "EQUAL",
                                "value": { "stringValue": "CA" }
                            }
                        }
                    },
                    "aggregations": [{ "count": {}, "alias": "total" }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase aggregation query should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["result"]["aggregateFields"]["total"],
        json!({ "integerValue": "2" })
    );
    assert!(entries[0]["readTime"].as_str().is_some());

    let empty_response = server
        .client()
        .post(
            server.http_url("/v1/projects/demo/databases/(default)/documents:runAggregationQuery"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredAggregationQuery": {
                    "structuredQuery": {
                        "from": [{ "collectionId": "cities" }],
                        "where": {
                            "fieldFilter": {
                                "field": { "fieldPath": "state" },
                                "op": "EQUAL",
                                "value": { "stringValue": "NV" }
                            }
                        }
                    },
                    "aggregations": [{ "count": {}, "alias": "total" }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase empty aggregation query should send");

    assert_eq!(empty_response.status(), StatusCode::OK);
    let empty_entries = response_json_lines(empty_response).await;
    assert_eq!(empty_entries.len(), 1);
    assert_eq!(
        empty_entries[0]["result"]["aggregateFields"]["total"],
        json!({ "integerValue": "0" })
    );
}

#[tokio::test]
async fn firebase_run_aggregation_query_under_parent_document_scopes_nested_collection_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "ggb"],
        [("name", json!("Golden Gate Bridge"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "ferry"],
        [("name", json!("Ferry Building"))],
    );
    seed_firebase_document(
        &service,
        &tenant_id,
        &["cities", "LA", "landmarks", "sign"],
        [("name", json!("Hollywood Sign"))],
    );
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url(
            "/v1/projects/demo/databases/(default)/documents/cities/SF:runAggregationQuery",
        ))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredAggregationQuery": {
                    "structuredQuery": {
                        "from": [{ "collectionId": "landmarks" }]
                    },
                    "aggregations": [{ "count": {}, "alias": "total" }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase parent aggregation query should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(
        entries[0]["result"]["aggregateFields"]["total"],
        json!({ "integerValue": "2" })
    );
}

#[tokio::test]
async fn firebase_run_aggregation_query_rejects_deferred_selectors_and_sum() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let transaction_response = server
        .client()
        .post(
            server.http_url("/v1/projects/demo/databases/(default)/documents:runAggregationQuery"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "transaction": "abc",
                "structuredAggregationQuery": {
                    "structuredQuery": {
                        "from": [{ "collectionId": "cities" }]
                    },
                    "aggregations": [{ "count": {} }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase transaction aggregation query should send");
    assert_eq!(transaction_response.status(), StatusCode::BAD_REQUEST);
    let transaction_body: serde_json::Value = transaction_response
        .json()
        .await
        .expect("transaction aggregation error should deserialize");
    assert!(
        transaction_body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("transaction"))
    );

    let sum_response = server
        .client()
        .post(
            server.http_url("/v1/projects/demo/databases/(default)/documents:runAggregationQuery"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredAggregationQuery": {
                    "structuredQuery": {
                        "from": [{ "collectionId": "cities" }]
                    },
                    "aggregations": [{
                        "sum": {
                            "field": { "fieldPath": "rank" }
                        },
                        "alias": "sum_rank"
                    }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase sum aggregation query should send");
    assert_eq!(sum_response.status(), StatusCode::BAD_REQUEST);
    let sum_body: serde_json::Value = sum_response
        .json()
        .await
        .expect("sum aggregation error should deserialize");
    assert!(
        sum_body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("sum aggregations")),
        "sum aggregation error should mention the deferred operator: {sum_body:?}"
    );
}

#[tokio::test]
async fn firebase_run_query_supports_composite_unary_filters_and_name_tiebreaks() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;
    for (document_id, rank, state) in [
        ("bravo", 1, json!("CA")),
        ("alpha", 1, serde_json::Value::Null),
        ("charlie", 2, json!("NV")),
    ] {
        let locator = crate::adapters::firebase::locator_for_document_path(
            &DocumentPath::from_segments(["cities", document_id])
                .expect("document path should parse"),
        )
        .expect("firebase locator should derive");
        service
            .insert_document_with_id(
                &tenant_id,
                locator.table.clone(),
                locator.id.clone(),
                serde_json::Map::from_iter([
                    ("rank".to_string(), json!(rank)),
                    ("state".to_string(), state),
                ]),
            )
            .expect("seed document should insert");
    }

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "compositeFilter": {
                            "op": "OR",
                            "filters": [
                                {
                                    "fieldFilter": {
                                        "field": { "fieldPath": "state" },
                                        "op": "EQUAL",
                                        "value": { "stringValue": "CA" }
                                    }
                                },
                                {
                                    "unaryFilter": {
                                        "field": { "fieldPath": "state" },
                                        "op": "IS_NULL"
                                    }
                                }
                            ]
                        }
                    },
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase run query should send");

    let status = response.status();
    let body = response
        .text()
        .await
        .expect("run query response body should deserialize to text");
    if status != StatusCode::OK {
        panic!("unexpected run query status {status}: {body}");
    }
    let entries = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("streaming JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0]["document"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/alpha")
    );
    assert_eq!(
        entries[1]["document"]["name"],
        json!("projects/demo/databases/(default)/documents/cities/bravo")
    );

    let ordering_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "orderBy": [{
                        "field": { "fieldPath": "rank" },
                        "direction": "ASCENDING"
                    }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase ordering run query should send");

    let ordering_status = ordering_response.status();
    let ordering_body = ordering_response
        .text()
        .await
        .expect("ordering run query response body should deserialize to text");
    if ordering_status != StatusCode::OK {
        panic!("unexpected ordered run query status {ordering_status}: {ordering_body}");
    }
    let ordering_entries = ordering_body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("streaming JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ordering_entries
            .iter()
            .map(|entry| entry["document"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string())
            .collect::<Vec<_>>(),
        vec![
            "projects/demo/databases/(default)/documents/cities/alpha".to_string(),
            "projects/demo/databases/(default)/documents/cities/bravo".to_string(),
            "projects/demo/databases/(default)/documents/cities/charlie".to_string(),
        ]
    );
}

#[tokio::test]
async fn firebase_run_query_under_parent_document_scopes_to_nested_collection_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    for (document_path, name) in [
        ("cities/SF/landmarks/golden-gate", "Golden Gate Bridge"),
        ("cities/SF/landmarks/alamo", "Alamo Square"),
        ("cities/LA/landmarks/griffith", "Griffith Observatory"),
    ] {
        let locator = crate::adapters::firebase::locator_for_document_path(
            &DocumentPath::from_segments(document_path.split('/'))
                .expect("nested document path should parse"),
        )
        .expect("firebase locator should derive");
        service
            .insert_document_with_id(
                &tenant_id,
                locator.table.clone(),
                locator.id.clone(),
                serde_json::Map::from_iter([("name".to_string(), json!(name))]),
            )
            .expect("seed nested document should insert");
    }

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "landmarks" }],
                    "limit": 10
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase parent run query should send");

    assert_eq!(response.status(), StatusCode::OK);
    let entries = response_json_lines(response).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["document"]["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/alamo",
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/golden-gate",
        ]
    );
}

#[tokio::test]
async fn firebase_run_query_collection_group_uses_path_metadata_for_scope_ordering_cursors_and_deletes()
 {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_firebase(
        service.clone(),
        FirebaseConfig::new(),
    ))
    .await;

    for (document_path, rank) in [
        ("cities/SF/districts/1/landmarks/zz-top", 1),
        ("cities/SF/landmarks/aa-top", 1),
        ("cities/SF/landmarks/bb-top", 2),
        ("cities/LA/landmarks/cc-top", 1),
    ] {
        seed_firebase_document(
            &service,
            &tenant_id,
            &document_path.split('/').collect::<Vec<_>>(),
            [("rank", json!(rank))],
        );
    }

    let root_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{
                        "collectionId": "landmarks",
                        "allDescendants": true
                    }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "__name__" },
                            "op": "GREATER_THAN_OR_EQUAL",
                            "value": {
                                "referenceValue": "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top"
                            }
                        }
                    },
                    "orderBy": [{
                        "field": { "fieldPath": "__name__" },
                        "direction": "ASCENDING"
                    }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase collection-group root run query should send");

    let root_status = root_response.status();
    let root_body = root_response
        .text()
        .await
        .expect("root collection-group response should deserialize to text");
    assert_eq!(
        root_status,
        StatusCode::OK,
        "unexpected root collection-group status {root_status}: {root_body}"
    );
    let root_entries = root_body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("root collection-group JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        root_entries
            .iter()
            .map(|entry| entry["document"]["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top",
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/bb-top",
        ],
        "root collection-group query should use full document paths for __name__ filters"
    );

    let scoped_cursor_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{
                        "collectionId": "landmarks",
                        "allDescendants": true
                    }],
                    "orderBy": [{
                        "field": { "fieldPath": "__name__" },
                        "direction": "ASCENDING"
                    }],
                    "startAt": {
                        "values": [{
                            "referenceValue": "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top"
                        }],
                        "before": true
                    }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase collection-group parent run query should send");

    let scoped_cursor_status = scoped_cursor_response.status();
    let scoped_cursor_body = scoped_cursor_response
        .text()
        .await
        .expect("scoped collection-group response should deserialize to text");
    assert_eq!(
        scoped_cursor_status,
        StatusCode::OK,
        "unexpected scoped collection-group status {scoped_cursor_status}: {scoped_cursor_body}"
    );
    let scoped_cursor_entries = scoped_cursor_body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("scoped collection-group JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        scoped_cursor_entries
            .iter()
            .map(|entry| entry["document"]["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/aa-top",
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/bb-top",
        ],
        "parent-scoped collection-group cursors should use full document-name ordering"
    );

    delete_firebase_document(
        &service,
        &tenant_id,
        &["cities", "SF", "landmarks", "aa-top"],
    );

    let post_delete_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents/cities/SF:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{
                        "collectionId": "landmarks",
                        "allDescendants": true
                    }],
                    "orderBy": [{
                        "field": { "fieldPath": "__name__" },
                        "direction": "ASCENDING"
                    }]
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase collection-group post-delete run query should send");

    let post_delete_status = post_delete_response.status();
    let post_delete_body = post_delete_response
        .text()
        .await
        .expect("post-delete collection-group response should deserialize to text");
    assert_eq!(
        post_delete_status,
        StatusCode::OK,
        "unexpected post-delete collection-group status {post_delete_status}: {post_delete_body}"
    );
    let post_delete_entries = post_delete_body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap_or_else(|error| {
                panic!("post-delete collection-group JSON line should parse ({error}): {line}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        post_delete_entries
            .iter()
            .map(|entry| entry["document"]["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "projects/demo/databases/(default)/documents/cities/SF/districts/1/landmarks/zz-top",
            "projects/demo/databases/(default)/documents/cities/SF/landmarks/bb-top",
        ],
        "collection-group queries should stay parent-scoped and drop deleted bindings"
    );
}

#[tokio::test]
async fn firebase_run_query_rejects_invalid_filter_combinations() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    fixture.create_tenant("demo", Service::create_tenant);
    let server = ServerFixture::start(build_router_with_firebase(
        fixture.service(),
        FirebaseConfig::new(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:runQuery"))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(
            json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "compositeFilter": {
                            "op": "AND",
                            "filters": [
                                {
                                    "fieldFilter": {
                                        "field": { "fieldPath": "tags" },
                                        "op": "ARRAY_CONTAINS_ANY",
                                        "value": {
                                            "arrayValue": {
                                                "values": [{ "stringValue": "bridge" }]
                                            }
                                        }
                                    }
                                },
                                {
                                    "fieldFilter": {
                                        "field": { "fieldPath": "labels" },
                                        "op": "ARRAY_CONTAINS_ANY",
                                        "value": {
                                            "arrayValue": {
                                                "values": [{ "stringValue": "gold" }]
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            })
            .to_string(),
        )
        .send()
        .await
        .expect("firebase invalid run query should send");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("invalid run query response should deserialize");
    assert_eq!(body["error"]["status"], json!("INVALID_ARGUMENT"));
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("ARRAY_CONTAINS_ANY")),
        "invalid filter combination should be called out: {body:?}"
    );
}
