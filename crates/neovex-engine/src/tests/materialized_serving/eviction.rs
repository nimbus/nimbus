use super::*;

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
