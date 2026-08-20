use super::*;

const COMMIT_PHASE_TIMEOUT: Duration = Duration::from_secs(5);

struct DurablePublishPause {
    faults: crate::CommitFaultHandle,
    released: bool,
}

impl DurablePublishPause {
    fn arm(engine: &Engine) -> Self {
        let faults = engine.commit_fault_handle_for_testing();
        faults.arm(crate::commit_fault_labels::DURABLE_BEFORE_PUBLISH);
        Self {
            faults,
            released: false,
        }
    }

    async fn wait_until_entered(
        &self,
        scenario_seed: u64,
        step: usize,
        tenant: &str,
        route: PpscRoute,
    ) {
        let faults = self.faults.clone();
        let entered = tokio::task::spawn_blocking(move || {
            faults.wait_until_entered(
                crate::commit_fault_labels::DURABLE_BEFORE_PUBLISH,
                COMMIT_PHASE_TIMEOUT,
            )
        })
        .await
        .expect("PPSC durable-before-publish waiter should join");
        assert!(
            entered,
            "PPSC seed {scenario_seed} step {step} tenant {tenant} route {route:?} did not reach durable-before-publish within {COMMIT_PHASE_TIMEOUT:?}"
        );
    }

    fn release(&mut self) {
        if !self.released {
            self.faults
                .release(crate::commit_fault_labels::DURABLE_BEFORE_PUBLISH);
            self.released = true;
        }
    }
}

impl Drop for DurablePublishPause {
    fn drop(&mut self) {
        self.release();
    }
}

impl PpscEngineRunner {
    pub(super) async fn exercise_commit_phase_fault(
        &self,
        step: usize,
        tenant: &str,
        route: PpscRoute,
        fault: PpscInjectedFault,
    ) -> PpscExpectedOutcome {
        match fault {
            PpscInjectedFault::DurableBeforePublish => {
                self.exercise_durable_before_publish(step, tenant, route)
                    .await;
                PpscExpectedOutcome::Committed
            }
            PpscInjectedFault::PanicAfterDurable => {
                self.exercise_panic_after_durable(step, tenant, route).await;
                PpscExpectedOutcome::AmbiguousRecovered
            }
            other => panic!(
                "PPSC commit-phase operation cannot execute fault '{}'",
                other.as_str()
            ),
        }
    }

    async fn exercise_durable_before_publish(&self, step: usize, tenant: &str, route: PpscRoute) {
        let tenant_id = self.tenant(tenant).clone();
        let before_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC pre-pause journal should read");
        let before = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC pre-pause frontiers should read");
        let mut pause = DurablePublishPause::arm(self.engine.as_ref());
        let (_, mut write) =
            self.spawn_route_insert(step, tenant, route, "durable-before-publish", 401);
        pause
            .wait_until_entered(self.scenario_seed, step, tenant, route)
            .await;
        assert!(
            timeout(Duration::from_millis(50), &mut write)
                .await
                .is_err(),
            "PPSC caller must remain pending while its durable record is held before publication"
        );

        let during_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC paused durable journal should read");
        assert_eq!(
            during_journal.len(),
            before_journal.len() + 1,
            "the commit-phase pause must be reached after exactly one durable append"
        );
        let during = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC paused frontiers should read");
        assert_eq!(
            during.published_head, before.published_head,
            "durable-before-publish must not expose the held record"
        );
        assert_eq!(
            during.applied_head, before.applied_head,
            "durable-before-publish must not advance the Engine applied frontier"
        );

        pause.release();
        expect_route_write(write, "durable-before-publish mutation")
            .await
            .expect("PPSC mutation should commit after the phase pause releases");
        let after = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC post-pause frontiers should read");
        assert_eq!(
            after.published_head.0,
            before.published_head.0 + 1,
            "release must publish exactly the held record"
        );
        assert_eq!(after.applied_head, after.published_head);
    }

    pub(super) async fn exercise_publication_predecessor_race(
        &self,
        step: usize,
        tenant: &str,
        predecessor_route: PpscRoute,
        successor_route: PpscRoute,
    ) -> PpscExpectedOutcome {
        let tenant_id = self.tenant(tenant).clone();
        let before_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC predecessor-race journal should read");
        let before = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC predecessor-race frontiers should read");
        let mut pause = DurablePublishPause::arm(self.engine.as_ref());
        let (_, predecessor) =
            self.spawn_route_insert(step, tenant, predecessor_route, "held-predecessor", 411);
        pause
            .wait_until_entered(self.scenario_seed, step, tenant, predecessor_route)
            .await;

        let (_, mut successor) =
            self.spawn_route_insert(step, tenant, successor_route, "blocked-successor", 412);
        assert!(
            timeout(Duration::from_millis(50), &mut successor)
                .await
                .is_err(),
            "a later commit must remain pending behind its held publication predecessor"
        );
        let during_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC held-predecessor journal should read");
        assert_eq!(
            during_journal.len(),
            before_journal.len() + 1,
            "the successor must not append while its predecessor owns publication order"
        );
        let during = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC held-predecessor frontiers should read");
        assert_eq!(during.published_head, before.published_head);

        pause.release();
        expect_route_write(predecessor, "held predecessor")
            .await
            .expect("PPSC predecessor should publish after release");
        expect_route_write(successor, "blocked successor")
            .await
            .expect("PPSC successor should publish after its predecessor");

        let after_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC predecessor-race final journal should read");
        assert_eq!(after_journal.len(), before_journal.len() + 2);
        assert_eq!(
            after_journal[before_journal.len()].sequence.0,
            before.published_head.0 + 1,
            "PPSC held-predecessor first sequence must follow the published prefix; before={before:?}, before_journal={:?}, after_journal={:?}",
            before_journal
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            after_journal
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            after_journal[before_journal.len() + 1].sequence.0,
            before.published_head.0 + 2,
            "PPSC held-predecessor successor sequence must remain contiguous; before={before:?}, before_journal={:?}, after_journal={:?}",
            before_journal
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            after_journal
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>()
        );
        let after = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC predecessor-race final frontiers should read");
        assert_eq!(after.published_head.0, before.published_head.0 + 2);
        assert_eq!(after.applied_head, after.published_head);
        PpscExpectedOutcome::Committed
    }

    async fn exercise_panic_after_durable(&self, step: usize, tenant: &str, route: PpscRoute) {
        let tenant_id = self.tenant(tenant).clone();
        // Retain a canonical long-lived Engine handle and its read snapshot
        // across recovery. This proves redb runtime replacement does not rely
        // on unrelated, already-fenced handles dropping before reopen.
        let stale_execution_unit = self
            .engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("PPSC stale execution-unit fixture should open");
        let before_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC pre-panic journal should read");
        let runtime_before = self
            .engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("PPSC pre-panic runtime identity should read");
        self.engine
            .commit_fault_handle_for_testing()
            .inject_panic_on_nth_hit(crate::commit_fault_labels::DURABLE_BEFORE_PUBLISH, 1);
        let (document_id, write) =
            self.spawn_route_insert(step, tenant, route, "panic-after-durable", 421);
        let error = expect_route_write(write, "panic-after-durable mutation")
            .await
            .expect_err("PPSC post-durable panic must report ambiguity to its caller");
        assert!(
            error.to_string().contains("crash-and-replay"),
            "post-durable panic must demand crash-and-replay: {error}"
        );

        let recovered = timeout(
            COMMIT_PHASE_TIMEOUT,
            self.engine
                .get_document_async(tenant_id.clone(), tasks_table(), document_id.clone()),
        )
        .await
        .expect("PPSC post-durable recovery should be bounded")
        .expect("PPSC next access should replay the durable record");
        assert_eq!(recovered.fields.get("value"), Some(&json!(421)));
        let stale_error = stale_execution_unit
            .get_document(&tasks_table(), document_id)
            .expect_err("a stale execution unit must remain fenced after runtime replacement");
        assert!(
            matches!(
                &stale_error,
                Error::Storage {
                    kind: nimbus_core::StorageErrorKind::Unavailable,
                    ..
                }
            ),
            "stale execution-unit access must fail as transiently unavailable: {stale_error}"
        );
        assert_ne!(
            self.engine
                .tenant_runtime_identity_for_testing(&tenant_id)
                .expect("PPSC recovered runtime identity should read"),
            runtime_before,
            "ambiguous post-durable panic must replace the tenant runtime"
        );
        let after_journal = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC recovered journal should read");
        assert_eq!(
            after_journal.len(),
            before_journal.len() + 1,
            "crash-and-replay must retain exactly one durable effect"
        );
        let after = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC recovered frontiers should read");
        assert_eq!(after.published_head, after.durable_head);
        assert_eq!(after.applied_head, after.published_head);
    }

    fn spawn_route_insert(
        &self,
        step: usize,
        tenant: &str,
        route: PpscRoute,
        key: &str,
        value: i64,
    ) -> (DocumentId, tokio::task::JoinHandle<nimbus_core::Result<()>>) {
        let engine = self.engine.current();
        let tenant_id = self.tenant(tenant).clone();
        let document_id = ppsc_document_id(self.scenario_seed, step, key);
        let task_document_id = document_id.clone();
        let fields = ppsc_fields(key, value);
        let write = tokio::spawn(async move {
            match route {
                PpscRoute::QueuedJournal => engine
                    .insert_document_async_with_id(
                        tenant_id,
                        tasks_table(),
                        task_document_id,
                        fields,
                    )
                    .await
                    .map(|_| ()),
                PpscRoute::Direct => tokio::task::spawn_blocking(move || {
                    engine
                        .insert_document_with_id(
                            &tenant_id,
                            tasks_table(),
                            task_document_id,
                            fields,
                        )
                        .map(|_| ())
                })
                .await
                .unwrap_or_else(|error| {
                    Err(Error::Internal(format!(
                        "PPSC direct route task panicked: {error}"
                    )))
                }),
                PpscRoute::ExecutionUnit => tokio::task::spawn_blocking(move || {
                    let unit = engine
                        .begin_mutation_execution_unit(tenant_id, PrincipalContext::anonymous())?;
                    unit.insert_document_with_id(tasks_table(), Some(task_document_id), fields)?;
                    unit.commit()?
                        .expect("PPSC execution-unit route should emit a durable record");
                    Ok(())
                })
                .await
                .unwrap_or_else(|error| {
                    Err(Error::Internal(format!(
                        "PPSC execution-unit route task panicked: {error}"
                    )))
                }),
            }
        });
        (document_id, write)
    }
}

async fn expect_route_write(
    write: tokio::task::JoinHandle<nimbus_core::Result<()>>,
    operation: &str,
) -> nimbus_core::Result<()> {
    timeout(COMMIT_PHASE_TIMEOUT, write)
        .await
        .unwrap_or_else(|_| {
            panic!("PPSC {operation} did not resolve within {COMMIT_PHASE_TIMEOUT:?}")
        })
        .unwrap_or_else(|error| panic!("PPSC {operation} task failed to join: {error}"))
}
