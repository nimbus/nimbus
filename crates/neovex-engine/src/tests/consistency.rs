use super::*;

#[tokio::test]
async fn service_reload_recovers_durable_journal_before_serving_async_reads_redb() {
    assert_service_reload_recovers_durable_journal_before_serving_async_reads(
        EmbeddedProviderKind::Redb,
    )
    .await;
}

#[tokio::test]
async fn service_reload_recovers_durable_journal_before_serving_async_reads_sqlite() {
    assert_service_reload_recovers_durable_journal_before_serving_async_reads(
        EmbeddedProviderKind::Sqlite,
    )
    .await;
}

async fn assert_service_reload_recovers_durable_journal_before_serving_async_reads(
    backend: EmbeddedProviderKind,
) {
    let data_dir = tempdir().expect("service tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let service = Service::new_with_embedded_provider(data_dir.path(), backend)
        .expect("service should create");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    drop(service);

    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("recovered"))]),
    );
    append_durable_records_for_backend(
        data_dir.path(),
        &tenant_id,
        backend,
        &[neovex_core::DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(60_000),
            vec![neovex_core::WriteOp {
                table: document.table.clone(),
                op_type: neovex_core::WriteOpType::Insert,
                doc_id: document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(document.clone()),
            }],
            None,
        )
        .expect("durable record should build")],
    );

    let reopened = Arc::new(
        Service::new_with_embedded_provider(data_dir.path(), backend)
            .expect("service should reopen"),
    );
    let documents = reopened
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("async read should recover and succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document.id);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("recovered")));
    assert_eq!(
        reopened
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("journal stats should read after recovery"),
        crate::tenant::MutationJournalStats {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
            apply_lag: 0,
            queue_depth: 0,
            queue_capacity: crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY,
            oldest_queue_age_nanos: 0,
            pending_response_count: 0,
            worker_running: false,
            worker_start_count: 0,
            worker_restart_count: 0,
            queue_rejection_count: 0,
            worker_failure_count: 0,
            read_wait_count: 0,
            total_read_wait_nanos: 0,
        }
    );

    let second_document_id = reopened
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after-reopen"))]),
        )
        .await
        .expect("follow-up async insert should succeed after recovery");
    let after_reopen_documents = reopened
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("async reads should continue to succeed after follow-up writes");
    assert_eq!(after_reopen_documents.len(), 2);
    assert!(
        after_reopen_documents
            .iter()
            .any(|candidate| candidate.id == document.id),
        "recovered durable writes should remain visible after follow-up traffic"
    );
    assert!(
        after_reopen_documents
            .iter()
            .any(|candidate| candidate.id == second_document_id),
        "follow-up async writes should succeed after the reopen path"
    );

    let recovered_stats = wait_for_mutation_journal_stats(
        &reopened,
        &tenant_id,
        "mutation journal worker to go idle after the follow-up async write",
        |stats| !stats.worker_running,
    )
    .await;
    assert_eq!(recovered_stats.durable_head, SequenceNumber(2));
    assert_eq!(recovered_stats.applied_head, SequenceNumber(2));
    assert_eq!(recovered_stats.apply_lag, 0);
    assert_eq!(recovered_stats.queue_depth, 0);
    assert_eq!(
        recovered_stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY
    );
    assert_eq!(recovered_stats.oldest_queue_age_nanos, 0);
    assert_eq!(recovered_stats.pending_response_count, 0);
    assert!(!recovered_stats.worker_running);
    assert_eq!(recovered_stats.worker_start_count, 1);
    assert_eq!(recovered_stats.worker_restart_count, 0);
    assert_eq!(recovered_stats.queue_rejection_count, 0);
    assert_eq!(recovered_stats.worker_failure_count, 0);
}

#[tokio::test]
async fn durable_journal_reads_return_strictly_ordered_authoritative_records() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("journal"))]),
        )
        .await
        .expect("insert should succeed");
    service
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("journal-updated"))]),
        )
        .await
        .expect("update should succeed");

    let records = service
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("durable journal should read");
    assert_eq!(
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
    assert_eq!(
        records[0].writes[0].op_type,
        neovex_core::WriteOpType::Insert
    );
    assert_eq!(
        records[1].writes[0].op_type,
        neovex_core::WriteOpType::Update
    );
    assert_eq!(
        records[1].writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("journal-updated"))
    );

    let filtered = service
        .read_durable_journal_async(tenant_id, SequenceNumber(1))
        .await
        .expect("filtered durable journal should read");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].sequence, SequenceNumber(2));
}

#[tokio::test]
async fn durable_journal_stream_resumes_from_sequence_cursor_with_duplicate_tolerant_pages() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        )
        .await
        .expect("first insert should succeed");
    service
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            first_document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("first-updated"))]),
        )
        .await
        .expect("update should succeed");
    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
        )
        .await
        .expect("second insert should succeed");

    let first_page = service
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("first journal page should read");
    assert_eq!(first_page.cursor_floor, SequenceNumber(0));
    assert_eq!(first_page.latest_sequence, SequenceNumber(3));
    assert!(first_page.has_more);
    assert_eq!(first_page.next_cursor, SequenceNumber(1));
    assert_eq!(first_page.records.len(), 1);
    assert_eq!(first_page.records[0].sequence, SequenceNumber(1));

    let replayed_first_page = service
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("replayed first journal page should read");
    assert_eq!(replayed_first_page.records, first_page.records);
    assert_eq!(replayed_first_page.next_cursor, first_page.next_cursor);

    let second_page = service
        .stream_durable_journal_async(tenant_id.clone(), first_page.next_cursor, 1)
        .await
        .expect("second journal page should read");
    assert!(second_page.has_more);
    assert_eq!(second_page.next_cursor, SequenceNumber(2));
    assert_eq!(second_page.records.len(), 1);
    assert_eq!(second_page.records[0].sequence, SequenceNumber(2));

    let third_page = service
        .stream_durable_journal_async(tenant_id.clone(), second_page.next_cursor, 1)
        .await
        .expect("third journal page should read");
    assert!(!third_page.has_more);
    assert_eq!(third_page.next_cursor, SequenceNumber(3));
    assert_eq!(third_page.records.len(), 1);
    assert_eq!(third_page.records[0].sequence, SequenceNumber(3));

    let empty_page = service
        .stream_durable_journal_async(tenant_id, third_page.next_cursor, 1)
        .await
        .expect("empty journal page should read");
    assert!(!empty_page.has_more);
    assert_eq!(empty_page.next_cursor, SequenceNumber(3));
    assert!(empty_page.records.is_empty());
}

#[tokio::test]
async fn durable_journal_bootstrap_metadata_reconstructs_same_state_as_live_reads() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(80_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("bootstrap"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let bootstrap = service
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await
        .expect("bootstrap metadata should read");
    assert_eq!(bootstrap.resume_after, SequenceNumber(0));
    assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(1));
    assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.durable_head, SequenceNumber(1));
    assert!(bootstrap.snapshot.documents.is_empty());

    let page = service
        .stream_durable_journal_async(tenant_id.clone(), bootstrap.resume_after, 10)
        .await
        .expect("journal tail should read");
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(1));

    faults.release();
    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join")
        .expect("mutation should succeed");

    let rebuilt = TenantStore::create_in_memory().expect("rebuild store should open");
    rebuilt
        .rebuild_materialized_journal_from_snapshot(
            &bootstrap.snapshot,
            &page.records,
            Some(bootstrap.bootstrap_cut),
        )
        .expect("snapshot plus stream tail should rebuild");

    faults.release();

    let live_documents = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("live read should succeed after apply");
    let rebuilt_documents = rebuilt
        .scan_table(&tasks_table())
        .expect("rebuilt store should scan");
    assert_eq!(rebuilt_documents, live_documents);
}

#[tokio::test]
async fn embedded_replica_bootstrap_matches_live_query_and_pagination_results() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(title)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    let live_query = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("live query should succeed");
    let replica_query = replica
        .query_documents(&query_for("tasks"))
        .expect("replica query should succeed");
    assert_eq!(replica_query, live_query);

    let paginated = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };
    let live_page = service
        .paginate_documents_async(tenant_id.clone(), paginated.clone())
        .await
        .expect("live page should succeed");
    let replica_page = replica
        .paginate_documents(&paginated)
        .expect("replica page should succeed");
    assert_eq!(replica_page, live_page);
}

#[tokio::test]
async fn embedded_replica_catches_up_after_reconnection() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("before"))]),
        )
        .await
        .expect("initial insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(1));

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after"))]),
        )
        .await
        .expect("follow-up insert should succeed");

    let stale_documents = replica
        .query_documents(&query_for("tasks"))
        .expect("stale replica query should succeed");
    assert_eq!(stale_documents.len(), 1);

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should succeed");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    let live_documents = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("live query should succeed");
    let replica_documents = replica
        .query_documents(&query_for("tasks"))
        .expect("replica query should succeed");
    assert_eq!(replica_documents, live_documents);
}

#[tokio::test]
async fn embedded_replica_catch_up_refreshes_policy_only_schema_changes() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = messages_table("messages_replica_policy");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let principal = principal_with_subject("user-123");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

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
    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Grace")),
            ]),
        )
        .expect("fixture insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_replica_policy",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should refresh schema even without new journal records");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    let live_documents = service
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("live principal query should succeed");
    let replica_documents = replica
        .query_documents_with_principal(&query, &principal)
        .expect("replica principal query should succeed");
    assert_eq!(document_bodies(&replica_documents), vec!["Ada"]);
    assert_eq!(replica_documents, live_documents);

    let live_anonymous = service
        .query_documents(&tenant_id, &query)
        .expect("live anonymous query should succeed");
    let replica_anonymous = replica
        .query_documents(&query)
        .expect("replica anonymous query should succeed");
    assert!(live_anonymous.is_empty());
    assert_eq!(replica_anonymous, live_anonymous);
}

#[tokio::test]
async fn embedded_replica_catch_up_rebuilds_indexes_for_schema_only_changes() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for rank in [1, 2, 3] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");

    service
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
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should refresh schema and indexes");

    let query = Query {
        table: tasks_table(),
        filters: vec![filter("rank", FilterOp::Eq, json!(2))],
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_documents = service
        .query_documents(&tenant_id, &query)
        .expect("live indexed query should succeed");
    let replica_documents = replica
        .query_documents(&query)
        .expect("replica indexed query should succeed");
    assert_eq!(replica_documents, live_documents);
    assert_eq!(replica_documents.len(), 1);
    assert_eq!(replica_documents[0].fields.get("rank"), Some(&json!(2)));
}

#[tokio::test]
async fn shadow_materializer_queries_match_live_service_path() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(title)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let shadow = service
        .build_shadow_materializer_async(
            tenant_id.clone(),
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .await
        .expect("shadow materializer should build");
    assert_eq!(shadow.manifest().current_sequence, SequenceNumber(3));
    assert_eq!(
        shadow.current_snapshot().applied_sequence,
        SequenceNumber(3)
    );
    let snapshot = shadow.current_snapshot();

    let ordered_query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_query = service
        .query_documents_async(tenant_id.clone(), ordered_query.clone())
        .await
        .expect("live query should succeed");
    let shadow_query = query_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &ordered_query,
        &PrincipalContext::anonymous(),
    )
    .expect("shadow query should succeed");
    assert_eq!(shadow_query, live_query);

    let paginated = PaginatedQuery {
        query: ordered_query,
        page_size: 2,
        after: None,
    };
    let live_page = service
        .paginate_documents_async(tenant_id, paginated.clone())
        .await
        .expect("live page should succeed");
    let shadow_page = paginate_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &paginated,
        &PrincipalContext::anonymous(),
    )
    .expect("shadow page should succeed");
    assert_eq!(shadow_page, live_page);
}

#[tokio::test]
async fn shadow_materializer_schema_aware_queries_match_live_service_path() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = messages_table("messages_shadow_schema");
    let principal = principal_with_subject("user-123");
    let hidden_owner = principal_with_subject("user-456");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_shadow_schema",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    for (owner, body) in [
        ("user-123", "Ada"),
        ("user-123", "Beta"),
        ("user-456", "Hidden"),
    ] {
        let principal = if owner == "user-123" {
            principal.clone()
        } else {
            hidden_owner.clone()
        };
        service
            .insert_document_with_principal(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
                &principal,
            )
            .expect("seed insert should succeed");
    }

    let shadow = service
        .build_shadow_materializer_async(
            tenant_id.clone(),
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .await
        .expect("shadow materializer should build");
    let snapshot = shadow.current_snapshot();

    let indexed_query = Query {
        table: table.clone(),
        filters: vec![filter("owner", FilterOp::Eq, json!("user-123"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_query = service
        .query_documents_async_with_principal(
            tenant_id.clone(),
            indexed_query.clone(),
            principal.clone(),
        )
        .await
        .expect("live schema-aware query should succeed");
    let shadow_query = query_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &indexed_query,
        &principal,
    )
    .expect("shadow schema-aware query should succeed");
    assert_eq!(document_bodies(&shadow_query), vec!["Ada", "Beta"]);
    assert_eq!(shadow_query, live_query);

    let paginated = PaginatedQuery {
        query: Query {
            table,
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "body".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 1,
        after: None,
    };
    let live_page = service
        .paginate_documents_async_with_principal(tenant_id, paginated.clone(), principal.clone())
        .await
        .expect("live schema-aware page should succeed");
    let shadow_page = paginate_documents_for_docs_with_principal(
        snapshot.documents,
        &snapshot.schema,
        &paginated,
        &principal,
    )
    .expect("shadow schema-aware page should succeed");
    assert_eq!(subscription_bodies(&shadow_page.data), vec!["Ada"]);
    assert_eq!(shadow_page, live_page);
}

#[tokio::test]
async fn online_consistency_verifier_matches_authoritative_shadow_and_replica_state() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
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
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    for rank in [1, 2, 3] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let report = service
        .verify_consistency_async(tenant_id.clone())
        .await
        .expect("consistency verification should succeed");
    assert!(report.ok, "{report:#?}");
    assert!(report.mismatches.is_empty());
    assert_eq!(report.authoritative.document_count, 3);
    assert_eq!(report.authoritative.schema_table_count, 1);
    assert_eq!(
        report.authoritative.applied_sequence,
        report.authoritative.durable_head
    );
    assert_eq!(report.authoritative.digest, report.shadow.digest);
    assert_eq!(report.authoritative.digest, report.embedded_replica.digest);
    assert!(report.bootstrap.resume_after_sequence <= report.bootstrap.bootstrap_cut_sequence);
    assert_eq!(
        report.bootstrap.bootstrap_cut_sequence,
        report.authoritative.durable_head
    );
    assert!(!report.bootstrap.snapshot_digest.is_empty());
}

#[test]
fn snapshot_comparison_reports_document_field_differences_with_identifier() {
    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("alpha"))]),
    );
    let left = materialized_snapshot_with_documents(vec![document.clone()]);
    let mut changed_document = document.clone();
    changed_document
        .fields
        .insert("title".to_string(), json!("beta"));
    let right = materialized_snapshot_with_documents(vec![changed_document]);

    let mismatch = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        &left,
        ConsistencyScope::ShadowMaterializer,
        &right,
    )
    .expect("document mismatch should be reported");

    assert_eq!(mismatch.invariant, "materialized_snapshot_match");
    assert_eq!(mismatch.path, format!("documents.tasks/{}", document.id));
    assert_eq!(mismatch.left_scope, ConsistencyScope::AuthoritativeSnapshot);
    assert_eq!(mismatch.right_scope, ConsistencyScope::ShadowMaterializer);
    assert!(mismatch.left_description.contains("alpha"));
    assert!(mismatch.right_description.contains("beta"));
}

#[test]
fn durable_journal_bootstrap_verifier_reports_resume_after_mismatch() {
    let snapshot = materialized_snapshot_with_documents(Vec::new());
    let bootstrap = DurableJournalBootstrap {
        snapshot: snapshot.clone(),
        resume_after: SequenceNumber(4),
        bootstrap_cut: snapshot.durable_head,
        cursor_floor: SequenceNumber(0),
    };

    let mismatches = collect_durable_journal_bootstrap_mismatches(&snapshot, &bootstrap);
    let resume_after = mismatches
        .iter()
        .find(|mismatch| mismatch.path == "bootstrap.resume_after_sequence")
        .expect("resume_after mismatch should be reported");
    assert_eq!(resume_after.invariant, "bootstrap_metadata_match");
    assert_eq!(
        resume_after.left_scope,
        ConsistencyScope::AuthoritativeSnapshot
    );
    assert_eq!(resume_after.right_scope, ConsistencyScope::JournalBootstrap);
    assert!(resume_after.left_description.contains('1'));
    assert!(resume_after.right_description.contains('4'));
}

#[tokio::test]
async fn generated_task_history_matches_model_across_live_shadow_and_embedded_replica_surfaces() {
    let history = GeneratedTaskHistory::seeded("engine-generated-history", 41, 48);
    assert_generated_task_history_matches_model_across_surfaces(
        &history,
        None,
        "generated_task_history_matches_model_across_live_shadow_and_embedded_replica_surfaces",
    )
    .await;
}

#[tokio::test]
#[ignore = "verification harness PR corpus runs in dedicated harness lanes"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let history = case.history("engine-generated-history");
        assert_generated_task_history_matches_model_across_surfaces(
            &history,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "verification harness nightly corpus runs in dedicated harness lanes"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let history = case.history("engine-generated-history");
        assert_generated_task_history_matches_model_across_surfaces(
            &history,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
async fn schema_async_write_path_rebuilds_and_removes_indexes_durably_redb() {
    assert_schema_async_write_path_rebuilds_and_removes_indexes_durably(EmbeddedProviderKind::Redb)
        .await;
}

#[tokio::test]
async fn schema_async_write_path_rebuilds_and_removes_indexes_durably_sqlite() {
    assert_schema_async_write_path_rebuilds_and_removes_indexes_durably(
        EmbeddedProviderKind::Sqlite,
    )
    .await;
}

async fn assert_schema_async_write_path_rebuilds_and_removes_indexes_durably(
    backend: EmbeddedProviderKind,
) {
    let data_dir = tempdir().expect("service tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    {
        let service = Arc::new(
            Service::new_with_embedded_provider(data_dir.path(), backend)
                .expect("service should create"),
        );
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(7))]),
            )
            .expect("insert should succeed");
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(9))]),
            )
            .expect("insert should succeed");
        service
            .set_table_schema_async(tenant_id.clone(), schema.clone())
            .await
            .expect("schema should save");
        service.quiesce().await;
        drop_service_sync(service).await;
    }

    {
        let reopened_service = open_service_after_embedded_lock_release(
            data_dir.path(),
            backend,
            "service should reopen after schema write",
        )
        .await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
        reopened_service
            .get_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("persisted schema should reload through the service path");
        drop_service_sync(reopened_service).await;
    }

    assert_eq!(
        index_scan_eq_count_for_backend(data_dir.path(), &tenant_id, backend, &json!(7)),
        1
    );

    {
        let service = open_service_after_embedded_lock_release(
            data_dir.path(),
            backend,
            "service should recreate",
        )
        .await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
        service
            .delete_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("schema should delete");
        service.quiesce().await;
        drop_service_sync(service).await;
    }

    assert!(
        index_scan_eq_count_for_backend(data_dir.path(), &tenant_id, backend, &json!(7)) == 0,
        "async schema deletion should clear rebuilt index entries"
    );
}

async fn drop_service_sync(service: Arc<Service>) {
    std::thread::spawn(move || drop(service))
        .join()
        .expect("service drop should join");
}

async fn open_service_after_embedded_lock_release(
    data_dir: &std::path::Path,
    backend: EmbeddedProviderKind,
    context: &'static str,
) -> Arc<Service> {
    let started = std::time::Instant::now();
    loop {
        match Service::new_with_embedded_provider(data_dir, backend) {
            Ok(service) => return Arc::new(service),
            Err(error)
                if backend == EmbeddedProviderKind::Redb
                    && error
                        .storage_message()
                        .is_some_and(|message| message.contains("Database already open"))
                    && started.elapsed() < std::time::Duration::from_secs(2) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(error) => panic!("{context}: {error:?}"),
        }
    }
}

async fn wait_for_embedded_tenant_unlock(
    data_dir: &std::path::Path,
    tenant_id: &TenantId,
    backend: EmbeddedProviderKind,
) {
    if backend != EmbeddedProviderKind::Redb {
        return;
    }

    let tenant_path = tenant_storage_path(data_dir, tenant_id, backend);
    let started = std::time::Instant::now();
    loop {
        match TenantStore::open(&tenant_path) {
            Ok(store) => {
                drop(store);
                return;
            }
            Err(error)
                if error
                    .storage_message()
                    .is_some_and(|message| message.contains("Database already open"))
                    && started.elapsed() < std::time::Duration::from_secs(2) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(error) => panic!("tenant store should reopen after prior service drop: {error:?}"),
        }
    }
}

fn tenant_storage_path(
    data_dir: &std::path::Path,
    tenant_id: &TenantId,
    backend: EmbeddedProviderKind,
) -> std::path::PathBuf {
    data_dir.join(format!(
        "{}.{}",
        tenant_id.as_str(),
        backend.tenant_file_extension()
    ))
}

fn append_durable_records_for_backend(
    data_dir: &std::path::Path,
    tenant_id: &TenantId,
    backend: EmbeddedProviderKind,
    records: &[neovex_core::DurableMutationRecord],
) {
    let path = tenant_storage_path(data_dir, tenant_id, backend);
    match backend {
        EmbeddedProviderKind::Redb => {
            let store = TenantStore::open(path).expect("tenant store should open");
            store
                .append_durable_records_batch(records)
                .expect("durable journal append should succeed");
        }
        EmbeddedProviderKind::Sqlite => {
            let store = SqliteTenantStore::open(path).expect("sqlite tenant store should open");
            store
                .append_durable_records_batch(records)
                .expect("durable journal append should succeed");
        }
    }
}

fn index_scan_eq_count_for_backend(
    data_dir: &std::path::Path,
    tenant_id: &TenantId,
    backend: EmbeddedProviderKind,
    value: &serde_json::Value,
) -> usize {
    let path = tenant_storage_path(data_dir, tenant_id, backend);
    match backend {
        EmbeddedProviderKind::Redb => {
            let store = TenantStore::open(path).expect("tenant store should reopen");
            store
                .index_scan_eq(&tasks_table(), "by_rank", value)
                .expect("index scan should succeed")
                .len()
        }
        EmbeddedProviderKind::Sqlite => {
            let store = SqliteTenantStore::open(path).expect("sqlite tenant store should reopen");
            let schema = store.load_schema().expect("sqlite schema should load");
            if schema.get_table(&tasks_table()).is_none() {
                return 0;
            }
            store
                .index_scan_eq(&tasks_table(), "by_rank", value)
                .expect("index scan should succeed")
                .len()
        }
    }
}
