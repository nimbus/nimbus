use nimbus_storage::NoopFaultInjector;

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn embedded_stores_never_enter_the_committer_lease_lifecycle() {
    for backend in [EmbeddedProviderKind::Redb, EmbeddedProviderKind::Sqlite] {
        let data_dir = tempdir().expect("embedded tempdir should build");
        let engine = Arc::new(
            Engine::new_with_simulation_and_embedded_provider(
                data_dir.path(),
                Arc::new(ManualClock::new(Timestamp(10_000))),
                Arc::new(NoopFaultInjector),
                backend,
            )
            .expect("embedded engine should create"),
        );
        let tenant_id =
            TenantId::new(format!("lease-free-{backend:?}")).expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let loaded = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("loaded stats should read");
        assert!(!loaded.committer_lease_acquired);
        assert_eq!(loaded.committer_lease_acquire_count, 0);
        assert!(!loaded.committer_lease_renewal_worker_running);

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("embedded"))]),
            )
            .await
            .expect("embedded assignment must bypass unsupported lease operations");

        let mutated = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("mutated stats should read");
        assert!(!mutated.committer_lease_acquired);
        assert_eq!(mutated.committer_lease_epoch, 0);
        assert_eq!(mutated.committer_lease_acquire_count, 0);
        assert_eq!(mutated.committer_lease_renewal_count, 0);
        assert_eq!(mutated.committer_lease_renewal_failure_count, 0);
        assert!(!mutated.committer_lease_renewal_worker_running);

        engine.quiesce().await;
    }
}
