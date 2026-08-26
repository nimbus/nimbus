use std::path::PathBuf;
use std::sync::Arc;

use nimbus_core::{Error, IdSource, Result, WallClock};
use nimbus_crypto::LocalKeyProvider;
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryTenantProvider;
use nimbus_storage::{
    EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider, EmbeddedRedbProvider,
    EmbeddedSqliteProvider, FaultInjector,
};
#[cfg(feature = "libsql")]
use nimbus_storage::{LibsqlReplicaProvider, LibsqlReplicaProviderConfig};
#[cfg(feature = "mysql")]
use nimbus_storage::{MySqlProvider, MySqlProviderConfig};
#[cfg(feature = "postgres")]
use nimbus_storage::{PostgresProvider, PostgresProviderConfig};

use super::process_fence::EngineProcessFence;
use super::{BackgroundExecutor, Engine, EngineBootstrapParts, encryption};
use crate::persistence::{ControlPlaneProvider, PersistenceProvider};
use crate::persistence_config::{
    ControlPlaneBootstrapPlan, EmbeddedTenantBootstrapPlan, EngineBootstrapPlan,
    EnginePersistenceConfig, LibsqlReplicaTenantBootstrapPlan, MetadataRetentionProfile,
    MySqlTenantBootstrapPlan, PostgresTenantBootstrapPlan, TenantProviderBootstrapPlan,
};

struct EngineSimulationSeams {
    clock: Arc<dyn WallClock>,
    id_source: Arc<dyn IdSource>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    metadata_retention: MetadataRetentionProfile,
    #[cfg(feature = "libsql")]
    libsql_replica_fault_injector: Option<Arc<dyn FaultInjector>>,
}

pub(super) async fn build_from_persistence_config(
    config: EnginePersistenceConfig,
    clock: Arc<dyn WallClock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
) -> Result<Engine> {
    build_from_persistence_config_with_libsql_replica_faults(
        config,
        clock,
        storage_fault_injector,
        None,
        id_source,
    )
    .await
}

pub(super) async fn build_from_persistence_config_with_libsql_replica_faults(
    config: EnginePersistenceConfig,
    clock: Arc<dyn WallClock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    libsql_replica_fault_injector: Option<Arc<dyn FaultInjector>>,
    id_source: Arc<dyn IdSource>,
) -> Result<Engine> {
    config.metadata_retention.validate()?;
    let metadata_retention = config.metadata_retention;
    let key_provider = encryption::initialize_encryption(&config)?;
    let encryption_status = encryption::EncryptionStatus::from_config(&config);
    let plan = config.bootstrap_plan()?;
    let encryption_provider = key_provider
        .as_ref()
        .map(encryption::InitializedKeyProvider::provider);

    #[cfg(not(feature = "libsql"))]
    let _ = libsql_replica_fault_injector;
    let simulation = EngineSimulationSeams {
        clock,
        id_source,
        storage_fault_injector,
        metadata_retention,
        #[cfg(feature = "libsql")]
        libsql_replica_fault_injector,
    };

    build_from_plan(
        plan,
        encryption_provider,
        simulation,
        Some(encryption_status),
    )
    .await
}

pub(super) fn build_embedded_engine(
    tenant_data_dir: PathBuf,
    control_data_dir: PathBuf,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn WallClock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
    embedded_provider_kind: EmbeddedProviderKind,
) -> Result<Engine> {
    let simulation = EngineSimulationSeams {
        clock,
        id_source,
        storage_fault_injector,
        metadata_retention: MetadataRetentionProfile::shipped(),
        #[cfg(feature = "libsql")]
        libsql_replica_fault_injector: None,
    };
    build_embedded_from_plan(
        tenant_data_dir.clone(),
        control_data_dir,
        EmbeddedTenantBootstrapPlan {
            provider_kind: embedded_provider_kind,
            data_dir: tenant_data_dir,
        },
        encryption_provider,
        simulation,
        None,
    )
}

#[cfg(any(test, feature = "test-hooks"))]
pub(super) fn build_memory_engine(
    data_dir: PathBuf,
    clock: Arc<dyn WallClock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
) -> Result<Engine> {
    let process_fence = EngineProcessFence::acquire([data_dir.clone()])?;
    let (engine_executor, storage_executor) = build_executors()?;
    let control_plane_provider =
        build_control_plane_provider(data_dir.clone(), None, &storage_executor)?;
    let persistence_provider =
        PersistenceProvider::Memory(Arc::new(MemoryTenantProvider::new_with_id_source(
            clock.clone(),
            storage_fault_injector.clone(),
            storage_executor.handle(),
            id_source.clone(),
        )));

    Ok(Engine::from_bootstrap_parts(EngineBootstrapParts {
        data_dir,
        embedded_provider_kind: None,
        persistence_provider,
        control_plane_provider,
        clock,
        id_source,
        storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status: None,
        metadata_retention: MetadataRetentionProfile::shipped(),
        process_fence,
    }))
}

async fn build_from_plan(
    plan: EngineBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    simulation: EngineSimulationSeams,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    let EngineBootstrapPlan {
        engine_data_dir,
        control_plane,
        tenant_provider,
    } = plan;
    let control_plane_data_dir = match control_plane {
        ControlPlaneBootstrapPlan::EmbeddedRedb { data_dir } => data_dir,
    };

    match tenant_provider {
        TenantProviderBootstrapPlan::Embedded(plan) => build_embedded_from_plan(
            engine_data_dir,
            control_plane_data_dir,
            plan,
            encryption_provider,
            simulation,
            encryption_status,
        ),
        TenantProviderBootstrapPlan::Postgres(plan) => {
            build_postgres_from_plan(
                engine_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                simulation,
                encryption_status,
            )
            .await
        }
        TenantProviderBootstrapPlan::LibsqlReplica(plan) => {
            build_libsql_replica_from_plan(
                engine_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                simulation,
                encryption_status,
            )
            .await
        }
        TenantProviderBootstrapPlan::MySql(plan) => {
            build_mysql_from_plan(
                engine_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                simulation,
                encryption_status,
            )
            .await
        }
    }
}

fn build_embedded_from_plan(
    engine_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: EmbeddedTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    simulation: EngineSimulationSeams,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    let process_fence = EngineProcessFence::acquire([
        engine_data_dir.clone(),
        control_data_dir.clone(),
        plan.data_dir.clone(),
    ])?;

    let (engine_executor, storage_executor) = build_executors()?;
    let control_plane_provider = build_control_plane_provider(
        control_data_dir,
        encryption_provider.clone(),
        &storage_executor,
    )?;
    let persistence_provider = match plan.provider_kind {
        EmbeddedProviderKind::Redb => {
            let provider = if let Some(provider) = encryption_provider {
                EmbeddedRedbProvider::new_encrypted_with_id_source(
                    plan.data_dir.clone(),
                    provider,
                    simulation.clock.clone(),
                    simulation.storage_fault_injector.clone(),
                    storage_executor.handle(),
                    simulation.id_source.clone(),
                )?
            } else {
                EmbeddedRedbProvider::new_with_id_source(
                    plan.data_dir.clone(),
                    simulation.clock.clone(),
                    simulation.storage_fault_injector.clone(),
                    storage_executor.handle(),
                    simulation.id_source.clone(),
                )?
            };
            PersistenceProvider::Redb(Arc::new(provider))
        }
        EmbeddedProviderKind::Sqlite => {
            let provider = if let Some(provider) = encryption_provider {
                EmbeddedSqliteProvider::new_encrypted_with_id_source(
                    plan.data_dir.clone(),
                    provider,
                    simulation.clock.clone(),
                    simulation.storage_fault_injector.clone(),
                    storage_executor.handle(),
                    simulation.id_source.clone(),
                )?
            } else {
                EmbeddedSqliteProvider::new_with_id_source(
                    plan.data_dir.clone(),
                    simulation.clock.clone(),
                    simulation.storage_fault_injector.clone(),
                    storage_executor.handle(),
                    simulation.id_source.clone(),
                )?
            };
            PersistenceProvider::Sqlite(Arc::new(provider))
        }
    };

    Ok(Engine::from_bootstrap_parts(EngineBootstrapParts {
        data_dir: engine_data_dir,
        embedded_provider_kind: Some(plan.provider_kind),
        persistence_provider,
        control_plane_provider,
        clock: simulation.clock,
        id_source: simulation.id_source,
        storage_fault_injector: simulation.storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
        metadata_retention: simulation.metadata_retention,
        process_fence,
    }))
}

/// Builds the PostgreSQL-backed engine. Paired with the `not(feature)`
/// definition below so the dispatch `match` in
/// [`build_from_plan`] stays exhaustive and provider
/// selection cannot silently degrade: an uncompiled provider is an error, never
/// a fallback to the embedded store.
#[cfg(feature = "postgres")]
async fn build_postgres_from_plan(
    engine_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: PostgresTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    simulation: EngineSimulationSeams,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    let process_fence =
        EngineProcessFence::acquire([engine_data_dir.clone(), control_data_dir.clone()])?;
    let (engine_executor, storage_executor) = build_executors()?;
    let control_plane_provider =
        build_control_plane_provider(control_data_dir, encryption_provider, &storage_executor)?;
    let provider_config = PostgresProviderConfig {
        connection_string: plan.connection_string,
        metadata_schema: plan.metadata_schema,
        tenant_schema_prefix: plan.tenant_schema_prefix,
        min_connections: plan.pool.min_connections,
        max_connections: plan.pool.max_connections,
    };
    let postgres_provider = Arc::new(
        PostgresProvider::connect_with_simulation_and_id_source(
            provider_config,
            storage_executor.handle(),
            simulation.clock.clone(),
            simulation.storage_fault_injector.clone(),
            simulation.id_source.clone(),
        )
        .await?,
    );

    Ok(Engine::from_bootstrap_parts(EngineBootstrapParts {
        data_dir: engine_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::Postgres(postgres_provider),
        control_plane_provider,
        clock: simulation.clock,
        id_source: simulation.id_source,
        storage_fault_injector: simulation.storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
        metadata_retention: simulation.metadata_retention,
        process_fence,
    }))
}

/// Builds the libSQL-replica-backed engine; see `build_postgres_from_plan` for
/// the paired-definition contract.
#[cfg(feature = "libsql")]
async fn build_libsql_replica_from_plan(
    engine_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: LibsqlReplicaTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    simulation: EngineSimulationSeams,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    let process_fence = EngineProcessFence::acquire([
        engine_data_dir.clone(),
        control_data_dir.clone(),
        plan.replica_cache_dir.clone(),
    ])?;
    let (engine_executor, storage_executor) = build_executors()?;
    let control_plane_provider = build_control_plane_provider(
        control_data_dir,
        encryption_provider.clone(),
        &storage_executor,
    )?;
    let provider_config = LibsqlReplicaProviderConfig {
        primary_url: plan.primary_url,
        auth_token: plan.auth_token,
        admin_api_url: plan.admin_api_url,
        admin_auth_header: plan.admin_auth_header,
        metadata_namespace: plan.metadata_namespace,
        tenant_namespace_prefix: plan.tenant_namespace_prefix,
        replica_cache_dir: plan.replica_cache_dir,
        encryption_provider,
    };
    let replica_fault_injector = simulation
        .libsql_replica_fault_injector
        .clone()
        .unwrap_or_else(|| simulation.storage_fault_injector.clone());
    let libsql_replica_provider = Arc::new(
        LibsqlReplicaProvider::connect_with_simulation_faults_and_id_source(
            provider_config,
            storage_executor.handle(),
            simulation.clock.clone(),
            simulation.storage_fault_injector.clone(),
            replica_fault_injector,
            simulation.id_source.clone(),
        )
        .await?,
    );

    Ok(Engine::from_bootstrap_parts(EngineBootstrapParts {
        data_dir: engine_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::LibsqlReplica(libsql_replica_provider),
        control_plane_provider,
        clock: simulation.clock,
        id_source: simulation.id_source,
        storage_fault_injector: simulation.storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
        metadata_retention: simulation.metadata_retention,
        process_fence,
    }))
}

/// Builds the MySQL-backed engine; see `build_postgres_from_plan` for the
/// paired-definition contract.
#[cfg(feature = "mysql")]
async fn build_mysql_from_plan(
    engine_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: MySqlTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    simulation: EngineSimulationSeams,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    let process_fence =
        EngineProcessFence::acquire([engine_data_dir.clone(), control_data_dir.clone()])?;
    let (engine_executor, storage_executor) = build_executors()?;
    let control_plane_provider =
        build_control_plane_provider(control_data_dir, encryption_provider, &storage_executor)?;
    let provider_config = MySqlProviderConfig {
        connection_string: plan.connection_string,
        metadata_database: plan.metadata_database,
        tenant_database_prefix: plan.tenant_database_prefix,
        min_connections: plan.pool.min_connections,
        max_connections: plan.pool.max_connections,
    };
    let mysql_provider = Arc::new(
        MySqlProvider::connect_with_simulation_and_id_source(
            provider_config,
            storage_executor.handle(),
            simulation.clock.clone(),
            simulation.storage_fault_injector.clone(),
            simulation.id_source.clone(),
        )
        .await?,
    );

    Ok(Engine::from_bootstrap_parts(EngineBootstrapParts {
        data_dir: engine_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::MySql(mysql_provider),
        control_plane_provider,
        clock: simulation.clock,
        id_source: simulation.id_source,
        storage_fault_injector: simulation.storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
        metadata_retention: simulation.metadata_retention,
        process_fence,
    }))
}

#[cfg(not(feature = "postgres"))]
async fn build_postgres_from_plan(
    _engine_data_dir: PathBuf,
    _control_data_dir: PathBuf,
    _plan: PostgresTenantBootstrapPlan,
    _encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    _simulation: EngineSimulationSeams,
    _encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    Err(Error::InvalidInput(
        "postgres support is not enabled in this build; rebuild with the postgres feature"
            .to_string(),
    ))
}

#[cfg(not(feature = "libsql"))]
async fn build_libsql_replica_from_plan(
    _engine_data_dir: PathBuf,
    _control_data_dir: PathBuf,
    _plan: LibsqlReplicaTenantBootstrapPlan,
    _encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    _simulation: EngineSimulationSeams,
    _encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    Err(Error::InvalidInput(
        "libsql support is not enabled in this build; rebuild with the libsql feature".to_string(),
    ))
}

#[cfg(not(feature = "mysql"))]
async fn build_mysql_from_plan(
    _engine_data_dir: PathBuf,
    _control_data_dir: PathBuf,
    _plan: MySqlTenantBootstrapPlan,
    _encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    _simulation: EngineSimulationSeams,
    _encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Engine> {
    Err(Error::InvalidInput(
        "mysql support is not enabled in this build; rebuild with the mysql feature".to_string(),
    ))
}

fn build_executors() -> Result<(BackgroundExecutor, BackgroundExecutor)> {
    Ok((
        BackgroundExecutor::new("nimbus-engine-bg", 2).map_err(internal_error)?,
        BackgroundExecutor::new("nimbus-storage-bg", 1).map_err(internal_error)?,
    ))
}

fn build_control_plane_provider(
    control_data_dir: PathBuf,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    storage_executor: &BackgroundExecutor,
) -> Result<ControlPlaneProvider> {
    Ok(ControlPlaneProvider::EmbeddedRedb(Arc::new(
        if let Some(provider) = encryption_provider {
            EmbeddedRedbControlPlaneProvider::new_encrypted(
                control_data_dir,
                provider,
                storage_executor.handle(),
            )?
        } else {
            EmbeddedRedbControlPlaneProvider::new(control_data_dir, storage_executor.handle())?
        },
    )))
}

fn internal_error(error: std::io::Error) -> Error {
    Error::Internal(error.to_string())
}

/// Pins the "uncompiled provider fails loudly" contract for PostgreSQL.
///
/// This module exists only in a build without the feature, which is the only
/// build where the contract is observable. It is the negative half of the
/// paired `build_postgres_from_plan` definitions above: the positive half is
/// covered by the live-fixture provider suite.
#[cfg(all(test, not(feature = "postgres")))]
mod postgres_disabled_tests {
    use super::*;
    use crate::persistence_config::EnginePersistenceConfig;

    #[tokio::test]
    async fn postgres_config_is_rejected_rather_than_served_by_an_embedded_engine() {
        let data_dir = tempfile::tempdir().expect("control-plane data dir should build");
        let error = Engine::new_with_persistence_config(EnginePersistenceConfig::postgres(
            data_dir.path(),
            "postgres://nimbus@127.0.0.1:5432/nimbus",
        ))
        .await
        .err()
        .expect("a postgres config must not build an engine without the postgres feature");
        assert!(
            matches!(
                &error,
                Error::InvalidInput(message)
                    if message == "postgres support is not enabled in this build; rebuild with the postgres feature"
            ),
            "expected the postgres build-support rejection, got {error:?}"
        );
    }
}

/// Pins the "uncompiled provider fails loudly" contract for MySQL; see
/// `postgres_disabled_tests` for the shape.
#[cfg(all(test, not(feature = "mysql")))]
mod mysql_disabled_tests {
    use super::*;
    use crate::persistence_config::EnginePersistenceConfig;

    #[tokio::test]
    async fn mysql_config_is_rejected_rather_than_served_by_an_embedded_engine() {
        let data_dir = tempfile::tempdir().expect("control-plane data dir should build");
        let error = Engine::new_with_persistence_config(EnginePersistenceConfig::mysql(
            data_dir.path(),
            "mysql://nimbus@127.0.0.1:3306/nimbus",
        ))
        .await
        .err()
        .expect("a mysql config must not build an engine without the mysql feature");
        assert!(
            matches!(
                &error,
                Error::InvalidInput(message)
                    if message == "mysql support is not enabled in this build; rebuild with the mysql feature"
            ),
            "expected the mysql build-support rejection, got {error:?}"
        );
    }
}

/// Pins the "uncompiled provider fails loudly" contract for the libSQL replica;
/// see `postgres_disabled_tests` for the shape.
#[cfg(all(test, not(feature = "libsql")))]
mod libsql_disabled_tests {
    use super::*;
    use crate::persistence_config::EnginePersistenceConfig;

    #[tokio::test]
    async fn libsql_config_is_rejected_rather_than_served_by_an_embedded_engine() {
        let data_dir = tempfile::tempdir().expect("control-plane data dir should build");
        let error = Engine::new_with_persistence_config(EnginePersistenceConfig::libsql_replica(
            data_dir.path(),
            "libsql://127.0.0.1:8080",
            None,
            "http://127.0.0.1:8081",
            None,
            data_dir.path().join("replica-cache"),
        ))
        .await
        .err()
        .expect("a libsql config must not build an engine without the libsql feature");
        assert!(
            matches!(
                &error,
                Error::InvalidInput(message)
                    if message == "libsql support is not enabled in this build; rebuild with the libsql feature"
            ),
            "expected the libsql build-support rejection, got {error:?}"
        );
    }
}
