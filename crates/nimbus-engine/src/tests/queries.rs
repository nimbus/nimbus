use super::*;

#[tokio::test]
async fn query_uses_index_for_equality_filter() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for index in 0..100 {
        let status = if index < 10 { "active" } else { "inactive" };
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
}

#[tokio::test]
async fn set_table_schema_publishes_reconciled_index_identity() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = tasks_table();
    let initial_index_id = nimbus_core::IndexId::new();
    let replacement_index_id = nimbus_core::IndexId::new();
    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: table.clone(),
                fields: vec![FieldSchema {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    required: false,
                }],
                indexes: vec![nimbus_core::IndexDefinition {
                    id: initial_index_id.clone(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_status".to_string(),
                    fields: vec!["status".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("initial schema should save");
    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: table.clone(),
                fields: vec![FieldSchema {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    required: false,
                }],
                indexes: vec![nimbus_core::IndexDefinition {
                    id: replacement_index_id.clone(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_status".to_string(),
                    fields: vec!["status".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("replacement schema should save");

    let runtime_schema = engine
        .get_table_schema(&tenant_id, &table)
        .expect("runtime schema should load");
    assert_eq!(runtime_schema.indexes[0].id, initial_index_id);
    assert_ne!(runtime_schema.indexes[0].id, replacement_index_id);
}

#[tokio::test]
async fn structured_query_executes_supported_subset() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![
                    FieldSchema {
                        name: "status".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: vec![nimbus_core::IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_status_rank".to_string(),
                    fields: vec!["status".to_string(), "rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("compound structured-query schema should save");

    for (status, rank) in [("active", 3), ("inactive", 2), ("active", 1)] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents_structured(
            &tenant_id,
            &tasks_table(),
            &nimbus_core::StructuredQuery {
                from: vec![nimbus_core::CollectionSelector::collection(
                    nimbus_core::CollectionName::new("tasks").expect("collection id should parse"),
                )],
                where_filter: Some(nimbus_core::QueryFilter::FieldFilter(
                    nimbus_core::FieldFilter {
                        field: nimbus_core::FieldReference::new("status"),
                        op: nimbus_core::FieldFilterOperator::Equal,
                        value: json!("active"),
                    },
                )),
                order_by: vec![nimbus_core::StructuredOrder {
                    field: nimbus_core::FieldReference::new("rank"),
                    direction: nimbus_core::QueryDirection::Ascending,
                }],
                limit: Some(2),
                ..nimbus_core::StructuredQuery::default()
            },
        )
        .expect("structured query should succeed");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].get_field("rank"), Some(&json!(1)));
    assert_eq!(documents[1].get_field("rank"), Some(&json!(3)));
    assert!(
        documents
            .iter()
            .all(|document| document.get_field("status") == Some(&json!("active")))
    );
}

#[tokio::test]
async fn structured_query_supports_repeated_order_cursor_offset_and_projection() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: vec![nimbus_core::IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_rank_title".to_string(),
                    fields: vec!["rank".to_string(), "title".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("repeated-order structured-query schema should save");

    for (title, rank) in [
        ("alpha", 5),
        ("bravo", 4),
        ("charlie", 4),
        ("delta", 2),
        ("echo", 1),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(title)),
                    ("rank".to_string(), json!(rank)),
                    ("status".to_string(), json!("active")),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents_structured(
            &tenant_id,
            &tasks_table(),
            &nimbus_core::StructuredQuery {
                from: vec![nimbus_core::CollectionSelector::collection(
                    nimbus_core::CollectionName::new("tasks").expect("collection id should parse"),
                )],
                order_by: vec![
                    nimbus_core::StructuredOrder {
                        field: nimbus_core::FieldReference::new("rank"),
                        direction: nimbus_core::QueryDirection::Descending,
                    },
                    nimbus_core::StructuredOrder {
                        field: nimbus_core::FieldReference::new("title"),
                        direction: nimbus_core::QueryDirection::Ascending,
                    },
                ],
                start_at: Some(nimbus_core::StructuredCursor {
                    values: vec![json!(4), json!("bravo")],
                    before: false,
                }),
                end_at: Some(nimbus_core::StructuredCursor {
                    values: vec![json!(1)],
                    before: false,
                }),
                offset: Some(1),
                limit: Some(2),
                select: Some(nimbus_core::Projection {
                    fields: vec![
                        nimbus_core::FieldReference::new("__name__"),
                        nimbus_core::FieldReference::new("title"),
                    ],
                }),
                ..nimbus_core::StructuredQuery::default()
            },
        )
        .expect("structured query should succeed");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].get_field("title"), Some(&json!("delta")));
    assert_eq!(documents[1].get_field("title"), Some(&json!("echo")));
    assert!(
        documents
            .iter()
            .all(|document| !document.fields.contains_key("status"))
    );
    assert!(
        documents
            .iter()
            .all(|document| document.fields.contains_key("title"))
    );
}

#[tokio::test]
async fn subscription_initial_evaluation_uses_indexed_query_path() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for status in ["active", "inactive", "active"] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let (tx, mut rx) = subscription_channel();
    let subscription = engine
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "sub-index-1".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("sub-index-1"));
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }
}

#[test]
fn subscription_initial_evaluation_uses_materialized_serving_path_for_full_scan_shape() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let query = query_for("tasks");
    let (tx, mut rx) = subscription_channel();
    let subscription = engine
        .subscribe(&tenant_id, query, "sub-fullscan-sync".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.blocking_recv().expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert_eq!(subscription_id, subscription.id());
            assert_eq!(request_id.as_deref(), Some("sub-fullscan-sync"));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ada"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let surface_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 1);

    let planning_stats = engine
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 1);
}

#[tokio::test]
async fn subscription_async_initial_evaluation_uses_materialized_serving_path_for_full_scan_shape()
{
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let subscription = engine
        .subscribe_async(
            tenant_id.clone(),
            query_for("tasks"),
            "sub-fullscan-async".to_string(),
            tx,
        )
        .await
        .expect("async subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert_eq!(subscription_id, subscription.id());
            assert_eq!(request_id.as_deref(), Some("sub-fullscan-async"));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ada"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let surface_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 1);

    let planning_stats = engine
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 1);
}

#[tokio::test]
async fn setting_schema_backfills_indexes_for_existing_documents() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    for status in ["active", "inactive", "active"] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 2);
}

#[tokio::test]
async fn query_uses_index_for_range_filter() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..100 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(90))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(90)));
    assert_eq!(documents[9].fields.get("rank"), Some(&json!(99)));
}

#[tokio::test]
async fn query_uses_index_for_eq_filter_and_still_applies_remaining_filters() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank) in [("active", 1), ("active", 2), ("inactive", 2)] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("status", FilterOp::Eq, json!("active")),
                    filter("rank", FilterOp::Gte, json!(2)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("active")));
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2)));
}

#[tokio::test]
async fn subscription_re_evaluation_uses_indexed_query_path() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = engine
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "sub-index-2".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    let active_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("active insert should succeed");
    let active_sequence = durable_journal_commits(&engine, &tenant_id, SequenceNumber(0))
        .into_iter()
        .find(|commit| commit.writes.iter().any(|write| write.doc_id == active_id))
        .expect("active insert commit should be durable")
        .sequence;

    timeout(Duration::from_secs(1), async {
        loop {
            let update = rx.recv().await.expect("active update should arrive");
            match update {
                SubscriptionUpdate::Result { snapshot, .. } => {
                    if snapshot.covered_sequence.0 < active_sequence.0 {
                        continue;
                    }
                    let data = snapshot.to_json_documents();
                    assert_eq!(data.len(), 2);
                    assert!(
                        data.iter()
                            .all(|document| document["status"] == json!("active"))
                    );
                    break;
                }
                other => panic!("unexpected active update: {other:?}"),
            }
        }
    })
    .await
    .expect("subscription update should cover the active insert");

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("inactive"))]),
        )
        .expect("inactive insert should succeed");
    let inactive_update = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        inactive_update.is_err(),
        "non-matching indexed insert should not invalidate the subscription"
    );
}

#[tokio::test]
async fn subscription_re_evaluation_uses_materialized_serving_path_for_full_scan_shape() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = engine
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "sub-fullscan-update".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    let initial_surface_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(initial_surface_stats.table_load_count, 1);
    assert_eq!(initial_surface_stats.evaluation_count, 1);

    let follow_up_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Beta"))]),
        )
        .expect("follow-up insert should succeed");
    let follow_up_sequence = durable_journal_commits(&engine, &tenant_id, SequenceNumber(0))
        .into_iter()
        .find(|commit| {
            commit
                .writes
                .iter()
                .any(|write| write.doc_id == follow_up_id)
        })
        .expect("follow-up insert commit should be durable")
        .sequence;

    timeout(Duration::from_secs(1), async {
        loop {
            let update = rx.recv().await.expect("subscription update should arrive");
            match update {
                SubscriptionUpdate::Result { snapshot, .. } => {
                    if snapshot.covered_sequence.0 < follow_up_sequence.0 {
                        continue;
                    }
                    let data = snapshot.to_json_documents();
                    assert_eq!(data.len(), 2);
                    break;
                }
                other => panic!("unexpected subscription event: {other:?}"),
            }
        }
    })
    .await
    .expect("subscription update should cover the follow-up insert");

    let surface_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 2);

    let planning_stats = engine
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 2);
}

#[tokio::test]
async fn query_uses_index_for_bounded_range_filter() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..50 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("rank", FilterOp::Gte, json!(20)),
                    filter("rank", FilterOp::Lt, json!(25)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 5);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(20)));
    assert_eq!(documents[4].fields.get("rank"), Some(&json!(24)));
}

#[tokio::test]
async fn query_uses_three_field_composite_range_index_through_planner() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (team, status, rank) in [
        ("alpha", "open", 1),
        ("alpha", "open", 2),
        ("alpha", "open", 3),
        ("alpha", "done", 2),
        ("beta", "open", 2),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("team".to_string(), json!(team)),
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("team", FilterOp::Eq, json!("alpha")),
                    filter("status", FilterOp::Eq, json!("open")),
                    filter("rank", FilterOp::Gte, json!(2)),
                    filter("rank", FilterOp::Lt, json!(4)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("three-field composite query should succeed");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2)));
    assert_eq!(documents[1].fields.get("rank"), Some(&json!(3)));

    let stats = engine
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(stats.query_composite_index_count, 1);
    assert_eq!(stats.query_single_field_index_count, 0);
    assert_eq!(stats.query_full_scan_count, 0);
}

#[tokio::test]
async fn query_documents_cancellable_stops_during_index_scan() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..64 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let mut checks = 0usize;
    let error = engine
        .query_documents_cancellable(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(0))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            &mut || {
                checks += 1;
                if checks > 8 {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("query should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_while_blocking_work_unwinds() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    for rank in 0..32 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            engine
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async query should resolve promptly after cancellation")
        .expect("query task should join successfully")
        .expect_err("query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    timeout(
        Duration::from_secs(1),
        probe.wait_until_released_from_first_check(),
    )
    .await
    .expect("blocking cancellation check should unwind after release");
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_during_index_scan() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                }],
                indexes: vec![IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    for rank in 0..32 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            engine
                .query_documents_async_cancellable(
                    tenant_id,
                    Query {
                        table: tasks_table(),
                        filters: vec![filter("rank", FilterOp::Gte, json!(0))],
                        order: Some(OrderBy {
                            field: "rank".to_string(),
                            direction: OrderDirection::Asc,
                        }),
                        limit: None,
                    },
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("indexed query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("indexed async query should resolve promptly after cancellation")
        .expect("indexed query task should join successfully")
        .expect_err("indexed query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    timeout(
        Duration::from_secs(1),
        probe.wait_until_released_from_first_check(),
    )
    .await
    .expect("blocking cancellation check should unwind after release");
}

#[tokio::test]
async fn paginated_query_uses_index_for_range_filter() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..10 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let first_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["rank"], json!(5));
    assert_eq!(first_page.data[1]["rank"], json!(6));
    assert!(first_page.has_more);

    let second_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: first_page.next_cursor.clone(),
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["rank"], json!(7));
    assert_eq!(second_page.data[1]["rank"], json!(8));
}

#[tokio::test]
async fn paginated_query_uses_composite_index_for_exact_prefix_and_cursor_progress() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank) in [
        ("open", 1),
        ("open", 2),
        ("open", 3),
        ("open", 4),
        ("done", 0),
        ("done", 5),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let first_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["status"], json!("open"));
    assert_eq!(first_page.data[0]["rank"], json!(1));
    assert_eq!(first_page.data[1]["status"], json!("open"));
    assert_eq!(first_page.data[1]["rank"], json!(2));
    assert!(first_page.has_more);

    let second_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: first_page.next_cursor.clone(),
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["status"], json!("open"));
    assert_eq!(second_page.data[0]["rank"], json!(3));
    assert_eq!(second_page.data[1]["status"], json!("open"));
    assert_eq!(second_page.data[1]["rank"], json!(4));
    assert!(!second_page.has_more);
    assert!(second_page.next_cursor.is_none());
}

#[tokio::test]
async fn paginated_query_cursor_survives_index_plan_change_between_pages() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let indexed_schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, indexed_schema)
        .expect("indexed schema should save");

    for (status, rank) in [
        ("open", 1),
        ("open", 2),
        ("open", 3),
        ("open", 4),
        ("done", 0),
        ("done", 5),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("open"))],
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let first_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: query.clone(),
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should use the composite index");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["rank"], json!(1));
    assert_eq!(first_page.data[1]["rank"], json!(2));
    let cursor = first_page
        .next_cursor
        .clone()
        .expect("first page should return a cursor");

    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![
                    FieldSchema {
                        name: "status".to_string(),
                        field_type: FieldType::String,
                        required: false,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: false,
                    },
                ],
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .expect("schema without index should save");

    let second_page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query,
                page_size: 2,
                after: Some(cursor),
            },
        )
        .expect("cursor should survive the plan change for the same user query");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["rank"], json!(3));
    assert_eq!(second_page.data[1]["rank"], json!(4));
    assert!(!second_page.has_more);
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn paginated_query_cursor_signature_excludes_principal_auth_filters() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "owner".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_owner_rank".to_string(),
            fields: vec!["owner".to_string(), "rank".to_string()],
        }],
        access_policy: Some(read_only_owner_policy()),
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("owner-indexed schema should save");

    for (owner, rank) in [
        ("alice", 1),
        ("alice", 2),
        ("bob", 3),
        ("bob", 4),
        ("carol", 5),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("document insert should succeed");
    }

    let user_query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let alice = principal_with_subject("alice");
    let first_page = engine
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: user_query.clone(),
                page_size: 1,
                after: None,
            },
            &alice,
        )
        .expect("first page should be authorized for alice");
    assert_eq!(first_page.data.len(), 1);
    assert_eq!(first_page.data[0]["owner"], json!("alice"));
    assert_eq!(first_page.data[0]["rank"], json!(1));
    let cursor = first_page
        .next_cursor
        .clone()
        .expect("alice page should produce a cursor");

    engine
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![
                    FieldSchema {
                        name: "owner".to_string(),
                        field_type: FieldType::String,
                        required: true,
                    },
                    FieldSchema {
                        name: "rank".to_string(),
                        field_type: FieldType::Number,
                        required: true,
                    },
                ],
                indexes: Vec::new(),
                access_policy: Some(read_only_owner_policy()),
            },
        )
        .expect("schema without index should save");

    let bob = principal_with_subject("bob");
    let second_page = engine
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: user_query,
                page_size: 2,
                after: Some(cursor),
            },
            &bob,
        )
        .expect("cursor should not embed alice's authorization filter");
    assert_eq!(second_page.data.len(), 2);
    assert!(
        second_page
            .data
            .iter()
            .all(|document| document["owner"] == json!("bob")),
        "the replaying principal's authorization filter must still constrain results"
    );
    assert_eq!(second_page.data[0]["rank"], json!(3));
    assert_eq!(second_page.data[1]["rank"], json!(4));
    assert!(!second_page.has_more);
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn query_planning_stats_distinguish_composite_single_field_and_fallback_paths() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            },
        ],
        indexes: vec![
            IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_status_rank".to_string(),
                fields: vec!["status".to_string(), "rank".to_string()],
            },
            IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            },
        ],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank, title) in [
        ("open", 1, "a"),
        ("open", 2, "b"),
        ("open", 3, "c"),
        ("done", 4, "d"),
    ] {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(title)),
                ]),
            )
            .expect("insert should succeed");
    }

    let composite = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("composite query should succeed");
    assert_eq!(composite.len(), 3);

    let single_field = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(2))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("single-field query should succeed");
    assert_eq!(single_field.len(), 3);

    let fallback = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("title", FilterOp::Eq, json!("b"))],
                order: None,
                limit: None,
            },
        )
        .expect("fallback query should succeed");
    assert_eq!(fallback.len(), 1);

    let page = engine
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("paginated composite query should succeed");
    assert_eq!(page.data.len(), 2);

    let stats = engine
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(stats.query_composite_index_count, 1);
    assert_eq!(stats.query_single_field_index_count, 1);
    assert_eq!(stats.query_full_scan_count, 1);
    assert_eq!(stats.paginated_composite_index_count, 1);
    assert_eq!(stats.paginated_single_field_index_count, 0);
    assert_eq!(stats.paginated_full_scan_count, 0);
}
