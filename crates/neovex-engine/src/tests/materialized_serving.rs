use super::*;

#[test]
fn full_scan_queries_warm_materialized_surface_and_warm_table_gets_reuse_it() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_reads");

    let keep_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("first insert should succeed");
    let warm_only_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("skip")),
                ("body".to_string(), json!("Hidden")),
            ]),
        )
        .expect("second insert should succeed");
    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("third insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let first = service
        .query_documents(&tenant_id, &query)
        .expect("first full-scan query should succeed");
    assert_eq!(document_bodies(&first), vec!["Ada", "Beta"]);
    assert_eq!(first[0].id, keep_id);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 1);
    assert_eq!(stats.paginated_count, 0);
    assert_eq!(stats.get_hit_count, 0);

    let second = service
        .query_documents(&tenant_id, &query)
        .expect("second full-scan query should succeed");
    assert_eq!(document_bodies(&second), vec!["Ada", "Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 2);

    let warm_only = service
        .get_document(&tenant_id, &table, warm_only_id)
        .expect("warm-table get should succeed from the materialized surface");
    assert_eq!(warm_only.get_field("body"), Some(&json!("Hidden")));

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.get_hit_count, 1);
}

#[test]
fn pinned_materialized_serving_snapshots_remain_stable_after_later_applies() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_serving_handle_stability");

    let _ = service
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

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let before_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let pinned = service
        .materialized_serving_snapshot_for_testing(&tenant_id, before_insert)
        .expect("serving snapshot should load")
        .expect("warmed table should expose a serving snapshot");
    assert_eq!(pinned.covered_sequence(), before_insert);
    let pinned_documents = pinned
        .table_documents(&table)
        .expect("pinned snapshot should include the warmed table");
    assert_eq!(document_bodies(&pinned_documents), vec!["Ada"]);

    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let after_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let current = service
        .materialized_serving_snapshot_for_testing(&tenant_id, after_insert)
        .expect("current serving snapshot should load")
        .expect("published serving snapshot should advance after apply");
    assert_eq!(current.covered_sequence(), after_insert);
    let current_documents = current
        .table_documents(&table)
        .expect("current snapshot should include the warmed table");
    let mut current_bodies = document_bodies(&current_documents)
        .into_iter()
        .collect::<Vec<_>>();
    current_bodies.sort_unstable();
    assert_eq!(current_bodies, vec!["Ada", "Beta"]);

    assert_eq!(pinned.covered_sequence(), before_insert);
    let pinned_documents = pinned
        .table_documents(&table)
        .expect("pinned snapshot should still include the warmed table");
    let pinned_bodies = document_bodies(&pinned_documents)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        pinned_bodies,
        vec!["Ada"],
        "a pinned serving snapshot should continue to reflect the exact frontier it captured"
    );
}

#[test]
fn materialized_surface_reacquires_retained_covering_version_for_older_required_sequence() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_serving_handle_retention");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 3)
        .expect("materialized surface version capacity should be configurable for tests");

    let _ = service
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

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let second_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let retained = service
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("retained serving snapshot should load")
        .expect("historical retained version should remain available");
    assert_eq!(retained.covered_sequence(), first_sequence);
    let retained_documents = retained
        .table_documents(&table)
        .expect("retained snapshot should include the warmed table");
    assert_eq!(document_bodies(&retained_documents), vec!["Ada"]);

    let current = service
        .materialized_serving_snapshot_for_testing(&tenant_id, second_sequence)
        .expect("current serving snapshot should load")
        .expect("current version should remain available");
    assert_eq!(current.covered_sequence(), second_sequence);
    let current_documents = current
        .table_documents(&table)
        .expect("current snapshot should include the warmed table");
    let mut current_bodies = document_bodies(&current_documents)
        .into_iter()
        .collect::<Vec<_>>();
    current_bodies.sort_unstable();
    assert_eq!(current_bodies, vec!["Ada", "Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.retained_version_count, 1);
    assert_eq!(stats.earliest_retained_sequence, Some(first_sequence));
    assert_eq!(stats.latest_retained_sequence, Some(first_sequence));
    assert_eq!(stats.latest_covered_sequence, Some(second_sequence));
}

#[test]
fn pinned_materialized_serving_snapshot_is_exact_across_multiple_loaded_tables() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_snapshot_alpha");
    let beta = messages_table("messages_snapshot_beta");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 4)
        .expect("materialized surface version capacity should be configurable for tests");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("alpha seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Gamma")),
            ]),
        )
        .expect("beta seed insert should succeed");

    let query_for = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    service
        .query_documents(&tenant_id, &query_for(alpha.clone()))
        .expect("alpha warm query should succeed");
    service
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta warm query should succeed");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("alpha update insert should succeed");
    let alpha_update_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("beta update insert should succeed");
    let latest_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let exact_snapshot = service
        .materialized_serving_snapshot_for_testing(&tenant_id, alpha_update_sequence)
        .expect("exact serving snapshot should load")
        .expect("snapshot at the alpha update frontier should be retained");
    assert_eq!(exact_snapshot.covered_sequence(), alpha_update_sequence);
    let alpha_documents = exact_snapshot
        .table_documents(&alpha)
        .expect("exact snapshot should include warmed alpha");
    let mut alpha_bodies = document_bodies(&alpha_documents)
        .into_iter()
        .collect::<Vec<_>>();
    alpha_bodies.sort_unstable();
    assert_eq!(alpha_bodies, vec!["Ada", "Beta"]);
    let beta_documents = exact_snapshot
        .table_documents(&beta)
        .expect("exact snapshot should include warmed beta");
    let beta_bodies = document_bodies(&beta_documents)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        beta_bodies,
        vec!["Gamma"],
        "the snapshot pinned at the earlier frontier should not include the later beta write"
    );

    let latest_snapshot = service
        .materialized_serving_snapshot_for_testing(&tenant_id, latest_sequence)
        .expect("latest serving snapshot should load")
        .expect("latest snapshot should remain available");
    assert_eq!(latest_snapshot.covered_sequence(), latest_sequence);
    let latest_beta_documents = latest_snapshot
        .table_documents(&beta)
        .expect("latest snapshot should include warmed beta");
    let mut latest_beta_bodies = document_bodies(&latest_beta_documents)
        .into_iter()
        .collect::<Vec<_>>();
    latest_beta_bodies.sort_unstable();
    assert_eq!(latest_beta_bodies, vec!["Delta", "Gamma"]);
}

#[tokio::test]
async fn serving_snapshot_waiter_wakes_when_new_frontier_is_published() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_snapshot_waiter");

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
    service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let required_sequence = SequenceNumber(first_sequence.0.saturating_add(1));

    let waiter = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .wait_for_materialized_serving_snapshot_for_testing(
                    tenant_id,
                    required_sequence,
                    std::future::pending::<()>(),
                )
                .await
        }
    });

    timeout(Duration::from_millis(200), async {
        loop {
            let stats = service
                .serving_snapshot_manager_stats_for_testing(&tenant_id)
                .expect("serving snapshot manager stats should load");
            if stats.waiter_count == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("waiter should register");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let snapshot = timeout(Duration::from_millis(200), waiter)
        .await
        .expect("snapshot waiter should wake")
        .expect("snapshot waiter task should join")
        .expect("snapshot waiter should succeed");
    assert_eq!(snapshot.covered_sequence(), required_sequence);
    let documents = snapshot
        .table_documents(&table)
        .expect("woken snapshot should include the target table");
    let mut bodies = document_bodies(&documents).into_iter().collect::<Vec<_>>();
    bodies.sort_unstable();
    assert_eq!(bodies, vec!["Ada", "Beta"]);

    let stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(stats.waiter_count, 0);
    assert_eq!(stats.latest_retained_sequence, Some(required_sequence));
}

#[test]
fn pinned_serving_snapshot_extends_retention_until_release() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_snapshot_pin_retention");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 2)
        .expect("materialized surface version capacity should be configurable for tests");

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
    service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let pinned = service
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("first serving snapshot should load")
        .expect("first serving snapshot should exist");

    for body in ["Beta", "Gamma"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("follow-up insert should succeed");
    }
    let third_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let pinned_stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(pinned_stats.retained_snapshot_count, 3);
    assert_eq!(
        pinned_stats.earliest_retained_sequence,
        Some(first_sequence)
    );
    assert_eq!(pinned_stats.latest_retained_sequence, Some(third_sequence));
    assert_eq!(pinned_stats.pinned_snapshot_count, 1);

    drop(pinned);

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("final insert should succeed");
    let fourth_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let released_stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(released_stats.retained_snapshot_count, 2);
    assert_eq!(
        released_stats.earliest_retained_sequence,
        Some(third_sequence)
    );
    assert_eq!(
        released_stats.latest_retained_sequence,
        Some(fourth_sequence)
    );
    assert_eq!(released_stats.pinned_snapshot_count, 0);
    assert!(
        released_stats.pruned_snapshot_count >= 2,
        "older snapshots should prune once the pin is released"
    );
}

#[test]
fn warmed_materialized_tables_track_global_applied_coverage_without_reloading() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_coverage");

    let _document_id = service
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

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should publish");
    assert_eq!(publication.covered_sequence, journal_stats.applied_head);
    assert_eq!(publication.document_count, 1);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should stay published");
    assert_eq!(publication.covered_sequence, journal_stats.applied_head);
    assert_eq!(publication.document_count, 1);

    let refreshed = service
        .query_documents(&tenant_id, &query)
        .expect("refreshed query should reuse the warmed publication");
    assert_eq!(document_bodies(&refreshed), vec!["Ada"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 2);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(journal_stats.applied_head)
    );
}

#[test]
fn warmed_tables_do_not_block_each_other_from_reusing_serving_snapshots() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_alpha_reuse");
    let beta = messages_table("messages_materialized_beta_reuse");

    for (table, body) in [(alpha.clone(), "Alpha"), (beta.clone(), "Beta")] {
        service
            .insert_document(
                &tenant_id,
                table,
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query_for = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    assert_eq!(
        document_bodies(
            &service
                .query_documents(&tenant_id, &query_for(alpha.clone()))
                .expect("alpha warm query should succeed"),
        ),
        vec!["Alpha"]
    );
    assert_eq!(
        document_bodies(
            &service
                .query_documents(&tenant_id, &query_for(beta.clone()))
                .expect("beta warm query should succeed"),
        ),
        vec!["Beta"]
    );

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let beta_again = service
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta query should reuse the warmed serving snapshot");
    assert_eq!(document_bodies(&beta_again), vec!["Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 2);
    assert_eq!(stats.table_load_count, 2);
    assert_eq!(stats.evaluation_count, 3);
    assert_eq!(stats.retained_version_count, 0);
    assert_eq!(stats.retained_estimated_bytes, 0);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let beta_publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &beta)
        .expect("beta publication stats should load")
        .expect("beta table should stay published");
    assert_eq!(
        beta_publication.covered_sequence,
        journal_stats.applied_head
    );
}

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

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.table_load_count, 1);
    assert!(stats.evaluation_count >= 66);
}

#[test]
fn materialized_surface_evicts_least_recently_used_tables_under_byte_budget() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_alpha");
    let beta = messages_table("messages_materialized_beta");

    service
        .set_materialized_read_surface_limits_for_testing(&tenant_id, 8, 1)
        .expect("materialized surface limits should be configurable for tests");

    for table in [alpha.clone(), beta.clone()] {
        service
            .insert_document(
                &tenant_id,
                table,
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!("payload that exceeds one byte")),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query_for_table = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let alpha_docs = service
        .query_documents(&tenant_id, &query_for_table(alpha.clone()))
        .expect("alpha warm query should succeed");
    assert_eq!(alpha_docs.len(), 1);

    let beta_docs = service
        .query_documents(&tenant_id, &query_for_table(beta.clone()))
        .expect("beta warm query should succeed");
    assert_eq!(beta_docs.len(), 1);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 2);
    assert_eq!(stats.eviction_count, 1);
    assert_eq!(stats.resident_document_count, 1);
    assert_eq!(stats.byte_capacity, 1);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
            .expect("alpha publication should load")
            .is_none(),
        "older table should be evicted under the byte budget"
    );
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &beta)
            .expect("beta publication should load")
            .is_some(),
        "newest table should remain resident"
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

    tokio::time::sleep(Duration::from_millis(25)).await;
    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
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

#[tokio::test]
async fn async_paginated_full_scans_reuse_and_refresh_materialized_surface_after_async_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_paginated");

    for body in ["Beta", "Delta", "Gamma"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query = PaginatedQuery {
        query: Query {
            table: table.clone(),
            filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
            order: Some(OrderBy {
                field: "body".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };

    let first_page = service
        .paginate_documents_async(tenant_id.clone(), query.clone())
        .await
        .expect("first paginated full-scan query should succeed");
    assert_eq!(subscription_bodies(&first_page.data), vec!["Beta", "Delta"]);
    assert!(first_page.has_more);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.paginated_count, 1);

    service
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Able")),
            ]),
        )
        .await
        .expect("async insert after warmup should succeed");

    let refreshed_page = service
        .paginate_documents_async(tenant_id.clone(), query)
        .await
        .expect("refreshed paginated full-scan query should succeed");
    assert_eq!(
        subscription_bodies(&refreshed_page.data),
        vec!["Able", "Beta"]
    );
    assert!(refreshed_page.has_more);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.paginated_count, 2);
}

#[tokio::test]
async fn materialized_surface_rewarms_evicted_tables_and_publishes_fresh_frontiers_after_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_rewarm_alpha");
    let beta = messages_table("messages_materialized_rewarm_beta");

    service
        .set_materialized_read_surface_limits_for_testing(&tenant_id, 1, usize::MAX)
        .expect("materialized surface limits should be configurable for tests");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("alpha seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Gamma")),
            ]),
        )
        .expect("beta seed insert should succeed");

    let alpha_query = Query {
        table: alpha.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let beta_query = Query {
        table: beta.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let warmed_alpha = service
        .query_documents(&tenant_id, &alpha_query)
        .expect("warming alpha should succeed");
    assert_eq!(document_bodies(&warmed_alpha), vec!["Ada"]);

    let alpha_publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("alpha should publish after the first warm load");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("resident alpha insert should succeed");
    let after_resident_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after resident insert");
    let alpha_after_resident_insert = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("resident alpha table should stay published");
    assert_eq!(
        alpha_after_resident_insert.generation, alpha_publication.generation,
        "resident apply should advance coverage in place instead of republishing the table"
    );
    assert_eq!(
        alpha_after_resident_insert.covered_sequence,
        after_resident_insert.applied_head
    );
    assert_eq!(alpha_after_resident_insert.document_count, 2);

    let warmed_beta = service
        .query_documents(&tenant_id, &beta_query)
        .expect("warming beta should succeed");
    assert_eq!(document_bodies(&warmed_beta), vec!["Gamma"]);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
            .expect("alpha publication should load")
            .is_none(),
        "warming beta under a one-table budget should evict alpha"
    );

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("evicted alpha insert should succeed");
    let after_rewarm_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after evicted insert");

    let rewarmed_alpha = service
        .query_documents(&tenant_id, &alpha_query)
        .expect("rewarming alpha should succeed");
    assert_eq!(
        document_bodies(&rewarmed_alpha),
        vec!["Ada", "Beta", "Delta"]
    );

    let republished_alpha = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("rewarmed alpha should publish again");
    assert!(
        republished_alpha.generation > alpha_publication.generation,
        "rewarming an evicted table should publish a newer generation"
    );
    assert_eq!(republished_alpha.document_count, 3);
    assert_eq!(
        republished_alpha.covered_sequence, after_rewarm_insert.applied_head,
        "rewarmed tables should publish the exact frontier they cover"
    );

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 3);
    assert_eq!(stats.eviction_count, 2);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_rewarm_insert.applied_head)
    );
}
