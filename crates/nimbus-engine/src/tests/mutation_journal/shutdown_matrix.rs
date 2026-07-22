use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_future_within,
};
use super::*;

struct ArmedFault {
    faults: crate::CommitFaultHandle,
    point: ShutdownPoint,
    released: bool,
}

#[derive(Clone, Copy)]
enum ShutdownPoint {
    AfterAssignment,
    DurableBeforePublish,
    PostPublishPreFanout,
}

impl ArmedFault {
    fn new(engine: &Engine, point: ShutdownPoint) -> Self {
        let faults = engine.commit_fault_handle_for_testing();
        match point {
            ShutdownPoint::AfterAssignment => {
                faults.arm(crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE);
            }
            ShutdownPoint::DurableBeforePublish => {
                faults.arm(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH);
            }
            ShutdownPoint::PostPublishPreFanout => {
                faults.arm(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
            }
        }
        Self {
            faults,
            point,
            released: false,
        }
    }

    async fn wait_until_entered(&self, message: &'static str) {
        expect_blocking_wait_reaches_state(message, {
            let faults = self.faults.clone();
            let point = self.point;
            move |timeout| match point {
                ShutdownPoint::AfterAssignment => faults.wait_until_entered(
                    crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
                    timeout,
                ),
                ShutdownPoint::DurableBeforePublish => faults.wait_until_entered(
                    crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH,
                    timeout,
                ),
                ShutdownPoint::PostPublishPreFanout => faults.wait_until_entered(
                    crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT,
                    timeout,
                ),
            }
        })
        .await;
    }

    fn release(&mut self) {
        if !self.released {
            match self.point {
                ShutdownPoint::AfterAssignment => self
                    .faults
                    .release(crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE),
                ShutdownPoint::DurableBeforePublish => self
                    .faults
                    .release(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH),
                ShutdownPoint::PostPublishPreFanout => self
                    .faults
                    .release(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT),
            }
            self.released = true;
        }
    }
}

impl Drop for ArmedFault {
    fn drop(&mut self) {
        self.release();
    }
}

fn shutdown_fixture(name: &str) -> (EngineFixture<Engine>, Arc<Engine>, TenantId) {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant(name, Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated shutdown work");
    (fixture, engine, tenant_id)
}

fn shutdown_insert(
    engine: Arc<Engine>,
    tenant_id: TenantId,
    marker: &'static str,
) -> tokio::task::JoinHandle<nimbus_core::Result<DocumentId>> {
    tokio::spawn(async move {
        engine
            .insert_document_async(
                tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("marker".to_string(), json!(marker))]),
            )
            .await
    })
}

async fn shutdown_after_assignment_rolls_back_unaccepted_suffix() {
    let (_fixture, engine, tenant_id) = shutdown_fixture("shutdown-after-assignment");
    let mut pause = ArmedFault::new(engine.as_ref(), ShutdownPoint::AfterAssignment);
    let write = shutdown_insert(engine.clone(), tenant_id.clone(), "assigned");
    pause
        .wait_until_entered("shutdown case should pause after assignment")
        .await;
    let assigned = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("assigned shutdown diagnostics should load");
    assert_eq!(assigned.active_assigned_head, SequenceNumber(1));
    assert_eq!(assigned.durable_head, SequenceNumber(0));

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "quiesce must wait for the in-progress assignment to settle",
    )
    .await;
    pause.release();

    let error = expect_future_within(write, "assigned shutdown write should resolve")
        .await
        .expect("assigned shutdown write task should join")
        .expect_err("a batch not yet accepted by the closed publisher must not commit");
    assert!(
        matches!(error, Error::Internal(ref message) if message.contains("publisher")),
        "shutdown rejection should retain its publisher ownership boundary: {error}"
    );
    expect_future_within(quiesce, "assignment rollback should let quiesce finish")
        .await
        .expect("assignment quiesce task should join");
    let settled = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("settled shutdown diagnostics should load");
    assert_eq!(settled.active_assigned_head, SequenceNumber(0));
    assert_eq!(settled.assigned_high_water, SequenceNumber(1));
    assert_eq!(settled.durable_head, SequenceNumber(0));
}

async fn shutdown_during_persistence_drains_durable_batch() {
    let (_fixture, engine, tenant_id) = shutdown_fixture("shutdown-during-persistence");
    let mut pause = ArmedFault::new(engine.as_ref(), ShutdownPoint::DurableBeforePublish);
    let write = shutdown_insert(engine.clone(), tenant_id.clone(), "durable");
    pause
        .wait_until_entered("shutdown case should pause after durable append")
        .await;
    let durable = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("durable shutdown diagnostics should load");
    assert_eq!(durable.durable_head, SequenceNumber(1));
    assert_eq!(durable.applied_head, SequenceNumber(0));

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "quiesce must wait for durable work to finish publication",
    )
    .await;
    pause.release();
    expect_future_within(write, "durable shutdown write should drain")
        .await
        .expect("durable shutdown write task should join")
        .expect("a publisher-accepted durable batch must finish");
    expect_future_within(quiesce, "durable shutdown quiesce should finish")
        .await
        .expect("durable shutdown quiesce task should join");
    let settled = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("settled durable diagnostics should load");
    assert_eq!(settled.durable_head, SequenceNumber(1));
    assert_eq!(settled.applied_head, SequenceNumber(1));
}

async fn shutdown_after_publication_drains_response_and_fanout() {
    let (_fixture, engine, tenant_id) = shutdown_fixture("shutdown-after-publication");
    let mut pause = ArmedFault::new(engine.as_ref(), ShutdownPoint::PostPublishPreFanout);
    let write = shutdown_insert(engine.clone(), tenant_id.clone(), "published");
    pause
        .wait_until_entered("shutdown case should pause before fan-out")
        .await;
    let published = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("published shutdown diagnostics should load");
    assert_eq!(published.applied_head, SequenceNumber(1));
    assert!(
        !write.is_finished(),
        "fan-out pause must retain the response"
    );

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "quiesce must wait for post-publication fan-out and response",
    )
    .await;
    pause.release();
    expect_future_within(write, "published shutdown write should answer")
        .await
        .expect("published shutdown write task should join")
        .expect("a published batch must acknowledge after fan-out");
    expect_future_within(quiesce, "published shutdown quiesce should finish")
        .await
        .expect("published shutdown quiesce task should join");
}

async fn shutdown_drains_accepted_batches_opaque_jobs_and_response_fences() {
    let (_fixture, engine, tenant_id) = shutdown_fixture("shutdown-accepted-messages");
    let initial_head = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("initial accepted-message diagnostics should load")
        .durable_head;
    let pause = engine
        .ordered_publisher_pause_handle_for_testing(&tenant_id)
        .expect("ordered publisher pause should load");
    pause.arm();

    let first = shutdown_insert(engine.clone(), tenant_id.clone(), "first");
    let entered = tokio::task::spawn_blocking({
        let pause = pause.clone();
        move || pause.wait_until_entered(Duration::from_secs(5))
    })
    .await
    .expect("publisher pause waiter should join");
    assert!(
        entered,
        "first accepted publisher batch should reach the pause"
    );

    let second = shutdown_insert(engine.clone(), tenant_id.clone(), "second");
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "second assigned batch should hold a publisher permit",
        |stats| stats.publisher_queue_depth >= 1,
    )
    .await;
    let schema = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .set_table_schema_async(
                    tenant_id,
                    TableSchema {
                        table: TableName::new("shutdown_schema")
                            .expect("shutdown schema table should build"),
                        fields: Vec::new(),
                        indexes: Vec::new(),
                        access_policy: None,
                    },
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "opaque schema job should hold a publisher permit",
        |stats| stats.publisher_queue_depth >= 2,
    )
    .await;
    let response_fence = engine
        .enqueue_publisher_response_fence_for_testing(&tenant_id)
        .await
        .expect("response fence should enter the publisher before shutdown");
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "response fence should hold the final accepted publisher permit",
        |stats| stats.publisher_queue_depth >= 3,
    )
    .await;

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "quiesce must drain accepted publisher permits in FIFO order",
    )
    .await;
    pause.release();

    for (name, write) in [("first", first), ("second", second)] {
        expect_future_within(write, "accepted shutdown mutation should drain")
            .await
            .unwrap_or_else(|error| panic!("{name} mutation task failed to join: {error}"))
            .unwrap_or_else(|error| panic!("{name} accepted mutation failed: {error}"));
    }
    expect_future_within(schema, "accepted opaque job should drain")
        .await
        .expect("accepted schema task should join")
        .expect("accepted opaque schema job should succeed");
    let fence_result = expect_future_within(response_fence, "accepted response fence should drain")
        .await
        .expect("accepted response fence should answer")
        .expect("accepted response fence should retain its result");
    assert!(matches!(
        fence_result,
        crate::tenant::QueuedMutationResult::Scheduled(false)
    ));
    expect_future_within(quiesce, "accepted publisher queue should quiesce")
        .await
        .expect("accepted-message quiesce task should join");
    let settled = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("accepted-message diagnostics should load");
    assert_eq!(settled.publisher_queue_depth, 0);
    assert_eq!(settled.applied_head, settled.durable_head);
    let records = engine
        .read_durable_journal(&tenant_id, initial_head)
        .expect("accepted shutdown journal suffix should read");
    assert_eq!(
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        ((initial_head.0 + 1)..=settled.durable_head.0)
            .map(SequenceNumber)
            .collect::<Vec<_>>(),
        "shutdown must leave one contiguous durable prefix"
    );
    assert_eq!(
        records
            .iter()
            .flat_map(|record| &record.events)
            .filter(|event| matches!(event, nimbus_core::TenantEventKind::DocumentWrite { .. }))
            .count(),
        2,
        "both publisher-accepted document batches must drain"
    );
    assert!(
        records
            .iter()
            .flat_map(|record| &record.events)
            .any(|event| {
                matches!(
                    event,
                    nimbus_core::TenantEventKind::SchemaChange { change }
                        if matches!(
                            change.as_ref(),
                            nimbus_core::SchemaChangeEvent::SetTable { table, .. }
                                if table.as_str() == "shutdown_schema"
                        )
                )
            }),
        "the publisher-accepted opaque schema job must drain"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_publisher_shutdown_matrix() {
    shutdown_after_assignment_rolls_back_unaccepted_suffix().await;
    shutdown_during_persistence_drains_durable_batch().await;
    shutdown_after_publication_drains_response_and_fanout().await;
    shutdown_drains_accepted_batches_opaque_jobs_and_response_fences().await;
    super::publisher_observers::assert_publisher_observers_are_strictly_ordered_and_quiesce_drains_them().await;
}
