use nimbus_storage::provider_test_fixtures::PostgresLeaseTimeControl;

use super::support::{with_postgres_engine_config, with_shared_postgres_engine_configs};
use super::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_ppsc_seeded_journal_differential() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        exercise_ppsc_provider_retained_differential(
            PpscBackend::Postgres,
            engine_config,
            Arc::new(PostgresLeaseTimeControl::new(provider_config)),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn ppsc_provider_takeover_extension_matches_postgres_mysql_and_libsql() {
    with_shared_postgres_engine_configs(
        |first_config, takeover_config, provider_config| async move {
            exercise_ppsc_provider_authority_extension(
                PpscBackend::Postgres,
                first_config,
                takeover_config,
                Arc::new(PostgresLeaseTimeControl::new(provider_config)),
            )
            .await;
        },
    )
    .await;
}
