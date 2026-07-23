use nimbus_testing::ppsc::PpscScenario;

use super::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_ppsc_seeded_journal_differential() {
    with_libsql_replica_engine_config(|engine_config, provider_config| async move {
        exercise_ppsc_provider_retained_differential(
            PpscBackend::Libsql,
            engine_config,
            Arc::new(LibsqlLeaseTimeControl::new(provider_config)),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_ppsc_seed_41_diagnostic() {
    with_libsql_replica_engine_config(|engine_config, provider_config| async move {
        exercise_ppsc_provider_scenario_differential(
            PpscBackend::Libsql,
            engine_config,
            Arc::new(LibsqlLeaseTimeControl::new(provider_config)),
            PpscScenario::seeded(41, 32).expect("diagnostic seed should build"),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_ppsc_seed_97_diagnostic() {
    with_libsql_replica_engine_config(|engine_config, provider_config| async move {
        exercise_ppsc_provider_scenario_differential(
            PpscBackend::Libsql,
            engine_config,
            Arc::new(LibsqlLeaseTimeControl::new(provider_config)),
            PpscScenario::seeded(97, 32).expect("diagnostic seed should build"),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn ppsc_provider_takeover_extension_matches_postgres_mysql_and_libsql() {
    with_shared_libsql_replica_engine_configs(
        |first_config, takeover_config, provider_config| async move {
            exercise_ppsc_provider_authority_extension(
                PpscBackend::Libsql,
                first_config,
                takeover_config,
                Arc::new(LibsqlLeaseTimeControl::new(provider_config)),
            )
            .await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_provider_publisher_contract() {
    with_libsql_replica_engine_config(|engine_config, _provider_config| async move {
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("libSQL-backed engine should create"),
        );
        exercise_provider_publisher_contract(
            engine,
            TenantId::new("libsql-publisher-contract").expect("tenant id should build"),
            None,
        )
        .await;
    })
    .await;
}
