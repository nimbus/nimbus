use super::support::with_postgres_engine_config;
use super::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_ppsc_seeded_three_route_differential() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        exercise_ppsc_provider_three_route_differential(PpscBackend::Postgres, engine_config).await;
    })
    .await;
}
