use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_provider_stale_lease_retention_is_atomic_and_restart_safe() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-contract").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let expected_floor = super::super::exercise_provider_retention_checkpoint(
            opened.store.as_ref(),
            "mysql_retention",
        );

        let reopened_provider = MySqlProvider::connect(config)
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
async fn mysql_provider_retention_checkpoint_fault_rolls_back_every_delete() {
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
            "mysql_retention_fault",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_provider_retention_sql_error_preserves_error_and_rolls_back() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("retention-provider-error").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let fixture = super::super::prepare_provider_retention_rollback(
            opened.store.as_ref(),
            "mysql_provider_error",
        );
        let database = provider
            .tenant_database_name(&tenant)
            .expect("tenant database name should build");
        let opts = Opts::from_url(&config.connection_string).expect("mysql URL should parse");
        let pool = Pool::new(opts);
        let mut conn = pool
            .get_conn()
            .await
            .expect("mysql fault connection should open");
        conn.query_drop(format!(
            "CREATE TRIGGER `{database}`.smr2_reject_retention_delete \
             BEFORE DELETE ON `{database}`.commit_log FOR EACH ROW \
             SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'smr2 provider delete failure'"
        ))
        .await
        .expect("mysql retention failure trigger should install");

        let error = opened
            .store
            .fenced_compact_retained_history(
                &fixture.lease.owner_id,
                fixture.lease.epoch,
                SequenceNumber(3),
                fixture.config,
            )
            .expect_err("mysql delete trigger should abort retention");
        assert!(matches!(
            error,
            crate::CommitterLeaseError::Storage(nimbus_core::Error::Storage {
                kind: nimbus_core::StorageErrorKind::Other,
                message,
            }) if message.contains("smr2 provider delete failure")
        ));
        super::super::assert_provider_retention_rollback(opened.store.as_ref(), &fixture);

        conn.query_drop(format!(
            "DROP TRIGGER `{database}`.smr2_reject_retention_delete"
        ))
        .await
        .expect("mysql retention failure trigger should drop");
        super::super::finish_provider_retention_rollback(opened.store.as_ref(), fixture);
        conn.disconnect()
            .await
            .expect("mysql fault connection should close");
        pool.disconnect()
            .await
            .expect("mysql fault pool should close");
    })
    .await;
}
