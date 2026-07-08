use super::*;

#[test]
fn pinned_materialized_serving_snapshots_remain_stable_after_later_applies() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_serving_handle_stability");

    let _ = engine
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
        filters: Vec::new(),
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

    // Settle the seed's cursor-advance commit before capturing `before_insert`
    // and pinning against it below. `MaterializedServingBackend` widens the
    // currently-published version's coverage through that zero-write commit
    // in place; without settling first, that widening can land between
    // capturing `before_insert` and pinning the snapshot for it, so the
    // "same" snapshot looked up a moment later would already cover a later
    // sequence than the one just captured.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let before_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should expose a publication",
    );
    let pinned = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, before_insert)
        .expect("serving snapshot should load")
        .expect("warmed table should expose a serving snapshot");
    assert_eq!(pinned.covered_sequence(), before_insert);
    let pinned_documents = pinned
        .table_documents(&table)
        .expect("pinned snapshot should include the warmed table");
    assert_eq!(document_bodies(&pinned_documents), vec!["Ada"]);

    let _ = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    // Settle Beta's own cursor-advance commit before capturing `after_insert`
    // and reading the snapshot back for it, for the same reason as the seed
    // settle above: without it, that commit's in-place widening of the
    // now-current version can land between the capture and the read-back.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let after_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after the second insert",
    );
    let current = engine
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
fn pinned_serving_read_shape_handle_preserves_identity_and_documents_after_later_applies() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_serving_mvcc_shape");
    let schema = serving_status_schema(&table);
    engine
        .set_table_schema(&tenant_id, schema.clone())
        .expect("schema should persist");
    let table_id = engine
        .table_id(&tenant_id, &table)
        .expect("table id lookup should succeed")
        .expect("schema write should create table identity");

    let ada_id = engine
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
        filters: Vec::new(),
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

    // Settle the seed's cursor-advance commit before capturing `before_insert`
    // and pinning against it below. `MaterializedServingBackend` widens the
    // currently-published version's coverage through that zero-write commit
    // in place; without settling first, that widening can land between
    // capturing `before_insert` and pinning the snapshot for it, so the
    // "same" snapshot looked up a moment later would already cover a later
    // sequence than the one just captured.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let before_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should expose a publication",
    );
    let read_shape = serving_read_shape(&table, &table_id, &schema, before_insert);
    let serving_snapshot = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, before_insert)
        .expect("serving snapshot should load")
        .expect("warmed table should expose a serving snapshot");
    let pinned = serving_snapshot
        .pin_read_shape(read_shape.clone())
        .expect("serving snapshot should pin the read-shape bundle it covers");
    assert_eq!(pinned.covered_sequence(), before_insert);
    assert_eq!(pinned.table_id(), &table_id);
    assert_eq!(pinned.read_shape(), &read_shape);
    assert_eq!(pinned.read_shape().queryable_indexes()[0].name, "by_status");
    assert_eq!(
        pinned
            .document(&ada_id)
            .expect("pinned read shape should find Ada")
            .get_field("body"),
        Some(&json!("Ada"))
    );

    let beta_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");
    let after_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after the second insert",
    );
    let current = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, after_insert)
        .expect("current serving snapshot should load")
        .expect("current snapshot should exist");
    let current_documents = current
        .table_documents(&table)
        .expect("current snapshot should include the warmed table");
    let mut current_bodies = document_bodies(&current_documents);
    current_bodies.sort_unstable();
    assert_eq!(current_bodies, vec!["Ada", "Beta"]);

    assert!(
        pinned.document(&beta_id).is_none(),
        "pinned read-shape handle must not see later writes"
    );
    assert_eq!(
        document_bodies(
            &pinned
                .table_documents()
                .expect("pinned read shape should expose table docs"),
        ),
        vec!["Ada"]
    );
}

#[test]
fn pinned_serving_read_shape_handle_fails_closed_when_snapshot_does_not_cover_shape() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_serving_mvcc_shape_fail_closed");
    let schema = serving_status_schema(&table);
    engine
        .set_table_schema(&tenant_id, schema.clone())
        .expect("schema should persist");
    let table_id = engine
        .table_id(&tenant_id, &table)
        .expect("table id lookup should succeed")
        .expect("schema write should create table identity");

    engine
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
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    engine
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    let before_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should expose a publication",
    );
    let snapshot = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, before_insert)
        .expect("serving snapshot should load")
        .expect("warmed table should expose a serving snapshot");

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");
    let after_insert = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after the second insert",
    );
    let newer_shape = serving_read_shape(&table, &table_id, &schema, after_insert);
    let error = match snapshot.pin_read_shape(newer_shape) {
        Ok(_) => panic!("older serving snapshot must reject a newer read shape"),
        Err(error) => error,
    };
    assert_eq!(
        error.historical_read_kind(),
        Some(nimbus_core::HistoricalReadErrorKind::SnapshotUnavailable)
    );
}

#[test]
fn materialized_surface_reacquires_retained_covering_version_for_older_required_sequence() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_serving_handle_retention");

    engine
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 3)
        .expect("materialized surface version capacity should be configurable for tests");

    let _ = engine
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

    // Settle the seed's cursor-advance commit before capturing `first_sequence`
    // below. `MaterializedServingBackend` now widens the currently-published
    // version's coverage through that zero-write commit, in place, before the
    // second insert below would otherwise push it into `retained` history --
    // without settling first, that widening can race the second insert, and
    // the retained historical entry this test looks up further down would
    // then cover a later sequence than the `first_sequence` snapshotted here.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let first_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should expose its first serving publication",
    );

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    // Settle Beta's own cursor-advance commit before capturing `second_sequence`
    // and the stats/current-snapshot reads below, for the same reason as the
    // seed settle above: without it, that commit's in-place widening of the
    // now-current version can land between the capture and the read-back.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let second_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after the second insert",
    );

    let retained = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("retained serving snapshot should load")
        .expect("historical retained version should remain available");
    assert_eq!(retained.covered_sequence(), first_sequence);
    let retained_documents = retained
        .table_documents(&table)
        .expect("retained snapshot should include the warmed table");
    assert_eq!(document_bodies(&retained_documents), vec!["Ada"]);

    let current = engine
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

    let stats = engine
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let alpha = messages_table("messages_snapshot_alpha");
    let beta = messages_table("messages_snapshot_beta");

    engine
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 4)
        .expect("materialized surface version capacity should be configurable for tests");

    engine
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("alpha seed insert should succeed");
    engine
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

    engine
        .query_documents(&tenant_id, &query_for(alpha.clone()))
        .expect("alpha warm query should succeed");
    engine
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta warm query should succeed");

    // This test pins historical serving snapshots at exact sequence numbers
    // captured mid-test, then asserts each pinned snapshot's `covered_sequence`
    // equals the exact value captured. The tenant's background
    // trigger-candidate feed advances a durable delivery cursor via its own
    // zero-write commits on this same sequence space, and
    // `MaterializedServingBackend` folds each such commit's coverage into
    // whichever version is currently the serving-snapshot manager's latest
    // retained entry (`ServingSnapshotManager::extend_latest_coverage`). A
    // settle call only proves the feed caught up as of that call -- it does
    // not stop a *later* cursor-advance commit from widening the same still-
    // latest entry again before the next real commit supersedes it. Arming
    // the pause here blocks the feed for the entire alpha-update-then-beta-
    // update sequence below, so alpha's retained entry is guaranteed frozen
    // at exactly the sequence its own real commit produced by the time
    // beta's real commit supersedes it, and likewise for beta's own entry
    // (which nothing ever supersedes, since it is the test's last write).
    let trigger_pause = engine
        .trigger_candidate_pause_handle_for_testing(&tenant_id)
        .expect("trigger candidate pause handle should load");
    trigger_pause.arm();

    engine
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("alpha update insert should succeed");
    assert!(
        trigger_pause.wait_until_entered(ci_or_local_duration(
            Duration::from_millis(500),
            Duration::from_secs(5)
        )),
        "trigger candidate worker should pause before alpha's update cursor advance"
    );

    let alpha_update_sequence = published_sequence(
        &engine,
        &tenant_id,
        &alpha,
        "alpha should publish after its update insert",
    );

    engine
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("beta update insert should succeed");

    let latest_sequence = published_sequence(
        &engine,
        &tenant_id,
        &beta,
        "beta should publish after its update insert",
    );

    let exact_snapshot = engine
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

    let latest_snapshot = engine
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

    // Release and settle now that every exact-sequence comparison above is
    // done; nothing after this point reads snapshot-manager state.
    trigger_pause.release();
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);
}

#[tokio::test]
async fn serving_snapshot_waiter_wakes_when_new_frontier_is_published() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_snapshot_waiter");

    // This test picks `required_sequence` as exactly one past the warmed
    // table's covered sequence, then relies on that specific sequence being
    // reached *only* once "Beta" below actually lands. The tenant's
    // background trigger-candidate feed also advances a durable delivery
    // cursor via its own zero-write commits on this same sequence space, and
    // those commits notify this exact same waiter mechanism
    // (`ServingSnapshotManager::extend_latest_coverage` calls
    // `take_ready_waiters_locked` too) -- so without pausing the feed, a
    // cursor-advance commit could itself land on `required_sequence` and
    // wake the waiter before "Beta" is ever applied, with a snapshot that
    // does not yet include it. Arming the pause *before* the seed insert
    // below (rather than after) is required: arming it any later risks the
    // worker having already raced ahead and materialized the seed's own
    // cursor advance before we can observe it paused.
    let trigger_pause = engine
        .trigger_candidate_pause_handle_for_testing(&tenant_id)
        .expect("trigger candidate pause handle should load");
    trigger_pause.arm();

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    assert!(
        tokio::task::spawn_blocking({
            let trigger_pause = trigger_pause.clone();
            move || {
                trigger_pause.wait_until_entered(ci_or_local_duration(
                    Duration::from_millis(500),
                    Duration::from_secs(5),
                ))
            }
        })
        .await
        .expect("pause waiter should join"),
        "trigger candidate worker should pause before the seed's cursor advance"
    );

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    engine
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should expose its first serving publication",
    );
    let required_sequence = SequenceNumber(first_sequence.0.saturating_add(1));

    let waiter = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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
            engine
                .serving_snapshot_manager_stats_for_testing(&tenant_id)
                .expect("serving snapshot manager stats should load")
        },
        |stats| stats.waiter_count == 1,
    )
    .await;

    engine
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
    assert!(
        snapshot.covered_sequence().0 >= required_sequence.0,
        "woken snapshot should cover at least the requested sequence"
    );
    let documents = snapshot
        .table_documents(&table)
        .expect("woken snapshot should include the target table");
    let mut bodies = document_bodies(&documents).into_iter().collect::<Vec<_>>();
    bodies.sort_unstable();
    assert_eq!(bodies, vec!["Ada", "Beta"]);

    let stats = engine
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(stats.waiter_count, 0);
    assert!(
        stats
            .latest_retained_sequence
            .is_some_and(|sequence| sequence.0 >= required_sequence.0),
        "latest retained snapshot should cover at least the waiter requirement"
    );

    trigger_pause.release();
    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id)
    })
    .await
    .expect("settle task should join");
}

#[test]
fn pinned_serving_snapshot_extends_retention_until_release() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_snapshot_pin_retention");

    engine
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 2)
        .expect("materialized surface version capacity should be configurable for tests");

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    // Settle the seed's cursor-advance commit before warming. The tenant's
    // background trigger-candidate feed advances a durable delivery cursor
    // via its own zero-write commit on this same sequence space, and
    // `MaterializedServingBackend` now folds that commit's coverage into
    // whichever version is currently the serving-snapshot manager's latest
    // retained entry (see `ServingSnapshotManager::extend_latest_coverage`).
    // This test compares sequence numbers captured via separate accessor
    // calls (`published_sequence` against `serving_snapshot_manager_stats`)
    // for exact equality further down, so each phase settles the feed first
    // to keep those calls quiescent instead of racing an in-flight advance.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    engine
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "warmed table should publish before pinning",
    );
    let pinned = engine
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("first serving snapshot should load")
        .expect("first serving snapshot should exist");

    for body in ["Beta", "Gamma"] {
        engine
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

    // Settle again before capturing `third_sequence` and the stats below, for
    // the same reason as above: this keeps the pair quiescent instead of
    // leaving a window where Beta's or Gamma's cursor-advance commit could
    // still widen the snapshot manager's latest entry between the two reads.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let third_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after follow-up inserts",
    );

    let pinned_stats = engine
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

    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("final insert should succeed");

    // Settle once more before capturing `fourth_sequence` and `released_stats`
    // below, for the same reason as the two settle points above.
    crate::tests::settle_trigger_cursor_blocking(&engine, &tenant_id);

    let fourth_sequence = published_sequence(
        &engine,
        &tenant_id,
        &table,
        "resident table should publish after the final insert",
    );

    let released_stats = engine
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(released_stats.retained_snapshot_count, 2);
    assert_eq!(
        released_stats.earliest_retained_sequence,
        Some(third_sequence),
        "third_sequence={:?} fourth_sequence={:?} released_stats.latest={:?} \
         pruned={} discarded_out_of_order={}",
        third_sequence,
        fourth_sequence,
        released_stats.latest_retained_sequence,
        released_stats.pruned_snapshot_count,
        released_stats.discarded_out_of_order_count,
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

fn serving_status_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "body".to_string(),
                field_type: FieldType::String,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    }
}

fn serving_read_shape(
    table: &TableName,
    table_id: &nimbus_core::TableId,
    schema: &TableSchema,
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadShape {
    let registry = nimbus_core::VersionedRegistry::from_records([
        nimbus_core::TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            nimbus_core::SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema.clone(),
            },
        )
        .expect("schema change event should build"),
    ])
    .expect("registry should build");
    registry
        .read_shape_at(table, serving_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

fn serving_historical_snapshot(sequence: SequenceNumber) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}
