use super::*;

async fn next_system_table_documents(
    updates: &mut tokio::sync::mpsc::Receiver<nimbus_engine::SubscriptionUpdate>,
    description: &str,
) -> Vec<serde_json::Value> {
    match timeout(Duration::from_secs(5), updates.recv()).await {
        Ok(Some(nimbus_engine::SubscriptionUpdate::Result { snapshot, .. })) => {
            snapshot.to_json_documents()
        }
        Ok(Some(nimbus_engine::SubscriptionUpdate::Error { message, .. })) => {
            panic!("{description} failed with subscription error: {message}")
        }
        Ok(None) => panic!("{description} failed because subscription channel closed"),
        Err(_) => panic!("timed out waiting for {description}"),
    }
}

async fn wait_for_system_table_documents(
    updates: &mut tokio::sync::mpsc::Receiver<nimbus_engine::SubscriptionUpdate>,
    description: &str,
    predicate: impl Fn(&[serde_json::Value]) -> bool,
) -> Vec<serde_json::Value> {
    timeout(Duration::from_secs(5), async {
        loop {
            let documents = next_system_table_documents(updates, description).await;
            if predicate(&documents) {
                return documents;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {description}"))
}

async fn wait_for_system_table_by_name(
    api: &HttpApiFixture<'_>,
    description: &str,
    tenant_id: &str,
    name: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let (_status, body) = wait_for_value(
        description,
        Duration::from_secs(15),
        Duration::from_millis(25),
        || async {
            let Ok(response) = timeout(
                Duration::from_secs(10),
                api.convex_named_query(
                    "_nimbus",
                    "tables:byName",
                    json!({ "tenantId": tenant_id, "name": name }),
                ),
            )
            .await
            else {
                return (
                    StatusCode::REQUEST_TIMEOUT,
                    "timed out waiting for system table query response".to_string(),
                );
            };
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("failed to read response body: {error}"));
            (status, body)
        },
        |(status, body)| {
            *status == StatusCode::OK
                && serde_json::from_str::<serde_json::Value>(body)
                    .is_ok_and(|value| predicate(&value))
        },
    )
    .await;
    serde_json::from_str::<serde_json::Value>(&body)
        .unwrap_or_else(|error| panic!("system table state query should parse: {error}: {body}"))
}

#[tokio::test]
async fn schema_crud_via_http() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true },
            { "name": "age", "field_type": "number", "required": false }
        ],
        "indexes": []
    });

    assert_eq!(
        api.set_table_schema("demo", "users", schema.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let full_schema = api.get_schema("demo").await;
    assert_eq!(full_schema.status(), StatusCode::OK);
    let full_body = full_schema
        .json::<serde_json::Value>()
        .await
        .expect("schema response should parse");
    assert_eq!(full_body["tables"]["users"], schema);

    let table_schema = api.get_table_schema("demo", "users").await;
    assert_eq!(table_schema.status(), StatusCode::OK);
    let table_body = table_schema
        .json::<serde_json::Value>()
        .await
        .expect("table schema response should parse");
    assert_eq!(table_body, schema);

    let valid_insert = api
        .insert_document("demo", "users", json!({ "name": "Alice", "age": 30 }))
        .await;
    assert_eq!(valid_insert.status(), StatusCode::CREATED);

    let invalid_insert = api
        .insert_document("demo", "users", json!({ "age": "old" }))
        .await;
    assert_eq!(invalid_insert.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(
        api.delete_table_schema("demo", "users").await.status(),
        StatusCode::NO_CONTENT
    );

    let permissive_insert = api
        .insert_document("demo", "users", json!({ "anything": { "goes": true } }))
        .await;
    assert_eq!(permissive_insert.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn schema_apply_reports_violations_and_never_applies_partially() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    // Three documents land before any schema exists. One of them breaks the
    // schema the operator is about to apply in two ways, and one document
    // carries only the required field.
    for fields in [
        json!({ "name": "Alice", "age": 30 }),
        json!({ "age": "old" }),
        json!({ "name": "Bob" }),
    ] {
        assert_eq!(
            api.insert_document("demo", "users", fields).await.status(),
            StatusCode::CREATED
        );
    }
    let listed = api
        .list_documents("demo", "users")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("document list should parse");
    let violating_id = listed["data"]
        .as_array()
        .expect("data should be an array")
        .iter()
        .find(|document| document["age"] == json!("old"))
        .and_then(|document| document["_id"].as_str())
        .expect("violating document should be listed")
        .to_string();

    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true },
            { "name": "age", "field_type": "number", "required": false }
        ],
        "indexes": [{ "name": "by_name", "fields": ["name"] }]
    });

    // A violating table refuses the whole schema and names the document.
    let refused = api
        .apply_table_schema("demo", "users", schema.clone(), false)
        .await;
    assert_eq!(refused.status(), StatusCode::OK);
    let refused_body = refused
        .json::<serde_json::Value>()
        .await
        .expect("apply response should parse");
    assert_eq!(refused_body["applied"], json!(false));
    assert_eq!(refused_body["scanned"], json!(3));
    assert_eq!(refused_body["violation_count"], json!(1));
    assert_eq!(
        refused_body["violations"],
        json!([{ "id": violating_id, "message": "missing required field: name" }])
    );

    // Nothing was applied: the table still has no schema and still accepts
    // a document the schema would reject.
    assert_eq!(
        api.get_table_schema("demo", "users").await.status(),
        StatusCode::NOT_FOUND
    );
    let unchecked = api
        .insert_document("demo", "users", json!({ "age": "older" }))
        .await;
    assert_eq!(unchecked.status(), StatusCode::CREATED);
    let unchecked_id = unchecked
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("insert should return an id")
        .to_string();

    // Repair both violating documents, then check without applying.
    for id in [&violating_id, &unchecked_id] {
        assert_eq!(
            api.delete_document("demo", "users", id).await.status(),
            StatusCode::NO_CONTENT
        );
    }
    let checked = api
        .apply_table_schema("demo", "users", schema.clone(), true)
        .await;
    assert_eq!(checked.status(), StatusCode::OK);
    let checked_body = checked
        .json::<serde_json::Value>()
        .await
        .expect("dry-run response should parse");
    assert_eq!(checked_body["applied"], json!(false));
    assert_eq!(checked_body["dry_run"], json!(true));
    assert_eq!(checked_body["scanned"], json!(2));
    assert_eq!(checked_body["violations"], json!([]));
    assert_eq!(
        api.get_table_schema("demo", "users").await.status(),
        StatusCode::NOT_FOUND
    );

    // A clean table applies, and enforcement starts at once.
    let applied = api
        .apply_table_schema("demo", "users", schema.clone(), false)
        .await;
    assert_eq!(applied.status(), StatusCode::OK);
    let applied_body = applied
        .json::<serde_json::Value>()
        .await
        .expect("apply response should parse");
    assert_eq!(applied_body["applied"], json!(true));
    assert_eq!(applied_body["scanned"], json!(2));
    assert_eq!(applied_body["violations"], json!([]));
    let stored = api
        .get_table_schema("demo", "users")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("table schema should parse");
    assert_eq!(stored["indexes"][0]["name"], json!("by_name"));
    assert_eq!(stored["indexes"][0]["state"], json!("enabled"));
    assert_eq!(
        api.insert_document("demo", "users", json!({ "age": "old" }))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // A schema whose table name disagrees with the path is refused before
    // any scan.
    let mismatched = api
        .apply_table_schema(
            "demo",
            "users",
            json!({ "table": "people", "fields": [] }),
            false,
        )
        .await;
    assert_eq!(mismatched.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn schema_and_document_writes_project_table_state_into_system_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare before subscribing");
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_system_convex_registry(
                ConvexRegistry::from_embedded_system_bundle()
                    .expect("embedded system Convex registry should load"),
            )
            .build(),
    )
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let (system_tx, mut system_rx) =
        tokio::sync::mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let system_subscription = service
        .subscribe_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            nimbus_core::Query {
                table: TableName::new("tables").expect("table should parse"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            "system-tables-watch".to_string(),
            system_tx,
            nimbus_engine::SubscribeOptions::anonymous(),
        )
        .await
        .expect("system tenant table directory should be subscribable");
    let initial =
        next_system_table_documents(&mut system_rx, "initial _nimbus tables snapshot").await;
    assert!(
        initial.is_empty(),
        "system table directory should start empty: {initial:?}"
    );

    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true }
        ],
        "indexes": []
    });
    assert_eq!(
        api.set_table_schema("demo", "users", schema.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let insert_response = api
        .insert_document("demo", "users", json!({ "name": "Ada" }))
        .await;
    assert_eq!(insert_response.status(), StatusCode::CREATED);
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("document id should be a string")
        .to_string();

    let active = wait_for_system_table_documents(
        &mut system_rx,
        "active _nimbus table-directory projection",
        |documents| {
            documents.iter().any(|document| {
                document["tenantId"] == json!("demo")
                    && document["name"] == json!("users")
                    && document["rowCount"] == json!(1)
            })
        },
    )
    .await;
    assert_eq!(
        active.len(),
        1,
        "expected one active table-directory document: {active:?}"
    );
    drop(system_subscription);

    let (cleanup_tx, mut cleanup_rx) =
        tokio::sync::mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let cleanup_subscription = service
        .subscribe_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            nimbus_core::Query {
                table: TableName::new("tables").expect("table should parse"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            "system-tables-cleanup-watch".to_string(),
            cleanup_tx,
            nimbus_engine::SubscribeOptions::anonymous(),
        )
        .await
        .expect("system tenant cleanup table directory should be subscribable");
    let cleanup_initial =
        next_system_table_documents(&mut cleanup_rx, "cleanup _nimbus tables snapshot").await;
    assert!(
        cleanup_initial.iter().any(|document| {
            document["tenantId"] == json!("demo")
                && document["name"] == json!("users")
                && document["rowCount"] == json!(1)
        }),
        "cleanup subscription should start from the active table projection: {cleanup_initial:?}"
    );

    assert_eq!(
        api.delete_document("demo", "users", &document_id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete_table_schema("demo", "users").await.status(),
        StatusCode::NO_CONTENT
    );

    let cleared = wait_for_system_table_documents(
        &mut cleanup_rx,
        "cleared _nimbus table-directory projection",
        |documents| documents.is_empty(),
    )
    .await;
    assert!(
        cleared.is_empty(),
        "deleting the last row and schema should remove table-directory state: {cleared:?}"
    );

    drop(cleanup_subscription);
}

#[tokio::test]
async fn table_state_projection_is_queryable_through_system_convex_bundle() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(
        RouterBuildConfig::core(service)
            .with_system_convex_registry(
                ConvexRegistry::from_embedded_system_bundle()
                    .expect("embedded system Convex registry should load"),
            )
            .build(),
    )
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true }
        ],
        "indexes": []
    });
    assert_eq!(
        api.set_table_schema("demo", "users", schema.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.insert_document("demo", "users", json!({ "name": "Ada" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let table_state = wait_for_system_table_by_name(
        &api,
        "packaged system table query should expose active users table",
        "demo",
        "users",
        |value| value["rowCount"] == json!(1),
    )
    .await;
    assert_eq!(table_state["tenantId"], "demo");
    assert_eq!(table_state["name"], "users");
    assert_eq!(table_state["schema"], schema);
    assert_eq!(table_state["rowCount"], 1);
    assert!(
        table_state["lastWriteAt"].as_f64().is_some(),
        "table state should expose a write timestamp: {table_state}"
    );

    let table_list = api
        .convex_named_query(
            "_nimbus",
            "tables:list",
            json!({ "tenantId": "demo", "limit": null }),
        )
        .await;
    assert_eq!(table_list.status(), StatusCode::OK);
    let table_list = table_list
        .json::<serde_json::Value>()
        .await
        .expect("system table list query should parse");
    assert_eq!(
        table_list
            .as_array()
            .expect("table list should be an array")
            .len(),
        1
    );
}
