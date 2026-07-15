use super::*;

#[test]
fn mutation_execution_unit_commits_id_from_injected_source() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let expected_id = DocumentId::from_key("00000000000000000000000000")
        .expect("deterministic ULID should be a valid document id");
    let engine = Arc::new(
        Engine::new_with_simulation_and_id_source(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(10_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(SeededIdSource::new(0)),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_injected_id_source");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let staged_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("deterministic"))]),
        )
        .expect("insert should stage");
    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("document insert should produce a commit");

    assert_eq!(staged_id, expected_id);
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].doc_id, expected_id);
    assert_eq!(
        commit.writes[0]
            .current
            .as_ref()
            .expect("insert commit should contain the current document")
            .id,
        expected_id
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, expected_id.clone())
            .expect("committed document should be readable")
            .id,
        expected_id
    );
}

#[test]
fn mutation_execution_unit_commit_timestamp_follows_manual_clock() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), clock.clone(), Arc::new(NoopFaultInjector))
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_injected_commit_clock");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .insert_document(
            table,
            serde_json::Map::from_iter([("body".to_string(), json!("clocked"))]),
        )
        .expect("insert should stage");

    let expected_timestamp = Timestamp(73_421);
    clock.set(expected_timestamp);
    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("document insert should produce a commit");

    assert_eq!(commit.timestamp, expected_timestamp);
}

#[tokio::test]
async fn mutation_execution_unit_pre_assign_label_forces_commit_interleaving() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_pre_assign_interleaving");
    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Seed"))]),
        )
        .expect("seed insert should establish the table identity");

    let first_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("first execution unit should start");
    let first_id = first_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("First"))]),
        )
        .expect("first insert should stage");
    let second_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("second execution unit should start");
    let second_id = second_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Second"))]),
        )
        .expect("second insert should stage");

    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(labels::PRE_ASSIGN);
    let first_commit = tokio::task::spawn_blocking({
        let first_unit = first_unit.clone();
        move || first_unit.commit()
    });
    let reached_label = tokio::task::spawn_blocking({
        let faults = faults.clone();
        move || faults.wait_until_entered(labels::PRE_ASSIGN, Duration::from_secs(5))
    })
    .await
    .expect("label wait should join");
    assert!(
        reached_label,
        "first commit should pause before entering the serial sequence path"
    );
    assert!(
        !first_commit.is_finished(),
        "first commit should remain held"
    );

    let second_commit = timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking({
            let second_unit = second_unit.clone();
            move || second_unit.commit()
        }),
    )
    .await
    .expect("second commit should finish while the first is held")
    .expect("second commit task should join")
    .expect("second commit should succeed")
    .expect("second insert should produce a commit");
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, second_id.clone())
            .expect("second document should be visible")
            .get_field("body"),
        Some(&json!("Second"))
    );
    assert!(matches!(
        engine.get_document(&tenant_id, &table, first_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
    assert!(
        !first_commit.is_finished(),
        "first commit must still be held after the second becomes visible"
    );

    faults.release(labels::PRE_ASSIGN);
    let first_commit = timeout(Duration::from_secs(5), first_commit)
        .await
        .expect("first commit should finish after release")
        .expect("first commit task should join")
        .expect("first commit should succeed")
        .expect("first insert should produce a commit");
    assert!(
        second_commit.sequence.0 < first_commit.sequence.0,
        "the unblocked second commit should be assigned before the held first commit"
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, first_id)
            .expect("first document should be visible after release")
            .get_field("body"),
        Some(&json!("First"))
    );
}

#[test]
fn mutation_execution_unit_pre_persist_fault_leaves_no_partial_state() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_pre_persist_fault");
    let runtime = engine
        .get_existing_tenant(&tenant_id)
        .expect("tenant runtime should exist");
    let durable_head_before = runtime.durable_head();
    let applied_head_before = runtime.applied_head();

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("must not persist"))]),
        )
        .expect("insert should stage");

    let faults = engine.commit_fault_handle_for_testing();
    faults.inject(
        labels::PRE_PERSIST,
        Fault::Error(Error::storage(
            StorageErrorKind::Io,
            "injected pre-persist failure",
        )),
    );
    let error = execution_unit
        .commit()
        .expect_err("injected pre-persist fault should fail the public commit API");

    assert_eq!(error.storage_kind(), Some(StorageErrorKind::Io));
    assert_eq!(
        error.storage_message(),
        Some("injected pre-persist failure")
    );
    assert_eq!(runtime.durable_head(), durable_head_before);
    assert_eq!(runtime.applied_head(), applied_head_before);
    let later_commits = runtime
        .store()
        .read_commit_log_from(SequenceNumber(durable_head_before.0.saturating_add(1)))
        .expect("commit log should remain readable");
    assert!(
        later_commits.is_empty(),
        "the failed commit must not append a durable journal entry"
    );
    assert!(matches!(
        engine.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_))
    ));
    let visible = engine
        .query_documents(
            &tenant_id,
            &Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("table query should remain readable");
    assert!(visible.is_empty(), "the failed commit must not be visible");
}

#[test]
fn mutation_execution_unit_aborts_on_overlapping_document_conflict() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_doc");

    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document = execution_unit
        .get_document(&table, document_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(document.get_field("body"), Some(&json!("Initial")));
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    engine
        .update_document(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should remain committed")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[tokio::test]
async fn mutation_execution_unit_conflict_scan_and_append_are_sequence_atomic() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_phantom_gap");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let first_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("first execution unit should start");
    let second_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("second execution unit should start");

    let first_visible = first_unit
        .query_documents_cancellable(&query, &mut || Ok(()))
        .expect("first table query should succeed");
    let second_visible = second_unit
        .query_documents_cancellable(&query, &mut || Ok(()))
        .expect("second table query should succeed");
    assert!(first_visible.is_empty());
    assert!(second_visible.is_empty());

    first_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("First")),
            ]),
        )
        .expect("first staged insert should succeed");
    second_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect("second staged insert should succeed");

    let pause = engine.commit_fault_handle_for_testing();
    pause.arm(labels::POST_VALIDATE_PRE_STAGE);

    let first_commit = tokio::task::spawn_blocking({
        let first_unit = first_unit.clone();
        move || first_unit.commit()
    });

    let pause_wait = pause.clone();
    let first_reached_pause = tokio::task::spawn_blocking(move || {
        pause_wait.wait_until_entered(labels::POST_VALIDATE_PRE_STAGE, Duration::from_secs(1))
    })
    .await
    .expect("pause wait should join");
    assert!(
        first_reached_pause,
        "first commit should pause after conflict scan while holding the sequence gate"
    );

    let second_commit = tokio::task::spawn_blocking({
        let second_unit = second_unit.clone();
        move || second_unit.commit()
    });
    let mut second_commit = second_commit;
    assert!(
        timeout(Duration::from_millis(100), &mut second_commit)
            .await
            .is_err(),
        "second commit should wait behind the first scan+append critical section"
    );

    pause.release(labels::POST_VALIDATE_PRE_STAGE);

    let first_commit = timeout(Duration::from_secs(1), first_commit)
        .await
        .expect("first commit should complete after pause release")
        .expect("first commit task should join")
        .expect("first commit should succeed")
        .expect("first commit should produce a commit entry");
    assert_eq!(first_commit.writes.len(), 1);

    let second_error = timeout(Duration::from_secs(1), second_commit)
        .await
        .expect("second commit should complete after first append is visible")
        .expect("second commit task should join")
        .expect_err("second commit should conflict with first phantom insert");
    assert!(matches!(second_error, Error::Conflict(_)));

    let documents = engine
        .query_documents(&tenant_id, &query)
        .expect("final query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].get_field("body"), Some(&json!("First")));
}

#[test]
fn mutation_execution_unit_write_dependencies_use_snapshot_table_identity() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_snapshot_write_deps");
    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Before")),
            ]),
        )
        .expect("seed document should insert");
    let runtime = engine
        .get_existing_tenant(&tenant_id)
        .expect("tenant runtime should exist");
    let snapshot_table_id = runtime
        .store()
        .table_id(&table)
        .expect("table id lookup should succeed")
        .expect("seed insert should create a table identity");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start with the original table snapshot");

    let replacement_table_id = TableId::new();
    let replaced_table_id = match runtime.store() {
        crate::persistence::TenantPersistence::Redb(store) => {
            store
                .stage_hidden_table_identity(&table, &replacement_table_id)
                .expect("replacement table identity should stage");
            store
                .activate_hidden_table_identity(&table, &replacement_table_id)
                .expect("replacement table identity should activate")
        }
        crate::persistence::TenantPersistence::Sqlite(store) => {
            store
                .stage_hidden_table_identity(&table, &replacement_table_id)
                .expect("replacement table identity should stage");
            store
                .activate_hidden_table_identity(&table, &replacement_table_id)
                .expect("replacement table identity should activate")
        }
        crate::persistence::TenantPersistence::LibsqlReplica(_)
        | crate::persistence::TenantPersistence::Postgres(_)
        | crate::persistence::TenantPersistence::MySql(_) => {
            panic!("engine fixture should use an embedded persistence provider")
        }
    };
    assert_eq!(replaced_table_id, Some(snapshot_table_id.clone()));
    assert_eq!(
        runtime
            .store()
            .table_id(&table)
            .expect("live table id lookup should succeed"),
        Some(replacement_table_id.clone()),
        "live table identity should differ from the execution-unit snapshot"
    );

    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("After"))]),
        )
        .expect("snapshot-era document update should stage");

    let write_dependencies = execution_unit.write_dependencies();
    assert!(
        write_dependencies.documents.iter().any(|dependency| {
            dependency.table == table
                && dependency.table_id == snapshot_table_id
                && dependency.document_id == document_id
        }),
        "write dependency should use the execution-unit snapshot table identity"
    );
    assert!(
        !write_dependencies.documents.iter().any(|dependency| {
            dependency.table == table
                && dependency.table_id == replacement_table_id
                && dependency.document_id == document_id
        }),
        "write dependency must not use a later live table identity"
    );
}

#[test]
fn mutation_execution_unit_commits_when_concurrent_write_is_disjoint() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_disjoint");

    let first_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("First")),
            ]),
        )
        .expect("first fixture insert should succeed");
    let second_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect("second fixture insert should succeed");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let read_back = execution_unit
        .get_document(&table, first_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(read_back.get_field("body"), Some(&json!("First")));
    execution_unit
        .update_document(
            table.clone(),
            first_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    engine
        .update_document(
            &tenant_id,
            table.clone(),
            second_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("disjoint update should commit");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, first_id.clone())
            .expect("first document should exist")
            .get_field("body"),
        Some(&json!("Tx update"))
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, second_id.clone())
            .expect("second document should exist")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_update_commits_as_single_insert() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_insert_update");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Updated"))]),
        )
        .expect("staged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert!(commit.writes[0].previous.is_none());
    assert_eq!(
        commit.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.get_field("body")),
        Some(&json!("Updated"))
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("inserted document should exist")
            .get_field("body"),
        Some(&json!("Updated"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_delete_commits_as_noop() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_insert_delete");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Transient")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .delete_document(table.clone(), document_id.clone())
        .expect("staged delete should succeed");

    let commit = execution_unit.commit().expect("commit should succeed");
    assert!(
        commit.is_none(),
        "insert followed by delete should collapse to a no-op"
    );
    let error = engine
        .get_document(&tenant_id, &table, document_id.clone())
        .expect_err("transient document should not exist");
    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn mutation_execution_unit_persists_trigger_write_origin_on_committed_writes() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_trigger_origin");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::system())
        .expect("execution unit should start");
    let origin = TriggerWriteOrigin::new(
        TriggerInvocationKey::new("firebase:messagesWritten", "evt-root")
            .expect("invocation key should parse"),
        2,
    );
    execution_unit
        .set_trigger_write_origin(origin.clone())
        .expect("trigger write origin should stage");
    execution_unit
        .insert_document(
            table,
            serde_json::Map::from_iter([("body".to_string(), json!("from trigger"))]),
        )
        .expect("staged insert should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");

    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].trigger_write_origin, Some(origin));
}

#[test]
fn mutation_execution_unit_restage_after_revert_commits_once() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_restage");

    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("First"))]),
        )
        .expect("first staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Initial"))]),
        )
        .expect("revert staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Second"))]),
        )
        .expect("restaged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(
        commit.writes.len(),
        1,
        "restaging after a revert should only produce one final write"
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should exist")
            .get_field("body"),
        Some(&json!("Second"))
    );
}

#[tokio::test]
async fn mutation_execution_unit_conflicts_with_durable_unapplied_write() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(92_000))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_occ_apply_lag");

    let mut outside_update = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let table = table.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    table,
                    serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-456")),
                        ("body".to_string(), json!("Outside insert")),
                    ]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut outside_update)
            .await
            .is_err(),
        "outside update should remain pending while apply is blocked"
    );

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("query should succeed");
    assert!(
        visible.is_empty(),
        "execution unit should still see the applied snapshot while the outside write lags"
    );
    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("staged insert should succeed");

    let commit_handle = tokio::task::spawn_blocking({
        let execution_unit = execution_unit.clone();
        move || execution_unit.commit()
    });
    let mut commit_handle = commit_handle;

    assert!(
        timeout(Duration::from_millis(100), &mut commit_handle)
            .await
            .is_err(),
        "execution-unit commit should wait behind the queued journal writer's sequence gate"
    );
    faults.release();
    timeout(Duration::from_secs(1), outside_update)
        .await
        .expect("outside update should finish after apply resumes")
        .expect("outside update task should join successfully")
        .expect("outside update should succeed");

    let commit_result = timeout(Duration::from_secs(1), commit_handle)
        .await
        .expect("commit should resolve after the journal writer releases the sequence gate")
        .expect("commit task should join successfully");
    let error = commit_result.expect_err(
        "commit should conflict with the durable journal write that was not part of the applied snapshot",
    );
    assert!(matches!(error, Error::Conflict(_)));
    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "body".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed after apply");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].get_field("body"),
        Some(&json!("Outside insert"))
    );
}

#[test]
fn mutation_execution_unit_structured_query_reads_staged_rows() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_structured_reads");

    let alpha_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("alpha"))]),
        )
        .expect("seed insert should succeed");
    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("bravo"))]),
        )
        .expect("second seed insert should succeed");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .update_document(
            table.clone(),
            alpha_id,
            serde_json::Map::from_iter([("body".to_string(), json!("zulu"))]),
        )
        .expect("staged update should succeed");
    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("beta"))]),
        )
        .expect("staged insert should succeed");

    let documents = execution_unit
        .query_documents_structured_cancellable(
            &table,
            &StructuredQuery {
                order_by: vec![StructuredOrder {
                    field: nimbus_core::FieldReference::new("body"),
                    direction: QueryDirection::Ascending,
                }],
                ..StructuredQuery::default()
            },
            &mut || Ok(()),
        )
        .expect("structured query should succeed");

    assert_eq!(
        documents
            .iter()
            .map(|document| document.get_field("body").cloned().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![json!("beta"), json!("bravo"), json!("zulu")]
    );
}

#[test]
fn mutation_execution_unit_conflicts_when_auth_filtered_visibility_changes() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_auth");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_occ_auth",
                vec![IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(owner_read_write_policy()),
            ),
        )
        .expect("schema should save");
    let hidden_owner = principal_with_subject("user-456");

    let hidden_id = engine
        .insert_document_with(
            &tenant_id,
            table.clone(),
            None,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Hidden")),
            ]),
            crate::MutationActor::with_principal(&hidden_owner),
        )
        .expect("hidden document insert should succeed");

    let principal = principal_with_subject("user-123");
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("authorized query should succeed");
    assert!(visible.is_empty(), "hidden row should not be visible yet");

    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("authorized staged insert should succeed");

    engine
        .update_document_with(
            &tenant_id,
            table.clone(),
            hidden_id,
            serde_json::Map::from_iter([("owner".to_string(), json!("user-123"))]),
            crate::MutationActor::with_principal(&hidden_owner),
        )
        .expect("external update should make the hidden row visible");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the auth-filtered visibility change");
    assert!(matches!(error, Error::Conflict(_)));
}

#[test]
fn mutation_execution_unit_paginated_cursor_excludes_principal_auth_filters() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_cursor_auth");

    engine
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_cursor_auth",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");
    for (owner, body) in [
        ("alice", "a1"),
        ("alice", "a2"),
        ("bob", "b3"),
        ("bob", "b4"),
        ("carol", "c5"),
    ] {
        engine
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let alice_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), principal_with_subject("alice"))
        .expect("alice execution unit should start");
    let first_page = alice_unit
        .paginate_documents_cancellable(
            &PaginatedQuery {
                query: query.clone(),
                page_size: 1,
                after: None,
            },
            &mut || Ok(()),
        )
        .expect("alice page should succeed");
    assert_eq!(first_page.data.len(), 1);
    assert_eq!(first_page.data[0]["owner"], json!("alice"));
    assert_eq!(first_page.data[0]["body"], json!("a1"));
    let cursor = first_page
        .next_cursor
        .clone()
        .expect("alice page should produce a cursor");

    let bob_unit = engine
        .begin_mutation_execution_unit(tenant_id, principal_with_subject("bob"))
        .expect("bob execution unit should start");
    let second_page = bob_unit
        .paginate_documents_cancellable(
            &PaginatedQuery {
                query,
                page_size: 2,
                after: Some(cursor),
            },
            &mut || Ok(()),
        )
        .expect("cursor should not embed alice's authorization filter");
    assert_eq!(second_page.data.len(), 2);
    assert!(
        second_page
            .data
            .iter()
            .all(|document| document["owner"] == json!("bob")),
        "the replaying principal's authorization filter must still constrain execution-unit results"
    );
    assert_eq!(second_page.data[0]["body"], json!("b3"));
    assert_eq!(second_page.data[1]["body"], json!("b4"));
    assert!(!second_page.has_more);
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_successful_commit() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_finalize_success");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Committed")),
            ]),
        )
        .expect("staged insert should succeed");
    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);

    let read_error = execution_unit
        .get_document(&table, document_id.clone())
        .expect_err("finalized execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect_err("finalized execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let commit_error = execution_unit
        .commit()
        .expect_err("finalized execution unit should reject a second commit");
    assert!(matches!(commit_error, Error::InvalidInput(message) if message.contains("finalized")));
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_failed_commit_attempt() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let table = messages_table("messages_occ_finalize_failure");

    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .get_document(&table, document_id.clone())
        .expect("point read should succeed")
        .expect("document should exist");
    execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    engine
        .update_document(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let commit_error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(commit_error, Error::Conflict(_)));

    let read_error = execution_unit
        .get_document(&table, document_id.clone())
        .expect_err("conflicted execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .update_document(
            table.clone(),
            document_id.clone(),
            serde_json::Map::from_iter([("body".to_string(), json!("Retry"))]),
        )
        .expect_err("conflicted execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let second_commit_error = execution_unit
        .commit()
        .expect_err("conflicted execution unit should reject a second commit");
    assert!(
        matches!(second_commit_error, Error::InvalidInput(message) if message.contains("finalized"))
    );
}
