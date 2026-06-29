use super::*;

#[test]
fn full_scan_queries_warm_materialized_surface_and_follow_up_full_scans_reuse_it() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_materialized_reads");

    let keep_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("first insert should succeed");
    let _warm_only_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("skip")),
                ("body".to_string(), json!("Hidden")),
            ]),
        )
        .expect("second insert should succeed");
    let _ = engine
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
    let skip_query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("skip"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let first = engine
        .query_documents(&tenant_id, &query)
        .expect("first full-scan query should succeed");
    assert_eq!(document_bodies(&first), vec!["Ada", "Beta"]);
    assert_eq!(first[0].id, keep_id);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 1);
    assert_eq!(stats.paginated_count, 0);
    assert_eq!(stats.get_hit_count, 0);

    let stats = warm_query_until_publication_covers_head(&engine, &tenant_id, &table, &query);
    let baseline_table_load_count = stats.table_load_count;
    let baseline_evaluation_count = stats.evaluation_count;

    let warm_only = engine
        .query_documents(&tenant_id, &skip_query)
        .expect("follow-up full-scan query should reuse the warmed materialized table");
    assert_eq!(document_bodies(&warm_only), vec!["Hidden"]);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, baseline_table_load_count);
    assert_eq!(stats.evaluation_count, baseline_evaluation_count + 1);
}

fn warm_query_until_publication_covers_head(
    engine: &Engine,
    tenant_id: &TenantId,
    table: &TableName,
    query: &Query,
) -> crate::MaterializedReadSurfaceStats {
    for _ in 0..4 {
        let documents = engine
            .query_documents(tenant_id, query)
            .expect("catch-up full-scan query should succeed");
        assert_eq!(document_bodies(&documents), vec!["Ada", "Beta"]);

        let stats = engine
            .materialized_read_surface_stats_for_testing(tenant_id)
            .expect("materialized surface stats should load");
        let publication = engine
            .materialized_table_publication_stats_for_testing(tenant_id, table)
            .expect("materialized publication stats should load")
            .expect("warmed table should publish");
        let journal = engine
            .mutation_journal_stats_for_testing(tenant_id)
            .expect("journal stats should load");
        if publication.covered_sequence.0 >= journal.durable_head.0 {
            assert_eq!(stats.loaded_table_count, 1);
            assert!(
                durable_journal_commits(engine, tenant_id, publication.covered_sequence).is_empty(),
                "publication should not miss document-bearing commits after coverage {}",
                publication.covered_sequence.0
            );
            return stats;
        }
    }

    panic!("materialized publication should catch up to the durable head");
}

#[test]
fn warmed_materialized_tables_track_global_applied_coverage_without_reloading() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_materialized_coverage");

    let _document_id = engine
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

    let warmed = engine
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let publication = engine
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should publish");
    let initial_covered_sequence = publication.covered_sequence;
    let initial_generation = publication.generation;
    assert_eq!(publication.document_count, 1);

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let publication = engine
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should stay published");
    assert_eq!(
        publication.generation, initial_generation,
        "unrelated writes should advance coverage without reloading the table"
    );
    assert!(
        publication.covered_sequence.0 > initial_covered_sequence.0,
        "unrelated commits should still advance the published coverage frontier"
    );
    assert_eq!(publication.document_count, 1);

    let refreshed = engine
        .query_documents(&tenant_id, &query)
        .expect("refreshed query should reuse the warmed publication");
    assert_eq!(document_bodies(&refreshed), vec!["Ada"]);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 2);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(publication.covered_sequence)
    );
}

#[test]
fn warmed_tables_do_not_block_each_other_from_reusing_serving_snapshots() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let alpha = messages_table("messages_materialized_alpha_reuse");
    let beta = messages_table("messages_materialized_beta_reuse");

    for (table, body) in [(alpha.clone(), "Alpha"), (beta.clone(), "Beta")] {
        engine
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
            &engine
                .query_documents(&tenant_id, &query_for(alpha.clone()))
                .expect("alpha warm query should succeed"),
        ),
        vec!["Alpha"]
    );
    assert_eq!(
        document_bodies(
            &engine
                .query_documents(&tenant_id, &query_for(beta.clone()))
                .expect("beta warm query should succeed"),
        ),
        vec!["Beta"]
    );

    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let beta_again = engine
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta query should reuse the warmed serving snapshot");
    assert_eq!(document_bodies(&beta_again), vec!["Beta"]);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 2);
    assert_eq!(stats.table_load_count, 2);
    assert_eq!(stats.evaluation_count, 3);
    assert_eq!(stats.retained_version_count, 0);
    assert_eq!(stats.retained_estimated_bytes, 0);

    let beta_publication = engine
        .materialized_table_publication_stats_for_testing(&tenant_id, &beta)
        .expect("beta publication stats should load")
        .expect("beta table should stay published");
    assert_eq!(
        stats.latest_covered_sequence,
        Some(beta_publication.covered_sequence)
    );
}

#[tokio::test]
async fn async_paginated_full_scans_reuse_and_refresh_materialized_surface_after_async_writes() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_materialized_paginated");

    for body in ["Beta", "Delta", "Gamma"] {
        engine
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

    let first_page = engine
        .paginate_documents_async(tenant_id.clone(), query.clone())
        .await
        .expect("first paginated full-scan query should succeed");
    assert_eq!(subscription_bodies(&first_page.data), vec!["Beta", "Delta"]);
    assert!(first_page.has_more);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.paginated_count, 1);

    engine
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

    let refreshed_page = engine
        .paginate_documents_async(tenant_id.clone(), query)
        .await
        .expect("refreshed paginated full-scan query should succeed");
    assert_eq!(
        subscription_bodies(&refreshed_page.data),
        vec!["Able", "Beta"]
    );
    assert!(refreshed_page.has_more);

    let stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert!(
        (1..=2).contains(&stats.table_load_count),
        "paginated full scans should reuse the warm table and allow at most one refresh load after the async write; got {}",
        stats.table_load_count
    );
    assert_eq!(stats.paginated_count, 2);
}
