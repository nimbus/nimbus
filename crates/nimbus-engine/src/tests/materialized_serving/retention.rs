use super::*;

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

    wait_for_value(
        "materialized serving waiter should register",
        Duration::from_millis(200),
        Duration::ZERO,
        || async {
            service
                .serving_snapshot_manager_stats_for_testing(&tenant_id)
                .expect("serving snapshot manager stats should load")
        },
        |stats| stats.waiter_count == 1,
    )
    .await;

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
