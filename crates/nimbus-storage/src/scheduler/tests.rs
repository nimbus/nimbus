use nimbus_core::{
    CronJob, CronSchedule, DocumentId, Mutation, ScheduledJob, ScheduledJobOutcome,
    ScheduledJobResult, Timestamp,
};
use serde_json::json;

use crate::TenantStore;

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: nimbus_core::DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        },
        created_at: Timestamp(1_000),
    }
}

#[test]
fn scheduled_job_insert_and_claim_due_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "due");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(1_000), usize::MAX)
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job.clone()]);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending list should succeed")
            .is_empty(),
        "claimed jobs should leave the pending queue"
    );
    assert!(
        store
            .claim_due_jobs(Timestamp(1_000), usize::MAX)
            .expect("second claim should succeed")
            .is_empty()
    );

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
}

#[test]
fn claim_due_jobs_respects_batch_limit_and_leaves_remainder_pending() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = scheduled_insert_job(Timestamp(1_000), "first");
    let second = scheduled_insert_job(Timestamp(1_000), "second");
    let third = scheduled_insert_job(Timestamp(1_000), "third");
    for job in [&first, &second, &third] {
        store
            .insert_scheduled_job(job)
            .expect("scheduled insert should succeed");
    }
    let mut expected = [first.clone(), second.clone(), third.clone()];
    expected.sort_by(|left, right| left.run_at.cmp(&right.run_at).then(left.id.cmp(&right.id)));

    let claimed = store
        .claim_due_jobs(Timestamp(1_000), 2)
        .expect("claim should succeed");
    assert_eq!(claimed, expected[..2].to_vec());
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("pending list should succeed"),
        expected[2..]
    );

    let remaining = store
        .claim_due_jobs(Timestamp(1_000), 2)
        .expect("second claim should succeed");
    assert_eq!(remaining, expected[2..].to_vec());
}

#[test]
fn scheduled_job_future_not_due() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "later");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(4_999), usize::MAX)
        .expect("claim should succeed");
    assert!(
        claimed.is_empty(),
        "future work should not be claimed early"
    );
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("pending list should succeed"),
        vec![job]
    );
}

#[test]
fn scheduled_job_with_firestore_style_id_roundtrips() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let explicit_id =
        DocumentId::from_key("jobs.alpha-1".to_string()).expect("job id should be valid");
    let job = ScheduledJob {
        id: explicit_id.clone(),
        run_at: Timestamp(1_000),
        mutation: Mutation::Insert {
            table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!("explicit"))]),
        },
        created_at: Timestamp(900),
    };
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(1_000), usize::MAX)
        .expect("claim should succeed");

    assert_eq!(claimed, vec![job.clone()]);
    store
        .complete_scheduled_job(&explicit_id)
        .expect("complete should succeed");
}

#[test]
fn cancel_scheduled_job_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "cancel me");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    assert!(
        store
            .cancel_scheduled_job(&job.id)
            .expect("cancel should succeed"),
        "pending job should be removed"
    );
    assert!(
        store
            .list_scheduled_jobs()
            .expect("pending list should succeed")
            .is_empty(),
        "canceled jobs should disappear from the queue"
    );
    assert!(
        !store
            .cancel_scheduled_job(&job.id)
            .expect("second cancel should succeed")
    );
}

#[test]
fn recover_running_jobs_moves_orphaned_work_back_to_pending() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "recover");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");
    store
        .claim_due_jobs(Timestamp(1_000), usize::MAX)
        .expect("claim should succeed");

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("recovery should succeed");
    let recovered = store
        .list_scheduled_jobs()
        .expect("pending list should succeed");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, job.id);
    // The job was DUE at 1_000 when it was claimed; recovery preserves that
    // original due time (min with recovery-now) instead of re-stamping the
    // recovery instant — re-stamping delayed recovered work and, under
    // wall-clock regression, deferred it past the next tick entirely.
    assert_eq!(recovered[0].run_at, Timestamp(1_000));
}

#[test]
fn recovered_job_stays_claimable_under_clock_regression() {
    // CI-flake regression (scheduler_recovery_campaign): recovery at T,
    // then a tick whose clock reads T' < T (NTP slew) must STILL claim the
    // recovered job — its original due time governs, not the recovery
    // instant.
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "regress");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");
    store
        .claim_due_jobs(Timestamp(1_000), usize::MAX)
        .expect("claim should succeed");

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("recovery should succeed");
    // The post-recovery tick observes an EARLIER wall clock than recovery.
    let claimed = store
        .claim_due_jobs(Timestamp(1_500), usize::MAX)
        .expect("claim should succeed");
    assert_eq!(claimed.len(), 1, "regressed-clock tick must claim");
    assert_eq!(claimed[0].id, job.id);
}

#[test]
fn cron_job_crud_and_restart_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let cron = CronJob {
        name: "heartbeat".to_string(),
        schedule: CronSchedule::Interval { seconds: 10 },
        mutation: Mutation::Insert {
            table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
        },
        enabled: true,
        last_run: None,
        next_run: Timestamp(10_000),
        created_at: Timestamp(1_000),
    };

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.save_cron_job(&cron).expect("save should succeed");
        let crons = store.load_cron_jobs().expect("load should succeed");
        assert_eq!(crons, vec![cron.clone()]);
        assert!(store.has_scheduled_work().expect("has work should succeed"));
        store
            .delete_cron_job("heartbeat")
            .expect("delete should succeed");
        assert!(
            store
                .load_cron_jobs()
                .expect("load after delete should succeed")
                .is_empty()
        );
        store.save_cron_job(&cron).expect("re-save should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let crons = reopened.load_cron_jobs().expect("load should succeed");
    assert_eq!(crons, vec![cron]);
}

#[test]
fn has_scheduled_work_detects_pending_or_running_jobs() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert!(
        !store
            .has_scheduled_work()
            .expect("empty store should read cleanly")
    );

    let job = scheduled_insert_job(Timestamp(1_000), "pending");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");
    assert!(
        store
            .has_scheduled_work()
            .expect("pending work should count")
    );

    store
        .claim_due_jobs(Timestamp(1_000), usize::MAX)
        .expect("claim should succeed");
    assert!(
        store
            .has_scheduled_work()
            .expect("running work should count")
    );

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
    assert!(!store.has_scheduled_work().expect("has work should succeed"));
}

#[test]
fn next_scheduled_work_at_prefers_earliest_pending_or_enabled_cron() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("empty store should read cleanly"),
        None
    );

    let future_job = scheduled_insert_job(Timestamp(5_000), "later");
    let earlier_job = scheduled_insert_job(Timestamp(2_000), "earlier");
    store
        .insert_scheduled_job(&future_job)
        .expect("scheduled insert should succeed");
    store
        .insert_scheduled_job(&earlier_job)
        .expect("scheduled insert should succeed");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next work should read cleanly"),
        Some(Timestamp(2_000))
    );

    store
        .claim_due_jobs(Timestamp(2_000), usize::MAX)
        .expect("claim should succeed");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next work should read cleanly"),
        Some(Timestamp(5_000))
    );

    store
        .save_cron_job(&CronJob {
            name: "disabled".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
                id: None,
                fields: serde_json::Map::from_iter([("title".to_string(), json!("disabled"))]),
            },
            enabled: false,
            last_run: None,
            next_run: Timestamp(1_000),
            created_at: Timestamp(500),
        })
        .expect("disabled cron should save");
    store
        .save_cron_job(&CronJob {
            name: "heartbeat".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
                id: None,
                fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
            },
            enabled: true,
            last_run: None,
            next_run: Timestamp(3_000),
            created_at: Timestamp(500),
        })
        .expect("enabled cron should save");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next work should read cleanly"),
        Some(Timestamp(3_000))
    );
}

#[test]
fn scheduled_job_result_roundtrip_and_lookup() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "done");
    let result = ScheduledJobResult {
        id: job.id,
        run_at: job.run_at,
        finished_at: Timestamp(2_000),
        mutation: job.mutation,
        outcome: ScheduledJobOutcome::Failed,
        error: Some("boom".to_string()),
    };

    store
        .record_scheduled_job_result(&result)
        .expect("result write should succeed");
    let loaded = store
        .get_scheduled_job_result(&result.id)
        .expect("result lookup should succeed")
        .expect("result should exist");

    assert_eq!(loaded, result);
}

#[test]
fn claim_due_jobs_includes_u64_max_boundary() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(u64::MAX), "max");
    store
        .insert_scheduled_job(&job)
        .expect("scheduled insert should succeed");

    let claimed = store
        .claim_due_jobs(Timestamp(u64::MAX), usize::MAX)
        .expect("claim should succeed");
    assert_eq!(claimed, vec![job]);
}
