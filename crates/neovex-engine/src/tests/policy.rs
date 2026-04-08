use super::*;

#[tokio::test]
async fn service_read_policy_filters_indexed_queries_and_hides_unauthorized_gets() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_indexed");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_indexed",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("authorized fixture insert should succeed");
    let unauthorized_id = service
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
    let documents = service
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
        service.get_document_with_principal(&tenant_id, &table, unauthorized_id, &principal),
        Err(Error::DocumentNotFound(_))
    ));
}

#[tokio::test]
async fn service_read_policy_filters_full_scans_pagination_and_subscription_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_scanned");

    service
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
        service
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
    let documents = service
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("full-scan query should succeed");
    assert_eq!(document_bodies(&documents), vec!["Ada-1", "Ada-2"]);

    let first_page = service
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

    let second_page = service
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
    let _subscription = service
        .subscribe_with_principal(&tenant_id, query, &principal, "req-1".to_string(), tx)
        .expect("subscription should succeed");

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
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
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }
}

#[tokio::test]
async fn materialized_surface_respects_read_policy_after_schema_change() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
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
        service
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

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada", "Grace"]);

    let warmed_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(warmed_stats.table_load_count, 1);

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_materialized_schema_change",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    let visible = service
        .query_documents_with_principal(&tenant_id, &query, &principal_with_subject("user-123"))
        .expect("authorized query should succeed after schema change");
    assert_eq!(document_bodies(&visible), vec!["Ada"]);

    let post_change_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(post_change_stats.table_load_count, 2);
    assert_eq!(post_change_stats.loaded_table_count, 1);
    assert!(post_change_stats.evaluation_count > warmed_stats.evaluation_count);
}

#[tokio::test]
async fn service_write_policy_rejects_create_update_and_delete_before_commit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_writes");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema("messages_writes", Vec::new(), Some(owner_write_policy())),
        )
        .expect("schema should save");

    let owner_principal = principal_with_subject("user-123");
    let intruder = principal_with_subject("user-999");
    let initial_sequence = service
        .latest_sequence(&tenant_id)
        .expect("latest sequence should load");

    let create_error = service
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Blocked create")),
            ]),
            &intruder,
        )
        .expect_err("create should be denied");
    assert!(matches!(create_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        initial_sequence
    );
    assert!(
        service
            .list_documents(&tenant_id, &table)
            .expect("list should succeed")
            .is_empty(),
        "denied create should not commit"
    );

    let document_id = service
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Allowed")),
            ]),
            &owner_principal,
        )
        .expect("authorized create should succeed");
    let committed_sequence = service
        .latest_sequence(&tenant_id)
        .expect("latest sequence should advance after authorized insert");

    let update_error = service
        .update_document_with_principal(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Intruder edit"))]),
            &intruder,
        )
        .expect_err("update should be denied");
    assert!(matches!(update_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        committed_sequence
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );

    let delete_error = service
        .delete_document_with_principal(&tenant_id, table.clone(), document_id, &intruder)
        .expect_err("delete should be denied");
    assert!(matches!(delete_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        committed_sequence
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );
}

#[tokio::test]
async fn policy_revision_changes_terminate_active_authorized_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_policy");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_policy",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");
    service
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
    let _subscription = service
        .subscribe_with_principal(
            &tenant_id,
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &principal,
            "req-1".to_string(),
            tx,
        )
        .expect("subscription should succeed");
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { data, .. } => {
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
    service
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
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        0
    );
}
