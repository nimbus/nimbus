use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_committer_lease_transitions_use_provider_time_and_fence_stale_owners() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("lease-transitions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_committer_lease_transitions(opened.store.as_ref());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_committer_lease_concurrent_acquire_has_exactly_one_winner() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("lease-concurrent").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        super::super::exercise_concurrent_committer_lease_acquire(opened.store.as_ref().clone());
    })
    .await;
}
