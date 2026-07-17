use std::sync::Arc;

use nimbus_core::{
    CreateCronRequest, FieldSchema, FieldType, Mutation, Query, ScheduleRequest,
    ScheduledJobOutcome, ScheduledJobResult, TableName, TableSchema, TenantId, Timestamp,
};
use nimbus_storage::{
    FaultOccurrence, FaultPoint, ManualClock, NoopFaultInjector, ScriptedFaultInjector,
};
use nimbus_testing::{
    DeterministicHarness, EngineFixture, RestartBoundary, RestartPoint, ScenarioMetadata,
    ScriptedRestartSchedule, wait_for_value,
};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, timeout};

use super::{super::Engine, scheduled_jobs::SCHEDULED_JOB_CLAIM_BATCH_LIMIT};
use crate::SubscriptionUpdate;
use crate::engine::SubscribeOptions;

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("table name should be valid")
}

fn query_for(table: &str) -> Query {
    Query {
        table: TableName::new(table).expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

fn subscription_channel() -> (
    mpsc::Sender<SubscriptionUpdate>,
    mpsc::Receiver<SubscriptionUpdate>,
) {
    mpsc::channel(16)
}

fn insert_task_mutation(title: &str) -> Mutation {
    Mutation::Insert {
        table: tasks_table(),
        id: None,
        fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    }
}

fn users_schema() -> TableSchema {
    TableSchema {
        table: TableName::new("users").expect("table name should be valid"),
        fields: vec![
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: Vec::new(),
        access_policy: None,
    }
}

async fn spawn_scheduler(
    engine: Arc<Engine>,
    interval: Duration,
) -> (watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        crate::scheduler::run_scheduler_with_interval(engine, shutdown_rx, interval).await;
    });
    (shutdown_tx, handle)
}

#[tokio::test]
async fn scheduler_async_write_path_round_trips_pending_running_and_history_state() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            ScheduleRequest {
                run_after_ms: 25,
                mutation: insert_task_mutation("scheduled-async"),
            },
        )
        .await
        .expect("schedule should succeed");
    assert_eq!(
        engine
            .list_scheduled_jobs_async(tenant_id.clone())
            .await
            .expect("list should succeed")
            .len(),
        1
    );

    let claimed = engine
        .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
        .await
        .expect("claim should succeed");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, job_id);
    assert!(
        engine
            .list_scheduled_jobs_async(tenant_id.clone())
            .await
            .expect("list should succeed")
            .is_empty()
    );

    let result = ScheduledJobResult {
        id: job_id.clone(),
        run_at: claimed[0].run_at,
        finished_at: Timestamp::now(),
        mutation: claimed[0].mutation.clone(),
        outcome: ScheduledJobOutcome::Completed,
        error: None,
    };
    engine
        .record_scheduled_job_result_async(tenant_id.clone(), result.clone())
        .await
        .expect("history should save");
    engine
        .complete_scheduled_job_async(tenant_id.clone(), job_id.clone())
        .await
        .expect("completion should succeed");

    let loaded = engine
        .get_scheduled_job_result_async(tenant_id.clone(), job_id.clone())
        .await
        .expect("history should load");
    assert_eq!(loaded.outcome, ScheduledJobOutcome::Completed);
}

#[tokio::test]
async fn scheduler_continues_claimed_batch_after_result_bookkeeping_failure() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let clock = Arc::new(ManualClock::new(Timestamp(1_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            clock,
            Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
                point: FaultPoint::ScheduledJobRecordResultBeforeWrite,
                visit: 1,
            }])),
        )
        .expect("engine should create"),
    );
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            ScheduleRequest {
                run_after_ms: 0,
                mutation: insert_task_mutation("first claimed job"),
            },
        )
        .await
        .expect("first job should schedule");
    let second_job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            ScheduleRequest {
                run_after_ms: 0,
                mutation: insert_task_mutation("second claimed job"),
            },
        )
        .await
        .expect("second job should schedule");

    crate::scheduler::tick_at_async(&engine, Timestamp(1_000))
        .await
        .expect("scheduler tick should isolate per-job bookkeeping failure");

    let documents = engine
        .list_documents(&tenant_id, &tasks_table())
        .expect("documents should list");
    let titles = documents
        .iter()
        .filter_map(|document| {
            document
                .fields
                .get("title")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        titles,
        std::collections::BTreeSet::from(["first claimed job", "second claimed job"])
    );

    let first_result = engine
        .get_scheduled_job_result_async(tenant_id.clone(), first_job_id)
        .await;
    let second_result = engine
        .get_scheduled_job_result_async(tenant_id.clone(), second_job_id)
        .await;
    let results = [first_result, second_result];
    let completed_results = results
        .iter()
        .filter(|result| {
            matches!(
                result,
                Ok(record) if record.outcome == ScheduledJobOutcome::Completed
            )
        })
        .count();
    let missing_results = results
        .iter()
        .filter(|result| matches!(result, Err(nimbus_core::Error::ScheduledJobNotFound(_))))
        .count();
    assert_eq!(
        completed_results, 1,
        "one job should persist result bookkeeping after the injected first failure"
    );
    assert_eq!(
        missing_results, 1,
        "the injected bookkeeping failure should affect only the failing job"
    );
}

#[test]
fn scheduler_claims_bounded_due_job_batch() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let engine = Engine::new(data_dir.path()).expect("engine should create");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for index in 0..(SCHEDULED_JOB_CLAIM_BATCH_LIMIT + 2) {
        engine
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation(format!("batch job {index}").as_str()),
                },
            )
            .expect("schedule should succeed");
    }

    let claimed = engine
        .claim_due_jobs(&tenant_id, Timestamp(u64::MAX))
        .expect("claim should succeed");
    assert_eq!(claimed.len(), SCHEDULED_JOB_CLAIM_BATCH_LIMIT);
    assert_eq!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("pending jobs should list")
            .len(),
        2
    );

    let remaining = engine
        .claim_due_jobs(&tenant_id, Timestamp(u64::MAX))
        .expect("second claim should succeed");
    assert_eq!(remaining.len(), 2);
}

#[tokio::test]
async fn scheduled_mutation_executes_and_triggers_reactive_update() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let (shutdown_tx, scheduler_handle) =
        spawn_scheduler(engine.clone(), Duration::from_millis(25)).await;

    let (tx, mut rx) = subscription_channel();
    let subscription = engine
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "sched-1".to_string(),
            tx,
            SubscribeOptions::anonymous(),
        )
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert_eq!(actual_id, subscription_id);
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial update: {other:?}"),
    }

    let job_id = engine
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 50,
                mutation: insert_task_mutation("Scheduled task"),
            },
        )
        .expect("schedule should succeed");

    let update = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("reactive update should arrive before timeout")
        .expect("reactive update channel should stay open");
    match update {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Scheduled task"));
        }
        other => panic!("unexpected scheduled update: {other:?}"),
    }

    assert!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        engine.complete_scheduled_job(&tenant_id, &job_id).is_ok(),
        "completing an already-finished job should be harmless"
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn manual_clock_advances_scheduled_work_without_wall_clock_sleep() {
    let harness = DeterministicHarness::scenario("manual-clock-scheduler", 1, Timestamp(1_000));
    let fixture = EngineFixture::new_with_harness(harness.clone(), |path, harness| {
        Engine::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 500,
                mutation: insert_task_mutation("clocked task"),
            },
        )
        .expect("schedule should succeed");

    crate::scheduler::tick_at_async(&engine, Timestamp(1_000))
        .await
        .expect("initial tick should succeed");
    assert!(
        engine
            .query_documents(&tenant_id, &query_for("tasks"))
            .expect("query should succeed")
            .is_empty()
    );

    let advanced = harness.clock().advance_ms(500);
    crate::scheduler::tick_at_async(&engine, advanced)
        .await
        .expect("advanced tick should succeed");
    let documents = engine
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].get_field("title"),
        Some(&json!("clocked task"))
    );
    assert_eq!(harness.describe(), "manual-clock-scheduler (seed 1)");
}

#[tokio::test]
async fn scheduled_mutation_validates_against_schema() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");
    let job_id = engine
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 0,
                mutation: Mutation::Insert {
                    table: TableName::new("users").expect("table name should be valid"),
                    id: None,
                    fields: serde_json::Map::from_iter([("age".to_string(), json!(42))]),
                },
            },
        )
        .expect("schedule should succeed");

    crate::scheduler::tick_at_async(&engine, Timestamp::now())
        .await
        .expect("tick should succeed");

    assert!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        engine
            .list_documents(
                &tenant_id,
                &TableName::new("users").expect("table name should be valid"),
            )
            .expect("list should succeed")
            .is_empty()
    );

    let result = engine
        .get_scheduled_job_result(&tenant_id, &job_id)
        .expect("job result should exist");
    assert_eq!(result.outcome, ScheduledJobOutcome::Failed);
    assert!(
        result
            .error
            .as_deref()
            .expect("failed result should include an error")
            .contains("schema validation error")
    );
}

#[tokio::test]
async fn cron_job_executes_repeatedly() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .create_cron_job(
            &tenant_id,
            CreateCronRequest {
                name: "heartbeat".to_string(),
                schedule: nimbus_core::CronSchedule::Interval { seconds: 1 },
                mutation: insert_task_mutation("heartbeat"),
            },
        )
        .expect("cron should create");

    for _ in 0..3 {
        let cron = engine
            .load_cron_jobs(&tenant_id)
            .expect("load should succeed")
            .into_iter()
            .next()
            .expect("cron should exist");
        crate::scheduler::tick_at_async(&engine, cron.next_run)
            .await
            .expect("tick should succeed");
    }

    let documents = engine
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 3);
}

#[tokio::test]
async fn cron_missed_ticks_execute_once() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .create_cron_job(
            &tenant_id,
            CreateCronRequest {
                name: "catchup".to_string(),
                schedule: nimbus_core::CronSchedule::Interval { seconds: 1 },
                mutation: insert_task_mutation("catchup"),
            },
        )
        .expect("cron should create");

    let mut cron = engine
        .load_cron_jobs(&tenant_id)
        .expect("load should succeed")
        .into_iter()
        .next()
        .expect("cron should exist");
    cron.next_run = Timestamp(1_000);
    cron.last_run = None;
    engine
        .update_cron_job(&tenant_id, &cron)
        .expect("cron should update");

    crate::scheduler::tick_at_async(&engine, Timestamp(10_000))
        .await
        .expect("tick should succeed");

    let documents = engine
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);

    let updated = engine
        .load_cron_jobs(&tenant_id)
        .expect("load should succeed")
        .into_iter()
        .next()
        .expect("cron should exist");
    assert_eq!(updated.last_run, Some(Timestamp(10_000)));
    assert!(updated.next_run.0 > 10_000);
}

#[tokio::test]
async fn load_tenants_with_scheduled_work_recovers_running_jobs() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    {
        let engine = Engine::new(data_dir.path()).expect("engine should create");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("Recovered task"),
                },
            )
            .expect("schedule should succeed");
        let claimed = engine
            .claim_due_jobs(&tenant_id, Timestamp::now())
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
    }

    let reloaded = Arc::new(Engine::new(data_dir.path()).expect("engine should reopen"));
    reloaded
        .load_tenants_with_scheduled_work()
        .expect("scheduled tenants should load");

    assert_eq!(reloaded.loaded_tenant_ids(), vec![tenant_id.clone()]);
    assert_eq!(
        reloaded
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .len(),
        1
    );

    crate::scheduler::tick_at_async(&reloaded, Timestamp::now())
        .await
        .expect("tick should succeed");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("Recovered task"))
    );
}

#[tokio::test]
async fn recovered_scheduled_job_does_not_double_apply_after_replay() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    let job_id = {
        let engine = Engine::new(data_dir.path()).expect("engine should create");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let job_id = engine
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("Only once"),
                },
            )
            .expect("schedule should succeed");
        let claimed = engine
            .claim_due_jobs(&tenant_id, Timestamp::now())
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        let execution_id = format!("scheduled:{job_id}");
        assert!(
            engine
                .execute_scheduled_mutation(&tenant_id, &execution_id, claimed[0].mutation.clone())
                .expect("first scheduled execution should succeed")
        );
        job_id
    };

    let reloaded = Arc::new(Engine::new(data_dir.path()).expect("engine should reopen"));
    reloaded
        .load_tenants_with_scheduled_work()
        .expect("scheduled tenants should load");

    crate::scheduler::tick_at_async(&reloaded, Timestamp::now())
        .await
        .expect("tick should succeed");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Only once")));

    let result = reloaded
        .get_scheduled_job_result(&tenant_id, &job_id)
        .expect("job result should exist");
    assert_eq!(result.outcome, ScheduledJobOutcome::Completed);
    assert!(result.error.is_none());
}

#[tokio::test]
async fn scheduler_recovery_campaign_survives_claim_and_completion_restart_boundaries() {
    let restart_schedule = ScriptedRestartSchedule::scripted(
        ScenarioMetadata::new("scheduler-restart-campaign", 61),
        [
            RestartPoint::new(0, RestartBoundary::SchedulerClaim),
            RestartPoint::new(1, RestartBoundary::SchedulerCompletion),
            RestartPoint::new(2, RestartBoundary::SchedulerClaim),
        ],
    );
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let clock = Arc::new(ManualClock::new(Timestamp(1_000)));
    let open_engine =
        || Engine::new_with_simulation(data_dir.path(), clock.clone(), Arc::new(NoopFaultInjector));

    let (first_job_id, second_job_id) = {
        let engine = open_engine().expect("engine should create");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let first_job_id = engine
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("scheduler-alpha"),
                },
            )
            .expect("first schedule should succeed");
        let second_job_id = engine
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("scheduler-beta"),
                },
            )
            .expect("second schedule should succeed");
        let claimed = engine
            .claim_due_jobs(&tenant_id, Timestamp(1_000))
            .expect("initial claim should succeed");
        assert_eq!(
            claimed.len(),
            2,
            "{}",
            restart_schedule.failure_context(
                "initial scheduler claim should move both jobs into running state",
                Some(0),
            )
        );
        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("initial engine trigger worker should stop before restart");
        engine.quiesce().await;
        (first_job_id, second_job_id)
    };

    let completed_job_id = {
        clock.set(Timestamp(2_000));
        let reloaded = open_engine().expect("engine should reopen after claim");
        reloaded
            .load_tenants_with_scheduled_work()
            .expect("scheduled tenants should load after claim restart");
        assert!(
            reloaded
                .list_documents(&tenant_id, &tasks_table())
                .expect("documents should list after claim recovery")
                .is_empty(),
            "{}",
            restart_schedule.failure_context(
                "claim-boundary restart should not make scheduled mutations visible",
                Some(0),
            )
        );
        assert_eq!(
            reloaded
                .list_scheduled_jobs(&tenant_id)
                .expect("pending jobs should list after claim recovery")
                .len(),
            2,
            "{}",
            restart_schedule.failure_context(
                "claim-boundary restart should recover running jobs back to pending",
                Some(0),
            )
        );

        let mut claimed = reloaded
            .claim_due_jobs(&tenant_id, Timestamp(2_000))
            .expect("claim after restart should succeed");
        assert!(
            claimed.iter().all(|job| job.run_at == Timestamp(1_000)),
            "recovery must preserve the already-due schedule timestamp"
        );
        claimed.sort_by_key(|job| job.id.to_string());
        let completed_job = claimed.remove(0);
        let execution_id = format!("scheduled:{}", completed_job.id);
        assert!(
            reloaded
                .execute_scheduled_mutation(
                    &tenant_id,
                    &execution_id,
                    completed_job.mutation.clone(),
                )
                .expect("scheduled mutation execution should succeed"),
            "{}",
            restart_schedule.failure_context(
                "executing a recovered scheduled mutation should apply exactly once",
                Some(1),
            )
        );
        reloaded
            .record_scheduled_job_result(
                &tenant_id,
                &ScheduledJobResult {
                    id: completed_job.id.clone(),
                    run_at: completed_job.run_at,
                    finished_at: Timestamp(2_000),
                    mutation: completed_job.mutation,
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .expect("completed job result should persist");
        reloaded
            .complete_scheduled_job(&tenant_id, &completed_job.id)
            .expect("completed job should leave the running set");
        reloaded
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("claim-recovery trigger worker should stop before restart");
        reloaded.quiesce().await;
        completed_job.id
    };

    {
        clock.set(Timestamp(3_000));
        let reloaded = open_engine().expect("engine should reopen after completion");
        reloaded
            .load_tenants_with_scheduled_work()
            .expect("scheduled tenants should load after completion restart");
        let documents = reloaded
            .list_documents(&tenant_id, &tasks_table())
            .expect("documents should list after completion recovery");
        assert_eq!(
            documents.len(),
            1,
            "{}",
            restart_schedule.failure_context(
                "completion-boundary restart should preserve the completed mutation exactly once",
                Some(1),
            )
        );
        assert_eq!(
            reloaded
                .list_scheduled_jobs(&tenant_id)
                .expect("pending jobs should list after completion restart")
                .len(),
            1,
            "{}",
            restart_schedule.failure_context(
                "completion-boundary restart should recover the still-running job",
                Some(1),
            )
        );
        assert_eq!(
            reloaded
                .get_scheduled_job_result(&tenant_id, &completed_job_id)
                .expect("completed job result should persist after restart")
                .outcome,
            ScheduledJobOutcome::Completed
        );
        let claimed = reloaded
            .claim_due_jobs(&tenant_id, Timestamp(3_000))
            .expect("remaining job should claim after completion restart");
        assert_eq!(
            claimed.len(),
            1,
            "{}",
            restart_schedule.failure_context(
                "second claim-boundary restart should leave exactly one pending job to recover",
                Some(2),
            )
        );
        reloaded
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("completion-recovery trigger worker should stop before restart");
        reloaded.quiesce().await;
    }

    clock.set(Timestamp(4_000));
    let reloaded = Arc::new(open_engine().expect("engine should reopen after second claim"));
    reloaded
        .load_tenants_with_scheduled_work()
        .expect("scheduled tenants should load after second claim restart");
    assert_eq!(
        reloaded
            .list_scheduled_jobs(&tenant_id)
            .expect("pending jobs should list after second claim restart")
            .len(),
        1,
        "{}",
        restart_schedule.failure_context(
            "second claim-boundary restart should recover the remaining running job",
            Some(2),
        )
    );
    let recovered = reloaded
        .list_scheduled_jobs(&tenant_id)
        .expect("recovered pending job should list");
    assert_eq!(recovered[0].run_at, Timestamp(1_000));
    crate::scheduler::tick_at_async(&reloaded, Timestamp(4_000))
        .await
        .expect("scheduler tick should complete the recovered job");
    let mut titles = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("documents should list after final recovery")
        .into_iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("scheduled mutation title should be present")
                .to_string()
        })
        .collect::<Vec<_>>();
    titles.sort();
    let first_result = reloaded
        .get_scheduled_job_result(&tenant_id, &first_job_id)
        .expect("first job result should exist after final recovery");
    let second_result = reloaded
        .get_scheduled_job_result(&tenant_id, &second_job_id)
        .expect("second job result should exist after final recovery");
    assert_eq!(
        titles,
        vec!["scheduler-alpha", "scheduler-beta"],
        "recorded results: first={first_result:?}, second={second_result:?}"
    );
    assert!(
        reloaded
            .list_scheduled_jobs(&tenant_id)
            .expect("pending jobs should list after final recovery")
            .is_empty(),
        "{}",
        restart_schedule.failure_context(
            "all recovered scheduled jobs should eventually complete",
            None,
        )
    );
    assert_eq!(first_result.outcome, ScheduledJobOutcome::Completed);
    assert_eq!(second_result.outcome, ScheduledJobOutcome::Completed);
    reloaded
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("final trigger worker should stop before test shutdown");
    reloaded.quiesce().await;
}

#[tokio::test]
async fn scheduler_wakes_promptly_when_earlier_work_arrives() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let (shutdown_tx, scheduler_handle) =
        spawn_scheduler(engine.clone(), Duration::from_secs(60 * 60)).await;

    engine
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 60_000,
                mutation: insert_task_mutation("Later task"),
            },
        )
        .expect("later schedule should succeed");
    // Let the scheduler observe the far-future wake-up before we inject the
    // earlier job that should force a prompt wake.
    tokio::time::sleep(Duration::from_millis(50)).await;

    engine
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 0,
                mutation: insert_task_mutation("Immediate task"),
            },
        )
        .expect("immediate schedule should succeed");

    let documents = wait_for_value(
        "scheduler should wake and execute immediate work",
        Duration::from_secs(2),
        Duration::ZERO,
        || async {
            engine
                .list_documents(&tenant_id, &tasks_table())
                .expect("list should succeed")
        },
        |documents| !documents.is_empty(),
    )
    .await;

    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("Immediate task"))
    );
    assert_eq!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .len(),
        1
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn scheduler_tick_processes_other_tenants_while_one_tenant_is_paused() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("tenant-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);

    let tenant_a_pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_a)
        .expect("tenant-a journal pause handle should load");
    tenant_a_pause.arm();

    engine
        .schedule_mutation(
            &tenant_a,
            ScheduleRequest {
                run_after_ms: 0,
                mutation: insert_task_mutation("tenant-a paused"),
            },
        )
        .expect("tenant-a schedule should succeed");
    engine
        .schedule_mutation(
            &tenant_b,
            ScheduleRequest {
                run_after_ms: 0,
                mutation: insert_task_mutation("tenant-b ready"),
            },
        )
        .expect("tenant-b schedule should succeed");

    let tick_task = tokio::spawn({
        let engine = engine.clone();
        async move { crate::scheduler::tick_at_async(&engine, Timestamp::now()).await }
    });

    let pause_wait = tenant_a_pause.clone();
    assert!(
        tokio::task::spawn_blocking(move || pause_wait.wait_until_entered(Duration::from_secs(1)))
            .await
            .expect("pause wait should join"),
        "tenant-a scheduled mutation should pause before the journal drain"
    );

    let tenant_b_documents = wait_for_value(
        "tenant-b scheduled work should not be blocked by tenant-a",
        Duration::from_secs(2),
        Duration::ZERO,
        || async {
            engine
                .list_documents(&tenant_b, &tasks_table())
                .expect("tenant-b documents should list")
        },
        |documents| !documents.is_empty(),
    )
    .await;

    assert_eq!(tenant_b_documents.len(), 1);
    assert_eq!(
        tenant_b_documents[0].fields.get("title"),
        Some(&json!("tenant-b ready"))
    );
    assert!(
        engine
            .list_documents(&tenant_a, &tasks_table())
            .expect("tenant-a documents should list")
            .is_empty(),
        "tenant-a should still be blocked while its journal pause is armed"
    );
    assert!(
        !tick_task.is_finished(),
        "the scheduler tick should still be waiting for the paused tenant after tenant-b completes"
    );

    tenant_a_pause.release();
    tick_task
        .await
        .expect("scheduler tick should join")
        .expect("scheduler tick should succeed");

    let tenant_a_documents = engine
        .list_documents(&tenant_a, &tasks_table())
        .expect("tenant-a documents should list after release");
    assert_eq!(tenant_a_documents.len(), 1);
    assert_eq!(
        tenant_a_documents[0].fields.get("title"),
        Some(&json!("tenant-a paused"))
    );
}
