use super::*;

impl PpscEngineRunner {
    pub(super) async fn cancel_queued_insert(&self, step: usize, tenant: &str) {
        let result = self
            .engine
            .insert_document_async_with(
                self.tenant(tenant).clone(),
                tasks_table(),
                Some(ppsc_document_id(
                    self.scenario_seed,
                    step,
                    "cancelled-queued",
                )),
                ppsc_fields("cancelled-queued", -1),
                crate::AsyncMutationContext::anonymous(std::future::pending(), || {
                    Err(Error::Cancelled)
                }),
            )
            .await;
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "PPSC queued cancellation must remain typed and pre-durable"
        );
    }

    pub(super) async fn cancel_execution_unit_admission(&self, tenant: &str) {
        let tenant_id = self.tenant(tenant).clone();
        let before = self
            .engine
            .tenant_engine_diagnostics(&tenant_id)
            .expect("PPSC cancellation diagnostics should load")
            .mutation_isolate_admission;
        let mut held = Vec::with_capacity(before.ceiling);
        for _ in 0..before.ceiling {
            held.push(
                self.engine
                    .acquire_mutation_isolate_permit_cancellable(&tenant_id, std::future::pending())
                    .await
                    .expect("PPSC cancellation setup should saturate isolate admission"),
            );
        }
        let saturated = self
            .engine
            .tenant_engine_diagnostics(&tenant_id)
            .expect("PPSC saturated cancellation diagnostics should load")
            .mutation_isolate_admission;
        assert_eq!(saturated.concurrent_count, saturated.ceiling);

        let result = timeout(
            Duration::from_secs(1),
            self.engine
                .acquire_mutation_isolate_permit_cancellable(&tenant_id, std::future::ready(())),
        )
        .await
        .expect("PPSC cancellation should resolve within its semantic timeout");
        let error = match result {
            Ok(_) => panic!("PPSC cancelled execution-unit admission must not receive a permit"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Cancelled));
        drop(held);

        let released = self
            .engine
            .tenant_engine_diagnostics(&tenant_id)
            .expect("PPSC released cancellation diagnostics should load")
            .mutation_isolate_admission;
        assert_eq!(released.concurrent_count, 0);
        assert_eq!(released.waiting_count, 0);
        assert_eq!(released.shed_count, before.shed_count);
    }

    pub(super) async fn force_publisher_overload(&self, tenant: &str) {
        let tenant_id = self.tenant(tenant).clone();
        let assignment_before = self
            .engine
            .write_log_assignment_for_testing(&tenant_id)
            .expect("PPSC overload assignment should read before saturation");
        let journal_before = self
            .engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("PPSC overload journal should read before saturation");
        let stats_before = self
            .engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("PPSC overload diagnostics should read before saturation");
        assert_eq!(stats_before.publisher_queue_capacity, 1);

        let pause = self
            .engine
            .ordered_publisher_pause_handle_for_testing(&tenant_id)
            .expect("PPSC overload publisher pause should load");
        pause.arm();
        let first = self
            .engine
            .enqueue_publisher_response_fence_for_testing(&tenant_id)
            .await
            .expect("PPSC first response fence should enter the publisher");
        let entered = tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(1))
        })
        .await;
        let entered = match entered {
            Ok(entered) => entered,
            Err(error) => {
                pause.release();
                panic!("PPSC publisher pause waiter should join: {error}");
            }
        };
        if !entered {
            pause.release();
            panic!("PPSC publisher did not reach its overload pause within one second");
        }
        let second = self
            .engine
            .enqueue_publisher_response_fence_for_testing(&tenant_id)
            .await;
        let second = match second {
            Ok(second) => second,
            Err(error) => {
                pause.release();
                panic!("PPSC second response fence should fill the publisher queue: {error}");
            }
        };
        let saturated = self.engine.mutation_journal_stats_for_testing(&tenant_id);
        let rejected = self
            .engine
            .enqueue_publisher_response_fence_for_testing(&tenant_id)
            .await;
        pause.release();
        let saturated = saturated.expect("PPSC saturated publisher diagnostics should load");

        let error = match rejected {
            Ok(_) => panic!("PPSC full publisher queue must reject the fence"),
            Err(error) => error,
        };
        assert!(
            matches!(error, Error::CommitterFull { capacity: 1, .. }),
            "PPSC overload must retain the publisher capacity: {error}"
        );
        assert_eq!(
            error.retryability(),
            nimbus_core::Retryability::RetryableAfterBackoff
        );
        assert_eq!(saturated.publisher_queue_depth, 1);
        timeout(Duration::from_secs(1), first)
            .await
            .expect("PPSC first response fence should drain after release")
            .expect("PPSC first response channel should remain open")
            .expect("PPSC first response fence should succeed");
        timeout(Duration::from_secs(1), second)
            .await
            .expect("PPSC second response fence should drain after release")
            .expect("PPSC second response channel should remain open")
            .expect("PPSC second response fence should succeed");

        assert_eq!(
            self.engine
                .write_log_assignment_for_testing(&tenant_id)
                .expect("PPSC overload assignment should read after rejection"),
            assignment_before,
            "response-only overload must not stage a sequence"
        );
        assert_eq!(
            self.engine
                .read_durable_journal(&tenant_id, SequenceNumber(0))
                .expect("PPSC overload journal should read after rejection"),
            journal_before,
            "response-only overload must not append a durable record"
        );
        assert_eq!(
            self.engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("PPSC overload diagnostics should read after rejection")
                .publisher_send_timeout_count,
            stats_before.publisher_send_timeout_count + 1
        );
    }
}
