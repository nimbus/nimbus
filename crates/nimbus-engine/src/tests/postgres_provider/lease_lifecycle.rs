use nimbus_core::{
    PrincipalContext, SequenceNumber, TenantEventKind, TenantEventRecord, TriggerDeliveryCursor,
};
use nimbus_storage::{ManualClock, NoopFaultInjector};

use super::support::*;
use crate::commit_fault_labels as labels;

async fn provider_engine(config: EnginePersistenceConfig, clock: Arc<ManualClock>) -> Arc<Engine> {
    Arc::new(
        Engine::new_with_simulation_and_persistence_config(
            config,
            clock,
            Arc::new(NoopFaultInjector),
        )
        .await
        .expect("postgres-backed engine should create"),
    )
}

async fn create_shared_tenant(
    engine_a: &Arc<Engine>,
    engine_b: &Arc<Engine>,
    name: &str,
) -> TenantId {
    let tenant_id = TenantId::new(name).expect("tenant id should build");
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("first engine should create tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("second engine should load tenant without acquiring");
    tenant_id
}

async fn inspection_store(
    provider_config: &PostgresProviderConfig,
    tenant_id: &TenantId,
) -> Arc<nimbus_storage::PostgresTenantStore> {
    PostgresProvider::connect(provider_config.clone())
        .await
        .expect("inspection provider should connect")
        .open_existing_opened_tenant(tenant_id)
        .await
        .expect("tenant lookup should succeed")
        .expect("tenant should exist")
        .store
}

fn assert_terminal_fenced(error: nimbus_core::Error) {
    assert!(
        matches!(error, nimbus_core::Error::CommitterFenced { epoch: 1, .. }),
        "fence loss must retain its typed owner/epoch identity: {error}"
    );
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert_eq!(error.conflicting_sequence(), None);
    assert_ne!(
        error.commit_class(),
        Some(nimbus_core::CommitErrorClass::Conflict),
        "a lease fence is not an OCC conflict"
    );
}

fn title(value: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("title".to_string(), json!(value))])
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_lease_is_lazy_idempotent_renewed_and_cancelled_with_the_runtime() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
        let engine = provider_engine(engine_config, clock.clone()).await;
        let tenant_id = TenantId::new("pg-lazy-lease").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let provider = PostgresProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");

        let loaded = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("loaded stats should read");
        assert!(!loaded.committer_lease_acquired);
        assert_eq!(loaded.committer_lease_acquire_count, 0);
        assert!(
            opened
                .store
                .read_committer_lease()
                .expect("lease read should succeed")
                .is_none(),
            "tenant construction must not acquire sequence authority"
        );

        let first_writer = engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        );
        let concurrent_first_writer = engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("concurrent"))]),
        );
        let (first_result, concurrent_result) = tokio::join!(first_writer, concurrent_first_writer);
        first_result.expect("first assignment should acquire and commit");
        concurrent_result.expect("concurrent first writer should reuse the acquired lease");
        let first = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("first-assignment stats should read");
        assert!(first.committer_lease_acquired);
        assert_eq!(first.committer_lease_epoch, 1);
        assert_eq!(first.committer_lease_acquire_count, 1);
        assert!(first.committer_lease_renewal_worker_running);
        let initial_expiry = first.committer_lease_expires_at;

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("later"))]),
            )
            .await
            .expect("later assignment should reuse the lease");
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("second-assignment stats should read")
                .committer_lease_acquire_count,
            1,
            "one runtime must acquire at most once"
        );

        clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("renewal worker should wake");
        let renewed = wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "manual-clock renewal should complete",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;
        assert!(renewed.committer_lease_expires_at > initial_expiry);
        assert_eq!(renewed.committer_lease_renewal_failure_count, 0);

        engine.quiesce().await;
        assert!(
            !engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("quiesced stats should read")
                .committer_lease_renewal_worker_running,
            "engine quiesce must join the tenant lease worker"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn two_postgres_engines_load_without_leases_and_only_one_first_writer_acquires_epoch() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualClock::new(Timestamp(20_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualClock::new(Timestamp(20_000)))).await;
        let tenant_id = TenantId::new("pg-two-engine-lease").expect("tenant id should build");
        engine_a
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("second engine should load the same tenant");

        for engine in [&engine_a, &engine_b] {
            let stats = engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("loaded stats should read");
            assert!(!stats.committer_lease_acquired);
            assert_eq!(stats.committer_lease_acquire_count, 0);
        }
        let provider = PostgresProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        assert!(
            opened
                .store
                .read_committer_lease()
                .expect("lease read should succeed")
                .is_none()
        );

        let write_a = engine_a.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("a"))]),
        );
        let write_b = engine_b.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("b"))]),
        );
        let (result_a, result_b) = tokio::join!(write_a, write_b);
        assert_eq!(
            usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
            1
        );

        let stats_a = engine_a
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("first engine stats should read");
        let stats_b = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("second engine stats should read");
        assert_eq!(
            usize::from(stats_a.committer_lease_acquired)
                + usize::from(stats_b.committer_lease_acquired),
            1
        );
        assert_ne!(
            (stats_a.committer_lease_epoch, stats_b.committer_lease_epoch),
            (1, 1),
            "two runtimes must never both believe they acquired epoch 1"
        );
        let lease = opened
            .store
            .read_committer_lease()
            .expect("lease read should succeed")
            .expect("one first writer should create the lease");
        assert_eq!(lease.epoch, 1);

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_acquisition_reconciles_predecessor_heads_and_records_fenced_renewal() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let clock_a = Arc::new(ManualClock::new(Timestamp(30_000)));
        let clock_b = Arc::new(ManualClock::new(Timestamp(30_000)));
        let engine_a = provider_engine(config_a, clock_a).await;
        let engine_b = provider_engine(config_b, clock_b.clone()).await;
        let tenant_id = TenantId::new("pg-reconcile-lease").expect("tenant id should build");
        engine_a
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("second engine should load the empty tenant");

        let unit = engine_b
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should begin");
        unit.insert_document(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("successor"))]),
        )
        .expect("successor insert should stage");
        let faults = engine_b.commit_fault_handle_for_testing();
        faults.arm(labels::PRE_ASSIGN);
        let commit = tokio::task::spawn_blocking({
            let unit = unit.clone();
            move || unit.commit()
        });
        let paused = tokio::task::spawn_blocking({
            let faults = faults.clone();
            move || faults.wait_until_entered(labels::PRE_ASSIGN, Duration::from_secs(5))
        })
        .await
        .expect("fault wait should join");
        assert!(paused, "successor must pause before lease acquisition");

        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("hint listeners should terminate before staging predecessor progress");
        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        let predecessor = TenantEventRecord::from_events(
            SequenceNumber(1),
            Timestamp(30_001),
            vec![TenantEventKind::TriggerDelivery {
                cursor: TriggerDeliveryCursor::new(SequenceNumber(0)),
            }],
        )
        .expect("predecessor record should build");
        opened
            .store
            .append_durable_records_batch(std::slice::from_ref(&predecessor))
            .expect("predecessor should append");
        opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&predecessor))
            .expect("predecessor should apply");
        assert_eq!(
            engine_b
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("paused runtime stats should read")
                .durable_head,
            SequenceNumber(0),
            "the loaded runtime must still need acquisition reconciliation"
        );

        faults.release(labels::PRE_ASSIGN);
        let successor = commit
            .await
            .expect("successor task should join")
            .expect("successor should commit")
            .expect("successor should append a record");
        assert_eq!(successor.sequence, SequenceNumber(2));
        let reconciled = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("reconciled stats should read");
        assert_eq!(reconciled.durable_head, SequenceNumber(2));
        assert_eq!(reconciled.applied_head, SequenceNumber(2));
        assert_eq!(reconciled.committer_lease_epoch, 1);

        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("first lease should expire deterministically");
        engine_a
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("new-owner"))]),
            )
            .await
            .expect("second runtime should acquire the next epoch");
        assert_eq!(
            engine_a
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("new owner stats should read")
                .committer_lease_epoch,
            2
        );

        clock_b.advance(Duration::from_secs(10));
        engine_b
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("old owner renewal should wake");
        let fenced = wait_for_mutation_journal_stats(
            &engine_b,
            &tenant_id,
            "old owner renewal should record fencing",
            |stats| stats.committer_lease_fenced,
        )
        .await;
        assert_eq!(fenced.committer_lease_epoch, 1);
        assert_eq!(fenced.committer_lease_renewal_failure_count, 1);
        assert!(!fenced.committer_lease_renewal_worker_running);
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("unit 3 records fencing without evicting the runtime");

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_fences_every_provider_record_writer_without_partial_persistence() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualClock::new(Timestamp(40_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualClock::new(Timestamp(40_000)))).await;

        // Queued async batch path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-queued").await;
        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("old-holder"))
            .await
            .expect("old holder queued write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("queued lease should expire");
        engine_b
            .insert_document_async(tenant_id.clone(), tasks_table(), title("healthy-holder"))
            .await
            .expect("healthy queued holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("queued head should read");
        let fenced_id = DocumentId::new();
        assert_terminal_fenced(
            engine_a
                .insert_document_async_with_id(
                    tenant_id.clone(),
                    tasks_table(),
                    fenced_id.clone(),
                    title("must-not-persist"),
                )
                .await
                .expect_err("stale queued holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("queued head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("queued document lookup should succeed")
                .is_none()
        );

        // Direct synchronous prepared-commit path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-direct").await;
        engine_a
            .insert_document(&tenant_id, tasks_table(), title("old-holder"))
            .expect("old holder direct write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("direct lease should expire");
        engine_b
            .insert_document(&tenant_id, tasks_table(), title("healthy-holder"))
            .expect("healthy direct holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("direct head should read");
        let fenced_id = DocumentId::new();
        assert_terminal_fenced(
            engine_a
                .insert_document_with_id(
                    &tenant_id,
                    tasks_table(),
                    fenced_id.clone(),
                    title("must-not-persist"),
                )
                .expect_err("stale direct holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("direct head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("direct document lookup should succeed")
                .is_none()
        );

        // Mutation execution-unit path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-execution-unit").await;
        let old_unit = engine_a
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("old holder execution unit should begin");
        old_unit
            .insert_document(tasks_table(), title("old-holder"))
            .expect("old holder write should stage");
        old_unit
            .commit()
            .expect("old holder execution unit should acquire and commit");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("execution-unit lease should expire");
        let healthy_unit = engine_b
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("healthy execution unit should begin");
        healthy_unit
            .insert_document(tasks_table(), title("healthy-holder"))
            .expect("healthy write should stage");
        healthy_unit
            .commit()
            .expect("healthy execution holder should take over and commit");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("execution head should read");
        let fenced_id = DocumentId::new();
        let fenced_unit = engine_a
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("stale execution unit should begin");
        fenced_unit
            .insert_document_with_id(
                tasks_table(),
                Some(fenced_id.clone()),
                title("must-not-persist"),
            )
            .expect("stale write should stage before fencing");
        assert_terminal_fenced(
            fenced_unit
                .commit()
                .expect_err("stale execution-unit holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("execution head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("execution document lookup should succeed")
                .is_none()
        );

        // Schema set path (delete shares the same fenced schema transaction seam).
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-schema").await;
        engine_a
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("old holder schema write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("schema lease should expire");
        let mut healthy_schema = tasks_schema();
        healthy_schema.fields.push(FieldSchema {
            name: "healthy".to_string(),
            field_type: FieldType::String,
            required: false,
        });
        engine_b
            .set_table_schema_async(tenant_id.clone(), healthy_schema.clone())
            .await
            .expect("healthy schema holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("schema head should read");
        let mut fenced_schema = tasks_schema();
        fenced_schema.fields.push(FieldSchema {
            name: "fenced".to_string(),
            field_type: FieldType::String,
            required: false,
        });
        assert_terminal_fenced(
            engine_a
                .set_table_schema_async(tenant_id.clone(), fenced_schema)
                .await
                .expect_err("stale schema holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("schema head should reread"),
            before
        );
        assert_eq!(
            store
                .load_schema()
                .expect("persisted schema should read")
                .get_table(&tasks_table()),
            Some(&healthy_schema)
        );

        // Schema delete uses its own provider transaction entry point.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-schema-delete").await;
        engine_a
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("old holder should seed schema before delete");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("schema-delete lease should expire");
        engine_b
            .delete_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("healthy schema-delete holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("schema-delete head should read");
        assert_terminal_fenced(
            engine_a
                .delete_table_schema_async(tenant_id.clone(), tasks_table())
                .await
                .expect_err("stale schema-delete holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("schema-delete head should reread"),
            before
        );
        assert!(
            store
                .load_schema()
                .expect("schema after delete should read")
                .get_table(&tasks_table())
                .is_none()
        );

        // Internal trigger-materialization/cursor path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-internal").await;
        engine_a
            .materialize_trigger_cursor_for_testing(
                &tenant_id,
                TriggerDeliveryCursor::new(SequenceNumber(1)),
            )
            .expect("old holder internal cursor write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("internal lease should expire");
        engine_b
            .materialize_trigger_cursor_for_testing(
                &tenant_id,
                TriggerDeliveryCursor::new(SequenceNumber(2)),
            )
            .expect("healthy internal holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("internal head should read");
        assert_terminal_fenced(
            engine_a
                .materialize_trigger_cursor_for_testing(
                    &tenant_id,
                    TriggerDeliveryCursor::new(SequenceNumber(3)),
                )
                .expect_err("stale internal holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("internal head should reread"),
            before
        );
        assert_eq!(
            store
                .trigger_delivery_cursor()
                .expect("trigger cursor should read"),
            TriggerDeliveryCursor::new(SequenceNumber(2))
        );

        // Ordered publisher persistence seam, exercised directly because provider
        // publisher authority is enabled by the later slice-C topology cleanup.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-publisher").await;
        engine_a
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "old-holder")
            .expect("old holder publisher write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("publisher lease should expire");
        engine_b
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "healthy-holder")
            .expect("healthy publisher holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("publisher head should read");
        assert_terminal_fenced(
            engine_a
                .persist_provider_publisher_barrier_for_testing(&tenant_id, "must-not-persist")
                .expect_err("stale publisher holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("publisher head should reread"),
            before
        );
        assert!(
            store
                .read_durable_journal_from(SequenceNumber(before.durable_head.0.saturating_add(1)))
                .expect("publisher suffix should read")
                .is_empty()
        );

        // Point-in-time restore journal import, which sits outside the mutation API.
        let source_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-source").await;
        engine_a
            .set_table_schema_async(source_id.clone(), tasks_schema())
            .await
            .expect("restore source schema should persist");
        let restored_document_id = DocumentId::new();
        engine_a
            .insert_document_async_with_id(
                source_id.clone(),
                tasks_table(),
                restored_document_id.clone(),
                title("restored"),
            )
            .await
            .expect("restore source document should persist");
        let archive = engine_a
            .export_latest_point_in_time_restore_archive(&source_id)
            .expect("restore archive should export");

        let healthy_id =
            create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-healthy").await;
        engine_a
            .import_point_in_time_restore_archive(&healthy_id, &archive)
            .expect("healthy restore holder should import through the fence");
        let healthy_store = inspection_store(&provider_config, &healthy_id).await;
        assert_eq!(
            healthy_store
                .journal_progress()
                .expect("healthy restore progress should read")
                .applied_head,
            archive.target_sequence
        );
        assert!(
            healthy_store
                .get(&tasks_table(), &restored_document_id)
                .expect("healthy restored document should read")
                .is_some()
        );

        let fenced_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-stale").await;
        engine_a
            .acquire_committer_lease_for_testing(&fenced_id)
            .expect("old restore holder should acquire without populating the tenant");
        expire_postgres_committer_lease(&provider_config, &fenced_id)
            .await
            .expect("restore lease should expire");
        engine_b
            .acquire_committer_lease_for_testing(&fenced_id)
            .expect("healthy restore holder should take over the empty tenant");
        let fenced_store = inspection_store(&provider_config, &fenced_id).await;
        let lease_before = fenced_store
            .read_committer_lease()
            .expect("restore lease should read");
        assert_terminal_fenced(
            engine_a
                .import_point_in_time_restore_archive(&fenced_id, &archive)
                .expect_err("stale restore holder must be fenced"),
        );
        let fenced_progress = fenced_store
            .journal_progress()
            .expect("fenced restore progress should read");
        assert_eq!(fenced_progress.durable_head, SequenceNumber(0));
        assert_eq!(fenced_progress.applied_head, SequenceNumber(0));
        assert!(
            fenced_store
                .load_schema()
                .expect("fenced restore schema should read")
                .tables
                .is_empty()
        );
        assert!(
            fenced_store
                .get(&tasks_table(), &restored_document_id)
                .expect("fenced restored document lookup should succeed")
                .is_none()
        );
        assert_eq!(
            fenced_store
                .read_committer_lease()
                .expect("restore lease should reread"),
            lease_before,
            "a rejected restore must not alter the healthy holder's lease"
        );

        for tenant_id in [
            "pg-fence-queued",
            "pg-fence-direct",
            "pg-fence-execution-unit",
            "pg-fence-schema",
            "pg-fence-schema-delete",
            "pg-fence-internal",
            "pg-fence-publisher",
            "pg-fence-restore-stale",
        ] {
            let tenant_id = TenantId::new(tenant_id).expect("tenant id should rebuild");
            let stats = engine_a
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("stale runtime should remain loaded");
            assert!(stats.committer_lease_fenced);
            assert_eq!(stats.committer_lease_epoch, 1);
        }

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}
