use super::*;

#[tokio::test]
async fn engine_read_policy_filters_indexed_queries_and_hides_unauthorized_gets() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_indexed");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_indexed",
                vec![IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("authorized fixture insert should succeed");
    let unauthorized_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Grace")),
            ]),
        )
        .expect("fixture insert should succeed");

    let principal = principal_with_subject("user-123");
    let documents = engine
        .query_documents_with_principal(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "body".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            &principal,
        )
        .expect("query should succeed");

    assert_eq!(document_bodies(&documents), vec!["Ada"]);
    assert!(matches!(
        engine.get_document_with_principal(&tenant_id, &table, unauthorized_id, &principal),
        Err(Error::DocumentNotFound(_))
    ));
}

#[tokio::test]
async fn engine_read_policy_filters_full_scans_pagination_and_subscription_results() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_scanned");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_scanned",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    for (owner, body) in [
        ("user-123", "Ada-1"),
        ("user-456", "Grace"),
        ("user-123", "Ada-2"),
    ] {
        engine
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let principal = principal_with_subject("user-123");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let documents = engine
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("full-scan query should succeed");
    assert_eq!(document_bodies(&documents), vec!["Ada-1", "Ada-2"]);

    let first_page = engine
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: query.clone(),
                page_size: 1,
                after: None,
            },
            &principal,
        )
        .expect("first page should succeed");
    assert_eq!(subscription_bodies(&first_page.data), vec!["Ada-1"]);
    assert!(first_page.has_more);

    let second_page = engine
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: query.clone(),
                page_size: 1,
                after: first_page.next_cursor.clone(),
            },
            &principal,
        )
        .expect("second page should succeed");
    assert_eq!(subscription_bodies(&second_page.data), vec!["Ada-2"]);
    assert!(!second_page.has_more);

    let (tx, mut rx) = subscription_channel();
    let _subscription = engine
        .subscribe(
            &tenant_id,
            query,
            "req-1".to_string(),
            tx,
            SubscribeOptions::for_principal(principal.clone()),
        )
        .expect("subscription should succeed");

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    engine
        .insert_document(
            &tenant_id,
            table,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-999")),
                ("body".to_string(), json!("Blocked")),
            ]),
        )
        .expect("unauthorized fixture insert should still commit for another owner");

    match rx.recv().await.expect("subscription update should arrive") {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }
}

#[tokio::test]
async fn materialized_surface_respects_read_policy_after_schema_change() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_materialized_schema_change");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        engine
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let warmed = engine
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada", "Grace"]);

    let warmed_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(warmed_stats.table_load_count, 1);

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_materialized_schema_change",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    let visible = engine
        .query_documents_with_principal(&tenant_id, &query, &principal_with_subject("user-123"))
        .expect("authorized query should succeed after schema change");
    assert_eq!(document_bodies(&visible), vec!["Ada"]);

    let post_change_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(post_change_stats.table_load_count, 2);
    assert_eq!(post_change_stats.loaded_table_count, 1);
    assert!(post_change_stats.evaluation_count > warmed_stats.evaluation_count);
}

#[tokio::test]
async fn engine_write_policy_rejects_create_update_and_delete_before_commit() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_writes");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema("messages_writes", Vec::new(), Some(owner_write_policy())),
        )
        .expect("schema should save");

    let owner_principal = principal_with_subject("user-123");
    let intruder = principal_with_subject("user-999");
    let initial_sequence = engine
        .latest_sequence(&tenant_id)
        .expect("latest sequence should load");

    let create_error = engine
        .insert_document_with(
            &tenant_id,
            table.clone(),
            None,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Blocked create")),
            ]),
            crate::MutationActor::with_principal(&intruder),
        )
        .expect_err("create should be denied");
    assert!(matches!(create_error, Error::PermissionDenied(_)));
    assert_eq!(
        engine
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        initial_sequence
    );
    assert!(
        engine
            .list_documents(&tenant_id, &table)
            .expect("list should succeed")
            .is_empty(),
        "denied create should not commit"
    );

    let document_id = engine
        .insert_document_with(
            &tenant_id,
            table.clone(),
            None,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Allowed")),
            ]),
            crate::MutationActor::with_principal(&owner_principal),
        )
        .expect("authorized create should succeed");
    let committed_document_records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("durable journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .count();

    let update_error = engine
        .update_document_with(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Intruder edit"))]),
            crate::MutationActor::with_principal(&intruder),
        )
        .expect_err("update should be denied");
    assert!(matches!(update_error, Error::PermissionDenied(_)));
    assert_eq!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("durable journal should read")
            .into_iter()
            .filter(|record| !record.writes.is_empty())
            .count(),
        committed_document_records
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );

    let delete_error = engine
        .delete_document_with(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            crate::MutationActor::with_principal(&intruder),
        )
        .expect_err("delete should be denied");
    assert!(matches!(delete_error, Error::PermissionDenied(_)));
    assert_eq!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("durable journal should read")
            .into_iter()
            .filter(|record| !record.writes.is_empty())
            .count(),
        committed_document_records
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );
}

#[tokio::test]
async fn policy_revision_changes_terminate_active_authorized_subscriptions() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_policy");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_policy",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");
    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("fixture insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let principal = principal_with_subject("user-123");
    let _subscription = engine
        .subscribe(
            &tenant_id,
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            "req-1".to_string(),
            tx,
            SubscribeOptions::for_principal(principal.clone()),
        )
        .expect("subscription should succeed");
    assert_eq!(
        engine
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(subscription_bodies(&data), vec!["Ada"]);
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let changed_policy = TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "body".to_string(),
        }),
        ..TableAccessPolicy::default()
    };
    engine
        .set_table_schema(
            &tenant_id,
            messages_schema("messages_policy", Vec::new(), Some(changed_policy)),
        )
        .expect("updated schema should save");

    match rx.recv().await.expect("policy-change error should arrive") {
        SubscriptionUpdate::Error { message, .. } => {
            assert!(
                message.contains("authorization policy changed; resubscribe"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected post-policy-change event: {other:?}"),
    }
    assert_eq!(
        engine
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        0
    );
}
