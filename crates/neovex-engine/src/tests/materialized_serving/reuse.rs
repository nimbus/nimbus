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
