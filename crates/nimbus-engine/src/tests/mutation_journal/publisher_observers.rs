use super::publisher_test_seams::wait_for_test_release;
use super::queued::run_paused_insert_burst;
use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within,
};
use super::*;
use nimbus_core::{ScheduleRequest, TenantEventRecord};

type OrderedObserverState = (
    Vec<SequenceNumber>,
    usize,
    usize,
    bool,
    Vec<crate::ProjectionToken>,
);

#[derive(Default)]
struct OrderedBlockingObserver {
    state: Mutex<OrderedObserverState>,
    entered: Condvar,
    release: Condvar,
}

impl OrderedBlockingObserver {
    fn wait_for_first(&self, timeout: Duration) -> bool {
        let state = self.state.lock().expect("observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| state.0.is_empty())
            .expect("observer wait should succeed");
        !state.0.is_empty()
    }

    fn release_first(&self) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.3 = true;
        self.release.notify_all();
    }
}

impl crate::CommittedMutationObserver for OrderedBlockingObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.1 += 1;
        state.2 = state.2.max(state.1);
        state.0.push(event.commit.sequence);
        state.4.push(event.projection_token);
        self.entered.notify_all();
        if state.0.len() == 1 {
            state = wait_for_test_release(
                &self.release,
                state,
                |state| state.3,
                "ordered blocking observer",
            );
        }
        state.1 -= 1;
    }
}

#[derive(Default)]
struct RecordingObserver {
    events: Mutex<Vec<crate::CommittedMutationEvent>>,
}

impl crate::CommittedMutationObserver for RecordingObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        self.events
            .lock()
            .expect("recording observer state should lock")
            .push(event);
    }
}

#[derive(Default)]
struct RecordingSchemaObserver {
    events: Mutex<Vec<crate::TableSchemaChangeEvent>>,
}

impl crate::TableSchemaChangeObserver for RecordingSchemaObserver {
    fn table_schema_changed(&self, event: crate::TableSchemaChangeEvent) {
        self.events
            .lock()
            .expect("recording schema observer state should lock")
            .push(event);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_zero_write_schema_catch_up_retains_source_token() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-zero-write", Engine::create_tenant);
    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("schema record should commit");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("schema journal should read")
        .into_iter()
        .filter(|record| record.schema_epoch_tables().contains(&tasks_table()))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert!(
        TenantEventRecord::as_commit_entry(&records[0])
            .writes
            .is_empty()
    );

    let observer = Arc::new(RecordingObserver::default());
    engine.install_committed_mutation_observer("zero-write-provenance-test", observer.clone());
    engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
        .await
        .expect("zero-write catch-up should dispatch");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("zero-write catch-up should flush");

    let events = observer
        .events
        .lock()
        .expect("recording observer state should lock");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].affected_tables, vec![tasks_table()]);
    assert!(events[0].commit.writes.is_empty());
    assert_eq!(events[0].projection_token.lease_epoch, 0);
    assert_eq!(
        events[0].projection_token.durable_sequence,
        records[0].sequence
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_provider_schema_refresh_waits_for_journal_frontier() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-schema-frontier", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add an unrelated journal record");
    let schema = TableSchema {
        table: tasks_table(),
        fields: Vec::new(),
        indexes: Vec::new(),
        access_policy: None,
    };
    let persisted_sequence = engine
        .persist_table_schema_without_publish_for_testing(&tenant_id, &schema)
        .expect("provider-style schema record should persist without runtime publication");
    let before = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("stale runtime journal stats should read");
    assert!(before.applied_head < persisted_sequence);
    assert!(
        engine
            .get_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .is_err(),
        "the loaded runtime must remain stale before provider catch-up"
    );

    let observer = Arc::new(RecordingSchemaObserver::default());
    engine.install_table_schema_change_observer("schema-frontier-test", observer.clone());
    engine
        .catch_up_provider_after_listener_attach_for_testing()
        .await
        .expect("provider catch-up should reconcile journal before schema notification");

    {
        let events = observer
            .events
            .lock()
            .expect("recording schema observer state should lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tenant_id, tenant_id);
        assert_eq!(events[0].table, tasks_table());
        assert_eq!(events[0].projection_token.lease_epoch, 0);
        assert_eq!(
            events[0].projection_token.durable_sequence, persisted_sequence,
            "schema publication provenance must cover the durable schema record"
        );
    }
    assert!(
        engine
            .get_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .is_ok(),
        "the callback must observe the reconciled loaded schema"
    );
    let after = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("reconciled runtime journal stats should read");
    assert!(
        after.applied_head >= persisted_sequence,
        "the runtime applied frontier must cover the schema callback's source record"
    );
}

pub(super) async fn assert_publisher_observers_are_strictly_ordered_and_quiesce_drains_them() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("ordered-observers", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("ordered-test", observer.clone());

    for index in 0..2 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("publisher response must not wait for the observer");
    }
    expect_blocking_wait_reaches_state("first observer callback should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    {
        let state = observer.state.lock().expect("observer state should lock");
        assert_eq!(state.0, vec![SequenceNumber(1)]);
        assert_eq!(state.2, 1, "observer callbacks must never overlap");
    }
    let observer_stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("observer queue diagnostics should load");
    assert_eq!(observer_stats.observer_queue_depth, 2);
    assert_eq!(observer_stats.observer_queue_capacity, 4_096);
    assert_eq!(observer_stats.observer_queue_high_watermark, 3_072);
    assert!(!observer_stats.observer_dispatch_poisoned);

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "engine quiesce must wait for the ordered observer queue to drain",
    )
    .await;
    observer.release_first();
    expect_future_within(quiesce, "observer queue should drain during quiesce")
        .await
        .expect("quiesce task should join");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(state.0.len(), 2);
    assert!(
        state.0[0] < state.0[1],
        "observer commits must stay ordered"
    );
    assert_eq!(state.2, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_observers_are_strictly_ordered_and_quiesce_drains_them() {
    assert_publisher_observers_are_strictly_ordered_and_quiesce_drains_them().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observer_queue_cap_breach_poison_is_nonblocking_and_visible() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("observer-cap-poison").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 1);
    let created = fixture.create_tenant("observer-cap-poison", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("cap-poison-test", observer.clone());

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("the first commit should enqueue its observer event");
    expect_blocking_wait_reaches_state("first observer callback should hold the queue budget", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    let at_capacity = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("observer queue diagnostics should load at capacity");
    assert_eq!(at_capacity.observer_queue_depth, 1);
    assert_eq!(at_capacity.observer_queue_high_water_warning_count, 1);
    assert_eq!(at_capacity.observer_queue_cap_breach_count, 0);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the reserved catch-up slot should accept a second live observer event");
    let full = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("full observer queue diagnostics should load");
    assert_eq!(full.observer_queue_depth, 2);
    assert_eq!(full.observer_queue_capacity, 2);
    assert!(!full.observer_dispatch_poisoned);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(3))]),
        )
        .await
        .expect("observer saturation must never block or fail a durable mutation response");
    let poisoned = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "observer hard-cap breach should poison the dispatcher",
        |stats| stats.observer_dispatch_poisoned,
    )
    .await;
    assert_eq!(poisoned.observer_queue_depth, 2);
    assert_eq!(poisoned.observer_queue_capacity, 2);
    assert_eq!(poisoned.observer_queue_high_watermark, 1);
    assert_eq!(poisoned.observer_queue_high_water_warning_count, 1);
    assert_eq!(poisoned.observer_queue_cap_breach_count, 1);

    observer.release_first();
    engine.quiesce().await;
    let state = observer.state.lock().expect("observer state should lock");
    let committed_sequences =
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0))
            .into_iter()
            .map(|commit| commit.sequence)
            .collect::<Vec<_>>();
    assert_eq!(
        state.0,
        committed_sequences[..2],
        "the poison policy must drain accepted work without accepting events beyond the cap"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observer_queue_capacity_clamps_above_serial_journal_dispatch_max() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("observer-cap-clamp").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 4);
    crate::tenant::configure_committer_arm_for_testing(
        tenant_id.clone(),
        crate::tenant::CommitterArm::SerialReference,
    );
    let created = fixture.create_tenant("observer-cap-clamp", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("cap-clamp-test", observer.clone());

    run_paused_insert_burst(&engine, &tenant_id, 4).await;
    expect_blocking_wait_reaches_state("full-batch observer dispatch should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("clamped observer diagnostics should load");
    assert_eq!(stats.observer_queue_depth, 4);
    assert_eq!(stats.observer_queue_capacity, 5);
    assert_eq!(stats.observer_queue_cap_breach_count, 0);
    assert!(!stats.observer_dispatch_poisoned);

    observer.release_first();
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("full single dispatch should drain without poison");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_chunks_observers_to_allowance_in_sequence_order() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-observer-chunks").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 2, 1, 1, 1);
    let created = fixture.create_tenant("provider-observer-chunks", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    for index in 0..5 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider-tail fixture write should commit");
    }
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider-tail fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    let expected_sequences = records
        .iter()
        .filter(|record| !record.writes.is_empty())
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    assert_eq!(expected_sequences.len(), 5);

    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-chunk-order-test", observer.clone());
    let mut catch_up = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
                .await
        }
    });
    expect_blocking_wait_reaches_state("the first provider observer chunk should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    assert_future_stays_pending(
        &mut catch_up,
        "provider catch-up must wait for capacity before handing off its next chunk",
    )
    .await;
    let at_capacity = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("provider observer diagnostics should load");
    assert_eq!(at_capacity.observer_queue_depth, 1);
    assert_eq!(at_capacity.observer_queue_capacity, 2);
    assert_eq!(at_capacity.observer_queue_cap_breach_count, 0);
    assert!(!at_capacity.observer_dispatch_poisoned);

    observer.release_first();
    expect_catch_up_future_within(catch_up, "provider observer chunks should drain")
        .await
        .expect("provider catch-up task should join")
        .expect("provider observer catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("provider observer tail should flush");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(state.0, expected_sequences);
    assert_eq!(state.2, 1, "provider observer chunks must remain serial");
    drop(state);
    let drained = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("drained provider observer diagnostics should load");
    assert_eq!(drained.observer_queue_depth, 0);
    assert_eq!(drained.observer_queue_cap_breach_count, 0);
    assert!(!drained.observer_dispatch_poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_reserves_live_max_dispatch_headroom_without_poison() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-observer-live-headroom")
        .expect("provider observer headroom tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 4, 1, 2, 2);
    assert_eq!(
        fixture.create_tenant("provider-observer-live-headroom", Engine::create_tenant),
        tenant_id
    );
    for index in 0..6 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider observer headroom fixture write should commit");
    }
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider observer headroom journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 6);
    let catch_up_records = records[..4].to_vec();
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-live-headroom-test", observer.clone());

    let mut catch_up = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &catch_up_records)
                .await
        }
    });
    expect_blocking_wait_reaches_state("catch-up should fill only its observer allowance", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    assert_future_stays_pending(
        &mut catch_up,
        "catch-up must stop at its allowance and preserve live dispatch headroom",
    )
    .await;
    let catch_up_full = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("catch-up headroom diagnostics should load");
    assert_eq!(catch_up_full.observer_queue_peak_depth, 2);
    assert_eq!(catch_up_full.observer_queue_capacity, 4);

    engine
        .process_applied_commit_batch_for_testing(&tenant_id, &records[4..])
        .expect("a live maximum-size dispatch should use the reserved headroom");
    let live_full = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("live headroom diagnostics should load");
    assert_eq!(live_full.observer_queue_peak_depth, 4);
    assert_eq!(live_full.observer_queue_cap_breach_count, 0);
    assert!(!live_full.observer_dispatch_poisoned);

    observer.release_first();
    expect_catch_up_future_within(catch_up, "catch-up headroom task should complete")
        .await
        .expect("catch-up headroom task should join")
        .expect("catch-up headroom task should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("catch-up and live observer work should drain");
    let state = observer.state.lock().expect("observer state should lock");
    let expected_dispatch_order = records[..2]
        .iter()
        .chain(&records[4..])
        .chain(&records[2..4])
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    assert_eq!(
        state.0, expected_dispatch_order,
        "the reserved live dispatch must enter FIFO order before catch-up resumes"
    );
    assert_eq!(state.2, 1, "catch-up and live callbacks must stay serial");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rapid_provider_catch_up_triggers_coalesce_into_one_tail_reader() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-observer-coalescing").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-observer-coalescing", Engine::create_tenant),
        tenant_id
    );
    const RECORDS: usize = 12;
    for index in 0..RECORDS {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider-tail fixture write should commit");
    }
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider-tail fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), RECORDS);

    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-coalescing-test", observer.clone());
    let latest_token = engine
        .projection_token_for_tenant_async(&tenant_id)
        .await
        .expect("latest provider projection token should resolve");
    let initial_token = crate::ProjectionToken {
        durable_sequence: records[1].sequence,
        ..latest_token
    };
    assert!(
        engine
            .trigger_provider_catch_up_observers_with_token_for_testing(
                &tenant_id,
                &records[..2],
                initial_token,
            )
            .expect("initial catch-up trigger should start"),
        "the first trigger must own the tenant's sole catch-up task"
    );
    expect_blocking_wait_reaches_state("the first coalesced catch-up callback should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;

    for record in &records[2..] {
        let projection_token = crate::ProjectionToken {
            durable_sequence: record.sequence,
            ..initial_token
        };
        assert!(
            !engine
                .trigger_provider_catch_up_observers_with_token_for_testing(
                    &tenant_id,
                    std::slice::from_ref(record),
                    projection_token,
                )
                .expect("later catch-up trigger should coalesce"),
            "a later frontier must not spawn another parked catch-up task"
        );
    }
    assert!(
        !engine
            .trigger_provider_catch_up_observers_with_token_for_testing(
                &tenant_id,
                std::slice::from_ref(records.last().expect("journal should have a tail")),
                latest_token,
            )
            .expect("latest projection token should coalesce while the observer is parked"),
        "a provenance-only frontier advance must not spawn a second catch-up task"
    );
    assert_eq!(
        engine
            .provider_catch_up_observer_task_count_for_testing(&tenant_id)
            .expect("catch-up task count should load"),
        1,
        "one stalled tenant must retain only one catch-up task regardless of trigger count"
    );

    observer.release_first();
    engine
        .enqueue_provider_catch_up_observers_for_testing(
            &tenant_id,
            std::slice::from_ref(records.last().expect("journal should have a tail")),
        )
        .await
        .expect("coalesced catch-up should reach the latest requested frontier");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("coalesced observer work should drain");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(
        state.0,
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        "the sole tail reader must deliver the latest durable frontier exactly once and in order"
    );
    assert_eq!(
        state.2, 1,
        "coalesced observer callbacks must remain serial"
    );
    assert_eq!(
        state.4[..2],
        [initial_token; 2],
        "the initially claimed range should retain its sampled provenance"
    );
    assert_eq!(
        state.4[2..],
        [latest_token; RECORDS - 2],
        "the coalesced range must use the maximum token supplied by later provider notifications"
    );
}

#[derive(Default)]
struct TenantSelectiveBlockingObserver {
    blocked_tenant: Mutex<Option<TenantId>>,
    state: Mutex<(
        std::collections::HashMap<TenantId, Vec<SequenceNumber>>,
        bool,
        bool,
    )>,
    entered: Condvar,
    release: Condvar,
}

impl TenantSelectiveBlockingObserver {
    fn block_tenant(&self, tenant_id: TenantId) {
        *self
            .blocked_tenant
            .lock()
            .expect("blocked tenant lock should acquire") = Some(tenant_id);
    }

    fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| !state.1)
            .expect("selective observer entered wait should succeed");
        state.1
    }

    fn release_blocked_tenant(&self) {
        let mut state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        state.2 = true;
        self.release.notify_all();
    }

    fn sequences(&self, tenant_id: &TenantId) -> Vec<SequenceNumber> {
        self.state
            .lock()
            .expect("selective observer state should lock")
            .0
            .get(tenant_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl crate::CommittedMutationObserver for TenantSelectiveBlockingObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        let should_block = self
            .blocked_tenant
            .lock()
            .expect("blocked tenant lock should acquire")
            .as_ref()
            == Some(&event.tenant_id);
        let mut state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        let first_for_tenant = state
            .0
            .entry(event.tenant_id.clone())
            .or_default()
            .is_empty();
        state
            .0
            .get_mut(&event.tenant_id)
            .expect("tenant observer sequence should exist")
            .push(event.commit.sequence);
        if should_block && first_for_tenant {
            state.1 = true;
            self.entered.notify_all();
            drop(wait_for_test_release(
                &self.release,
                state,
                |state| state.2,
                "tenant-selective blocking observer",
            ));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn poisoned_provider_catch_up_tenant_does_not_block_fresh_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("provider-poison-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("provider-poison-b", Engine::create_tenant);
    let scheduled_tenant =
        fixture.create_tenant("provider-poison-scheduled", Engine::create_tenant);
    for tenant_id in [&tenant_a, &tenant_b] {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
            .await
            .expect("provider catch-up fixture write should commit");
        engine
            .shutdown_trigger_candidates_for_testing(tenant_id)
            .expect("trigger cursor should not add unrelated records");
    }
    let records_a = engine
        .read_durable_journal(&tenant_a, SequenceNumber(0))
        .expect("tenant A journal should read");
    let records_b = engine
        .read_durable_journal(&tenant_b, SequenceNumber(0))
        .expect("tenant B journal should read");
    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-poison-isolation-test", observer.clone());
    engine
        .schedule_mutation_async(
            scheduled_tenant.clone(),
            ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: serde_json::Map::from_iter([("index".to_string(), json!(99))]),
                },
            },
        )
        .await
        .expect("scheduled fixture work should persist");
    engine
        .evict_runtime_without_deleting_for_testing(&scheduled_tenant)
        .await
        .expect("scheduled tenant should unload without deleting durable work");
    assert!(!engine.loaded_tenant_ids().contains(&scheduled_tenant));
    engine
        .poison_committed_mutation_observers_for_testing(&tenant_a)
        .expect("tenant A observer dispatcher should poison");

    let error = engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_a, &records_a)
        .await
        .expect_err("poisoned tenant A must refuse catch-up observers");
    assert!(error.to_string().contains("poisoned"));
    let stats_a = engine
        .mutation_journal_stats_for_testing(&tenant_a)
        .expect("tenant A observer diagnostics should load");
    assert_eq!(stats_a.observer_catch_up_enqueue_failure_count, 1);

    tokio::time::timeout(
        Duration::from_secs(2),
        engine.enqueue_provider_catch_up_observers_for_testing(&tenant_b, &records_b),
    )
    .await
    .expect("tenant B catch-up must not wait behind poisoned tenant A")
    .expect("tenant B catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_b)
        .await
        .expect("tenant B observers should flush");
    assert_eq!(observer.sequences(&tenant_b), vec![SequenceNumber(1)]);
    let stats_b = engine
        .mutation_journal_stats_for_testing(&tenant_b)
        .expect("tenant B observer diagnostics should load");
    assert_eq!(stats_b.observer_catch_up_enqueue_failure_count, 0);
    assert!(!stats_b.observer_dispatch_poisoned);

    engine
        .catch_up_provider_after_listener_attach_for_testing()
        .await
        .expect("listener-attach catch-up should continue into scheduled-work loading");
    assert!(
        engine.loaded_tenant_ids().contains(&scheduled_tenant),
        "poisoned tenant A must not prevent an unloaded scheduled tenant from loading"
    );
    assert_eq!(
        engine
            .list_scheduled_jobs(&scheduled_tenant)
            .expect("reloaded scheduled work should remain queryable")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_attach_contains_mid_eviction_tenant_and_loads_remaining_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("provider-evicting-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("provider-refresh-b", Engine::create_tenant);
    let scheduled_tenant =
        fixture.create_tenant("provider-refresh-scheduled", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_a)
        .expect("trigger cursor should not add unrelated records");

    let document_id = engine
        .insert_document_async(
            tenant_b.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("tenant B seed should commit");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_b)
        .expect("trigger cursor should not add unrelated records");
    engine
        .schedule_mutation_async(
            scheduled_tenant.clone(),
            ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: serde_json::Map::from_iter([("index".to_string(), json!(99))]),
                },
            },
        )
        .await
        .expect("scheduled fixture work should persist");
    engine
        .evict_runtime_without_deleting_for_testing(&scheduled_tenant)
        .await
        .expect("scheduled tenant should unload without deleting durable work");

    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer(
        "provider-whole-body-isolation-test",
        observer.clone(),
    );
    let pending_b = engine
        .stage_assigned_pending_update_for_testing(
            &tenant_b,
            &tasks_table(),
            &document_id,
            "index",
            json!(1),
        )
        .expect("tenant B provider-style update should stage");
    engine
        .apply_assigned_pending_record_without_publish_for_testing(&tenant_b, &pending_b)
        .expect("tenant B durable state should advance without its runtime watermark");
    let evicting_runtime = engine
        .begin_runtime_eviction_for_testing(&tenant_a)
        .expect("tenant A should enter the mid-eviction state");

    engine
        .catch_up_provider_after_listener_attach_for_testing()
        .await
        .expect("tenant A failure must not abort the remaining attach catch-up");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if observer.sequences(&tenant_b) == vec![pending_b.sequence] {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("tenant B observer catch-up should reach its durable tail");
    assert!(
        engine
            .provider_catch_up_failure_count_for_testing(&tenant_a)
            .expect("tenant A failure count should remain inspectable during eviction")
            >= 1,
        "the contained tenant failure must be visible in per-tenant diagnostics"
    );
    assert!(
        engine.loaded_tenant_ids().contains(&scheduled_tenant),
        "tenant A must not prevent a later scheduled-work tenant from loading"
    );
    assert_eq!(
        engine
            .list_scheduled_jobs(&scheduled_tenant)
            .expect("reloaded scheduled work should remain queryable")
            .len(),
        1
    );
    drop(evicting_runtime);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_provider_catch_up_tenant_does_not_head_of_line_block_other_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = TenantId::new("provider-busy-a").expect("tenant A id should build");
    let tenant_b = TenantId::new("provider-busy-b").expect("tenant B id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_a.clone(), 2, 1, 1, 1);
    crate::tenant::configure_observer_limits_for_testing(tenant_b.clone(), 2, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-busy-a", Engine::create_tenant),
        tenant_a
    );
    assert_eq!(
        fixture.create_tenant("provider-busy-b", Engine::create_tenant),
        tenant_b
    );
    for index in 0..5 {
        engine
            .insert_document_async(
                tenant_a.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("tenant A provider tail should commit");
    }
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_a)
        .expect("trigger cursor should not add unrelated records");
    engine
        .insert_document_async(
            tenant_b.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(10))]),
        )
        .await
        .expect("tenant B provider tail should commit");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_b)
        .expect("trigger cursor should not add unrelated records");
    let records_a = engine
        .read_durable_journal(&tenant_a, SequenceNumber(0))
        .expect("tenant A journal should read");
    let expected_a = records_a
        .iter()
        .filter(|record| !record.writes.is_empty())
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    let records_b = engine
        .read_durable_journal(&tenant_b, SequenceNumber(0))
        .expect("tenant B journal should read");
    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    observer.block_tenant(tenant_a.clone());
    engine.install_committed_mutation_observer("provider-busy-isolation-test", observer.clone());

    let mut catch_up_a = tokio::spawn({
        let engine = engine.clone();
        let tenant_a = tenant_a.clone();
        async move {
            engine
                .enqueue_provider_catch_up_observers_for_testing(&tenant_a, &records_a)
                .await
        }
    });
    expect_blocking_wait_reaches_state("tenant A observer should sustain a full backlog", {
        let observer = observer.clone();
        move |timeout| observer.wait_until_blocked(timeout)
    })
    .await;
    assert_future_stays_pending(
        &mut catch_up_a,
        "tenant A catch-up should wait for its own partial-capacity chunks",
    )
    .await;

    tokio::time::timeout(
        Duration::from_secs(2),
        engine.enqueue_provider_catch_up_observers_for_testing(&tenant_b, &records_b),
    )
    .await
    .expect("tenant B hint must stay responsive while tenant A is saturated")
    .expect("tenant B catch-up should enqueue");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_b)
        .await
        .expect("tenant B observer should drain");
    assert_eq!(observer.sequences(&tenant_b), vec![SequenceNumber(1)]);

    observer.release_blocked_tenant();
    expect_catch_up_future_within(catch_up_a, "tenant A chunks should eventually drain")
        .await
        .expect("tenant A catch-up task should join")
        .expect("tenant A catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_a)
        .await
        .expect("tenant A observer should flush");
    assert_eq!(
        observer.sequences(&tenant_a),
        expected_a,
        "tenant A catch-up chunks must preserve commit order"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_spawn_rejection_releases_task_state_after_quiesce() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("provider-catch-up-quiesce", Engine::create_tenant);
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("provider catch-up fixture write should commit");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider catch-up fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    engine.install_committed_mutation_observer(
        "provider-catch-up-quiesce-test",
        Arc::new(TenantSelectiveBlockingObserver::default()),
    );
    engine.quiesce().await;

    for _ in 0..2 {
        let error = engine
            .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
            .await
            .expect_err("quiesce must reject new provider catch-up tasks without panicking");
        assert!(matches!(error, Error::ResourceExhausted(_)));
        assert_eq!(
            engine
                .provider_catch_up_observer_task_count_for_testing(&tenant_id)
                .expect("catch-up task count should load"),
            0,
            "a rejected spawn must release the tenant's sole-task state"
        );
    }
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("catch-up spawn rejection diagnostics should load");
    assert_eq!(stats.observer_catch_up_enqueue_failure_count, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn panicking_provider_catch_up_releases_ownership_and_successor_delivers() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("provider-catch-up-panic", Engine::create_tenant);
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("provider catch-up fixture write should commit");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider catch-up fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-catch-up-panic-test", observer.clone());
    engine.panic_next_provider_catch_up_for_testing(tenant_id.clone());

    assert!(
        engine
            .trigger_provider_catch_up_observers_for_testing(&tenant_id, &records)
            .expect("initial catch-up trigger should start"),
        "the panicking task must first win catch-up ownership"
    );
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "panicking catch-up should increment its failure counter",
        |stats| stats.observer_catch_up_enqueue_failure_count == 1,
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if engine
                .provider_catch_up_observer_task_count_for_testing(&tenant_id)
                .expect("catch-up task count should load")
                == 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("panicking catch-up task should release its task state");

    engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
        .await
        .expect("a successor catch-up task should acquire the republished request");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("successor observer work should drain");
    assert_eq!(observer.sequences(&tenant_id), vec![SequenceNumber(1)]);
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("catch-up panic diagnostics should load");
    assert_eq!(stats.observer_catch_up_enqueue_failure_count, 1);
}

#[derive(Default)]
struct BlockingPanickingObserver {
    state: Mutex<(bool, bool)>,
    entered: Condvar,
    release: Condvar,
}

impl BlockingPanickingObserver {
    fn wait_until_entered(&self, timeout: Duration) -> bool {
        let state = self.state.lock().expect("observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| !state.0)
            .expect("observer wait should succeed");
        state.0
    }

    fn release_to_panic(&self) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.1 = true;
        self.release.notify_all();
    }
}

impl crate::CommittedMutationObserver for BlockingPanickingObserver {
    fn committed_mutation_applied(&self, _event: crate::CommittedMutationEvent) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.0 = true;
        self.entered.notify_all();
        state = wait_for_test_release(
            &self.release,
            state,
            |state| state.1,
            "blocking panicking observer",
        );
        drop(state);
        panic!("injected committed observer panic");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn panicking_observer_poison_drops_queued_depth_without_phantom_backlog() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("observer-panic", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(BlockingPanickingObserver::default());
    engine.install_committed_mutation_observer("panic-test", observer.clone());

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("first insert should dispatch to the blocking observer");
    expect_blocking_wait_reaches_state("first observer dispatch should block before panicking", {
        let observer = observer.clone();
        move |timeout| observer.wait_until_entered(timeout)
    })
    .await;
    for index in 2..=3 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("later inserts should queue behind the blocked observer dispatch");
    }
    let queued = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("queued observer diagnostics should load");
    assert_eq!(queued.observer_queue_depth, 3);
    assert!(!queued.observer_dispatch_poisoned);

    observer.release_to_panic();
    let stats = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "panicking observer should poison with an accurate drained depth",
        |stats| stats.observer_dispatch_poisoned && stats.observer_queue_depth == 0,
    )
    .await;
    assert_eq!(stats.observer_queue_cap_breach_count, 0);
    tokio::time::timeout(Duration::from_secs(5), engine.quiesce())
        .await
        .expect("observer panic must not strand engine quiesce");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committed_observers_preserve_order_across_execution_unit_and_direct_handoffs() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("mixed-ordered-observers", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("mixed-ordered-test", observer.clone());

    let first = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("first execution unit should begin");
    first
        .insert_document(
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .expect("first insert should stage");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
    let first_commit = tokio::task::spawn_blocking(move || first.commit());
    let wait_faults = faults.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_faults.wait_until_entered(
                crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT,
                Duration::from_secs(5),
            )
        })
        .await
        .expect("commit pause waiter should join"),
        "first commit should pause after its serialized observer enqueue"
    );

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("second direct commit task should join")
    .expect("second direct commit should succeed");

    expect_blocking_wait_reaches_state("first observer callback should arrive", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    {
        let state = observer.state.lock().expect("observer state should lock");
        assert_eq!(
            state.0,
            vec![SequenceNumber(1)],
            "the later direct caller must not enqueue ahead of the earlier execution-unit commit"
        );
    }

    observer.release_first();
    faults.release(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
    first_commit
        .await
        .expect("first execution-unit task should join")
        .expect("first execution-unit commit should succeed")
        .expect("first execution-unit insert should commit");
}

/// Inserts `count` documents and returns the write-bearing journal records the
/// provider catch-up path would have to replay for them.
async fn write_provider_catch_up_tail(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    count: usize,
) -> Vec<TenantEventRecord> {
    for index in 0..count {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider catch-up tail fixture write should commit");
    }
    let records = engine
        .read_durable_journal(tenant_id, SequenceNumber(0))
        .expect("provider catch-up tail journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), count);
    records
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_pages_a_large_tail_instead_of_materialising_it() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-catch-up-paged-tail").expect("tenant id should build");
    // capacity 4 with a single-event live reservation yields a catch-up chunk
    // budget of 2, so a 9-record tail cannot be read in one page.
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 4, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-catch-up-paged-tail", Engine::create_tenant),
        tenant_id
    );
    let records = write_provider_catch_up_tail(&engine, &tenant_id, 9).await;
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let expected_sequences = records
        .iter()
        .map(|record| record.sequence)
        .collect::<Vec<_>>();

    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-paged-tail-test", observer.clone());
    engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
        .await
        .expect("a paged provider catch-up should deliver its whole tail");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("paged provider catch-up work should drain");

    assert_eq!(
        observer.sequences(&tenant_id),
        expected_sequences,
        "every catch-up event must be delivered exactly once, in sequence order"
    );

    let pages =
        crate::engine::committed_mutations::provider_catch_up_page_reads_for_testing(&tenant_id);
    assert!(
        pages.len() >= 5,
        "a 9-record tail read two records at a time needs at least 5 pages, saw {pages:?}"
    );
    assert!(
        pages.iter().all(|page| *page <= 2),
        "no catch-up journal read may exceed the chunk budget that bounds it, saw {pages:?}"
    );
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("paged catch-up diagnostics should load");
    assert_eq!(stats.observer_queue_depth, 0);
    assert_eq!(stats.observer_catch_up_enqueue_failure_count, 0);
    assert!(!stats.observer_dispatch_poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_page_failure_republishes_the_undelivered_tail() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id =
        TenantId::new("provider-catch-up-page-failure").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 4, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-catch-up-page-failure", Engine::create_tenant),
        tenant_id
    );
    let records = write_provider_catch_up_tail(&engine, &tenant_id, 9).await;
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let expected_sequences = records
        .iter()
        .map(|record| record.sequence)
        .collect::<Vec<_>>();

    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-page-failure-test", observer.clone());
    // Fail deep enough into the tail that whole dispatch chunks have already
    // been handed off, so the failure lands partway through paging rather than
    // before any delivery.
    engine.fail_provider_catch_up_after_pages_for_testing(tenant_id.clone(), 4);

    let error = engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
        .await
        .expect_err("a failed page read must surface as a catch-up failure");
    assert!(
        error
            .to_string()
            .contains("injected provider catch-up page"),
        "the page read failure should propagate verbatim: {error}"
    );
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("the pages delivered before the failure should drain");
    // Delivery must have stopped at a strict prefix of the requested tail.
    let delivered_before_failure = observer.sequences(&tenant_id);
    assert!(
        !delivered_before_failure.is_empty()
            && delivered_before_failure.len() < expected_sequences.len(),
        "the failing read must leave a non-empty strict prefix delivered, saw {delivered_before_failure:?} of {expected_sequences:?}"
    );
    assert_eq!(
        delivered_before_failure,
        expected_sequences[..delivered_before_failure.len()],
        "the pages delivered before the failure must be the tail's leading events, in order"
    );
    let failed = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("failed catch-up diagnostics should load");
    assert_eq!(failed.observer_catch_up_enqueue_failure_count, 1);
    assert!(
        !failed.observer_dispatch_poisoned,
        "a failed page read must not poison the tenant's dispatcher"
    );

    // Ownership was abandoned back to the request's original first sequence,
    // so a successor replays the whole range rather than resuming past the
    // records the failed attempt never delivered.
    engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
        .await
        .expect("a successor catch-up should acquire the republished request");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("successor catch-up work should drain");

    let mut expected_total = delivered_before_failure.clone();
    expected_total.extend(expected_sequences.iter().copied());
    assert_eq!(
        observer.sequences(&tenant_id),
        expected_total,
        "the successor must redeliver the abandoned request from its original first sequence"
    );
}
