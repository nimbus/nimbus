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

pub(super) fn new_faulted_service(
    timestamp: u64,
) -> (TempDir, Arc<Service>, TenantId, Arc<BlockingFaultInjector>) {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(timestamp))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    (data_dir, service, tenant_id, faults)
}
