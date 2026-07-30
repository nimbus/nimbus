//! Test-only selection policy for external provider fixtures.
//!
//! Docker lifecycle belongs to `scripts/external-provider-fixture.sh`. Rust
//! provider tests either consume the explicit URLs exported by that interface,
//! deliberately omit provider execution in an ordinary workspace lane, or fail
//! with an actionable command. They never provision an implicit container.

use std::env;
use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "libsql")]
use libsql::Builder;
#[cfg(feature = "mysql")]
use mysql_async::prelude::Queryable;
#[cfg(feature = "mysql")]
use mysql_async::{Opts, Pool};
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use nimbus_core::Error;
#[cfg(feature = "libsql")]
use nimbus_core::StorageErrorKind;
use nimbus_core::{Result, TenantId};
#[cfg(feature = "postgres")]
use tokio_postgres::NoTls;

#[cfg(feature = "libsql")]
use crate::libsql::libsql_transport_connector;
#[cfg(feature = "libsql")]
use crate::{LibsqlReplicaProvider, LibsqlReplicaProviderConfig};
#[cfg(feature = "mysql")]
use crate::{MySqlProvider, MySqlProviderConfig};
#[cfg(feature = "postgres")]
use crate::{PostgresProvider, PostgresProviderConfig};

pub const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";
pub const DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProviderFixtureMode {
    UseExplicit,
    Omit,
}

/// Provider-authoritative lease-time control used only by conformance tests.
///
/// Implementations change the provider-owned expiry value and nothing else.
/// Lease acquisition, renewal, fencing, takeover, and reconciliation continue
/// through the production Engine and storage interfaces. The boxed future keeps
/// this test seam dependency-free and object-safe for shared scenario runners.
pub trait ProviderLeaseTimeControl: Send + Sync {
    fn provider_name(&self) -> &'static str;

    fn expire_lease<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

#[cfg(feature = "postgres")]
#[derive(Clone)]
pub struct PostgresLeaseTimeControl {
    config: PostgresProviderConfig,
}

#[cfg(feature = "postgres")]
impl PostgresLeaseTimeControl {
    pub fn new(config: PostgresProviderConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "postgres")]
impl ProviderLeaseTimeControl for PostgresLeaseTimeControl {
    fn provider_name(&self) -> &'static str {
        "postgres"
    }

    fn expire_lease<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let provider = PostgresProvider::connect(self.config.clone()).await?;
            let schema_name = provider.tenant_schema_name(tenant_id)?;
            let (client, connection) =
                tokio_postgres::connect(&self.config.connection_string, NoTls)
                    .await
                    .map_err(internal_error)?;
            let connection_task = tokio::spawn(connection);
            let query = format!(
                "UPDATE \"{schema_name}\".\"committer_lease\" \
                 SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second' \
                 WHERE singleton = TRUE"
            );
            let result = client.execute(query.as_str(), &[]).await;
            connection_task.abort();
            let updated = result.map_err(internal_error)?;
            require_single_expired_row("PostgreSQL", tenant_id, updated)
        })
    }
}

#[cfg(feature = "mysql")]
#[derive(Clone)]
pub struct MySqlLeaseTimeControl {
    config: MySqlProviderConfig,
}

#[cfg(feature = "mysql")]
impl MySqlLeaseTimeControl {
    pub fn new(config: MySqlProviderConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "mysql")]
impl ProviderLeaseTimeControl for MySqlLeaseTimeControl {
    fn provider_name(&self) -> &'static str {
        "mysql"
    }

    fn expire_lease<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let provider = MySqlProvider::connect(self.config.clone()).await?;
            let database_name = provider.tenant_database_name(tenant_id)?;
            let options = Opts::from_url(&self.config.connection_string).map_err(internal_error)?;
            let pool = Pool::new(options);
            let mut connection = pool.get_conn().await.map_err(internal_error)?;
            let statement = format!(
                "UPDATE `{database_name}`.`committer_lease` \
                 SET expires_at = TIMESTAMPADD(SECOND, -1, CURRENT_TIMESTAMP(6)) \
                 WHERE singleton = TRUE"
            );
            connection
                .query_drop(statement)
                .await
                .map_err(internal_error)?;
            let updated = connection
                .query_first::<u64, _>("SELECT ROW_COUNT()")
                .await
                .map_err(internal_error)?
                .ok_or_else(|| Error::Internal("MySQL expiry row count was absent".to_string()))?;
            connection.disconnect().await.map_err(internal_error)?;
            pool.disconnect().await.map_err(internal_error)?;
            require_single_expired_row("MySQL", tenant_id, updated)
        })
    }
}

#[cfg(feature = "libsql")]
#[derive(Clone)]
pub struct LibsqlLeaseTimeControl {
    config: LibsqlReplicaProviderConfig,
}

#[cfg(feature = "libsql")]
impl LibsqlLeaseTimeControl {
    pub fn new(config: LibsqlReplicaProviderConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "libsql")]
impl ProviderLeaseTimeControl for LibsqlLeaseTimeControl {
    fn provider_name(&self) -> &'static str {
        "libsql"
    }

    fn expire_lease<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let provider = LibsqlReplicaProvider::connect(self.config.clone()).await?;
            let namespace = provider.tenant_namespace(tenant_id)?;
            let database = Builder::new_remote(
                self.config.primary_url.clone(),
                self.config.auth_token.clone().unwrap_or_default(),
            )
            .namespace(namespace)
            .connector(libsql_transport_connector()?)
            .build()
            .await
            .map_err(storage_error)?;
            let connection = database.connect().map_err(storage_error)?;
            let updated = connection
                .execute(
                    "UPDATE committer_lease \
                     SET expires_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) - 1000 \
                     WHERE singleton = 1",
                    (),
                )
                .await
                .map_err(storage_error)?;
            require_single_expired_row("libSQL", tenant_id, updated)
        })
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
fn require_single_expired_row(provider: &str, tenant_id: &TenantId, updated: u64) -> Result<()> {
    if updated == 1 {
        Ok(())
    } else {
        Err(Error::Internal(format!(
            "expected one {provider} committer lease row to expire for tenant {tenant_id}, updated {updated}"
        )))
    }
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
fn internal_error(error: impl std::fmt::Display) -> Error {
    Error::Internal(error.to_string())
}

#[cfg(feature = "libsql")]
fn storage_error(error: impl std::fmt::Display) -> Error {
    Error::storage(StorageErrorKind::Other, error.to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureInputDecision {
    UseExplicit,
    Omit,
    Reject,
}

fn classify_fixture_inputs(
    required_env_present: &[bool],
    fixtures_required: bool,
    fixtures_disabled: bool,
) -> FixtureInputDecision {
    if required_env_present.iter().all(|present| *present) {
        return FixtureInputDecision::UseExplicit;
    }

    let any_present = required_env_present.iter().any(|present| *present);
    if any_present || fixtures_required || !fixtures_disabled {
        return FixtureInputDecision::Reject;
    }

    FixtureInputDecision::Omit
}

fn nonempty_env(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// Select the only legal fixture mode for provider-backed tests.
///
/// `UseExplicit` means every required URL is present. `Omit` is reserved for
/// ordinary workspace lanes that explicitly disable external providers. Every
/// other configuration fails; in particular, a direct provider test can no
/// longer start a drifting Testcontainers image or silently skip itself.
pub fn external_provider_fixture_mode(
    provider: &str,
    provider_label: &str,
    required_env_names: &[&str],
) -> ExternalProviderFixtureMode {
    let required_env_present: Vec<bool> = required_env_names
        .iter()
        .map(|name| nonempty_env(name))
        .collect();
    let fixtures_required = env::var_os(REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some();
    let fixtures_disabled = env::var_os(DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some();

    match classify_fixture_inputs(&required_env_present, fixtures_required, fixtures_disabled) {
        FixtureInputDecision::UseExplicit => ExternalProviderFixtureMode::UseExplicit,
        FixtureInputDecision::Omit => {
            eprintln!(
                "omitting {provider_label} execution because {DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV}=1; this workspace result is not external-provider evidence"
            );
            ExternalProviderFixtureMode::Omit
        }
        FixtureInputDecision::Reject => {
            let missing: Vec<&str> = required_env_names
                .iter()
                .copied()
                .zip(required_env_present)
                .filter_map(|(name, present)| (!present).then_some(name))
                .collect();
            panic!(
                "{provider_label} tests require the pinned shared fixture; missing non-empty environment variable(s): {}. Run `make test-external-provider PROVIDER={provider}`. Ordinary workspace lanes that intentionally omit provider execution must set {DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV}=1. Automatic per-test containers are not supported.",
                missing.join(", ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_input_decision_table_is_exhaustive() {
        for (present, required, disabled, expected) in [
            (vec![true], false, false, FixtureInputDecision::UseExplicit),
            (vec![true], true, true, FixtureInputDecision::UseExplicit),
            (vec![false], false, true, FixtureInputDecision::Omit),
            (vec![false], false, false, FixtureInputDecision::Reject),
            (vec![false], true, false, FixtureInputDecision::Reject),
            (vec![false], true, true, FixtureInputDecision::Reject),
            (vec![true, false], false, true, FixtureInputDecision::Reject),
            (vec![false, true], false, true, FixtureInputDecision::Reject),
        ] {
            assert_eq!(
                classify_fixture_inputs(&present, required, disabled),
                expected,
                "unexpected decision for present={present:?}, required={required}, disabled={disabled}"
            );
        }
    }

    /// Guards the adapter set only when all three providers are compiled in;
    /// the claim it makes ("three real adapters") is not meaningful in a
    /// single-provider build.
    #[cfg(all(feature = "libsql", feature = "mysql", feature = "postgres"))]
    #[test]
    fn provider_lease_time_control_has_three_real_adapters() {
        let controls: Vec<Box<dyn ProviderLeaseTimeControl>> = vec![
            Box::new(PostgresLeaseTimeControl::new(PostgresProviderConfig::new(
                "postgresql://localhost/nimbus",
            ))),
            Box::new(MySqlLeaseTimeControl::new(MySqlProviderConfig::new(
                "mysql://localhost/nimbus",
            ))),
            Box::new(LibsqlLeaseTimeControl::new(
                LibsqlReplicaProviderConfig::new(
                    "http://localhost:8080",
                    "http://localhost:8080",
                    std::path::PathBuf::from("unused-provider-lease-cache"),
                ),
            )),
        ];

        assert_eq!(
            controls
                .iter()
                .map(|control| control.provider_name())
                .collect::<Vec<_>>(),
            ["postgres", "mysql", "libsql"]
        );
    }
}
