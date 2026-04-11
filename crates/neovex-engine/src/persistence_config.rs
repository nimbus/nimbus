use std::path::PathBuf;

use neovex_storage::EmbeddedProviderKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePersistenceConfig {
    pub tenant_provider: TenantProviderConfig,
    pub control_plane: ControlPlaneConfig,
}

impl ServicePersistenceConfig {
    pub fn embedded_default(data_dir: impl Into<PathBuf>) -> Self {
        Self::embedded(data_dir, EmbeddedProviderKind::default())
    }

    pub fn embedded(
        data_dir: impl Into<PathBuf>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Self {
        let data_dir = data_dir.into();
        Self {
            tenant_provider: TenantProviderConfig::embedded(
                data_dir.clone(),
                embedded_provider_kind,
            ),
            control_plane: ControlPlaneConfig::embedded_redb(data_dir),
        }
    }

    pub fn postgres(
        control_data_dir: impl Into<PathBuf>,
        connection_string: impl Into<String>,
    ) -> Self {
        Self {
            tenant_provider: TenantProviderConfig::postgres(connection_string),
            control_plane: ControlPlaneConfig::embedded_redb(control_data_dir),
        }
    }

    pub fn libsql_replica(
        control_data_dir: impl Into<PathBuf>,
        primary_url: impl Into<String>,
        auth_token: Option<String>,
        admin_api_url: impl Into<String>,
        admin_auth_header: Option<String>,
        replica_cache_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            tenant_provider: TenantProviderConfig::libsql_replica(
                primary_url,
                auth_token,
                admin_api_url,
                admin_auth_header,
                replica_cache_dir,
            ),
            control_plane: ControlPlaneConfig::embedded_redb(control_data_dir),
        }
    }

    pub fn mysql(
        control_data_dir: impl Into<PathBuf>,
        connection_string: impl Into<String>,
    ) -> Self {
        Self {
            tenant_provider: TenantProviderConfig::mysql(connection_string),
            control_plane: ControlPlaneConfig::embedded_redb(control_data_dir),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantProviderConfig {
    pub dialect: PersistenceDialect,
    pub topology: PersistenceTopology,
    pub routing: TenantRoutingConfig,
    pub pool: PoolConfig,
    pub credentials: ProviderCredentials,
}

impl TenantProviderConfig {
    pub fn embedded(
        data_dir: impl Into<PathBuf>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Self {
        let data_dir = data_dir.into();
        Self {
            dialect: match embedded_provider_kind {
                EmbeddedProviderKind::Redb => PersistenceDialect::Redb,
                EmbeddedProviderKind::Sqlite => PersistenceDialect::Sqlite,
            },
            topology: PersistenceTopology::EmbeddedStandalone,
            routing: TenantRoutingConfig::DirectoryPerTenant { data_dir },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::None,
        }
    }

    pub fn postgres(connection_string: impl Into<String>) -> Self {
        Self {
            dialect: PersistenceDialect::Postgres,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::SchemaPerTenant {
                metadata_schema: "neovex_provider".to_string(),
                tenant_schema_prefix: "tenant_".to_string(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::ConnectionString(connection_string.into()),
        }
    }

    pub fn libsql_replica(
        primary_url: impl Into<String>,
        auth_token: Option<String>,
        admin_api_url: impl Into<String>,
        admin_auth_header: Option<String>,
        replica_cache_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            dialect: PersistenceDialect::Sqlite,
            topology: PersistenceTopology::ExternalPrimaryWithReplicas,
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace: "neovex_provider".to_string(),
                tenant_namespace_prefix: "tenant_".to_string(),
                replica_cache_dir: replica_cache_dir.into(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::LibsqlReplica {
                primary_url: primary_url.into(),
                auth_token,
                admin_api_url: admin_api_url.into(),
                admin_auth_header,
            },
        }
    }

    pub fn mysql(connection_string: impl Into<String>) -> Self {
        Self {
            dialect: PersistenceDialect::MySql,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::DatabasePerTenant {
                metadata_database: "neovex_provider".to_string(),
                tenant_database_prefix: "tenant_".to_string(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::ConnectionString(connection_string.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceDialect {
    Redb,
    Sqlite,
    Postgres,
    MySql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceTopology {
    EmbeddedStandalone,
    ExternalPrimary,
    ExternalPrimaryWithReplicas,
    CoordinatedEmbedded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantRoutingConfig {
    DirectoryPerTenant {
        data_dir: PathBuf,
    },
    SchemaPerTenant {
        metadata_schema: String,
        tenant_schema_prefix: String,
    },
    NamespacePerTenant {
        metadata_namespace: String,
        tenant_namespace_prefix: String,
        replica_cache_dir: PathBuf,
    },
    DatabasePerTenant {
        metadata_database: String,
        tenant_database_prefix: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolConfig {
    pub min_connections: Option<usize>,
    pub max_connections: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderCredentials {
    None,
    ConnectionString(String),
    LibsqlReplica {
        primary_url: String,
        auth_token: Option<String>,
        admin_api_url: String,
        admin_auth_header: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlPlaneConfig {
    EmbeddedRedb { data_dir: PathBuf },
}

impl ControlPlaneConfig {
    pub fn embedded_redb(data_dir: impl Into<PathBuf>) -> Self {
        Self::EmbeddedRedb {
            data_dir: data_dir.into(),
        }
    }
}
