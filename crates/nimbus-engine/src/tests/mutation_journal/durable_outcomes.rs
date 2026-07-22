use super::publisher_test_seams::ArmedOneShotDirectFaultInjector;
use super::support::{mutation_journal_poll_interval, mutation_journal_progress_timeout};
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
            Arc::new(ManualWallClock::new(Timestamp(47_000))),
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

async fn exercise_execution_unit_outcome(case: OutcomeCase, tenant: &str) {
    let point = match case {
        OutcomeCase::Advanced => FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        OutcomeCase::Unchanged | OutcomeCase::Unreadable => {
            FaultPoint::StorageCommitBeforeVisibility
        }
    };
    let data_dir = tempdir().expect("execution-unit outcome tempdir should build");
    let faults = ArmedOneShotDirectFaultInjector::new(point);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(47_100))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(47_101)),
        )
        .expect("execution-unit outcome engine should create"),
    );
    let tenant_id = TenantId::new(tenant).expect("execution-unit tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("execution-unit tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_for_testing(&tenant_id)
        .expect("execution-unit runtime should stay retained for identity proof");
    let runtime_identity_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("execution-unit runtime identity should load");

    let table =
        TableName::new(format!("{tenant}_documents")).expect("execution-unit table should build");
    let unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");
    unit.insert_document(
        table.clone(),
        serde_json::Map::from_iter([("value".to_string(), json!("first"))]),
    )
    .expect("execution-unit insert should stage");
    if matches!(case, OutcomeCase::Unreadable) {
        engine.fail_durable_outcome_progress_for_testing(
            tenant_id.clone(),
            DurableWriteRoute::ExecutionUnit,
        );
    }
    faults.arm();

    let error = unit
        .commit()
        .expect_err("faulted execution-unit commit should return an error");
    match case {
        OutcomeCase::Unchanged => {
            assert!(
                !error.to_string().contains("crash-and-replay"),
                "unchanged durable head should preserve the typed execution-unit error: {error}"
            );
            assert_eq!(
                engine
                    .tenant_runtime_identity_for_testing(&tenant_id)
                    .expect("definitive execution-unit runtime should stay loaded"),
                runtime_identity_before,
                "definitive execution-unit failure must keep the tenant runtime live"
            );
            let (_, pending) = engine
                .write_log_assignment_for_testing(&tenant_id)
                .expect("definitive execution-unit assignment state should load");
            assert!(
                pending.is_empty(),
                "definitive execution-unit failure must discard its staged write-log suffix"
            );
        }
        OutcomeCase::Advanced | OutcomeCase::Unreadable => {
            assert!(
                error.to_string().contains("crash-and-replay"),
                "ambiguous execution-unit outcome should demand crash-and-replay: {error}"
            );
        }
    }

    let documents = engine
        .query_documents_async(
            tenant_id.clone(),
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .await
        .expect("execution-unit outcome should remain queryable after classification");
    assert_eq!(
        documents.len(),
        usize::from(matches!(case, OutcomeCase::Advanced)),
        "replay must expose exactly the execution-unit write that durably landed"
    );

    if matches!(case, OutcomeCase::Unchanged) {
        assert!(
            engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
            "definitive execution-unit failure must retain the original runtime"
        );
    } else {
        assert_ne!(
            engine
                .tenant_runtime_identity_for_testing(&tenant_id)
                .expect("replacement execution-unit runtime identity should load"),
            runtime_identity_before,
            "ambiguous execution-unit outcome must replace the tenant runtime"
        );
        assert!(
            !engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
            "ambiguous execution-unit outcome must deregister the failed runtime"
        );
    }

    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("replacement trigger cursor should not add unrelated records");
    let head_before_follow_up = engine
        .latest_sequence(&tenant_id)
        .expect("execution-unit durable head should load");
    let follow_up = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("follow-up execution unit should begin");
    follow_up
        .insert_document(
            table,
            serde_json::Map::from_iter([("value".to_string(), json!("follow-up"))]),
        )
        .expect("follow-up execution-unit insert should stage");
    follow_up
        .commit()
        .expect("execution-unit route should commit after classification");
    assert_eq!(
        engine
            .latest_sequence(&tenant_id)
            .expect("follow-up execution-unit durable head should load"),
        SequenceNumber(head_before_follow_up.0 + 1),
        "execution-unit route must continue at the next durable sequence"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn execution_unit_unchanged_head_is_definitive_discards_suffix_and_stays_live() {
    exercise_execution_unit_outcome(OutcomeCase::Unchanged, "execution-unit-definitive").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn execution_unit_advanced_head_evicts_replays_and_does_not_reuse_sequence() {
    exercise_execution_unit_outcome(OutcomeCase::Advanced, "execution-unit-advanced").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn execution_unit_unreadable_progress_evicts_and_replays() {
    exercise_execution_unit_outcome(OutcomeCase::Unreadable, "execution-unit-unreadable").await;
}

async fn exercise_trigger_cursor_outcome(case: OutcomeCase, tenant: &str) {
    let point = match case {
        OutcomeCase::Advanced => FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        OutcomeCase::Unchanged | OutcomeCase::Unreadable => {
            FaultPoint::StorageCommitBeforeVisibility
        }
    };
    let data_dir = tempdir().expect("trigger-cursor outcome tempdir should build");
    let faults = ArmedOneShotDirectFaultInjector::new(point);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(47_200))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(47_201)),
        )
        .expect("trigger-cursor outcome engine should create"),
    );
    let tenant_id = TenantId::new(tenant).expect("trigger-cursor tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("trigger-cursor tenant should create");
    engine
        .replace_trigger_registrations_for_testing(&tenant_id, Vec::new())
        .expect("empty trigger registry should be ready");

    let pause = engine
        .trigger_candidate_pause_handle_for_testing(&tenant_id)
        .expect("trigger-cursor pause handle should load");
    pause.arm();
    let table =
        TableName::new(format!("{tenant}_documents")).expect("trigger-cursor table should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!("seed"))]),
        )
        .await
        .expect("trigger-cursor seed insert should commit");
    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "trigger-cursor worker should pause before materialization"
    );

    let runtime_before = engine
        .tenant_runtime_for_testing(&tenant_id)
        .expect("trigger-cursor runtime should stay retained for identity proof");
    let runtime_identity_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("trigger-cursor runtime identity should load");
    if matches!(case, OutcomeCase::Unreadable) {
        engine.fail_durable_outcome_progress_for_testing(
            tenant_id.clone(),
            DurableWriteRoute::TriggerCursor,
        );
    }
    faults.arm();
    pause.release();

    if matches!(case, OutcomeCase::Unchanged) {
        wait_for_value(
            "definitive trigger-cursor failure should requeue and retry",
            mutation_journal_progress_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .trigger_delivery_cursor_for_testing(&tenant_id)
                    .expect("trigger delivery cursor should load")
            },
            |cursor| *cursor == nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(1)),
        )
        .await;
        assert_eq!(
            engine
                .tenant_runtime_identity_for_testing(&tenant_id)
                .expect("definitive trigger-cursor runtime should stay loaded"),
            runtime_identity_before,
            "definitive trigger-cursor failure must keep the tenant runtime live"
        );
        assert!(
            engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
            "definitive trigger-cursor failure must retain the original runtime"
        );
        assert_eq!(
            engine
                .latest_sequence(&tenant_id)
                .expect("retried trigger-cursor durable head should load"),
            SequenceNumber(2),
            "caller cleanup must retry exactly one cursor record after the definitive failure"
        );
        return;
    }

    wait_for_value(
        "ambiguous trigger-cursor failure should replace the runtime",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .get_schema_async(tenant_id.clone())
                .await
                .expect("trigger-cursor replacement schema should load");
            engine
                .tenant_runtime_identity_for_testing(&tenant_id)
                .expect("trigger-cursor replacement runtime identity should load")
        },
        |runtime_identity| *runtime_identity != runtime_identity_before,
    )
    .await;
    assert!(
        !engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
        "ambiguous trigger-cursor outcome must deregister the failed runtime"
    );

    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("replacement trigger-cursor worker should shut down");
    if matches!(case, OutcomeCase::Advanced) {
        assert_eq!(
            engine
                .trigger_delivery_cursor_for_testing(&tenant_id)
                .expect("replayed trigger delivery cursor should load"),
            nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(1)),
            "crash replay must retain the cursor record that durably landed"
        );
        let head_before_follow_up = engine
            .latest_sequence(&tenant_id)
            .expect("trigger-cursor durable head should load");
        engine
            .set_table_schema_async(
                tenant_id.clone(),
                TableSchema {
                    table,
                    fields: Vec::new(),
                    indexes: Vec::new(),
                    access_policy: None,
                },
            )
            .await
            .expect("replacement runtime should commit after trigger-cursor replay");
        assert_eq!(
            engine
                .latest_sequence(&tenant_id)
                .expect("follow-up trigger-cursor durable head should load"),
            SequenceNumber(head_before_follow_up.0 + 1),
            "replacement runtime must continue after the durable head without reusing a sequence"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trigger_cursor_unchanged_head_is_definitive_requeues_and_stays_live() {
    exercise_trigger_cursor_outcome(OutcomeCase::Unchanged, "trigger-cursor-definitive").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trigger_cursor_advanced_head_evicts_replays_and_does_not_reuse_sequence() {
    exercise_trigger_cursor_outcome(OutcomeCase::Advanced, "trigger-cursor-advanced").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn trigger_cursor_unreadable_progress_evicts_and_replays() {
    exercise_trigger_cursor_outcome(OutcomeCase::Unreadable, "trigger-cursor-unreadable").await;
}

async fn exercise_point_in_time_restore_outcome(case: OutcomeCase, tenant: &str) {
    let source_dir = tempdir().expect("restore source tempdir should build");
    let source_engine =
        Arc::new(Engine::new(source_dir.path()).expect("restore source engine should create"));
    let source_tenant =
        TenantId::new(format!("{tenant}-source")).expect("restore source tenant id should build");
    source_engine
        .create_tenant_async(source_tenant.clone())
        .await
        .expect("restore source tenant should create");
    let table = TableName::new(format!("{tenant}_documents")).expect("restore table should build");
    source_engine
        .insert_document_async(
            source_tenant.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!("restored"))]),
        )
        .await
        .expect("restore source document should commit");
    source_engine
        .shutdown_trigger_candidates_for_testing(&source_tenant)
        .expect("restore source trigger worker should shut down");
    let archive = source_engine
        .export_latest_point_in_time_restore_archive(&source_tenant)
        .expect("point-in-time restore archive should export");

    let point = match case {
        OutcomeCase::Advanced => FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        OutcomeCase::Unchanged | OutcomeCase::Unreadable => {
            FaultPoint::StorageCommitBeforeVisibility
        }
    };
    // Restore first commits its empty sequence-zero base snapshot, then appends
    // the archive tail. The advanced-head case must fail the second boundary so
    // the durable journal record, rather than only the empty base, is visible.
    let faults = if matches!(case, OutcomeCase::Advanced) {
        ArmedOneShotDirectFaultInjector::new_on_visit(point, 2)
    } else {
        ArmedOneShotDirectFaultInjector::new(point)
    };
    let destination_dir = tempdir().expect("restore destination tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            destination_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(47_300))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(47_301)),
        )
        .expect("restore destination engine should create"),
    );
    let tenant_id = TenantId::new(tenant).expect("restore destination tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("restore destination tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("restore destination trigger worker should shut down");
    let runtime_before = engine
        .tenant_runtime_for_testing(&tenant_id)
        .expect("restore runtime should stay retained for identity proof");
    let runtime_identity_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("restore runtime identity should load");
    if matches!(case, OutcomeCase::Unreadable) {
        engine.fail_durable_outcome_progress_for_testing(
            tenant_id.clone(),
            DurableWriteRoute::PointInTimeRestore,
        );
    }
    faults.arm();

    let error = engine
        .import_point_in_time_restore_archive(&tenant_id, &archive)
        .expect_err("faulted point-in-time restore should return an error");
    match case {
        OutcomeCase::Unchanged => {
            assert!(
                !error.to_string().contains("crash-and-replay"),
                "unchanged durable head should preserve the typed restore error: {error}"
            );
            assert_eq!(
                engine
                    .tenant_runtime_identity_for_testing(&tenant_id)
                    .expect("definitive restore runtime should stay loaded"),
                runtime_identity_before,
                "definitive restore failure must keep the tenant runtime live"
            );
            assert!(
                engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
                "definitive restore failure must retain the original runtime"
            );
            assert!(
                engine
                    .query_documents_async(
                        tenant_id.clone(),
                        Query {
                            table: table.clone(),
                            filters: Vec::new(),
                            order: None,
                            limit: None,
                        },
                    )
                    .await
                    .expect("definitive restore destination should remain queryable")
                    .is_empty(),
                "definitive restore rollback must leave the destination empty"
            );
            engine
                .import_point_in_time_restore_archive(&tenant_id, &archive)
                .expect("definitive restore failure should be retryable on the live runtime");
            assert_eq!(
                engine
                    .tenant_runtime_identity_for_testing(&tenant_id)
                    .expect("retried restore runtime should stay loaded"),
                runtime_identity_before,
                "definitive restore retry must not replace the tenant runtime"
            );
            let restored = engine
                .query_documents_async(
                    tenant_id.clone(),
                    Query {
                        table,
                        filters: Vec::new(),
                        order: None,
                        limit: None,
                    },
                )
                .await
                .expect("retried restore should be queryable");
            assert_eq!(restored.len(), 1);
            assert_eq!(restored[0].fields.get("value"), Some(&json!("restored")));
            return;
        }
        OutcomeCase::Advanced | OutcomeCase::Unreadable => {
            assert!(
                error.to_string().contains("crash-and-replay"),
                "ambiguous restore outcome should demand crash-and-replay: {error}"
            );
        }
    }

    let restored = engine
        .query_documents_async(
            tenant_id.clone(),
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .await
        .expect("restore outcome should remain queryable after classification");
    assert_eq!(
        restored.len(),
        usize::from(matches!(case, OutcomeCase::Advanced)),
        "crash replay must expose the restore archive only when its durable tail landed"
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement restore runtime identity should load"),
        runtime_identity_before,
        "ambiguous restore outcome must replace the tenant runtime"
    );
    assert!(
        !engine.runtime_is_registered_for_testing(&tenant_id, &runtime_before),
        "ambiguous restore outcome must deregister the failed runtime"
    );

    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("replacement restore trigger worker should shut down");
    if matches!(case, OutcomeCase::Advanced) {
        assert_eq!(restored[0].fields.get("value"), Some(&json!("restored")));
        let head_before_follow_up = engine
            .latest_sequence(&tenant_id)
            .expect("restore durable head should load");
        let follow_up_table = TableName::new(format!("{tenant}_follow_up"))
            .expect("restore follow-up table should build");
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
            .expect("replacement runtime should commit after restore replay");
        assert_eq!(
            engine
                .latest_sequence(&tenant_id)
                .expect("follow-up restore durable head should load"),
            SequenceNumber(head_before_follow_up.0 + 1),
            "replacement runtime must continue after the durable head without reusing a sequence"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn point_in_time_restore_unchanged_head_is_definitive_can_retry_and_stays_live() {
    exercise_point_in_time_restore_outcome(OutcomeCase::Unchanged, "restore-definitive").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn point_in_time_restore_advanced_head_evicts_replays_and_does_not_reuse_sequence() {
    exercise_point_in_time_restore_outcome(OutcomeCase::Advanced, "restore-advanced").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn point_in_time_restore_unreadable_progress_evicts_and_replays() {
    exercise_point_in_time_restore_outcome(OutcomeCase::Unreadable, "restore-unreadable").await;
}
