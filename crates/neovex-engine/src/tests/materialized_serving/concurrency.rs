use super::*;

#[test]
fn materialized_surface_handles_concurrent_reads_and_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_concurrent");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("seed")),
            ]),
        )
        .expect("seed insert should succeed");
    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["seed"]);

    let barrier = Arc::new(Barrier::new(2));
    let reader_service = service.clone();
    let reader_tenant = tenant_id.clone();
    let reader_query = query.clone();
    let reader_barrier = barrier.clone();
    let reader = std::thread::spawn(move || {
        reader_barrier.wait();
        for _ in 0..64 {
            let documents = reader_service
                .query_documents(&reader_tenant, &reader_query)
                .expect("concurrent materialized query should succeed");
            let bodies = document_bodies(&documents);
            let mut sorted = bodies.clone();
            sorted.sort_unstable();
            assert_eq!(bodies, sorted);
            let unique = bodies.iter().copied().collect::<BTreeSet<_>>();
            assert_eq!(unique.len(), bodies.len());
        }
    });

    let writer_service = service.clone();
    let writer_tenant = tenant_id.clone();
    let writer_table = table.clone();
    let writer_barrier = barrier;
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for index in 0..32 {
            writer_service
                .insert_document(
                    &writer_tenant,
                    writer_table.clone(),
                    serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-123")),
                        ("body".to_string(), json!(format!("msg-{index:02}"))),
                    ]),
                )
                .expect("concurrent insert should succeed");
        }
    });

    reader.join().expect("reader thread should finish");
    writer.join().expect("writer thread should finish");

    let documents = service
        .query_documents(&tenant_id, &query)
        .expect("final query should succeed");
    let bodies = document_bodies(&documents);
    let mut sorted = bodies.clone();
    sorted.sort_unstable();
    assert_eq!(bodies, sorted);
    assert_eq!(bodies.len(), 33);
    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.in_flight_load_count, 0);
    assert!(
        (1..=2).contains(&stats.table_load_count),
        "concurrent writes may force one catch-up warm load beyond the initial publication"
    );
    assert!(stats.evaluation_count >= 66);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(journal_stats.applied_head)
    );
}

#[tokio::test]
async fn paused_first_load_catches_up_before_publication() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_bypass");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let publish_pause = service
        .materialized_read_publish_pause_handle_for_testing(&tenant_id)
        .expect("publish pause handle should load");
    publish_pause.arm();

    let first_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        async move { service.query_documents_async(tenant_id, query).await }
    });

    assert!(
        tokio::task::spawn_blocking({
            let publish_pause = publish_pause.clone();
            move || publish_pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause waiter should join"),
        "first warmer should pause before publication"
    );

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.in_flight_load_count, 1);
    assert_eq!(stats.bypass_count, 0);

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("concurrent insert should succeed");

    publish_pause.release();

    let first_query = first_query
        .await
        .expect("first query task should join")
        .expect("first query should succeed");
    assert_eq!(document_bodies(&first_query), vec!["Ada", "Beta"]);

    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("publication should load")
        .expect("first query should publish its snapshot");
    let after_insert_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    assert_eq!(
        publication.covered_sequence,
        after_insert_stats.applied_head
    );
    assert_eq!(publication.document_count, 2);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.bypass_count, 0);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.in_flight_load_count, 0);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_insert_stats.applied_head)
    );
}

#[tokio::test]
async fn concurrent_first_load_only_publishes_caught_up_newest_materialized_table() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_concurrent_publish");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let publish_pause = service
        .materialized_read_publish_pause_handle_for_testing(&tenant_id)
        .expect("publish pause handle should load");
    publish_pause.arm();

    let first_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        async move { service.query_documents_async(tenant_id, query).await }
    });

    assert!(
        tokio::task::spawn_blocking({
            let publish_pause = publish_pause.clone();
            move || publish_pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause waiter should join"),
        "first loader should pause before publication"
    );
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &table)
            .expect("materialized publication should load")
            .is_none(),
        "no partially caught-up table should be visible before publication"
    );

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("concurrent insert should succeed");
    let after_insert_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");

    let second_query = tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        move || service.query_documents(&tenant_id, &query)
    });

    let stats = wait_for_value(
        "second materialized reader should stay in-flight without publishing a second table",
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            service
                .materialized_read_surface_stats_for_testing(&tenant_id)
                .expect("materialized surface stats should load")
        },
        |stats| stats.in_flight_load_count == 1,
    )
    .await;
    assert_eq!(stats.in_flight_load_count, 1);
    assert_eq!(stats.table_load_count, 0);
    assert_eq!(stats.bypass_count, 0);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &table)
            .expect("materialized publication should load")
            .is_none(),
        "waiting readers should not publish a second in-flight table"
    );

    publish_pause.release();

    let first_query = first_query
        .await
        .expect("first query task should join")
        .expect("first query should succeed");
    assert_eq!(document_bodies(&first_query), vec!["Ada", "Beta"]);
    let second_query = second_query
        .await
        .expect("second query task should join")
        .expect("second query should succeed");
    assert_eq!(document_bodies(&second_query), vec!["Ada", "Beta"]);

    let publication_after_release = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should remain published");
    assert_eq!(
        publication_after_release.covered_sequence,
        after_insert_stats.applied_head
    );
    assert_eq!(publication_after_release.document_count, 2);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.bypass_count, 0);
    assert_eq!(stats.in_flight_load_count, 0);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_insert_stats.applied_head)
    );
}
