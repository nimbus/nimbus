use super::publisher_test_seams::ArmedOneShotDirectFaultInjector;
use super::*;
use crate::engine::DurableWriteRoute;

#[derive(Clone, Copy)]
enum SchemaOperation {
    Set,
    Delete,
}

#[derive(Clone, Copy)]
enum OutcomeCase {
    Unchanged,
    Advanced,
    Unreadable,
}

async fn exercise_schema_outcome(operation: SchemaOperation, case: OutcomeCase, tenant: &str) {
    let point = match case {
        OutcomeCase::Advanced => FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        OutcomeCase::Unchanged | OutcomeCase::Unreadable => {
            FaultPoint::StorageCommitBeforeVisibility
        }
    };
    let data_dir = tempdir().expect("schema outcome tempdir should build");
    let faults = ArmedOneShotDirectFaultInjector::new(point);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(47_000))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(47_001)),
        )
        .expect("schema outcome engine should create"),
    );
    let tenant_id = TenantId::new(tenant).expect("schema outcome tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("schema outcome tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let table_name = format!("{tenant}_messages");
    let table = messages_table(&table_name);
    let original_schema = messages_schema(&table_name, Vec::new(), Some(read_only_owner_policy()));
    engine
        .set_table_schema_async(tenant_id.clone(), original_schema.clone())
        .await
        .expect("original schema should persist before arming the fault");

    let (sender, mut receiver) = subscription_channel();
    let _subscription = engine
        .subscribe(
            &tenant_id,
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            format!("{tenant}-subscription"),
            sender,
            SubscribeOptions::for_principal(principal_with_subject("user-123")),
        )
        .expect("policy subscription should register");
    receiver
        .recv()
        .await
        .expect("initial policy subscription result should arrive");
    assert_eq!(
        engine
            .active_subscription_count(&tenant_id)
            .expect("active subscription count should load"),
        1
    );

    let runtime_before = engine
        .tenant_runtime_for_testing(&tenant_id)
        .expect("schema outcome runtime should stay retained for identity proof");
    let runtime_identity_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("schema outcome runtime identity should load");
    let route = match operation {
        SchemaOperation::Set => DurableWriteRoute::SchemaSet,
        SchemaOperation::Delete => DurableWriteRoute::SchemaDelete,
    };
    if matches!(case, OutcomeCase::Unreadable) {
        engine.fail_durable_outcome_progress_for_testing(tenant_id.clone(), route);
    }
    faults.arm();

    let mut changed_schema = original_schema.clone();
    changed_schema.access_policy = Some(TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "body".to_string(),
        }),
        ..TableAccessPolicy::default()
    });
    let error = match operation {
        SchemaOperation::Set => engine
            .set_table_schema_async(tenant_id.clone(), changed_schema.clone())
            .await
            .expect_err("faulted schema set should return an error"),
        SchemaOperation::Delete => engine
            .delete_table_schema_async(tenant_id.clone(), table.clone())
            .await
            .expect_err("faulted schema delete should return an error"),
    };

    match case {
        OutcomeCase::Unchanged => {
            assert!(
                !error.to_string().contains("crash-and-replay"),
                "unchanged durable head should preserve the typed persistence error: {error}"
            );
            assert_eq!(
                engine
                    .tenant_runtime_identity_for_testing(&tenant_id)
                    .expect("definitive schema runtime should stay loaded"),
                runtime_identity_before,
                "definitive schema failure must keep the tenant runtime live"
            );
            assert_eq!(
                engine
                    .get_table_schema_async(tenant_id.clone(), table.clone())
                    .await
                    .expect("original schema should remain visible"),
                original_schema
            );
            assert_eq!(
                engine
                    .active_subscription_count(&tenant_id)
                    .expect("restored subscription count should load"),
                1,
                "definitive schema failure must restore the pre-marked policy subscription"
            );
            super::support::assert_future_stays_pending(
                receiver.recv(),
                "restored policy subscription should not receive a terminal error",
            )
            .await;
            return;
        }
        OutcomeCase::Advanced | OutcomeCase::Unreadable => {
            assert!(
                error.to_string().contains("crash-and-replay"),
                "ambiguous schema outcome should demand crash-and-replay: {error}"
            );
        }
    }

    let schema_after = engine.get_schema_async(tenant_id.clone()).await.expect(
        "loading schema after ambiguity should wait for eviction and open a replacement runtime",
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement schema runtime identity should load"),
        runtime_identity_before,
        "ambiguous schema outcome must replace the tenant runtime"
    );
    assert!(
        !engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
        "ambiguous schema outcome must deregister the failed tenant runtime"
    );
    match (operation, case) {
        (SchemaOperation::Set, OutcomeCase::Advanced) => {
            assert_eq!(schema_after.get_table(&table), Some(&changed_schema));
        }
        (SchemaOperation::Delete, OutcomeCase::Advanced) => {
            assert!(schema_after.get_table(&table).is_none());
        }
        (_, OutcomeCase::Unreadable) => {
            assert_eq!(schema_after.get_table(&table), Some(&original_schema));
        }
        (_, OutcomeCase::Unchanged) => unreachable!(),
    }

    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("replacement trigger cursor should not add unrelated records");
    let head_before_follow_up = engine
        .latest_sequence(&tenant_id)
        .expect("schema outcome durable head should load");
    let follow_up_table =
        TableName::new(format!("{tenant}_follow_up")).expect("follow-up table name should build");
    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: follow_up_table,
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("replacement runtime should commit the next schema sequence");
    assert_eq!(
        engine
            .latest_sequence(&tenant_id)
            .expect("follow-up schema durable head should load"),
        SequenceNumber(head_before_follow_up.0 + 1),
        "replacement runtime must continue after the durable head without reusing a sequence"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_set_unchanged_head_is_definitive_and_restores_policy_subscription() {
    exercise_schema_outcome(
        SchemaOperation::Set,
        OutcomeCase::Unchanged,
        "schema-set-definitive",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_set_advanced_head_evicts_replays_and_does_not_reuse_sequence() {
    exercise_schema_outcome(
        SchemaOperation::Set,
        OutcomeCase::Advanced,
        "schema-set-advanced",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_set_unreadable_progress_evicts_and_replays() {
    exercise_schema_outcome(
        SchemaOperation::Set,
        OutcomeCase::Unreadable,
        "schema-set-unreadable",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_delete_unchanged_head_is_definitive_and_restores_policy_subscription() {
    exercise_schema_outcome(
        SchemaOperation::Delete,
        OutcomeCase::Unchanged,
        "schema-delete-definitive",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_delete_advanced_head_evicts_replays_and_does_not_reuse_sequence() {
    exercise_schema_outcome(
        SchemaOperation::Delete,
        OutcomeCase::Advanced,
        "schema-delete-advanced",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schema_delete_unreadable_progress_evicts_and_replays() {
    exercise_schema_outcome(
        SchemaOperation::Delete,
        OutcomeCase::Unreadable,
        "schema-delete-unreadable",
    )
    .await;
}
