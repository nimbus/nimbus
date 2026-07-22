use super::*;

#[derive(Clone, Copy)]
pub(crate) struct ProviderPipelineExpectation {
    pub(crate) adapter: &'static str,
    pub(crate) configured_max_in_flight: u64,
    pub(crate) max_observed_in_flight: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ProviderPublisherContractSnapshot {
    durable_sequence_delta: u64,
    document_markers: Vec<String>,
    journal_event_kinds: Vec<&'static str>,
}

struct ArmedPause {
    faults: crate::CommitFaultHandle,
    released: bool,
}

impl ArmedPause {
    fn release(&mut self) {
        if !self.released {
            self.faults
                .release(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
            self.released = true;
        }
    }
}

impl Drop for ArmedPause {
    fn drop(&mut self) {
        self.release();
    }
}

fn contract_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "marker".to_string(),
            field_type: FieldType::String,
            required: true,
        }],
        indexes: Vec::new(),
        access_policy: None,
    }
}

fn marker(value: &'static str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("marker".to_string(), json!(value))])
}

/// Runs the common ordered-publisher behavior through the public Engine
/// interface. Callers supply only a configured engine and the optional
/// provider-I/O diagnostic contract; lifecycle, route, error, retry, and
/// cancellation assertions stay identical across every persistence adapter.
pub(crate) async fn exercise_provider_publisher_contract(
    engine: Arc<Engine>,
    tenant_id: TenantId,
    pipeline: Option<ProviderPipelineExpectation>,
) -> ProviderPublisherContractSnapshot {
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("contract tenant should be admitted through the async lifecycle");
    engine
        .disable_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should remain suppressed for the parity contract");
    assert_eq!(
        engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("committer-arm diagnostics should load")
            .committer_arm,
        crate::tenant::CommitterArm::OrderedPublisher,
        "every production persistence adapter must install the ordered publisher"
    );
    let initial_sequence = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("initial sequence should load");

    // Schema persistence is a zero-document-write ordered opaque job.
    engine
        .set_table_schema_async(tenant_id.clone(), contract_schema())
        .await
        .expect("schema route should commit");

    engine
        .insert_document_async(tenant_id.clone(), tasks_table(), marker("queued"))
        .await
        .expect("queued mutation path should commit");
    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || engine.insert_document(&tenant_id, tasks_table(), marker("direct"))
    })
    .await
    .expect("direct mutation task should join")
    .expect("direct mutation path should commit");
    let unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");
    unit.insert_document(tasks_table(), marker("execution-unit"))
        .expect("execution-unit write should stage");
    tokio::task::spawn_blocking(move || unit.commit())
        .await
        .expect("execution-unit task should join")
        .expect("execution-unit path should commit")
        .expect("execution-unit path should append a record");

    let scheduled_job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 5_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("scheduled"),
                },
            },
        )
        .await
        .expect("scheduled mutation state should persist");
    let claimed = engine
        .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
        .await
        .expect("scheduled job should claim");
    assert_eq!(claimed.len(), 1);
    engine
        .record_scheduled_job_result_async(
            tenant_id.clone(),
            nimbus_core::ScheduledJobResult {
                id: scheduled_job_id.clone(),
                run_at: claimed[0].run_at,
                finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                mutation: claimed[0].mutation.clone(),
                outcome: nimbus_core::ScheduledJobOutcome::Completed,
                error: None,
            },
        )
        .await
        .expect("scheduled result should persist");
    engine
        .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id)
        .await
        .expect("scheduled job completion should persist");

    // A provider may durably commit a scheduler transition and lose only its
    // acknowledgement. The storage-owned pre/post-state proof must return the
    // original result instead of surfacing an error that invites a duplicate
    // insert or strands a claimed job.
    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::SCHEDULER_DURABLE_BEFORE_ACK,
            1,
            Error::Internal("injected scheduler insert acknowledgement loss".to_string()),
        );
    let ack_lost_job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 0,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("scheduler-ack-loss"),
                },
            },
        )
        .await
        .expect("committed scheduler insert must survive acknowledgement loss");
    assert_eq!(
        engine
            .list_scheduled_jobs_async(tenant_id.clone())
            .await
            .expect("acknowledged scheduler state should read")
            .iter()
            .filter(|job| job.id == ack_lost_job_id)
            .count(),
        1,
        "acknowledgement recovery must not duplicate the scheduled job"
    );
    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::SCHEDULER_DURABLE_BEFORE_ACK,
            1,
            Error::Internal("injected scheduler claim acknowledgement loss".to_string()),
        );
    let ack_lost_claim = engine
        .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
        .await
        .expect("committed scheduler claim must return its original jobs after ack loss");
    assert_eq!(
        ack_lost_claim
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![ack_lost_job_id.clone()]
    );
    engine
        .complete_scheduled_job_async(tenant_id.clone(), ack_lost_job_id)
        .await
        .expect("ack-loss test job should complete");

    // Cancellation must reach the one-shot recovery transaction, not only the
    // requested write that follows it. A cancelled recovery stays armed and
    // leaves the orphaned running job untouched for the next authority-owned
    // scheduler operation.
    let recovery_job_id = engine
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 0,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("scheduler-recovery-cancel"),
                },
            },
        )
        .await
        .expect("recovery cancellation job should schedule");
    assert_eq!(
        engine
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("recovery cancellation job should enter running state")
            .len(),
        1
    );
    engine
        .arm_scheduler_recovery_for_testing(&tenant_id)
        .expect("scheduler recovery should arm for the cancellation test");
    let cancellation_checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cancellation_checks_for_write = cancellation_checks.clone();
    let error = engine
        .schedule_mutation_async_cancellable(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 1_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("must-not-land-after-recovery-cancel"),
                },
            },
            std::future::pending(),
            move || {
                if cancellation_checks_for_write.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    == 0
                {
                    Ok(())
                } else {
                    Err(Error::Cancelled)
                }
            },
        )
        .await
        .expect_err("cancellation during recovery must reach the caller");
    assert!(matches!(error, Error::Cancelled));
    assert!(
        engine
            .list_scheduled_jobs_async(tenant_id.clone())
            .await
            .expect("cancelled recovery state should remain readable")
            .is_empty(),
        "cancelled recovery must not move the running job back to pending"
    );
    let recovered_claim = engine
        .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
        .await
        .expect("the next scheduler write should retry recovery and claim the orphan");
    assert_eq!(
        recovered_claim
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![recovery_job_id.clone()]
    );
    engine
        .complete_scheduled_job_async(tenant_id.clone(), recovery_job_id)
        .await
        .expect("recovered cancellation job should complete");

    // A failed assignment must discard its staged suffix. Retrying the exact
    // document identity then commits once, proving the public replay boundary
    // does not leak a phantom sequence or duplicate document.
    let retry_id = DocumentId::new();
    let before_failed_assignment = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("pre-error sequence should load");
    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
            Error::InvalidInput("injected provider contract assignment error".to_string()),
        );
    let error = engine
        .insert_document_async_with_id(
            tenant_id.clone(),
            tasks_table(),
            retry_id.clone(),
            marker("retried"),
        )
        .await
        .expect_err("injected assignment error should reach the caller");
    assert!(
        matches!(error, Error::InvalidInput(ref message) if message == "injected provider contract assignment error")
    );
    assert_eq!(
        engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("post-error sequence should load"),
        before_failed_assignment,
        "a pre-persistence assignment error must not advance durable state"
    );
    engine
        .insert_document_async_with_id(
            tenant_id.clone(),
            tasks_table(),
            retry_id.clone(),
            marker("retried"),
        )
        .await
        .expect("exact retry should commit once");
    assert_eq!(
        engine
            .get_document_async(tenant_id.clone(), tasks_table(), retry_id)
            .await
            .expect("retried document should exist")
            .fields
            .get("marker"),
        Some(&json!("retried"))
    );

    // Hold the production publisher after durable/apply publication. A queued
    // follower cancelled while waiting must resolve as Cancelled and append no
    // durable record after the held predecessor drains.
    let faults = engine.commit_fault_handle_for_testing();
    let pause_label = crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT;
    faults.arm(pause_label);
    let mut release_on_unwind = ArmedPause {
        faults: faults.clone(),
        released: false,
    };
    let blocker = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(tenant_id, tasks_table(), marker("blocker"))
                .await
        }
    });
    let wait_faults = faults.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_faults.wait_until_entered(pause_label, Duration::from_secs(5))
        })
        .await
        .expect("publisher pause waiter should join"),
        "publisher should reach the post-publication pause"
    );
    let before_cancelled_follower = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("held predecessor sequence should be durable");
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let cancelled = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    marker("cancelled"),
                    crate::AsyncMutationContext::anonymous(
                        async move { cancel_for_wait.notified().await },
                        || Ok(()),
                    ),
                )
                .await
        }
    });
    let scheduler_cancel = Arc::new(Notify::new());
    let scheduler_cancel_for_wait = scheduler_cancel.clone();
    let cancelled_scheduler_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .schedule_mutation_async_cancellable(
                    tenant_id,
                    nimbus_core::ScheduleRequest {
                        run_after_ms: 10_000,
                        mutation: nimbus_core::Mutation::Insert {
                            table: tasks_table(),
                            id: None,
                            fields: marker("cancelled-scheduler"),
                        },
                    },
                    async move { scheduler_cancel_for_wait.notified().await },
                    || Ok(()),
                )
                .await
        }
    });
    cancel.notify_one();
    scheduler_cancel.notify_one();
    timeout(
        Duration::from_secs(5),
        engine.wait_for_queued_mutation_cancellation_observed_for_testing(&tenant_id),
    )
    .await
    .expect("cancelled follower should be observed")
    .expect("cancellation observation should succeed");
    release_on_unwind.release();
    timeout(Duration::from_secs(5), blocker)
        .await
        .expect("held predecessor should finish")
        .expect("held predecessor task should join")
        .expect("held predecessor should commit");
    let cancellation_error = timeout(Duration::from_secs(5), cancelled)
        .await
        .expect("cancelled follower should resolve")
        .expect("cancelled follower task should join")
        .expect_err("cancelled follower must not commit");
    assert!(matches!(cancellation_error, Error::Cancelled));
    let scheduler_cancellation_error = timeout(Duration::from_secs(5), cancelled_scheduler_write)
        .await
        .expect("cancelled scheduler write should resolve")
        .expect("cancelled scheduler task should join")
        .expect_err("cancelled scheduler write must not persist");
    assert!(matches!(scheduler_cancellation_error, Error::Cancelled));
    assert!(
        engine
            .list_scheduled_jobs_async(tenant_id.clone())
            .await
            .expect("scheduled state should remain readable after cancellation")
            .is_empty(),
        "cancellation before the scheduler transaction must leave no pending job"
    );
    assert_eq!(
        engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("post-cancellation sequence should load"),
        before_cancelled_follower,
        "the cancelled follower must not append after its predecessor"
    );

    let diagnostic = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("provider diagnostics should load")
        .provider_write_pipeline;
    match (diagnostic, pipeline) {
        (None, None) => {}
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.adapter, expected.adapter);
            assert_eq!(
                actual.configured_max_in_flight,
                expected.configured_max_in_flight
            );
            assert_eq!(
                actual.journal_statement_count, actual.batch_attempt_count,
                "each provider journal batch should use one journal statement"
            );
            assert!(actual.journal_record_count >= actual.journal_statement_count);
            assert_eq!(
                actual.max_observed_in_flight,
                expected.max_observed_in_flight
            );
        }
        (actual, expected) => panic!(
            "pipeline diagnostic presence differed: actual={}, expected={}",
            actual.is_some(),
            expected.is_some()
        ),
    }

    let mut documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("contract documents should query");
    documents.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let mut document_markers = documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("marker")
                .and_then(serde_json::Value::as_str)
                .expect("contract document should retain its marker")
                .to_string()
        })
        .collect::<Vec<_>>();
    document_markers.sort();
    assert!(!document_markers.iter().any(|marker| marker == "cancelled"));

    let records = engine
        .read_durable_journal_async(tenant_id.clone(), initial_sequence)
        .await
        .expect("contract journal should read");
    let journal_event_kinds = records
        .iter()
        .flat_map(|record| record.events.iter())
        .map(|event| match event {
            nimbus_core::TenantEventKind::DocumentWrite { .. } => "document_write",
            nimbus_core::TenantEventKind::SchemaChange { .. } => "schema_change",
            nimbus_core::TenantEventKind::TableLifecycle { .. } => "table_lifecycle",
            nimbus_core::TenantEventKind::IndexLifecycle { .. } => "index_lifecycle",
            nimbus_core::TenantEventKind::ScheduledExecution { .. } => "scheduled_execution",
            nimbus_core::TenantEventKind::TriggerDelivery { .. } => "trigger_delivery",
            nimbus_core::TenantEventKind::Barrier { .. } => "barrier",
        })
        .collect::<Vec<_>>();
    assert!(
        !journal_event_kinds.contains(&"trigger_delivery"),
        "the parity contract must not admit timing-dependent trigger cursor writes"
    );
    let final_sequence = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("final sequence should load");
    let snapshot = ProviderPublisherContractSnapshot {
        durable_sequence_delta: final_sequence.0.saturating_sub(initial_sequence.0),
        document_markers,
        journal_event_kinds,
    };
    engine.quiesce().await;
    snapshot
}

/// Proves that scheduler-only state changes participate in provider lease
/// fencing even though they do not advance the durable journal sequence.
pub(crate) async fn exercise_provider_scheduler_fence_contract<Expire, ExpireFuture>(
    engine_a: Arc<Engine>,
    engine_b: Arc<Engine>,
    tenant_id: TenantId,
    expire_lease: Expire,
) where
    Expire: FnOnce() -> ExpireFuture,
    ExpireFuture: Future<Output = ()>,
{
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("first scheduler writer should create the provider tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("second scheduler writer should load the provider tenant");

    let first_id = engine_a
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("first-holder"),
                },
            },
        )
        .await
        .expect("first scheduler write should acquire the provider lease");
    assert_eq!(
        engine_a
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("scheduler-only durable head should read"),
        SequenceNumber(0),
        "scheduler state must not manufacture a journal record"
    );

    expire_lease().await;
    let successor_id = engine_b
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("successor-holder"),
                },
            },
        )
        .await
        .expect("successor scheduler writer should take over the expired lease");

    let error = engine_a
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: marker("must-not-persist"),
                },
            },
        )
        .await
        .expect_err("stale scheduler writer must be fenced");
    assert!(
        matches!(error, Error::CommitterFenced { epoch: 1, .. }),
        "scheduler fence must retain the stale owner epoch: {error}"
    );
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);

    let mut persisted_ids = engine_b
        .list_scheduled_jobs_async(tenant_id.clone())
        .await
        .expect("successor should read provider scheduler state")
        .into_iter()
        .map(|job| job.id)
        .collect::<Vec<_>>();
    persisted_ids.sort();
    let mut expected_ids = vec![first_id, successor_id];
    expected_ids.sort();
    assert_eq!(
        persisted_ids, expected_ids,
        "the stale scheduler transaction must roll back without partial state"
    );
    assert_eq!(
        engine_b
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("post-takeover durable head should read"),
        SequenceNumber(0),
        "lease validation for scheduler state must not advance the journal"
    );

    engine_a.quiesce().await;
    engine_b.quiesce().await;
}
