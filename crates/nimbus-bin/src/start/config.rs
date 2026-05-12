use std::path::{Path, PathBuf};

use clap::ValueEnum;
use nimbus::{
    AwsKmsConfig, EmbeddedProviderKind, Error, KeyDirectoryConfig, LocalEncryptionConfig,
    LocalKeyProviderConfig, MasterKeyFileConfig, ServicePersistenceConfig,
};
use serde::Deserialize;

use super::StartCommand;

const DEFAULT_DATA_DIR: &str = "./data";
const CONFIG_FILE_ENV: &str = "NIMBUS_CONFIG";
const DATA_DIR_ENV: &str = "NIMBUS_DATA_DIR";
const CONTROL_DATA_DIR_ENV: &str = "NIMBUS_CONTROL_DATA_DIR";
const TENANT_PROVIDER_ENV: &str = "NIMBUS_TENANT_PROVIDER";
const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
const LIBSQL_METADATA_NAMESPACE_ENV: &str = "NIMBUS_LIBSQL_METADATA_NAMESPACE";
const LIBSQL_TENANT_NAMESPACE_PREFIX_ENV: &str = "NIMBUS_LIBSQL_TENANT_NAMESPACE_PREFIX";
const LIBSQL_REPLICA_CACHE_DIR_ENV: &str = "NIMBUS_LIBSQL_REPLICA_CACHE_DIR";
const POSTGRES_URL_ENV: &str = "NIMBUS_POSTGRES_URL";
const POSTGRES_METADATA_SCHEMA_ENV: &str = "NIMBUS_POSTGRES_METADATA_SCHEMA";
const POSTGRES_TENANT_SCHEMA_PREFIX_ENV: &str = "NIMBUS_POSTGRES_TENANT_SCHEMA_PREFIX";
const POSTGRES_MIN_CONNECTIONS_ENV: &str = "NIMBUS_POSTGRES_MIN_CONNECTIONS";
const POSTGRES_MAX_CONNECTIONS_ENV: &str = "NIMBUS_POSTGRES_MAX_CONNECTIONS";
const MYSQL_URL_ENV: &str = "NIMBUS_MYSQL_URL";
const MYSQL_METADATA_DATABASE_ENV: &str = "NIMBUS_MYSQL_METADATA_DATABASE";
const MYSQL_TENANT_DATABASE_PREFIX_ENV: &str = "NIMBUS_MYSQL_TENANT_DATABASE_PREFIX";
const MYSQL_MIN_CONNECTIONS_ENV: &str = "NIMBUS_MYSQL_MIN_CONNECTIONS";
const MYSQL_MAX_CONNECTIONS_ENV: &str = "NIMBUS_MYSQL_MAX_CONNECTIONS";

// Encryption config environment variables
const ENCRYPTION_KEY_PROVIDER_ENV: &str = "NIMBUS_ENCRYPTION_KEY_PROVIDER";
const ENCRYPTION_MASTER_KEY_FILE_ENV: &str = "NIMBUS_ENCRYPTION_MASTER_KEY_FILE";
const ENCRYPTION_KEY_DIR_ENV: &str = "NIMBUS_ENCRYPTION_KEY_DIR";
const ENCRYPTION_AWS_KMS_KEY_ID_ENV: &str = "NIMBUS_ENCRYPTION_AWS_KMS_KEY_ID";
const ENCRYPTION_AWS_REGION_ENV: &str = "NIMBUS_ENCRYPTION_AWS_REGION";
const ENCRYPTION_AWS_ENDPOINT_URL_ENV: &str = "NIMBUS_ENCRYPTION_AWS_ENDPOINT_URL";

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
    fn parse_name(value: &str) -> nimbus::Result<Self> {
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

/// CLI key provider selection for local encryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CliKeyProvider {
    /// Single master key file wraps per-subject DEKs.
    MasterKeyFile,
    /// Per-subject or per-role key files in a directory.
    KeyDir,
    /// AWS KMS envelope encryption for enterprise-managed keys.
    AwsKms,
}

impl CliKeyProvider {
    fn parse_name(value: &str) -> nimbus::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "master-key-file" | "master_key_file" => Ok(Self::MasterKeyFile),
            "key-dir" | "key_dir" => Ok(Self::KeyDir),
            "aws-kms" | "aws_kms" => Ok(Self::AwsKms),
            other => Err(Error::InvalidInput(format!(
                "unsupported encryption key provider '{other}'; expected master-key-file, key-dir, or aws-kms"
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
    // Encryption config
    pub(crate) encryption_key_provider: Option<CliKeyProvider>,
    pub(crate) encryption_master_key_file: Option<PathBuf>,
    pub(crate) encryption_key_dir: Option<PathBuf>,
    pub(crate) encryption_aws_kms_key_id: Option<String>,
    pub(crate) encryption_aws_region: Option<String>,
    pub(crate) encryption_aws_endpoint_url: Option<String>,
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
    // Encryption config
    pub(crate) encryption_key_provider: Option<CliKeyProvider>,
    pub(crate) encryption_master_key_file: Option<PathBuf>,
    pub(crate) encryption_key_dir: Option<PathBuf>,
    pub(crate) encryption_aws_kms_key_id: Option<String>,
    pub(crate) encryption_aws_region: Option<String>,
    pub(crate) encryption_aws_endpoint_url: Option<String>,
}

impl PersistenceEnv {
    pub(crate) fn load() -> nimbus::Result<Self> {
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
            // Encryption config
            encryption_key_provider: optional_env_key_provider(ENCRYPTION_KEY_PROVIDER_ENV)?,
            encryption_master_key_file: std::env::var_os(ENCRYPTION_MASTER_KEY_FILE_ENV)
                .map(PathBuf::from),
            encryption_key_dir: std::env::var_os(ENCRYPTION_KEY_DIR_ENV).map(PathBuf::from),
            encryption_aws_kms_key_id: std::env::var(ENCRYPTION_AWS_KMS_KEY_ID_ENV).ok(),
            encryption_aws_region: std::env::var(ENCRYPTION_AWS_REGION_ENV).ok(),
            encryption_aws_endpoint_url: std::env::var(ENCRYPTION_AWS_ENDPOINT_URL_ENV).ok(),
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
    // Encryption config
    encryption_key_provider: Option<CliKeyProvider>,
    encryption_master_key_file: Option<PathBuf>,
    encryption_key_dir: Option<PathBuf>,
    encryption_aws_kms_key_id: Option<String>,
    encryption_aws_region: Option<String>,
    encryption_aws_endpoint_url: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedEncryptionInputs {
    key_provider: Option<CliKeyProvider>,
    master_key_file: Option<PathBuf>,
    key_dir: Option<PathBuf>,
    aws_kms_key_id: Option<String>,
    aws_region: Option<String>,
    aws_endpoint_url: Option<String>,
}

impl ResolvedEncryptionInputs {
    fn from_inputs(inputs: &ResolvedPersistenceInputs) -> Self {
        Self {
            key_provider: inputs.encryption_key_provider,
            master_key_file: inputs.encryption_master_key_file.clone(),
            key_dir: inputs.encryption_key_dir.clone(),
            aws_kms_key_id: inputs.encryption_aws_kms_key_id.clone(),
            aws_region: inputs.encryption_aws_region.clone(),
            aws_endpoint_url: inputs.encryption_aws_endpoint_url.clone(),
        }
    }

    fn into_local_encryption_config(self) -> nimbus::Result<LocalEncryptionConfig> {
        let Some(key_provider) = self.key_provider else {
            self.reject_orphaned_options()?;
            return Ok(LocalEncryptionConfig::Disabled);
        };

        let key_provider_config = match key_provider {
            CliKeyProvider::MasterKeyFile => {
                self.reject_key_dir_options()?;
                self.reject_aws_kms_options()?;
                let path = self.master_key_file.ok_or_else(|| {
                    Error::InvalidInput(
                        "--encryption-master-key-file, NIMBUS_ENCRYPTION_MASTER_KEY_FILE, or persistence.encryption_master_key_file is required when encryption key provider is master-key-file"
                            .to_string(),
                    )
                })?;
                LocalKeyProviderConfig::MasterKeyFile(MasterKeyFileConfig { path })
            }
            CliKeyProvider::KeyDir => {
                self.reject_master_key_file_options()?;
                self.reject_aws_kms_options()?;
                let path = self.key_dir.ok_or_else(|| {
                    Error::InvalidInput(
                        "--encryption-key-dir, NIMBUS_ENCRYPTION_KEY_DIR, or persistence.encryption_key_dir is required when encryption key provider is key-dir"
                            .to_string(),
                    )
                })?;
                LocalKeyProviderConfig::KeyDirectory(KeyDirectoryConfig { path })
            }
            CliKeyProvider::AwsKms => {
                self.reject_master_key_file_options()?;
                self.reject_key_dir_options()?;
                let key_id = self.aws_kms_key_id.ok_or_else(|| {
                    Error::InvalidInput(
                        "--encryption-aws-kms-key-id, NIMBUS_ENCRYPTION_AWS_KMS_KEY_ID, or persistence.encryption_aws_kms_key_id is required when encryption key provider is aws-kms"
                            .to_string(),
                    )
                })?;
                LocalKeyProviderConfig::AwsKms(AwsKmsConfig {
                    key_id,
                    region: self.aws_region,
                    endpoint_url: self.aws_endpoint_url,
                })
            }
        };

        Ok(LocalEncryptionConfig::Enabled(key_provider_config))
    }

    fn reject_orphaned_options(&self) -> nimbus::Result<()> {
        if self.master_key_file.is_some()
            || self.key_dir.is_some()
            || self.aws_kms_key_id.is_some()
            || self.aws_region.is_some()
            || self.aws_endpoint_url.is_some()
        {
            return Err(Error::InvalidInput(
                "encryption options require --encryption-key-provider (or NIMBUS_ENCRYPTION_KEY_PROVIDER or persistence.encryption_key_provider)"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_master_key_file_options(&self) -> nimbus::Result<()> {
        if self.master_key_file.is_some() {
            return Err(Error::InvalidInput(
                "--encryption-master-key-file only applies when --encryption-key-provider=master-key-file"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_key_dir_options(&self) -> nimbus::Result<()> {
        if self.key_dir.is_some() {
            return Err(Error::InvalidInput(
                "--encryption-key-dir only applies when --encryption-key-provider=key-dir"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_aws_kms_options(&self) -> nimbus::Result<()> {
        if self.aws_kms_key_id.is_some()
            || self.aws_region.is_some()
            || self.aws_endpoint_url.is_some()
        {
            return Err(Error::InvalidInput(
                "AWS KMS encryption options only apply when --encryption-key-provider=aws-kms"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

enum ResolvedTenantProviderConfig {
    Embedded {
        data_dir: PathBuf,
        control_data_dir: PathBuf,
        provider_kind: EmbeddedProviderKind,
    },
    LibsqlReplica {
        control_data_dir: PathBuf,
        libsql_url: String,
        libsql_auth_token: Option<String>,
        libsql_admin_url: String,
        libsql_admin_auth_header: Option<String>,
        libsql_metadata_namespace: Option<String>,
        libsql_tenant_namespace_prefix: Option<String>,
        libsql_replica_cache_dir: PathBuf,
    },
    Postgres {
        control_data_dir: PathBuf,
        postgres_url: String,
        postgres_metadata_schema: Option<String>,
        postgres_tenant_schema_prefix: Option<String>,
        postgres_min_connections: Option<usize>,
        postgres_max_connections: Option<usize>,
    },
    MySql {
        control_data_dir: PathBuf,
        mysql_url: String,
        mysql_metadata_database: Option<String>,
        mysql_tenant_database_prefix: Option<String>,
        mysql_min_connections: Option<usize>,
        mysql_max_connections: Option<usize>,
    },
}

impl ResolvedTenantProviderConfig {
    fn from_inputs(inputs: ResolvedPersistenceInputs) -> nimbus::Result<Self> {
        match inputs.tenant_provider {
            CliTenantProvider::Sqlite => {
                inputs.reject_external_provider_overrides()?;
                Ok(Self::Embedded {
                    data_dir: inputs.data_dir,
                    control_data_dir: inputs.control_data_dir,
                    provider_kind: EmbeddedProviderKind::Sqlite,
                })
            }
            CliTenantProvider::Redb => {
                inputs.reject_external_provider_overrides()?;
                Ok(Self::Embedded {
                    data_dir: inputs.data_dir,
                    control_data_dir: inputs.control_data_dir,
                    provider_kind: EmbeddedProviderKind::Redb,
                })
            }
            CliTenantProvider::LibsqlReplica => {
                inputs.reject_postgres_overrides()?;
                inputs.reject_mysql_overrides()?;
                let libsql_url = inputs.libsql_url.ok_or_else(|| {
                    Error::InvalidInput(
                        "--libsql-url, NIMBUS_LIBSQL_URL, or persistence.libsql_url is required when the tenant provider is libsql-replica"
                            .to_string(),
                    )
                })?;
                let libsql_replica_cache_dir = inputs.libsql_replica_cache_dir.ok_or_else(|| {
                    Error::InvalidInput(
                        "--libsql-replica-cache-dir, NIMBUS_LIBSQL_REPLICA_CACHE_DIR, or persistence.libsql_replica_cache_dir is required when the tenant provider is libsql-replica"
                            .to_string(),
                    )
                })?;
                let libsql_admin_url = inputs.libsql_admin_url.ok_or_else(|| {
                    Error::InvalidInput(
                        "--libsql-admin-url, NIMBUS_LIBSQL_ADMIN_URL, or persistence.libsql_admin_url is required when the tenant provider is libsql-replica"
                            .to_string(),
                    )
                })?;
                Ok(Self::LibsqlReplica {
                    control_data_dir: inputs.control_data_dir,
                    libsql_url,
                    libsql_auth_token: inputs.libsql_auth_token,
                    libsql_admin_url,
                    libsql_admin_auth_header: inputs.libsql_admin_auth_header,
                    libsql_metadata_namespace: inputs.libsql_metadata_namespace,
                    libsql_tenant_namespace_prefix: inputs.libsql_tenant_namespace_prefix,
                    libsql_replica_cache_dir,
                })
            }
            CliTenantProvider::Postgres => {
                inputs.reject_mysql_overrides()?;
                inputs.reject_libsql_replica_overrides()?;
                let postgres_url = inputs.postgres_url.ok_or_else(|| {
                    Error::InvalidInput(
                        "--postgres-url, NIMBUS_POSTGRES_URL, or persistence.postgres_url is required when the tenant provider is postgres"
                            .to_string(),
                    )
                })?;
                Ok(Self::Postgres {
                    control_data_dir: inputs.control_data_dir,
                    postgres_url,
                    postgres_metadata_schema: inputs.postgres_metadata_schema,
                    postgres_tenant_schema_prefix: inputs.postgres_tenant_schema_prefix,
                    postgres_min_connections: inputs.postgres_min_connections,
                    postgres_max_connections: inputs.postgres_max_connections,
                })
            }
            CliTenantProvider::Mysql => {
                inputs.reject_postgres_overrides()?;
                inputs.reject_libsql_replica_overrides()?;
                let mysql_url = inputs.mysql_url.ok_or_else(|| {
                    Error::InvalidInput(
                        "--mysql-url, NIMBUS_MYSQL_URL, or persistence.mysql_url is required when the tenant provider is mysql"
                            .to_string(),
                    )
                })?;
                Ok(Self::MySql {
                    control_data_dir: inputs.control_data_dir,
                    mysql_url,
                    mysql_metadata_database: inputs.mysql_metadata_database,
                    mysql_tenant_database_prefix: inputs.mysql_tenant_database_prefix,
                    mysql_min_connections: inputs.mysql_min_connections,
                    mysql_max_connections: inputs.mysql_max_connections,
                })
            }
        }
    }

    fn into_persistence_config(self) -> nimbus::Result<ServicePersistenceConfig> {
        match self {
            Self::Embedded {
                data_dir,
                control_data_dir,
                provider_kind,
            } => Ok(ServicePersistenceConfig {
                tenant_provider: nimbus::TenantProviderConfig::embedded(data_dir, provider_kind),
                control_plane: nimbus::ControlPlaneConfig::embedded_redb(control_data_dir),
                local_encryption: LocalEncryptionConfig::Disabled,
            }),
            Self::LibsqlReplica {
                control_data_dir,
                libsql_url,
                libsql_auth_token,
                libsql_admin_url,
                libsql_admin_auth_header,
                libsql_metadata_namespace,
                libsql_tenant_namespace_prefix,
                libsql_replica_cache_dir,
            } => {
                let mut config = ServicePersistenceConfig::libsql_replica(
                    control_data_dir,
                    libsql_url,
                    libsql_auth_token,
                    libsql_admin_url,
                    libsql_admin_auth_header,
                    libsql_replica_cache_dir,
                );
                if let nimbus::TenantRoutingConfig::NamespacePerTenant {
                    metadata_namespace,
                    tenant_namespace_prefix,
                    ..
                } = &mut config.tenant_provider.routing
                {
                    if let Some(value) = libsql_metadata_namespace {
                        *metadata_namespace = value;
                    }
                    if let Some(value) = libsql_tenant_namespace_prefix {
                        *tenant_namespace_prefix = value;
                    }
                }
                Ok(config)
            }
            Self::Postgres {
                control_data_dir,
                postgres_url,
                postgres_metadata_schema,
                postgres_tenant_schema_prefix,
                postgres_min_connections,
                postgres_max_connections,
            } => {
                let mut config = ServicePersistenceConfig::postgres(control_data_dir, postgres_url);
                if let nimbus::TenantRoutingConfig::SchemaPerTenant {
                    metadata_schema,
                    tenant_schema_prefix,
                } = &mut config.tenant_provider.routing
                {
                    if let Some(value) = postgres_metadata_schema {
                        *metadata_schema = value;
                    }
                    if let Some(value) = postgres_tenant_schema_prefix {
                        *tenant_schema_prefix = value;
                    }
                }
                config.tenant_provider.pool.min_connections = postgres_min_connections;
                config.tenant_provider.pool.max_connections = postgres_max_connections;
                Ok(config)
            }
            Self::MySql {
                control_data_dir,
                mysql_url,
                mysql_metadata_database,
                mysql_tenant_database_prefix,
                mysql_min_connections,
                mysql_max_connections,
            } => {
                let mut config = ServicePersistenceConfig::mysql(control_data_dir, mysql_url);
                if let nimbus::TenantRoutingConfig::DatabasePerTenant {
                    metadata_database,
                    tenant_database_prefix,
                } = &mut config.tenant_provider.routing
                {
                    if let Some(value) = mysql_metadata_database {
                        *metadata_database = value;
                    }
                    if let Some(value) = mysql_tenant_database_prefix {
                        *tenant_database_prefix = value;
                    }
                }
                config.tenant_provider.pool.min_connections = mysql_min_connections;
                config.tenant_provider.pool.max_connections = mysql_max_connections;
                Ok(config)
            }
        }
    }
}

impl ResolvedPersistenceInputs {
    fn from_sources(
        command: &StartCommand,
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
            // Encryption config
            encryption_key_provider: command
                .encryption_key_provider
                .or(env.encryption_key_provider)
                .or(file.encryption_key_provider),
            encryption_master_key_file: command
                .encryption_master_key_file
                .clone()
                .or_else(|| env.encryption_master_key_file.clone())
                .or_else(|| file.encryption_master_key_file.clone()),
            encryption_key_dir: command
                .encryption_key_dir
                .clone()
                .or_else(|| env.encryption_key_dir.clone())
                .or_else(|| file.encryption_key_dir.clone()),
            encryption_aws_kms_key_id: command
                .encryption_aws_kms_key_id
                .clone()
                .or_else(|| env.encryption_aws_kms_key_id.clone())
                .or_else(|| file.encryption_aws_kms_key_id.clone()),
            encryption_aws_region: command
                .encryption_aws_region
                .clone()
                .or_else(|| env.encryption_aws_region.clone())
                .or_else(|| file.encryption_aws_region.clone()),
            encryption_aws_endpoint_url: command
                .encryption_aws_endpoint_url
                .clone()
                .or_else(|| env.encryption_aws_endpoint_url.clone())
                .or_else(|| file.encryption_aws_endpoint_url.clone()),
        }
    }

    fn into_persistence_config(self) -> nimbus::Result<ServicePersistenceConfig> {
        let encryption_config =
            ResolvedEncryptionInputs::from_inputs(&self).into_local_encryption_config()?;
        let base_config =
            ResolvedTenantProviderConfig::from_inputs(self)?.into_persistence_config()?;
        let config = base_config.with_local_encryption(encryption_config);

        // Validate the encryption config against the provider
        config.validate_encryption().map_err(|error| {
            Error::InvalidInput(format!("encryption configuration error: {error}"))
        })?;

        Ok(config)
    }

    fn reject_external_provider_overrides(&self) -> nimbus::Result<()> {
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

    fn reject_postgres_overrides(&self) -> nimbus::Result<()> {
        if self.postgres_overrides_present() {
            return Err(Error::InvalidInput(
                "Postgres config requires --tenant-provider=postgres or the equivalent env/config setting"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_mysql_overrides(&self) -> nimbus::Result<()> {
        if self.mysql_overrides_present() {
            return Err(Error::InvalidInput(
                "MySQL config requires --tenant-provider=mysql or the equivalent env/config setting"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn reject_libsql_replica_overrides(&self) -> nimbus::Result<()> {
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

pub(crate) fn persistence_config_from_start_command(
    command: &StartCommand,
) -> nimbus::Result<ServicePersistenceConfig> {
    let config_path = command
        .config
        .clone()
        .or_else(|| std::env::var_os(CONFIG_FILE_ENV).map(PathBuf::from));
    let file_config = load_runtime_config_file(config_path.as_deref())?;
    let env = PersistenceEnv::load()?;
    persistence_config_from_sources(command, &file_config.persistence, &env)
}

pub(crate) fn persistence_config_from_sources(
    command: &StartCommand,
    file: &PersistenceFileConfig,
    env: &PersistenceEnv,
) -> nimbus::Result<ServicePersistenceConfig> {
    ResolvedPersistenceInputs::from_sources(command, file, env).into_persistence_config()
}

pub(super) fn control_data_dir_from_persistence_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        nimbus::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

pub(crate) fn load_runtime_config_file(path: Option<&Path>) -> nimbus::Result<RuntimeConfigFile> {
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

fn optional_env_tenant_provider(key: &str) -> nimbus::Result<Option<CliTenantProvider>> {
    std::env::var(key)
        .ok()
        .map(|value| CliTenantProvider::parse_name(&value))
        .transpose()
}

fn optional_env_key_provider(key: &str) -> nimbus::Result<Option<CliKeyProvider>> {
    std::env::var(key)
        .ok()
        .map(|value| CliKeyProvider::parse_name(&value))
        .transpose()
}

fn optional_env_usize(key: &str) -> nimbus::Result<Option<usize>> {
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
