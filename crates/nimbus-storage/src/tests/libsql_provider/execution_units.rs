use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_trigger_delivery_cursor_round_trips_in_remote_metadata() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("trigger-cursor").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should load"),
            nimbus_core::TriggerDeliveryCursor::default()
        );

        opened
            .store
            .set_trigger_delivery_cursor(nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(
                17,
            )))
            .expect("cursor should persist");

        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("cursor should round trip"),
            nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(17))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_disabled_cron_job_still_reports_scheduled_work() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("disabled-cron").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::exercise_disabled_cron_job_still_reports_scheduled_work(
            opened.store.as_ref(),
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_execution_unit_batch_and_scheduler_state_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("batch").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table_schema = TableSchema {
            table: TableName::new("tasks").expect("table name should build"),
            fields: vec![FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            }],
            indexes: Vec::new(),
            access_policy: None,
        };
        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");
        timeout(Duration::from_secs(5), async {
            loop {
                let freshness = opened
                    .store
                    .replica_freshness_stats()
                    .expect("freshness stats should load while schema refresh runs");
                if freshness.full_snapshot_refresh_count >= 1 {
                    assert_eq!(
                        freshness.last_refresh_cause,
                        LibsqlReplicaRefreshCause::SchemaWrite
                    );
                    assert_eq!(
                        freshness.last_refresh_path,
                        LibsqlReplicaRefreshPath::FullSnapshotRebuild
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("schema write should trigger a full snapshot refresh");
        let document = crate::tests::sample_document("tasks", "batched");
        let scheduled_job = scheduled_insert_job(Timestamp(5_000), "queued");

        let commit = opened
            .store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: document.clone(),
                    indexes: Vec::new(),
                    resource_path_binding: None,
                }],
                &[ResolvedScheduleOp::Insert {
                    job: scheduled_job.clone(),
                }],
            )
            .expect("batch should succeed")
            .expect("batch with writes should emit a commit");
        assert_eq!(commit.sequence, SequenceNumber(2));
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );
        assert_eq!(
            opened
                .store
                .list_scheduled_jobs()
                .expect("pending jobs should read"),
            vec![scheduled_job.clone()]
        );

        let claimed = opened
            .store
            .claim_due_jobs(Timestamp(5_000), usize::MAX)
            .expect("claim should succeed");
        assert_eq!(claimed, vec![scheduled_job.clone()]);

        opened
            .store
            .recover_running_jobs(Timestamp(6_000))
            .expect("running-job recovery should succeed");
        let recovered = opened
            .store
            .list_scheduled_jobs()
            .expect("pending jobs should read");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, scheduled_job.id);
        // Recovery preserves the ORIGINAL due time (min with recovery-now);
        // re-stamping the recovery instant delayed recovered work and
        // flaked under wall-clock regression.
        assert_eq!(recovered[0].run_at, scheduled_job.run_at);

        let claimed = opened
            .store
            .claim_due_jobs(Timestamp(6_000), usize::MAX)
            .expect("second claim should succeed");
        let result = ScheduledJobResult {
            id: scheduled_job.id.clone(),
            run_at: claimed[0].run_at,
            finished_at: Timestamp(6_500),
            mutation: claimed[0].mutation.clone(),
            outcome: ScheduledJobOutcome::Completed,
            error: None,
        };
        opened
            .store
            .record_scheduled_job_result(&result)
            .expect("result should persist");
        opened
            .store
            .complete_scheduled_job(&scheduled_job.id)
            .expect("complete should succeed");
        assert_eq!(
            opened
                .store
                .get_scheduled_job_result(&scheduled_job.id)
                .expect("result lookup should succeed"),
            Some(result)
        );

        let cron = CronJob {
            name: "heartbeat".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should build"),
                id: None,
                fields: serde_json::Map::from_iter([(
                    "title".to_string(),
                    serde_json::json!("heartbeat"),
                )]),
            },
            enabled: true,
            last_run: None,
            next_run: Timestamp(7_000),
            created_at: Timestamp(500),
        };
        opened
            .store
            .save_cron_job(&cron)
            .expect("cron save should succeed");
        assert_eq!(
            opened
                .store
                .load_cron_jobs()
                .expect("cron load should succeed"),
            vec![cron.clone()]
        );
        assert_eq!(
            opened
                .store
                .next_scheduled_work_at()
                .expect("next scheduled work should read"),
            Some(Timestamp(7_000))
        );
        assert!(
            opened
                .store
                .has_scheduled_work()
                .expect("scheduler work should be present")
        );
        opened
            .store
            .delete_cron_job(&cron.name)
            .expect("cron delete should succeed");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_execution_unit_batch_round_trips_resource_path_bindings() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("resource-paths").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("landmarks_store").expect("table name should build");
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(1))]),
        );
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), document.id.clone()),
            DocumentPath::from_segments(["cities", "SF", "landmarks", "golden-gate"])
                .expect("document path should parse"),
        );

        let insert_commit = opened
            .store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: document.clone(),
                    indexes: Vec::new(),
                    resource_path_binding: Some(binding.clone()),
                }],
                &[],
            )
            .expect("insert batch should succeed")
            .expect("insert batch should emit a commit");
        assert_eq!(insert_commit.sequence, SequenceNumber(1));

        let snapshot = opened
            .store
            .read_snapshot()
            .expect("replica snapshot should open after insert");
        assert_eq!(
            snapshot
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator.clone())
        );
        assert_eq!(
            snapshot
                .scan_collection_group_bindings(
                    &CollectionName::new("landmarks").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed"),
            vec![binding.clone()]
        );
        drop(snapshot);

        let delete_commit = opened
            .store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Delete {
                    previous: document,
                    indexes: Vec::new(),
                }],
                &[],
            )
            .expect("delete batch should succeed")
            .expect("delete batch should emit a commit");
        assert_eq!(delete_commit.sequence, SequenceNumber(2));

        let snapshot = opened
            .store
            .read_snapshot()
            .expect("replica snapshot should open after delete");
        assert!(
            snapshot
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed")
                .is_none(),
            "delete batch should remove the sidecar binding"
        );
        assert!(
            snapshot
                .scan_collection_group_bindings(
                    &CollectionName::new("landmarks").expect("collection group should parse"),
                )
                .expect("collection-group scan should succeed")
                .is_empty(),
            "delete batch should clear the collection-group index row"
        );
    })
    .await;
}
