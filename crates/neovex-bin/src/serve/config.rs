use std::path::{Path, PathBuf};

use clap::ValueEnum;
use neovex::{EmbeddedProviderKind, Error, ServicePersistenceConfig};
use serde::Deserialize;

use super::ServeCommand;

const DEFAULT_DATA_DIR: &str = "./data";
const CONFIG_FILE_ENV: &str = "NEOVEX_CONFIG";
const DATA_DIR_ENV: &str = "NEOVEX_DATA_DIR";
const CONTROL_DATA_DIR_ENV: &str = "NEOVEX_CONTROL_DATA_DIR";
const TENANT_PROVIDER_ENV: &str = "NEOVEX_TENANT_PROVIDER";
const LIBSQL_URL_ENV: &str = "NEOVEX_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NEOVEX_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NEOVEX_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NEOVEX_LIBSQL_ADMIN_AUTH_HEADER";
const LIBSQL_METADATA_NAMESPACE_ENV: &str = "NEOVEX_LIBSQL_METADATA_NAMESPACE";
const LIBSQL_TENANT_NAMESPACE_PREFIX_ENV: &str = "NEOVEX_LIBSQL_TENANT_NAMESPACE_PREFIX";
const LIBSQL_REPLICA_CACHE_DIR_ENV: &str = "NEOVEX_LIBSQL_REPLICA_CACHE_DIR";
const POSTGRES_URL_ENV: &str = "NEOVEX_POSTGRES_URL";
const POSTGRES_METADATA_SCHEMA_ENV: &str = "NEOVEX_POSTGRES_METADATA_SCHEMA";
const POSTGRES_TENANT_SCHEMA_PREFIX_ENV: &str = "NEOVEX_POSTGRES_TENANT_SCHEMA_PREFIX";
const POSTGRES_MIN_CONNECTIONS_ENV: &str = "NEOVEX_POSTGRES_MIN_CONNECTIONS";
const POSTGRES_MAX_CONNECTIONS_ENV: &str = "NEOVEX_POSTGRES_MAX_CONNECTIONS";
const MYSQL_URL_ENV: &str = "NEOVEX_MYSQL_URL";
const MYSQL_METADATA_DATABASE_ENV: &str = "NEOVEX_MYSQL_METADATA_DATABASE";
const MYSQL_TENANT_DATABASE_PREFIX_ENV: &str = "NEOVEX_MYSQL_TENANT_DATABASE_PREFIX";
const MYSQL_MIN_CONNECTIONS_ENV: &str = "NEOVEX_MYSQL_MIN_CONNECTIONS";
const MYSQL_MAX_CONNECTIONS_ENV: &str = "NEOVEX_MYSQL_MAX_CONNECTIONS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CliTenantProvider {
    Sqlite,
    LibsqlReplica,
    Redb,
    Postgres,
    Mysql,
}

impl CliTenantProvider {
    fn parse_name(value: &str) -> neovex::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sqlite" => Ok(Self::Sqlite),
            "libsql-replica" | "libsql_replica" => Ok(Self::LibsqlReplica),
            "redb" => Ok(Self::Redb),
            "postgres" => Ok(Self::Postgres),
            "mysql" => Ok(Self::Mysql),
            other => Err(Error::InvalidInput(format!(
                "unsupported tenant provider '{other}'; expected sqlite, libsql-replica, redb, postgres, or mysql"
            ))),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RuntimeConfigFile {
    pub(crate) persistence: PersistenceFileConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PersistenceFileConfig {
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) control_data_dir: Option<PathBuf>,
    pub(crate) tenant_provider: Option<CliTenantProvider>,
    pub(crate) libsql_url: Option<String>,
    pub(crate) libsql_auth_token: Option<String>,
    pub(crate) libsql_admin_url: Option<String>,
    pub(crate) libsql_admin_auth_header: Option<String>,
    pub(crate) libsql_metadata_namespace: Option<String>,
    pub(crate) libsql_tenant_namespace_prefix: Option<String>,
    pub(crate) libsql_replica_cache_dir: Option<PathBuf>,
    pub(crate) postgres_url: Option<String>,
    pub(crate) postgres_metadata_schema: Option<String>,
    pub(crate) postgres_tenant_schema_prefix: Option<String>,
    pub(crate) postgres_min_connections: Option<usize>,
    pub(crate) postgres_max_connections: Option<usize>,
    pub(crate) mysql_url: Option<String>,
    pub(crate) mysql_metadata_database: Option<String>,
    pub(crate) mysql_tenant_database_prefix: Option<String>,
    pub(crate) mysql_min_connections: Option<usize>,
    pub(crate) mysql_max_connections: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PersistenceEnv {
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) control_data_dir: Option<PathBuf>,
    pub(crate) tenant_provider: Option<CliTenantProvider>,
    pub(crate) libsql_url: Option<String>,
    pub(crate) libsql_auth_token: Option<String>,
    pub(crate) libsql_admin_url: Option<String>,
    pub(crate) libsql_admin_auth_header: Option<String>,
    pub(crate) libsql_metadata_namespace: Option<String>,
    pub(crate) libsql_tenant_namespace_prefix: Option<String>,
    pub(crate) libsql_replica_cache_dir: Option<PathBuf>,
    pub(crate) postgres_url: Option<String>,
    pub(crate) postgres_metadata_schema: Option<String>,
    pub(crate) postgres_tenant_schema_prefix: Option<String>,
    pub(crate) postgres_min_connections: Option<usize>,
    pub(crate) postgres_max_connections: Option<usize>,
    pub(crate) mysql_url: Option<String>,
    pub(crate) mysql_metadata_database: Option<String>,
    pub(crate) mysql_tenant_database_prefix: Option<String>,
    pub(crate) mysql_min_connections: Option<usize>,
    pub(crate) mysql_max_connections: Option<usize>,
}

impl PersistenceEnv {
    pub(crate) fn load() -> neovex::Result<Self> {
        Ok(Self {
            data_dir: std::env::var_os(DATA_DIR_ENV).map(PathBuf::from),
            control_data_dir: std::env::var_os(CONTROL_DATA_DIR_ENV).map(PathBuf::from),
            tenant_provider: optional_env_tenant_provider(TENANT_PROVIDER_ENV)?,
            libsql_url: std::env::var(LIBSQL_URL_ENV).ok(),
            libsql_auth_token: std::env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            libsql_admin_url: std::env::var(LIBSQL_ADMIN_URL_ENV).ok(),
            libsql_admin_auth_header: std::env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
            libsql_metadata_namespace: std::env::var(LIBSQL_METADATA_NAMESPACE_ENV).ok(),
            libsql_tenant_namespace_prefix: std::env::var(LIBSQL_TENANT_NAMESPACE_PREFIX_ENV).ok(),
            libsql_replica_cache_dir: std::env::var_os(LIBSQL_REPLICA_CACHE_DIR_ENV)
                .map(PathBuf::from),
            postgres_url: std::env::var(POSTGRES_URL_ENV).ok(),
            postgres_metadata_schema: std::env::var(POSTGRES_METADATA_SCHEMA_ENV).ok(),
            postgres_tenant_schema_prefix: std::env::var(POSTGRES_TENANT_SCHEMA_PREFIX_ENV).ok(),
            postgres_min_connections: optional_env_usize(POSTGRES_MIN_CONNECTIONS_ENV)?,
            postgres_max_connections: optional_env_usize(POSTGRES_MAX_CONNECTIONS_ENV)?,
            mysql_url: std::env::var(MYSQL_URL_ENV).ok(),
            mysql_metadata_database: std::env::var(MYSQL_METADATA_DATABASE_ENV).ok(),
            mysql_tenant_database_prefix: std::env::var(MYSQL_TENANT_DATABASE_PREFIX_ENV).ok(),
            mysql_min_connections: optional_env_usize(MYSQL_MIN_CONNECTIONS_ENV)?,
            mysql_max_connections: optional_env_usize(MYSQL_MAX_CONNECTIONS_ENV)?,
        })
    }
}

#[derive(Debug, Clone)]
struct ResolvedPersistenceInputs {
    data_dir: PathBuf,
    control_data_dir: PathBuf,
    tenant_provider: CliTenantProvider,
    libsql_url: Option<String>,
    libsql_auth_token: Option<String>,
    libsql_admin_url: Option<String>,
    libsql_admin_auth_header: Option<String>,
    libsql_metadata_namespace: Option<String>,
    libsql_tenant_namespace_prefix: Option<String>,
    libsql_replica_cache_dir: Option<PathBuf>,
    postgres_url: Option<String>,
    postgres_metadata_schema: Option<String>,
    postgres_tenant_schema_prefix: Option<String>,
    postgres_min_connections: Option<usize>,
    postgres_max_connections: Option<usize>,
    mysql_url: Option<String>,
    mysql_metadata_database: Option<String>,
    mysql_tenant_database_prefix: Option<String>,
    mysql_min_connections: Option<usize>,
    mysql_max_connections: Option<usize>,
}

impl ResolvedPersistenceInputs {
    fn from_sources(
        command: &ServeCommand,
        file: &PersistenceFileConfig,
        env: &PersistenceEnv,
    ) -> Self {
        let data_dir = command
            .data_dir
            .clone()
            .or_else(|| env.data_dir.clone())
            .or_else(|| file.data_dir.clone())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        let control_data_dir = command
            .control_data_dir
            .clone()
            .or_else(|| env.control_data_dir.clone())
            .or_else(|| file.control_data_dir.clone())
            .unwrap_or_else(|| data_dir.clone());

        Self {
            data_dir,
            control_data_dir,
            tenant_provider: command
                .tenant_provider
                .or(env.tenant_provider)
                .or(file.tenant_provider)
                .unwrap_or(CliTenantProvider::Sqlite),
            libsql_url: command
                .libsql_url
                .clone()
                .or_else(|| env.libsql_url.clone())
                .or_else(|| file.libsql_url.clone()),
            libsql_auth_token: command
                .libsql_auth_token
                .clone()
                .or_else(|| env.libsql_auth_token.clone())
                .or_else(|| file.libsql_auth_token.clone()),
            libsql_admin_url: command
                .libsql_admin_url
                .clone()
                .or_else(|| env.libsql_admin_url.clone())
                .or_else(|| file.libsql_admin_url.clone()),
            libsql_admin_auth_header: command
                .libsql_admin_auth_header
                .clone()
                .or_else(|| env.libsql_admin_auth_header.clone())
                .or_else(|| file.libsql_admin_auth_header.clone()),
            libsql_metadata_namespace: command
                .libsql_metadata_namespace
                .clone()
                .or_else(|| env.libsql_metadata_namespace.clone())
                .or_else(|| file.libsql_metadata_namespace.clone()),
            libsql_tenant_namespace_prefix: command
                .libsql_tenant_namespace_prefix
                .clone()
                .or_else(|| env.libsql_tenant_namespace_prefix.clone())
                .or_else(|| file.libsql_tenant_namespace_prefix.clone()),
            libsql_replica_cache_dir: command
                .libsql_replica_cache_dir
                .clone()
                .or_else(|| env.libsql_replica_cache_dir.clone())
                .or_else(|| file.libsql_replica_cache_dir.clone()),
            postgres_url: command
                .postgres_url
                .clone()
                .or_else(|| env.postgres_url.clone())
                .or_else(|| file.postgres_url.clone()),
            postgres_metadata_schema: command
                .postgres_metadata_schema
                .clone()
                .or_else(|| env.postgres_metadata_schema.clone())
                .or_else(|| file.postgres_metadata_schema.clone()),
            postgres_tenant_schema_prefix: command
                .postgres_tenant_schema_prefix
                .clone()
                .or_else(|| env.postgres_tenant_schema_prefix.clone())
                .or_else(|| file.postgres_tenant_schema_prefix.clone()),
            postgres_min_connections: command
                .postgres_min_connections
                .or(env.postgres_min_connections)
                .or(file.postgres_min_connections),
            postgres_max_connections: command
                .postgres_max_connections
                .or(env.postgres_max_connections)
                .or(file.postgres_max_connections),
            mysql_url: command
                .mysql_url
                .clone()
                .or_else(|| env.mysql_url.clone())
                .or_else(|| file.mysql_url.clone()),
            mysql_metadata_database: command
                .mysql_metadata_database
                .clone()
                .or_else(|| env.mysql_metadata_database.clone())
                .or_else(|| file.mysql_metadata_database.clone()),
            mysql_tenant_database_prefix: command
                .mysql_tenant_database_prefix
                .clone()
                .or_else(|| env.mysql_tenant_database_prefix.clone())
                .or_else(|| file.mysql_tenant_database_prefix.clone()),
            mysql_min_connections: command
                .mysql_min_connections
                .or(env.mysql_min_connections)
                .or(file.mysql_min_connections),
            mysql_max_connections: command
                .mysql_max_connections
                .or(env.mysql_max_connections)
                .or(file.mysql_max_connections),
        }
    }

    fn into_service_persistence_config(self) -> neovex::Result<ServicePersistenceConfig> {
        match self.tenant_provider {
            CliTenantProvider::Sqlite => self.into_embedded_config(EmbeddedProviderKind::Sqlite),
            CliTenantProvider::Redb => self.into_embedded_config(EmbeddedProviderKind::Redb),
            CliTenantProvider::LibsqlReplica => self.into_libsql_replica_config(),
            CliTenantProvider::Postgres => self.into_postgres_config(),
            CliTenantProvider::Mysql => self.into_mysql_config(),
        }
    }

    fn into_embedded_config(
        self,
        provider_kind: EmbeddedProviderKind,
    ) -> neovex::Result<ServicePersistenceConfig> {
        self.reject_external_provider_overrides()?;
        Ok(ServicePersistenceConfig {
            tenant_provider: neovex::TenantProviderConfig::embedded(self.data_dir, provider_kind),
            control_plane: neovex::ControlPlaneConfig::embedded_redb(self.control_data_dir),
        })
    }

    fn into_libsql_replica_config(self) -> neovex::Result<ServicePersistenceConfig> {
        self.reject_postgres_overrides()?;
        self.reject_mysql_overrides()?;
        let libsql_url = self.libsql_url.ok_or_else(|| {
            Error::InvalidInput(
                "--libsql-url, NEOVEX_LIBSQL_URL, or persistence.libsql_url is required when the tenant provider is libsql-replica"
                    .to_string(),
            )
        })?;
        let libsql_replica_cache_dir = self.libsql_replica_cache_dir.ok_or_else(|| {
            Error::InvalidInput(
                "--libsql-replica-cache-dir, NEOVEX_LIBSQL_REPLICA_CACHE_DIR, or persistence.libsql_replica_cache_dir is required when the tenant provider is libsql-replica"
                    .to_string(),
            )
        })?;
        let libsql_admin_url = self.libsql_admin_url.ok_or_else(|| {
            Error::InvalidInput(
                "--libsql-admin-url, NEOVEX_LIBSQL_ADMIN_URL, or persistence.libsql_admin_url is required when the tenant provider is libsql-replica"
                    .to_string(),
            )
        })?;

        let mut config = ServicePersistenceConfig::libsql_replica(
            self.control_data_dir,
            libsql_url,
            self.libsql_auth_token,
            libsql_admin_url,
            self.libsql_admin_auth_header,
            libsql_replica_cache_dir,
        );
        if let neovex::TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace,
            tenant_namespace_prefix,
            ..
        } = &mut config.tenant_provider.routing
        {
            if let Some(value) = self.libsql_metadata_namespace {
                *metadata_namespace = value;
            }
            if let Some(value) = self.libsql_tenant_namespace_prefix {
                *tenant_namespace_prefix = value;
            }
        }
        Ok(config)
    }

    fn into_postgres_config(self) -> neovex::Result<ServicePersistenceConfig> {
        self.reject_mysql_overrides()?;
        self.reject_libsql_replica_overrides()?;
        let postgres_url = self.postgres_url.ok_or_else(|| {
            Error::InvalidInput(
                "--postgres-url, NEOVEX_POSTGRES_URL, or persistence.postgres_url is required when the tenant provider is postgres"
                    .to_string(),
            )
        })?;

        let mut config = ServicePersistenceConfig::postgres(self.control_data_dir, postgres_url);
        if let neovex::TenantRoutingConfig::SchemaPerTenant {
            metadata_schema,
            tenant_schema_prefix,
        } = &mut config.tenant_provider.routing
        {
            if let Some(value) = self.postgres_metadata_schema {
                *metadata_schema = value;
            }
            if let Some(value) = self.postgres_tenant_schema_prefix {
                *tenant_schema_prefix = value;
            }
        }
        config.tenant_provider.pool.min_connections = self.postgres_min_connections;
        config.tenant_provider.pool.max_connections = self.postgres_max_connections;
        Ok(config)
    }

    fn into_mysql_config(self) -> neovex::Result<ServicePersistenceConfig> {
        self.reject_postgres_overrides()?;
        self.reject_libsql_replica_overrides()?;
        let mysql_url = self.mysql_url.ok_or_else(|| {
            Error::InvalidInput(
                "--mysql-url, NEOVEX_MYSQL_URL, or persistence.mysql_url is required when the tenant provider is mysql"
                    .to_string(),
            )
        })?;

        let mut config = ServicePersistenceConfig::mysql(self.control_data_dir, mysql_url);
        if let neovex::TenantRoutingConfig::DatabasePerTenant {
            metadata_database,
            tenant_database_prefix,
        } = &mut config.tenant_provider.routing
        {
            if let Some(value) = self.mysql_metadata_database {
                *metadata_database = value;
            }
            if let Some(value) = self.mysql_tenant_database_prefix {
                *tenant_database_prefix = value;
            }
        }
        config.tenant_provider.pool.min_connections = self.mysql_min_connections;
        config.tenant_provider.pool.max_connections = self.mysql_max_connections;
        Ok(config)
    }

    fn reject_external_provider_overrides(&self) -> neovex::Result<()> {
        if self.postgres_overrides_present()
            || self.mysql_overrides_present()
            || self.libsql_replica_overrides_present()
        {
            return Err(Error::InvalidInput(
                "External provider config requires --tenant-provider=libsql-replica, --tenant-provider=postgres, or --tenant-provider=mysql (or the equivalent env/config setting)"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_postgres_overrides(&self) -> neovex::Result<()> {
        if self.postgres_overrides_present() {
            return Err(Error::InvalidInput(
                "Postgres config requires --tenant-provider=postgres or the equivalent env/config setting"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_mysql_overrides(&self) -> neovex::Result<()> {
        if self.mysql_overrides_present() {
            return Err(Error::InvalidInput(
                "MySQL config requires --tenant-provider=mysql or the equivalent env/config setting"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_libsql_replica_overrides(&self) -> neovex::Result<()> {
        if self.libsql_replica_overrides_present() {
            return Err(Error::InvalidInput(
                "Replica-connected SQLite config requires --tenant-provider=libsql-replica or the equivalent env/config setting"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn libsql_replica_overrides_present(&self) -> bool {
        self.libsql_url.is_some()
            || self.libsql_auth_token.is_some()
            || self.libsql_admin_url.is_some()
            || self.libsql_admin_auth_header.is_some()
            || self.libsql_metadata_namespace.is_some()
            || self.libsql_tenant_namespace_prefix.is_some()
            || self.libsql_replica_cache_dir.is_some()
    }

    fn postgres_overrides_present(&self) -> bool {
        self.postgres_url.is_some()
            || self.postgres_metadata_schema.is_some()
            || self.postgres_tenant_schema_prefix.is_some()
            || self.postgres_min_connections.is_some()
            || self.postgres_max_connections.is_some()
    }

    fn mysql_overrides_present(&self) -> bool {
        self.mysql_url.is_some()
            || self.mysql_metadata_database.is_some()
            || self.mysql_tenant_database_prefix.is_some()
            || self.mysql_min_connections.is_some()
            || self.mysql_max_connections.is_some()
    }
}

pub(crate) fn service_persistence_config_from_serve_command(
    command: &ServeCommand,
) -> neovex::Result<ServicePersistenceConfig> {
    let config_path = command
        .config
        .clone()
        .or_else(|| std::env::var_os(CONFIG_FILE_ENV).map(PathBuf::from));
    let file_config = load_runtime_config_file(config_path.as_deref())?;
    let env = PersistenceEnv::load()?;
    service_persistence_config_from_sources(command, &file_config.persistence, &env)
}

pub(crate) fn service_persistence_config_from_sources(
    command: &ServeCommand,
    file: &PersistenceFileConfig,
    env: &PersistenceEnv,
) -> neovex::Result<ServicePersistenceConfig> {
    ResolvedPersistenceInputs::from_sources(command, file, env).into_service_persistence_config()
}

pub(super) fn control_data_dir_from_service_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        neovex::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

pub(crate) fn load_runtime_config_file(path: Option<&Path>) -> neovex::Result<RuntimeConfigFile> {
    let Some(path) = path else {
        return Ok(RuntimeConfigFile::default());
    };
    let bytes = std::fs::read(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read config file {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse config file {} as JSON: {error}",
            path.display()
        ))
    })
}

fn optional_env_tenant_provider(key: &str) -> neovex::Result<Option<CliTenantProvider>> {
    std::env::var(key)
        .ok()
        .map(|value| CliTenantProvider::parse_name(&value))
        .transpose()
}

fn optional_env_usize(key: &str) -> neovex::Result<Option<usize>> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value.parse::<usize>().map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to parse {key} as an unsigned integer: {error}"
                ))
            })
        })
        .transpose()
}
