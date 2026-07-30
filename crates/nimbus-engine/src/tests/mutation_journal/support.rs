use super::*;

pub(super) fn mutation_journal_progress_timeout() -> Duration {
    ci_or_local_duration(Duration::from_secs(1), Duration::from_secs(3))
}

pub(super) fn mutation_journal_pending_window() -> Duration {
    ci_or_local_duration(Duration::from_millis(100), Duration::from_millis(250))
}

pub(super) fn mutation_journal_catch_up_timeout() -> Duration {
    ci_or_local_duration(Duration::from_secs(3), Duration::from_secs(6))
}

pub(super) fn mutation_journal_poll_interval() -> Duration {
    ci_or_local_duration(Duration::from_millis(1), Duration::from_millis(5))
}

pub(super) async fn expect_blocking_wait_reaches_state<F>(description: &str, wait: F)
where
    F: FnOnce(Duration) -> bool + Send + 'static,
{
    let timeout_budget = mutation_journal_progress_timeout();
    let reached = tokio::task::spawn_blocking(move || wait(timeout_budget))
        .await
        .expect("blocking wait task should join successfully");
    assert!(
        reached,
        "{description} within the bounded state-transition timeout of {timeout_budget:?}"
    );
}

pub(super) async fn assert_future_stays_pending<T, F>(future: F, description: &str)
where
    F: Future<Output = T>,
{
    let pending_window = mutation_journal_pending_window();
    assert!(
        timeout(pending_window, future).await.is_err(),
        "{description} during the bounded pending window of {pending_window:?}"
    );
}

pub(super) async fn expect_future_within<T, F>(future: F, description: &str) -> T
where
    F: Future<Output = T>,
{
    let timeout_budget = mutation_journal_progress_timeout();
    timeout(timeout_budget, future).await.unwrap_or_else(|_| {
        panic!("{description} within the bounded state-transition timeout of {timeout_budget:?}")
    })
}

pub(super) async fn expect_catch_up_future_within<T, F>(future: F, description: &str) -> T
where
    F: Future<Output = T>,
{
    let timeout_budget = mutation_journal_catch_up_timeout();
    timeout(timeout_budget, future).await.unwrap_or_else(|_| {
        panic!("{description} within the bounded state-transition timeout of {timeout_budget:?}")
    })
}

/// Loads a tenant schema across the runtime-restart window that a
/// crash-and-replay outcome opens.
///
/// `TenantLifecycle::operation_rejection_if_deleted` answers `Unavailable`
/// while durable-recovery eviction is in flight so an admission race cannot
/// become a false 404, and `Error::retryability` classifies that answer
/// `RetryableAfterBackoff`. A caller that reaches the tenant inside the window
/// is therefore required to retry. Only that refusal is retried here: every
/// other error fails the caller immediately, and exhausting the budget fails
/// it too.
pub(super) async fn load_schema_across_runtime_restart(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    description: &str,
) -> nimbus_core::Schema {
    let timeout_budget = mutation_journal_catch_up_timeout();
    let deadline = std::time::Instant::now() + timeout_budget;
    loop {
        let error = match engine.get_schema_async(tenant_id.clone()).await {
            Ok(schema) => return schema,
            Err(error) => error,
        };
        assert!(
            matches!(
                &error,
                nimbus_core::Error::Storage {
                    kind: nimbus_core::StorageErrorKind::Unavailable,
                    message,
                } if message.contains("restarting after durable recovery")
            ),
            "{description}: {error}"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "{description} within the bounded runtime-restart timeout of {timeout_budget:?}"
        );
        tokio::time::sleep(mutation_journal_poll_interval()).await;
    }
}

/// Waits for a crash-and-replay outcome to install a replacement runtime and
/// returns its identity.
///
/// Both reads below can legitimately refuse while the replacement is being
/// installed. `get_schema_async` answers the transient `Unavailable` described
/// on [`load_schema_across_runtime_restart`], and the blocking
/// `tenant_runtime_identity_for_testing` hook reaches
/// `Engine::require_embedded_provider_kind` whenever the tenant is absent from
/// the registry — the exact window between deregistering the failed runtime and
/// registering its successor — which answers `InvalidInput` on a simulation
/// engine. Neither answer means the replacement failed, only that it is not
/// observable yet, so both are polled rather than fatal.
///
/// This does not soften what the caller proves: passing still requires reading
/// a schema successfully *and* observing an identity that differs from
/// `identity_before`. A replacement that never arrives exhausts the budget and
/// fails with the last refusal attached.
pub(super) async fn wait_for_replacement_runtime_identity(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    identity_before: u64,
    description: &str,
) -> u64 {
    let timeout_budget = mutation_journal_catch_up_timeout();
    let deadline = std::time::Instant::now() + timeout_budget;
    let mut last_refusal: String;
    loop {
        load_schema_across_runtime_restart(engine, tenant_id, description).await;
        match engine.tenant_runtime_identity_for_testing(tenant_id) {
            Ok(identity) if identity != identity_before => return identity,
            Ok(_) => last_refusal = "the runtime identity never changed".to_string(),
            Err(error) => last_refusal = error.to_string(),
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{description} within the bounded runtime-restart timeout of \
             {timeout_budget:?}: {last_refusal}"
        );
        tokio::time::sleep(mutation_journal_poll_interval()).await;
    }
}

pub(super) fn new_faulted_engine(
    timestamp: u64,
) -> (TempDir, Arc<Engine>, TenantId, Arc<BlockingFaultInjector>) {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(timestamp))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    (data_dir, engine, tenant_id, faults)
}
