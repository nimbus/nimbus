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

struct ArmedJournalPause {
    pause: crate::tenant::MutationJournalPauseHandle,
    released: bool,
}

impl ArmedJournalPause {
    fn release(&mut self) {
        if !self.released {
            self.pause.release();
            self.released = true;
        }
    }
}

impl Drop for ArmedJournalPause {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(feature = "postgres")]
struct ArmedBlockingFaultPause {
    pause: Arc<BlockingFaultInjector>,
    released: bool,
}

#[cfg(feature = "postgres")]
impl ArmedBlockingFaultPause {
    fn new(pause: Arc<BlockingFaultInjector>) -> Self {
        Self {
            pause,
            released: false,
        }
    }

    fn release(&mut self) {
        if !self.released {
            self.pause.release();
            self.released = true;
        }
    }
}

#[cfg(feature = "postgres")]
impl Drop for ArmedBlockingFaultPause {
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

    // Hold the production journal before assignment. A queued follower
    // cancelled while waiting must resolve as Cancelled and append no durable
    // record. Sequence assignment is the cancellation boundary, so pausing a
    // previously assigned publisher batch would not prove this contract.
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("mutation journal pause should load");
    pause.arm();
    let mut release_on_unwind = ArmedJournalPause {
        pause: pause.clone(),
        released: false,
    };
    let before_cancelled_follower = engine
        .latest_sequence_async(tenant_id.clone())
        .await
        .expect("pre-cancellation sequence should load");
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
    let wait_pause = pause.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_pause.wait_until_entered(Duration::from_secs(5))
        })
        .await
        .expect("journal pause waiter should join"),
        "queued follower should reach the pre-assignment pause"
    );
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
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
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

/// Runs the schedule-only `MutationExecutionUnit` lease contract through the
/// same public Engine interface for every external provider adapter.
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) async fn exercise_provider_schedule_only_execution_unit_fence_contract(
    engine_a: Arc<Engine>,
    engine_b: Arc<Engine>,
    tenant_id: TenantId,
    lease_time: Arc<dyn nimbus_storage::provider_test_fixtures::ProviderLeaseTimeControl>,
) {
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("first execution-unit writer should create the provider tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("second execution-unit writer should load the provider tenant");

    let healthy_unit = engine_a
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("healthy schedule-only execution unit should begin");
    let first_id = healthy_unit
        .schedule_mutation_at(
            nimbus_core::Mutation::Insert {
                table: tasks_table(),
                id: None,
                fields: marker("first-unit-job"),
            },
            60_000,
        )
        .expect("first healthy schedule operation should stage");
    let second_id = healthy_unit
        .schedule_mutation_at(
            nimbus_core::Mutation::Insert {
                table: tasks_table(),
                id: None,
                fields: marker("second-unit-job"),
            },
            60_000,
        )
        .expect("second healthy schedule operation should stage");
    assert_eq!(
        healthy_unit
            .commit()
            .expect("healthy schedule-only unit should commit atomically"),
        None
    );
    assert_eq!(
        engine_a
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("schedule-only durable head should read"),
        SequenceNumber(0),
        "schedule-only execution units must not manufacture journal records"
    );

    let missing_job_id = DocumentId::new();
    let rejected_cancel_unit = engine_a
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("missing-job cancellation execution unit should begin");
    rejected_cancel_unit
        .cancel_scheduled_job(missing_job_id.clone())
        .expect("missing-job cancellation should stage without an eager provider read");
    assert!(matches!(
        rejected_cancel_unit
            .commit()
            .expect_err("a rolled-back missing-job cancellation must not reconcile as committed"),
        Error::ScheduledJobNotFound(job_id) if job_id == missing_job_id
    ));

    lease_time
        .expire_lease(&tenant_id)
        .await
        .expect("old execution-unit holder lease should expire");
    engine_b
        .acquire_committer_lease_for_testing(&tenant_id)
        .expect("successor should acquire the provider lease");

    let stale_unit = engine_a
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("stale schedule-only execution unit should begin");
    let rejected_id = stale_unit
        .schedule_mutation_at(
            nimbus_core::Mutation::Insert {
                table: tasks_table(),
                id: None,
                fields: marker("must-not-persist"),
            },
            60_000,
        )
        .expect("stale insert should stage");
    stale_unit
        .cancel_scheduled_job(first_id.clone())
        .expect("stale cancellation should stage in the same batch");
    let error = stale_unit
        .commit()
        .expect_err("stale schedule-only execution unit must be fenced");
    assert!(
        matches!(error, Error::CommitterFenced { epoch: 1, .. }),
        "execution-unit fence must retain the stale owner epoch: {error}"
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
    let mut expected_ids = vec![first_id, second_id];
    expected_ids.sort();
    assert_eq!(
        persisted_ids, expected_ids,
        "the stale multi-op batch must neither insert nor cancel a job"
    );
    assert!(!persisted_ids.contains(&rejected_id));
    assert_eq!(
        engine_b
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("post-takeover durable head should read"),
        SequenceNumber(0)
    );

    engine_a.quiesce().await;
    engine_b.quiesce().await;
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
fn pending_provider_trigger_invocation(
    registration_id: &str,
    event_id: &str,
    sequence: SequenceNumber,
    timestamp: Timestamp,
) -> nimbus_core::TriggerInvocationRecord {
    let document_path = nimbus_core::DocumentPath::from_segments(["tasks", "trigger-fence"])
        .expect("document path should build");
    nimbus_core::TriggerInvocationRecord::pending(
        nimbus_core::TriggerInvocationKey::new(registration_id, event_id)
            .expect("trigger invocation key should build"),
        sequence,
        nimbus_core::TriggerEvent::new(
            nimbus_core::TriggerCloudEvent::new(
                event_id,
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                nimbus_core::FirestoreCloudEventType::Written,
                timestamp,
                "documents/tasks/trigger-fence",
            ),
            nimbus_core::FirestoreTriggerMetadata::new(
                "demo",
                "(default)",
                document_path,
                Default::default(),
            ),
            nimbus_core::DocumentEventData::new(None, None, None),
            nimbus_core::TriggerCommitMetadata::new(sequence, timestamp),
            nimbus_core::TriggerExecutionPrincipal::service_account(PrincipalContext::anonymous()),
        ),
    )
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
struct BlockingProviderTriggerExecutor {
    started: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    calls: Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
impl crate::TriggerInvocationExecutor for BlockingProviderTriggerExecutor {
    fn execute_invocation(
        &self,
        _tenant_id: &TenantId,
        _record: &nimbus_core::TriggerInvocationRecord,
    ) -> crate::TriggerInvocationExecution {
        self.calls.fetch_add(1, Ordering::AcqRel);
        if let Some(started) = self
            .started
            .lock()
            .expect("blocking trigger started lock should not be poisoned")
            .take()
        {
            started
                .send(())
                .expect("blocking trigger start should remain observed");
        }
        self.release
            .lock()
            .expect("blocking trigger release lock should not be poisoned")
            .recv_timeout(Duration::from_secs(5))
            .expect("blocking trigger must be released before the test deadline");
        crate::TriggerInvocationExecution::retryable("stale holder retryable outcome")
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
struct CompletingProviderTriggerExecutor {
    calls: Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
impl crate::TriggerInvocationExecutor for CompletingProviderTriggerExecutor {
    fn execute_invocation(
        &self,
        _tenant_id: &TenantId,
        _record: &nimbus_core::TriggerInvocationRecord,
    ) -> crate::TriggerInvocationExecution {
        self.calls.fetch_add(1, Ordering::AcqRel);
        crate::TriggerInvocationExecution::completed()
    }
}

#[cfg(feature = "postgres")]
struct BlockingCompletingProviderTriggerExecutor {
    started: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    calls: Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(feature = "postgres")]
impl crate::TriggerInvocationExecutor for BlockingCompletingProviderTriggerExecutor {
    fn execute_invocation(
        &self,
        _tenant_id: &TenantId,
        _record: &nimbus_core::TriggerInvocationRecord,
    ) -> crate::TriggerInvocationExecution {
        self.calls.fetch_add(1, Ordering::AcqRel);
        if let Some(started) = self
            .started
            .lock()
            .expect("blocking completing trigger started lock should not be poisoned")
            .take()
        {
            started
                .send(())
                .expect("blocking completing trigger start should remain observed");
        }
        self.release
            .lock()
            .expect("blocking completing trigger release lock should not be poisoned")
            .recv_timeout(Duration::from_secs(5))
            .expect("blocking completing trigger must be released before the test deadline");
        crate::TriggerInvocationExecution::completed()
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
async fn wait_for_provider_trigger_state(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    key: &nimbus_core::TriggerInvocationKey,
    description: &str,
    predicate: impl Fn(&nimbus_core::TriggerInvocationState) -> bool,
) -> nimbus_core::TriggerInvocationRecord {
    wait_for_value(
        description,
        Duration::from_secs(5),
        Duration::from_millis(10),
        || async {
            engine
                .list_trigger_invocations_for_testing(tenant_id)
                .expect("provider trigger invocations should remain readable")
                .into_iter()
                .find(|record| &record.key == key)
        },
        |record| {
            record
                .as_ref()
                .is_some_and(|record| predicate(&record.state))
        },
    )
    .await
    .expect("provider trigger invocation should remain present")
}

/// Runs the provider trigger-lifecycle takeover contract through the
/// production worker and `TriggerInvocationExecutor` seam.
///
/// Both handlers may run across takeover (the documented at-least-once
/// boundary), but only the current lease holder may durably transition the
/// invocation record.
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) async fn exercise_provider_trigger_invocation_fence_contract(
    engine_a: Arc<Engine>,
    engine_b: Arc<Engine>,
    tenant_id: TenantId,
    lease_time: Arc<dyn nimbus_storage::provider_test_fixtures::ProviderLeaseTimeControl>,
) {
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("old trigger owner should create the provider tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("successor trigger owner should load the provider tenant");
    engine_a
        .acquire_committer_lease_for_testing(&tenant_id)
        .expect("old trigger owner should acquire the first provider lease");

    let record = pending_provider_trigger_invocation(
        "trigger-fence",
        "trigger-fence-event",
        SequenceNumber(0),
        Timestamp(45_000),
    );
    let key = record.key.clone();
    engine_a
        .save_trigger_invocation_for_testing(&tenant_id, &record)
        .expect("test setup should seed one pending provider invocation");

    let old_calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let successor_calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    engine_a
        .install_trigger_invocation_executor(Arc::new(BlockingProviderTriggerExecutor {
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(release_rx),
            calls: old_calls.clone(),
        }))
        .expect("old holder trigger executor should install");
    tokio::task::spawn_blocking(move || {
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("old holder trigger handler should start")
    })
    .await
    .expect("trigger-start observation task should join");
    wait_for_provider_trigger_state(
        &engine_b,
        &tenant_id,
        &key,
        "old holder should durably claim the trigger before takeover",
        |state| {
            matches!(
                state,
                nimbus_core::TriggerInvocationState::Running { attempt: 1, .. }
            )
        },
    )
    .await;
    let stale_runtime = engine_a
        .registered_runtime_for_testing(&tenant_id)
        .expect("old trigger runtime should remain registered while its handler is blocked");

    lease_time
        .expire_lease(&tenant_id)
        .await
        .expect("old trigger holder lease should expire");
    engine_b
        .acquire_committer_lease_for_testing(&tenant_id)
        .expect("successor should acquire the provider lease");
    engine_b
        .install_trigger_invocation_executor(Arc::new(CompletingProviderTriggerExecutor {
            calls: successor_calls.clone(),
        }))
        .expect("successor trigger executor should install");
    wait_for_provider_trigger_state(
        &engine_b,
        &tenant_id,
        &key,
        "successor should durably complete the recovered running invocation",
        |state| {
            matches!(
                state,
                nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
            )
        },
    )
    .await;

    release_tx
        .send(())
        .expect("old holder trigger handler should be released");
    wait_for_value(
        "old trigger owner should observe its definitive provider fence",
        Duration::from_secs(5),
        Duration::from_millis(10),
        || async {
            stale_runtime
                .mutation_journal_stats()
                .committer_lease_fenced
        },
        |fenced| *fenced,
    )
    .await;
    let final_record = wait_for_provider_trigger_state(
        &engine_b,
        &tenant_id,
        &key,
        "stale trigger outcome must not regress successor state",
        |state| {
            matches!(
                state,
                nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
            )
        },
    )
    .await;
    assert!(matches!(
        final_record.state,
        nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
    ));
    assert_eq!(old_calls.load(Ordering::Acquire), 1);
    assert_eq!(successor_calls.load(Ordering::Acquire), 1);

    engine_a.quiesce().await;
    engine_b.quiesce().await;
}

/// Proves that a trigger lifecycle transition and a journal write share one
/// per-tenant sequence authority. The trigger transition pauses after
/// observing the durable head; the journal write must remain queued until the
/// transition finishes, then advance the head without falsely fencing the
/// still-current trigger worker.
#[cfg(feature = "postgres")]
pub(crate) async fn exercise_provider_trigger_transition_serialization_contract(
    engine: Arc<Engine>,
    tenant_id: TenantId,
    transition_pause: Arc<BlockingFaultInjector>,
) {
    let mut transition_pause = ArmedBlockingFaultPause::new(transition_pause);
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("trigger serialization tenant should create");
    let record = pending_provider_trigger_invocation(
        "trigger-serialization",
        "trigger-serialization-event",
        SequenceNumber(0),
        Timestamp(46_000),
    );
    let key = record.key.clone();
    engine
        .save_trigger_invocation_for_testing(&tenant_id, &record)
        .expect("test setup should seed one pending provider invocation");
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    engine
        .install_trigger_invocation_executor(Arc::new(CompletingProviderTriggerExecutor {
            calls: calls.clone(),
        }))
        .expect("trigger serialization executor should install");

    timeout(
        Duration::from_secs(5),
        transition_pause.pause.wait_until_entered(),
    )
    .await
    .expect("trigger transition should pause after observing the durable head");

    let journal_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(tenant_id, tasks_table(), marker("serialized-after-trigger"))
                .await
        }
    });
    let queued = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "journal work should enter the committer inbox behind the paused trigger transition",
        |stats| stats.committer_inbox_depth == 1 && stats.pending_response_count == 1,
    )
    .await;
    assert_eq!(queued.committer_inbox_depth, 1);
    assert_eq!(queued.pending_response_count, 1);
    assert!(
        !journal_write.is_finished(),
        "journal response must remain pending while its observed actor message is queued behind \
         the trigger transition"
    );
    transition_pause.release();
    journal_write
        .await
        .expect("serialized journal task should join")
        .expect("journal write should commit after the trigger transition");
    let final_record = wait_for_provider_trigger_state(
        &engine,
        &tenant_id,
        &key,
        "serialized trigger transition should complete without a false fence",
        |state| {
            matches!(
                state,
                nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
            )
        },
    )
    .await;
    assert!(matches!(
        final_record.state,
        nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(
        engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("serialized journal head should read"),
        SequenceNumber(1)
    );
    assert!(
        !engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("serialized runtime diagnostics should read")
            .committer_lease_fenced,
        "a same-owner journal advance must not be misclassified as lease loss"
    );

    engine.quiesce().await;
}

/// Proves that losing the provider acknowledgement for a completed trigger
/// transition retries the exact record without invoking the handler again.
#[cfg(feature = "postgres")]
pub(crate) async fn exercise_provider_trigger_outcome_acknowledgement_loss_contract(
    engine: Arc<Engine>,
    tenant_id: TenantId,
    arm_acknowledgement_loss: impl FnOnce(),
    acknowledgement_loss_fired: impl Fn() -> bool,
) {
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("trigger acknowledgement-loss tenant should create");
    let record = pending_provider_trigger_invocation(
        "trigger-ack-loss",
        "trigger-ack-loss-event",
        SequenceNumber(0),
        Timestamp(47_000),
    );
    let key = record.key.clone();
    engine
        .save_trigger_invocation_for_testing(&tenant_id, &record)
        .expect("test setup should seed one pending provider invocation");

    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    engine
        .install_trigger_invocation_executor(Arc::new(BlockingCompletingProviderTriggerExecutor {
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(release_rx),
            calls: calls.clone(),
        }))
        .expect("trigger acknowledgement-loss executor should install");
    tokio::task::spawn_blocking(move || {
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("trigger handler should start after Running is durable")
    })
    .await
    .expect("trigger-start observation task should join");

    arm_acknowledgement_loss();
    release_tx
        .send(())
        .expect("trigger handler should be released after the fault is armed");
    let final_record = wait_for_provider_trigger_state(
        &engine,
        &tenant_id,
        &key,
        "acknowledgement-loss retry should preserve the computed completed outcome",
        |state| {
            matches!(
                state,
                nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
            )
        },
    )
    .await;
    assert!(matches!(
        final_record.state,
        nimbus_core::TriggerInvocationState::Completed { attempt: 1, .. }
    ));
    assert!(
        acknowledgement_loss_fired(),
        "the post-visibility acknowledgement-loss fault must fire exactly once"
    );
    assert_eq!(
        calls.load(Ordering::Acquire),
        1,
        "retrying the idempotent completed record must not re-run the trigger handler"
    );
    assert_eq!(
        engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("trigger acknowledgement-loss journal head should read"),
        SequenceNumber(0),
        "trigger lifecycle transitions must not advance the mutation journal"
    );

    engine.quiesce().await;
}
