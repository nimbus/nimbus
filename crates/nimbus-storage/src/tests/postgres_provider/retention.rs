use std::sync::Arc;

use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_stale_lease_retention_is_atomic_and_restart_safe() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-contract").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let expected_floor = super::super::exercise_provider_retention_checkpoint(
            opened.store.as_ref(),
            "postgres_retention",
        );

        let reopened_provider = PostgresProvider::connect(config)
            .await
            .expect("provider should reconnect after retention compaction");
        let reopened = reopened_provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("retained tenant should reopen")
            .expect("retained tenant should exist");
        let state = reopened
            .store
            .retention_history_state(
                crate::RetentionGcConfig::new(1).expect("retention config should build"),
            )
            .expect("retention checkpoint should survive provider restart");
        assert_eq!(state.confirmed_floor, expected_floor);
        assert_eq!(state.physical_floor, expected_floor);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_retention_checkpoint_fault_rolls_back_every_delete() {
    let faults = std::sync::Arc::new(crate::ScriptedFaultInjector::new([
        crate::FaultOccurrence {
            point: crate::FaultPoint::RetentionCheckpointBeforeCommit,
            visit: 1,
        },
    ]));
    with_test_provider_and_fault_injector(faults, |provider, _config| async move {
        let tenant = TenantId::new("retention-fault").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_provider_retention_fault_rollback(
            opened.store.as_ref(),
            "postgres_retention_fault",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_page_rejects_concurrent_retention_prune() {
    let (faults, rows_read, resume) = super::super::pause_after_retention_read_page();
    let pause = Arc::clone(&faults);
    with_test_provider_and_fault_injector(faults, |provider, _config| async move {
        let tenant = TenantId::new("retention-page-race").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_provider_retention_concurrent_prune_page(
            opened.store,
            "postgres_retention_page",
            pause,
            rows_read,
            resume,
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_retention_sql_error_preserves_error_and_rolls_back() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-provider-error").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let fixture = super::super::prepare_provider_retention_rollback(
            opened.store.as_ref(),
            "postgres_provider_error",
        );
        let schema = provider
            .tenant_schema_name(&tenant)
            .expect("tenant schema name should build");
        let (client, connection) =
            tokio_postgres::connect(&config.connection_string, tokio_postgres::NoTls)
                .await
                .expect("postgres fault connection should open");
        let connection_task = tokio::spawn(connection);
        client
            .batch_execute(
                format!(
                    "CREATE FUNCTION {schema}.smr2_reject_retention_delete() RETURNS trigger \
                     LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'smr2 provider delete failure'; END $$; \
                     CREATE TRIGGER smr2_reject_retention_delete BEFORE DELETE ON {schema}.commit_log \
                     FOR EACH ROW EXECUTE FUNCTION {schema}.smr2_reject_retention_delete()"
                )
                .as_str(),
            )
            .await
            .expect("postgres retention failure trigger should install");

        let error = opened
            .store
            .fenced_compact_retained_history(
                &fixture.lease.owner_id,
                fixture.lease.epoch,
                SequenceNumber(3),
                fixture.config,
            )
            .expect_err("postgres delete trigger should abort retention");
        assert!(matches!(
            error,
            crate::CommitterLeaseError::Storage(nimbus_core::Error::Storage {
                kind: nimbus_core::StorageErrorKind::Other,
                message,
            }) if message.contains("smr2 provider delete failure")
        ));
        super::super::assert_provider_retention_rollback(opened.store.as_ref(), &fixture);

        client
            .batch_execute(
                format!(
                    "DROP TRIGGER smr2_reject_retention_delete ON {schema}.commit_log; \
                     DROP FUNCTION {schema}.smr2_reject_retention_delete()"
                )
                .as_str(),
            )
            .await
            .expect("postgres retention failure trigger should drop");
        super::super::finish_provider_retention_rollback(opened.store.as_ref(), fixture);
        drop(client);
        connection_task
            .await
            .expect("postgres fault connection task should join")
            .expect("postgres fault connection should close cleanly");
    })
    .await;
}
