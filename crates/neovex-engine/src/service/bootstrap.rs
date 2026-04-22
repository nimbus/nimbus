use std::path::PathBuf;
use std::sync::Arc;

use neovex_core::{Error, Result};
use neovex_storage::{
    Clock, EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider, EmbeddedRedbProvider,
    EmbeddedSqliteProvider, FaultInjector, LibsqlReplicaProvider, LibsqlReplicaProviderConfig,
    LocalKeyProvider, MySqlProvider, MySqlProviderConfig, PostgresProvider, PostgresProviderConfig,
};

use super::{BackgroundExecutor, Service, ServiceBootstrapParts, encryption};
use crate::persistence::{ControlPlaneProvider, PersistenceProvider};
use crate::persistence_config::{
    ControlPlaneBootstrapPlan, EmbeddedTenantBootstrapPlan, LibsqlReplicaTenantBootstrapPlan,
    MySqlTenantBootstrapPlan, PostgresTenantBootstrapPlan, ServiceBootstrapPlan,
    ServicePersistenceConfig, TenantProviderBootstrapPlan,
};

pub(super) async fn build_from_persistence_config(
    config: ServicePersistenceConfig,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
) -> Result<Service> {
    let key_provider = encryption::initialize_encryption(&config)?;
    let encryption_status = encryption::EncryptionStatus::from_config(&config);
    let plan = config.bootstrap_plan()?;
    let encryption_provider = key_provider
        .as_ref()
        .map(encryption::InitializedKeyProvider::provider);

    build_from_plan(
        plan,
        encryption_provider,
        clock,
        storage_fault_injector,
        Some(encryption_status),
    )
    .await
}

pub(super) fn build_embedded_service(
    tenant_data_dir: PathBuf,
    control_data_dir: PathBuf,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    embedded_provider_kind: EmbeddedProviderKind,
) -> Result<Service> {
    build_embedded_from_plan(
        tenant_data_dir.clone(),
        control_data_dir,
        EmbeddedTenantBootstrapPlan {
            provider_kind: embedded_provider_kind,
            data_dir: tenant_data_dir,
        },
        encryption_provider,
        clock,
        storage_fault_injector,
        None,
    )
}

async fn build_from_plan(
    plan: ServiceBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Service> {
    let ServiceBootstrapPlan {
        service_data_dir,
        control_plane,
        tenant_provider,
    } = plan;
    let control_plane_data_dir = match control_plane {
        ControlPlaneBootstrapPlan::EmbeddedRedb { data_dir } => data_dir,
    };

    match tenant_provider {
        TenantProviderBootstrapPlan::Embedded(plan) => build_embedded_from_plan(
            service_data_dir,
            control_plane_data_dir,
            plan,
            encryption_provider,
            clock,
            storage_fault_injector,
            encryption_status,
        ),
        TenantProviderBootstrapPlan::Postgres(plan) => {
            build_postgres_from_plan(
                service_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                clock,
                storage_fault_injector,
                encryption_status,
            )
            .await
        }
        TenantProviderBootstrapPlan::LibsqlReplica(plan) => {
            build_libsql_replica_from_plan(
                service_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                clock,
                storage_fault_injector,
                encryption_status,
            )
            .await
        }
        TenantProviderBootstrapPlan::MySql(plan) => {
            build_mysql_from_plan(
                service_data_dir,
                control_plane_data_dir,
                plan,
                encryption_provider,
                clock,
                storage_fault_injector,
                encryption_status,
            )
            .await
        }
    }
}

fn build_embedded_from_plan(
    service_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: EmbeddedTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Service> {
    std::fs::create_dir_all(&plan.data_dir).map_err(internal_error)?;
    if control_data_dir != plan.data_dir {
        std::fs::create_dir_all(&control_data_dir).map_err(internal_error)?;
    }

    let (engine_executor, storage_executor) = build_executors();
    let control_plane_provider = build_control_plane_provider(
        control_data_dir,
        encryption_provider.clone(),
        &storage_executor,
    )?;
    let persistence_provider = match plan.provider_kind {
        EmbeddedProviderKind::Redb => {
            let provider = if let Some(provider) = encryption_provider {
                EmbeddedRedbProvider::new_encrypted(
                    plan.data_dir.clone(),
                    provider,
                    clock.clone(),
                    storage_fault_injector.clone(),
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedRedbProvider::new(
                    plan.data_dir.clone(),
                    clock.clone(),
                    storage_fault_injector.clone(),
                    storage_executor.handle(),
                )?
            };
            PersistenceProvider::Redb(Arc::new(provider))
        }
        EmbeddedProviderKind::Sqlite => {
            let provider = if let Some(provider) = encryption_provider {
                EmbeddedSqliteProvider::new_encrypted(
                    plan.data_dir.clone(),
                    provider,
                    clock.clone(),
                    storage_fault_injector.clone(),
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedSqliteProvider::new(
                    plan.data_dir.clone(),
                    clock.clone(),
                    storage_fault_injector.clone(),
                    storage_executor.handle(),
                )?
            };
            PersistenceProvider::Sqlite(Arc::new(provider))
        }
    };

    Ok(Service::from_bootstrap_parts(ServiceBootstrapParts {
        data_dir: service_data_dir,
        embedded_provider_kind: Some(plan.provider_kind),
        persistence_provider,
        control_plane_provider,
        clock,
        storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
    }))
}

async fn build_postgres_from_plan(
    service_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: PostgresTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Service> {
    std::fs::create_dir_all(&control_data_dir).map_err(internal_error)?;
    let (engine_executor, storage_executor) = build_executors();
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
        PostgresProvider::connect_with_simulation(
            provider_config,
            storage_executor.handle(),
            clock.clone(),
            storage_fault_injector.clone(),
        )
        .await?,
    );

    Ok(Service::from_bootstrap_parts(ServiceBootstrapParts {
        data_dir: service_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::Postgres(postgres_provider),
        control_plane_provider,
        clock,
        storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
    }))
}

async fn build_libsql_replica_from_plan(
    service_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: LibsqlReplicaTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Service> {
    std::fs::create_dir_all(&control_data_dir).map_err(internal_error)?;
    let (engine_executor, storage_executor) = build_executors();
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
    let libsql_replica_provider = Arc::new(
        LibsqlReplicaProvider::connect_with_simulation(
            provider_config,
            storage_executor.handle(),
            clock.clone(),
            storage_fault_injector.clone(),
        )
        .await?,
    );

    Ok(Service::from_bootstrap_parts(ServiceBootstrapParts {
        data_dir: service_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::LibsqlReplica(libsql_replica_provider),
        control_plane_provider,
        clock,
        storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
    }))
}

async fn build_mysql_from_plan(
    service_data_dir: PathBuf,
    control_data_dir: PathBuf,
    plan: MySqlTenantBootstrapPlan,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    encryption_status: Option<encryption::EncryptionStatus>,
) -> Result<Service> {
    std::fs::create_dir_all(&control_data_dir).map_err(internal_error)?;
    let (engine_executor, storage_executor) = build_executors();
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
        MySqlProvider::connect_with_simulation(
            provider_config,
            storage_executor.handle(),
            clock.clone(),
            storage_fault_injector.clone(),
        )
        .await?,
    );

    Ok(Service::from_bootstrap_parts(ServiceBootstrapParts {
        data_dir: service_data_dir,
        embedded_provider_kind: None,
        persistence_provider: PersistenceProvider::MySql(mysql_provider),
        control_plane_provider,
        clock,
        storage_fault_injector,
        engine_executor,
        storage_executor,
        encryption_status,
    }))
}

fn build_executors() -> (BackgroundExecutor, BackgroundExecutor) {
    (
        BackgroundExecutor::new("neovex-engine-bg", 2),
        BackgroundExecutor::new("neovex-storage-bg", 1),
    )
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
