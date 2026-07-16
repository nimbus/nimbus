use super::*;

#[tokio::test]
async fn engine_reload_recovers_durable_journal_before_serving_async_reads_redb() {
    assert_engine_reload_recovers_durable_journal_before_serving_async_reads(
        EmbeddedProviderKind::Redb,
    )
    .await;
}

#[tokio::test]
async fn engine_reload_recovers_durable_journal_before_serving_async_reads_sqlite() {
    assert_engine_reload_recovers_durable_journal_before_serving_async_reads(
        EmbeddedProviderKind::Sqlite,
    )
    .await;
}

async fn assert_engine_reload_recovers_durable_journal_before_serving_async_reads(
    backend: EmbeddedProviderKind,
) {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let engine =
        Engine::new_with_embedded_provider(data_dir.path(), backend).expect("engine should create");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    drop(engine);

    let document = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("recovered"))]),
    );
    append_durable_records_for_backend(
        data_dir.path(),
        &tenant_id,
        backend,
        &[nimbus_core::TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(60_000),
            vec![nimbus_core::WriteOp {
                table: document.table.clone(),
                table_id: nimbus_core::TableId::new(),
                op_type: nimbus_core::WriteOpType::Insert,
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
        Engine::new_with_embedded_provider(data_dir.path(), backend).expect("engine should reopen"),
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
            worker_running: true,
            worker_start_count: 1,
            worker_restart_count: 0,
            queue_rejection_count: 0,
            worker_failure_count: 0,
            read_wait_count: 0,
            total_read_wait_nanos: 0,
            committer_inbox_depth: 0,
            committer_inbox_capacity: 128,
            committer_send_timeout_count: 0,
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
        "mutation committer to drain after the follow-up async write",
        |stats| stats.queue_depth == 0 && stats.pending_response_count == 0,
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
    assert!(recovered_stats.worker_running);
    assert_eq!(recovered_stats.worker_start_count, 1);
    assert_eq!(recovered_stats.worker_restart_count, 0);
    assert_eq!(recovered_stats.queue_rejection_count, 0);
    assert_eq!(recovered_stats.worker_failure_count, 0);
}

#[tokio::test]
async fn durable_journal_reads_return_strictly_ordered_authoritative_records() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("journal"))]),
        )
        .await
        .expect("insert should succeed");
    engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("journal-updated"))]),
        )
        .await
        .expect("update should succeed");

    let records = engine
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("durable journal should read");
    assert!(
        records
            .windows(2)
            .all(|window| window[0].sequence < window[1].sequence),
        "durable journal records should be strictly ordered: {records:?}"
    );
    let document_records = records
        .iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(document_records.len(), 2);
    assert_eq!(
        document_records[0].writes[0].op_type,
        nimbus_core::WriteOpType::Insert
    );
    assert_eq!(
        document_records[1].writes[0].op_type,
        nimbus_core::WriteOpType::Update
    );
    assert_eq!(
        document_records[1].writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("journal-updated"))
    );

    let filtered = engine
        .read_durable_journal_async(tenant_id, SequenceNumber(1))
        .await
        .expect("filtered durable journal should read");
    assert!(filtered.iter().all(|record| record.sequence.0 > 1));
    let filtered_document_records = filtered
        .iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(filtered_document_records.len(), 1);
    assert_eq!(
        filtered_document_records[0].writes[0].op_type,
        nimbus_core::WriteOpType::Update
    );
}

#[tokio::test]
async fn durable_journal_stream_resumes_from_sequence_cursor_with_duplicate_tolerant_pages() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        )
        .await
        .expect("first insert should succeed");
    engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            first_document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("first-updated"))]),
        )
        .await
        .expect("update should succeed");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
        )
        .await
        .expect("second insert should succeed");

    let latest_sequence = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("latest sequence should load");
    assert!(latest_sequence.0 >= 3);

    let first_page = engine
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("first journal page should read");
    assert_eq!(first_page.cursor_floor, SequenceNumber(0));
    assert!(
        first_page.latest_sequence.0 >= latest_sequence.0,
        "stream pages may observe tenant metadata events appended after the initial latest-sequence read"
    );
    let mut observed_latest_sequence = first_page.latest_sequence;
    assert!(first_page.has_more);
    assert_eq!(first_page.next_cursor, SequenceNumber(1));
    assert_eq!(first_page.records.len(), 1);
    assert_eq!(first_page.records[0].sequence, SequenceNumber(1));

    let replayed_first_page = engine
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("replayed first journal page should read");
    assert_eq!(replayed_first_page.records, first_page.records);
    assert_eq!(replayed_first_page.next_cursor, first_page.next_cursor);

    let second_page = engine
        .stream_durable_journal_async(tenant_id.clone(), first_page.next_cursor, 1)
        .await
        .expect("second journal page should read");
    assert!(second_page.has_more);
    assert_eq!(second_page.next_cursor, SequenceNumber(2));
    assert_eq!(second_page.records.len(), 1);
    assert_eq!(second_page.records[0].sequence, SequenceNumber(2));

    let mut cursor = second_page.next_cursor;
    let mut streamed_records = vec![
        first_page.records[0].clone(),
        second_page.records[0].clone(),
    ];
    observed_latest_sequence = observed_latest_sequence.max(second_page.latest_sequence);
    while cursor.0 < observed_latest_sequence.0 {
        let page = engine
            .stream_durable_journal_async(tenant_id.clone(), cursor, 1)
            .await
            .expect("next journal page should read");
        observed_latest_sequence = observed_latest_sequence.max(page.latest_sequence);
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].sequence, SequenceNumber(cursor.0 + 1));
        cursor = page.next_cursor;
        streamed_records.push(page.records[0].clone());
        assert!(
            !page.has_more || cursor.0 < page.latest_sequence.0,
            "a non-final page should report a latest sequence beyond its returned cursor"
        );
    }
    assert!(cursor.0 >= observed_latest_sequence.0);
    assert_eq!(
        streamed_records
            .iter()
            .filter(|record| !record.writes.is_empty())
            .count(),
        3
    );

    let tail_page = engine
        .stream_durable_journal_async(tenant_id, cursor, 1)
        .await
        .expect("tail journal page should read");
    if tail_page.records.is_empty() {
        assert!(!tail_page.has_more);
        assert_eq!(tail_page.next_cursor, cursor);
    } else {
        assert_eq!(tail_page.records[0].sequence, SequenceNumber(cursor.0 + 1));
        assert!(
            tail_page
                .records
                .iter()
                .all(|record| record.writes.is_empty()),
            "only background tenant events may race after the streamed document records"
        );
    }
    assert!(tail_page.latest_sequence.0 >= cursor.0);
}

#[tokio::test]
async fn durable_journal_bootstrap_metadata_reconstructs_same_state_as_live_reads() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(80_000))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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

    let bootstrap = engine
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await
        .expect("bootstrap metadata should read");
    assert_eq!(bootstrap.resume_after, SequenceNumber(0));
    assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(1));
    assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.durable_head, SequenceNumber(1));
    assert!(bootstrap.snapshot.documents.is_empty());

    let page = engine
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

    let live_documents = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        engine
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

    let replica = EmbeddedReplica::bootstrap_in_memory(&engine, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    let live_query = engine
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
    let live_page = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("before"))]),
        )
        .await
        .expect("initial insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&engine, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    let latest_after_catch_up = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("latest sequence should load");
    assert!(
        replica.sequence_cursor().0 <= latest_after_catch_up.0,
        "a background tenant event may advance the source after catch-up chooses its target"
    );

    engine
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
        .catch_up(&engine, 1)
        .await
        .expect("replica catch-up should succeed");
    let latest_after_catch_up = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("latest sequence should load");
    assert!(
        replica.sequence_cursor().0 <= latest_after_catch_up.0,
        "a background tenant event may advance the source after catch-up chooses its target"
    );

    let live_documents = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
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
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

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
    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Grace")),
            ]),
        )
        .expect("fixture insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&engine, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    assert_eq!(
        replica.sequence_cursor(),
        engine
            .latest_sequence(&tenant_id)
            .expect("latest sequence should load")
    );

    engine
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
        .catch_up(&engine, 1)
        .await
        .expect("replica catch-up should refresh schema even without new journal records");
    assert_eq!(
        replica.sequence_cursor(),
        engine
            .latest_sequence(&tenant_id)
            .expect("latest sequence should load")
    );

    let live_documents = engine
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("live principal query should succeed");
    let replica_documents = replica
        .query_documents_with_principal(&query, &principal)
        .expect("replica principal query should succeed");
    assert_eq!(document_bodies(&replica_documents), vec!["Ada"]);
    assert_eq!(replica_documents, live_documents);

    let live_anonymous = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for rank in [1, 2, 3] {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&engine, tenant_id.clone())
        .await
        .expect("replica should bootstrap");

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

    replica
        .catch_up(&engine, 1)
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
    let live_documents = engine
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
async fn shadow_materializer_queries_match_live_engine_path() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        engine
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

    let shadow = engine
        .build_shadow_materializer_async(
            tenant_id.clone(),
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .await
        .expect("shadow materializer should build");
    let latest_sequence = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("latest sequence should load");
    let shadow_sequence = shadow.manifest().current_sequence;
    let snapshot = shadow.current_snapshot();
    assert_eq!(snapshot.applied_sequence, shadow_sequence);
    assert_eq!(snapshot.durable_head, shadow_sequence);
    assert!(
        shadow_sequence.0 <= latest_sequence.0,
        "shadow materializer sequence {} must not exceed latest durable sequence {}",
        shadow_sequence.0,
        latest_sequence.0
    );
    if shadow_sequence.0 < latest_sequence.0 {
        let tail_after_shadow = engine
            .read_durable_journal_async(tenant_id.clone(), shadow_sequence)
            .await
            .expect("tail after shadow cut should read");
        let document_bearing_tail = tail_after_shadow
            .iter()
            .filter(|record| !record.writes.is_empty())
            .map(|record| record.sequence)
            .collect::<Vec<_>>();
        assert!(
            document_bearing_tail.is_empty(),
            "shadow materializer cut missed document-bearing records after its bootstrap cut: {document_bearing_tail:?}"
        );
    }

    let ordered_query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_query = engine
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
    let live_page = engine
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
async fn shadow_materializer_schema_aware_queries_match_live_engine_path() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = messages_table("messages_shadow_schema");
    let principal = principal_with_subject("user-123");
    let hidden_owner = principal_with_subject("user-456");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_shadow_schema",
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
        engine
            .insert_document_with(
                &tenant_id,
                table.clone(),
                None,
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
                crate::MutationActor::with_principal(&principal),
            )
            .expect("seed insert should succeed");
    }

    let shadow = engine
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
    let live_query = engine
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
    let live_page = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
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

    for rank in [1, 2, 3] {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let report = engine
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
    let document = nimbus_core::Document::new(
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

/// Builds a snapshot carrying explicit table identities and no documents, so a
/// test can drive table-identity drift in isolation from document/schema state.
fn snapshot_with_table_identities(
    table_identities: Vec<crate::TableIdentitySnapshotEntry>,
) -> crate::MaterializedJournalSnapshot {
    crate::MaterializedJournalSnapshot {
        version: 2,
        applied_sequence: SequenceNumber(1),
        durable_head: SequenceNumber(1),
        table_identities,
        schema: nimbus_core::Schema::default(),
        documents: Vec::new(),
        scheduled_execution_ids: Vec::new(),
    }
}

#[test]
fn snapshot_comparison_reports_table_identity_state_drift() {
    // Same namespace/table/table_id on both sides; only the lifecycle state
    // diverges. The digest hashes table_identities, so the equality verifier
    // must surface this mismatch rather than reporting green on drift.
    let table_id = nimbus_core::TableId::new();
    let identity = |state: nimbus_core::TableState| crate::TableIdentitySnapshotEntry {
        namespace: "default".to_string(),
        table: tasks_table(),
        table_id: table_id.clone(),
        state,
    };
    let left = snapshot_with_table_identities(vec![identity(nimbus_core::TableState::Active)]);
    let right = snapshot_with_table_identities(vec![identity(nimbus_core::TableState::Deleting)]);

    let mismatch = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        &left,
        ConsistencyScope::ShadowMaterializer,
        &right,
    )
    .expect("table identity state drift should be reported");

    assert_eq!(mismatch.invariant, "materialized_snapshot_match");
    assert!(
        mismatch.path.starts_with("table_identities."),
        "expected per-identity path, got {}",
        mismatch.path
    );
    assert_eq!(mismatch.left_scope, ConsistencyScope::AuthoritativeSnapshot);
    assert_eq!(mismatch.right_scope, ConsistencyScope::ShadowMaterializer);
    assert!(
        mismatch.left_description.contains("active"),
        "{}",
        mismatch.left_description
    );
    assert!(
        mismatch.right_description.contains("deleting"),
        "{}",
        mismatch.right_description
    );
}

#[test]
fn snapshot_comparison_reports_table_identity_table_id_drift() {
    // Identical namespace/table/state, but the stable table_id differs. The
    // identity-key cardinality check (namespace/table) passes, so this must be
    // caught by the per-entry comparison, not silently hashed-but-ignored.
    let identity = |table_id: nimbus_core::TableId| crate::TableIdentitySnapshotEntry {
        namespace: "default".to_string(),
        table: tasks_table(),
        table_id,
        state: nimbus_core::TableState::Active,
    };
    let left = snapshot_with_table_identities(vec![identity(nimbus_core::TableId::new())]);
    let right = snapshot_with_table_identities(vec![identity(nimbus_core::TableId::new())]);

    let mismatch = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        &left,
        ConsistencyScope::ShadowMaterializer,
        &right,
    )
    .expect("table identity table_id drift should be reported");

    assert_eq!(mismatch.invariant, "materialized_snapshot_match");
    assert_eq!(
        mismatch.path,
        format!("table_identities.default/{}", tasks_table())
    );
    assert!(
        mismatch
            .left_description
            .contains(left.table_identities[0].table_id.as_str()),
        "{}",
        mismatch.left_description
    );
    assert!(
        mismatch
            .right_description
            .contains(right.table_identities[0].table_id.as_str()),
        "{}",
        mismatch.right_description
    );
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
#[ignore = "verification harness required corpus runs in dedicated harness lanes"]
async fn verification_harness_required_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Required)
        .expect("required corpus should resolve")
    {
        let history = case.history("engine-generated-history");
        assert_generated_task_history_matches_model_across_surfaces(
            &history,
            Some(case),
            "verification_harness_required_generated_history_seed_corpus_matches_model",
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
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

    {
        let engine = Arc::new(
            Engine::new_with_embedded_provider(data_dir.path(), backend)
                .expect("engine should create"),
        );
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(7))]),
            )
            .expect("insert should succeed");
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(9))]),
            )
            .expect("insert should succeed");
        engine
            .set_table_schema_async(tenant_id.clone(), schema.clone())
            .await
            .expect("schema should save");
        engine.quiesce().await;
        drop_engine_sync(engine).await;
    }

    {
        let reopened_engine = open_engine_after_embedded_lock_release(
            data_dir.path(),
            backend,
            "engine should reopen after schema write",
        )
        .await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
        reopened_engine
            .get_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("persisted schema should reload through the engine path");
        reopened_engine.quiesce().await;
        drop_engine_sync(reopened_engine).await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
    }

    assert_eq!(
        index_scan_eq_count_for_backend(data_dir.path(), &tenant_id, backend, &json!(7)),
        1
    );

    {
        let engine = open_engine_after_embedded_lock_release(
            data_dir.path(),
            backend,
            "engine should recreate",
        )
        .await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
        engine
            .delete_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("schema should delete");
        engine.quiesce().await;
        drop_engine_sync(engine).await;
        wait_for_embedded_tenant_unlock(data_dir.path(), &tenant_id, backend).await;
    }

    assert!(
        index_scan_eq_count_for_backend(data_dir.path(), &tenant_id, backend, &json!(7)) == 0,
        "async schema deletion should clear rebuilt index entries"
    );
}

async fn drop_engine_sync(engine: Arc<Engine>) {
    std::thread::spawn(move || drop(engine))
        .join()
        .expect("engine drop should join");
}

async fn open_engine_after_embedded_lock_release(
    data_dir: &std::path::Path,
    backend: EmbeddedProviderKind,
    context: &'static str,
) -> Arc<Engine> {
    let started = std::time::Instant::now();
    loop {
        match Engine::new_with_embedded_provider(data_dir, backend) {
            Ok(engine) => return Arc::new(engine),
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
            Err(error) => panic!("tenant store should reopen after prior engine drop: {error:?}"),
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
    records: &[nimbus_core::TenantEventRecord],
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
            let schema = store.load_schema().expect("tenant schema should load");
            if schema.get_table(&tasks_table()).is_none() {
                return 0;
            }
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
