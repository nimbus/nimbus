use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn postgres_committer_lease_transitions_use_provider_time_and_fence_stale_owners() {
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
async fn postgres_committer_lease_concurrent_acquire_has_exactly_one_winner() {
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

#[tokio::test(flavor = "multi_thread")]
async fn postgres_fenced_durable_apply_contract_is_atomic() {
    with_test_provider(|provider, _config| async move {
        for (tenant_name, exercise) in [
            (
                "fenced-happy",
                super::super::exercise_fenced_durable_apply_happy_path as fn(&_, &str),
            ),
            (
                "fenced-rollback",
                super::super::exercise_fenced_durable_apply_total_rollback,
            ),
            (
                "fenced-expired",
                super::super::exercise_fenced_durable_apply_expired,
            ),
            (
                "fenced-gap",
                super::super::exercise_fenced_durable_apply_sequence_gap,
            ),
            (
                "fenced-prefix",
                super::super::exercise_fenced_durable_apply_prefix_guard,
            ),
        ] {
            let tenant = TenantId::new(tenant_name).expect("tenant id should build");
            let opened = provider
                .create_opened_tenant(&tenant)
                .await
                .expect("tenant should create and open");
            exercise(opened.store.as_ref(), tenant_name);
        }
    })
    .await;
}
