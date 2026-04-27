use super::support::*;

#[test]
fn sqlite_scheduled_execution_marker_deduplicates_insert_commit() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "Hello once");

    let first = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("first insert should succeed");
    let second = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("second insert should succeed");

    assert!(first.is_some(), "first scheduled execution should commit");
    assert!(
        second.is_none(),
        "second scheduled execution should be skipped"
    );
    assert!(
        store
            .scheduled_execution_exists("scheduled:test-job")
            .expect("scheduled execution marker should read")
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should read"),
        SequenceNumber(1)
    );
    let tasks = store
        .scan_table_matching_with_filters_cancellable(
            &TableName::new("tasks").expect("table name should be valid"),
            &[],
            &mut || Ok(()),
            |_| Ok(true),
        )
        .expect("scan should succeed");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Hello once")));
}

#[test]
fn sqlite_execution_unit_batch_rolls_back_when_schedule_ops_fail() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "batched");

    let error = store
        .apply_execution_unit_batch(
            &[ResolvedWrite::Insert {
                document: document.clone(),
                indexes: Vec::new(),
                resource_path_binding: None,
            }],
            &[ResolvedScheduleOp::Cancel {
                job_id: DocumentId::new(),
            }],
        )
        .expect_err("batch should fail when a scheduled cancel misses");
    assert!(matches!(error, Error::ScheduledJobNotFound(_)));
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .is_none(),
        "failed batches must roll back document writes"
    );
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read")
            .is_empty()
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should remain empty"),
        SequenceNumber(0)
    );
}

#[test]
fn sqlite_execution_unit_batch_commits_documents_and_schedule_ops_together() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "batched");
    let scheduled_job = scheduled_insert_job(Timestamp(5_000), "queued");

    let commit = store
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

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].op_type, WriteOpType::Insert);
    assert_eq!(
        store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .as_ref(),
        Some(&document)
    );
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read"),
        vec![scheduled_job]
    );
}

#[test]
fn sqlite_scheduler_state_round_trips_results_crons_and_recovery() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "due");

    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next scheduled work should read"),
        Some(Timestamp(1_000))
    );
    assert!(
        store
            .has_scheduled_work()
            .expect("pending work should count"),
    );

    let claimed = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job.clone()]);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending jobs should read")
            .is_empty()
    );

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("running-job recovery should succeed");
    let recovered = store
        .list_scheduled_jobs()
        .expect("pending jobs should read");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, job.id);
    assert_eq!(recovered[0].run_at, Timestamp(2_000));

    let claimed = store
        .claim_due_jobs(Timestamp(2_000))
        .expect("second claim should succeed");
    assert_eq!(claimed.len(), 1);
    let result = ScheduledJobResult {
        id: job.id.clone(),
        run_at: Timestamp(2_000),
        finished_at: Timestamp(2_500),
        mutation: claimed[0].mutation.clone(),
        outcome: ScheduledJobOutcome::Completed,
        error: None,
    };
    store
        .record_scheduled_job_result(&result)
        .expect("result should persist");
    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
    assert_eq!(
        store
            .get_scheduled_job_result(&job.id)
            .expect("result lookup should succeed"),
        Some(result)
    );

    let cron = CronJob {
        name: "heartbeat".to_string(),
        schedule: CronSchedule::Interval { seconds: 10 },
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
        },
        enabled: true,
        last_run: None,
        next_run: Timestamp(3_000),
        created_at: Timestamp(500),
    };
    store
        .save_cron_job(&cron)
        .expect("cron save should succeed");
    assert_eq!(
        store.load_cron_jobs().expect("cron load should succeed"),
        vec![cron.clone()]
    );
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next scheduled work should read"),
        Some(Timestamp(3_000))
    );
    assert!(
        store
            .has_scheduled_work()
            .expect("cron should count as work")
    );
    store
        .delete_cron_job(&cron.name)
        .expect("cron delete should succeed");
    assert!(
        !store
            .has_scheduled_work()
            .expect("no work should remain after cleanup"),
    );
}

#[test]
fn sqlite_claim_due_jobs_includes_u64_max_boundary() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let job = scheduled_insert_job(Timestamp(u64::MAX), "max");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(u64::MAX))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job]);
}
