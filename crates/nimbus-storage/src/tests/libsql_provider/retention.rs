use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_stale_lease_retention_rebuilds_cache_and_survives_restart() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-contract").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let expected_floor = super::super::exercise_provider_retention_checkpoint(
            opened.store.as_ref(),
            "libsql_retention",
        );
        let freshness = opened
            .store
            .replica_freshness_stats()
            .expect("libSQL cache freshness should read");
        assert!(
            freshness.full_snapshot_refresh_count >= 1,
            "remote prefix pruning must record a checkpoint-compatible full cache rebuild; \
             a later incremental refresh may legitimately replace last_refresh_path"
        );

        let reopened_provider = LibsqlReplicaProvider::connect(config)
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
#[serial]
async fn libsql_provider_retention_checkpoint_fault_rolls_back_every_delete() {
    let faults = Arc::new(crate::ScriptedFaultInjector::new([
        crate::FaultOccurrence {
            point: crate::FaultPoint::RetentionCheckpointBeforeCommit,
            visit: 1,
        },
    ]));
    with_test_provider_with_faults(faults, |provider, _config| async move {
        let tenant = TenantId::new("retention-fault").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_provider_retention_fault_rollback(
            opened.store.as_ref(),
            "libsql_retention_fault",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_page_rejects_concurrent_retention_prune() {
    let (faults, rows_read, resume) = super::super::pause_after_retention_read_page();
    let pause = Arc::clone(&faults);
    with_test_provider_with_faults(faults, |provider, _config| async move {
        let tenant = TenantId::new("retention-page-race").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_provider_retention_concurrent_prune_page(
            opened.store,
            "libsql_retention_page",
            pause,
            rows_read,
            resume,
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_retention_sql_error_preserves_error_and_rolls_back() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-provider-error").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let fixture = super::super::prepare_provider_retention_rollback(
            opened.store.as_ref(),
            "libsql_provider_error",
        );
        let namespace = provider
            .tenant_namespace(&tenant)
            .expect("tenant namespace should build");
        let database = open_remote_namespace_database(&config, &namespace)
            .await
            .expect("remote tenant namespace should open");
        let conn = database
            .connect()
            .expect("remote tenant connection should open");
        conn.execute_batch(
            "CREATE TRIGGER smr2_reject_retention_delete \
             BEFORE DELETE ON commit_log BEGIN \
             SELECT RAISE(ABORT, 'smr2 provider delete failure'); END",
        )
        .await
        .expect("libSQL retention failure trigger should install");

        let error = opened
            .store
            .fenced_compact_retained_history(
                &fixture.lease.owner_id,
                fixture.lease.epoch,
                SequenceNumber(3),
                fixture.config,
            )
            .expect_err("libSQL delete trigger should abort retention");
        assert!(
            matches!(
            &error,
            crate::CommitterLeaseError::Storage(nimbus_core::Error::Storage {
                kind: nimbus_core::StorageErrorKind::Unavailable,
                message,
            }) if message.contains("smr2 provider delete failure")
            ),
            "unexpected libSQL retention error classification: {error:?}"
        );
        super::super::assert_provider_retention_rollback(opened.store.as_ref(), &fixture);

        conn.execute_batch("DROP TRIGGER smr2_reject_retention_delete")
            .await
            .expect("libSQL retention failure trigger should drop");
        super::super::finish_provider_retention_rollback(opened.store.as_ref(), fixture);
    })
    .await;
}
