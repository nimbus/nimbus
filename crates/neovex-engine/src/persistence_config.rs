use std::path::PathBuf;

use neovex_core::Error;
use neovex_storage::EmbeddedProviderKind;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePersistenceConfig {
    pub tenant_provider: TenantProviderConfig,
    pub control_plane: ControlPlaneConfig,
    pub local_encryption: LocalEncryptionConfig,
}

/// Configuration for optional encryption at rest of Neovex-owned local files.
///
/// This covers:
/// - Embedded SQLite tenant databases
/// - Embedded redb tenant databases
/// - The retained redb control-plane database
/// - Local libsql replica cache files
///
/// External providers (Postgres, MySQL, remote libsql/Turso primary) manage their
/// own at-rest encryption; Neovex does not claim to encrypt those.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LocalEncryptionConfig {
    /// No local encryption. Current plaintext behavior.
    #[default]
    Disabled,
    /// Local encryption enabled with a configured key provider.
    Enabled(LocalKeyProviderConfig),
}

impl LocalEncryptionConfig {
    /// Returns `true` if local encryption is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    /// Returns the key provider config if encryption is enabled.
    pub fn key_provider(&self) -> Option<&LocalKeyProviderConfig> {
        match self {
            Self::Disabled => None,
            Self::Enabled(config) => Some(config),
        }
    }

    /// Returns a diagnostics-safe descriptor for the encryption config.
    pub fn descriptor(&self) -> EncryptionConfigDescriptor {
        match self {
            Self::Disabled => EncryptionConfigDescriptor::Disabled,
            Self::Enabled(config) => EncryptionConfigDescriptor::Enabled(config.descriptor()),
        }
    }
}

/// Key provider configuration for local encryption.
///
/// The key provider determines how Neovex wraps per-database data-encryption
/// keys (DEKs). The same key provider is used across all Neovex-owned local
/// databases and persisted encrypted artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalKeyProviderConfig {
    /// Single master key file wraps per-subject DEKs via HKDF derivation.
    ///
    /// This is the recommended self-hosted opt-in default because it:
    /// - Requires one operator-managed 32-byte root key outside the data directory
    /// - Avoids per-tenant key sprawl for small deployments
    /// - Still derives or wraps per-subject keys so each protected local object
    ///   has an independent DEK
    MasterKeyFile(MasterKeyFileConfig),

    /// Explicit per-subject or per-role key files for advanced deployments.
    KeyDirectory(KeyDirectoryConfig),

    /// AWS KMS envelope encryption for enterprise-managed keys.
    ///
    /// This reuses the shared manifest-backed wrapped-DEK contract; AWS KMS
    /// changes the wrapping provider, not the database identity model.
    AwsKms(AwsKmsConfig),
}

impl LocalKeyProviderConfig {
    /// Returns a diagnostics-safe descriptor for the key provider.
    pub fn descriptor(&self) -> KeyProviderDescriptor {
        match self {
            Self::MasterKeyFile(config) => KeyProviderDescriptor::MasterKeyFile {
                path: config.path.display().to_string(),
            },
            Self::KeyDirectory(config) => KeyProviderDescriptor::KeyDirectory {
                path: config.path.display().to_string(),
            },
            Self::AwsKms(config) => KeyProviderDescriptor::AwsKms {
                key_id: config.key_id.clone(),
                region: config.region.clone(),
            },
        }
    }
}

/// Configuration for the `master-key-file` key provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterKeyFileConfig {
    /// Path to the master key file containing a 32-byte root key.
    ///
    /// This file should be outside the data directory and have restricted
    /// permissions. Neovex reads it at startup but does not store the key
    /// material on disk anywhere else.
    pub path: PathBuf,
}

/// Configuration for the `key-dir` key provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDirectoryConfig {
    /// Directory containing per-subject or per-role key files.
    ///
    /// Key file naming follows a predictable pattern based on the protected
    /// subject identity, allowing operators to manage keys per-tenant or
    /// per-role as needed.
    pub path: PathBuf,
}

/// Configuration for the AWS KMS key provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsKmsConfig {
    /// AWS KMS key ID (ARN or alias) used for envelope encryption.
    pub key_id: String,

    /// Optional AWS region override. If not specified, uses the default region
    /// from the AWS SDK credential chain.
    pub region: Option<String>,

    /// Optional endpoint URL override for testing, LocalStack, or VPC endpoints.
    pub endpoint_url: Option<String>,
}

/// Diagnostics-safe descriptor for the encryption config.
///
/// This is safe to include in status endpoints and logs because it does not
/// contain any key material or sensitive credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EncryptionConfigDescriptor {
    Disabled,
    Enabled(KeyProviderDescriptor),
}

/// Diagnostics-safe descriptor for a key provider.
///
/// This is safe to include in status endpoints and logs because it contains
/// only identifiers and paths, not key material or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum KeyProviderDescriptor {
    MasterKeyFile {
        path: String,
    },
    KeyDirectory {
        path: String,
    },
    AwsKms {
        key_id: String,
        region: Option<String>,
    },
}

impl std::fmt::Display for KeyProviderDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MasterKeyFile { path } => write!(f, "master-key-file:{path}"),
            Self::KeyDirectory { path } => write!(f, "key-dir:{path}"),
            Self::AwsKms { key_id, region } => {
                write!(f, "aws-kms:{key_id}")?;
                if let Some(region) = region {
                    write!(f, " (region={region})")?;
                }
                Ok(())
            }
        }
    }
}

/// Describes which local persistence families can be encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalPersistenceFamily {
    /// Embedded SQLite tenant databases.
    EmbeddedSqlite,
    /// Embedded redb tenant databases.
    EmbeddedRedb,
    /// The retained redb control-plane database.
    ControlPlaneRedb,
    /// Local libsql replica cache files.
    LibsqlReplicaCache,
}

impl LocalPersistenceFamily {
    /// Returns `true` if this family stores tenant data.
    pub fn is_tenant_data(&self) -> bool {
        matches!(
            self,
            Self::EmbeddedSqlite | Self::EmbeddedRedb | Self::LibsqlReplicaCache
        )
    }

    /// Returns `true` if this family stores control-plane data.
    pub fn is_control_plane(&self) -> bool {
        matches!(self, Self::ControlPlaneRedb)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceBootstrapPlan {
    pub(crate) service_data_dir: PathBuf,
    pub(crate) control_plane: ControlPlaneBootstrapPlan,
    pub(crate) tenant_provider: TenantProviderBootstrapPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlPlaneBootstrapPlan {
    EmbeddedRedb { data_dir: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TenantProviderBootstrapPlan {
    Embedded(EmbeddedTenantBootstrapPlan),
    Postgres(PostgresTenantBootstrapPlan),
    LibsqlReplica(LibsqlReplicaTenantBootstrapPlan),
    MySql(MySqlTenantBootstrapPlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbeddedTenantBootstrapPlan {
    pub(crate) provider_kind: EmbeddedProviderKind,
    pub(crate) data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostgresTenantBootstrapPlan {
    pub(crate) connection_string: String,
    pub(crate) metadata_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) pool: PoolConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibsqlReplicaTenantBootstrapPlan {
    pub(crate) primary_url: String,
    pub(crate) auth_token: Option<String>,
    pub(crate) admin_api_url: String,
    pub(crate) admin_auth_header: Option<String>,
    pub(crate) metadata_namespace: String,
    pub(crate) tenant_namespace_prefix: String,
    pub(crate) replica_cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MySqlTenantBootstrapPlan {
    pub(crate) connection_string: String,
    pub(crate) metadata_database: String,
    pub(crate) tenant_database_prefix: String,
    pub(crate) pool: PoolConfig,
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
            local_encryption: LocalEncryptionConfig::Disabled,
        }
    }

    pub fn postgres(
        control_data_dir: impl Into<PathBuf>,
        connection_string: impl Into<String>,
    ) -> Self {
        Self {
            tenant_provider: TenantProviderConfig::postgres(connection_string),
            control_plane: ControlPlaneConfig::embedded_redb(control_data_dir),
            local_encryption: LocalEncryptionConfig::Disabled,
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
            local_encryption: LocalEncryptionConfig::Disabled,
        }
    }

    pub fn mysql(
        control_data_dir: impl Into<PathBuf>,
        connection_string: impl Into<String>,
    ) -> Self {
        Self {
            tenant_provider: TenantProviderConfig::mysql(connection_string),
            control_plane: ControlPlaneConfig::embedded_redb(control_data_dir),
            local_encryption: LocalEncryptionConfig::Disabled,
        }
    }

    /// Sets the local encryption config for this persistence config.
    pub fn with_local_encryption(mut self, config: LocalEncryptionConfig) -> Self {
        self.local_encryption = config;
        self
    }

    pub(crate) fn bootstrap_plan(&self) -> neovex_core::Result<ServiceBootstrapPlan> {
        match (
            &self.tenant_provider.dialect,
            &self.tenant_provider.topology,
            &self.tenant_provider.routing,
            &self.control_plane,
        ) {
            (
                PersistenceDialect::Redb,
                PersistenceTopology::EmbeddedStandalone,
                TenantRoutingConfig::DirectoryPerTenant { data_dir },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => Ok(ServiceBootstrapPlan {
                service_data_dir: data_dir.clone(),
                control_plane: ControlPlaneBootstrapPlan::EmbeddedRedb {
                    data_dir: control_data_dir.clone(),
                },
                tenant_provider: TenantProviderBootstrapPlan::Embedded(
                    EmbeddedTenantBootstrapPlan {
                        provider_kind: EmbeddedProviderKind::Redb,
                        data_dir: data_dir.clone(),
                    },
                ),
            }),
            (
                PersistenceDialect::Sqlite,
                PersistenceTopology::EmbeddedStandalone,
                TenantRoutingConfig::DirectoryPerTenant { data_dir },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => Ok(ServiceBootstrapPlan {
                service_data_dir: data_dir.clone(),
                control_plane: ControlPlaneBootstrapPlan::EmbeddedRedb {
                    data_dir: control_data_dir.clone(),
                },
                tenant_provider: TenantProviderBootstrapPlan::Embedded(
                    EmbeddedTenantBootstrapPlan {
                        provider_kind: EmbeddedProviderKind::Sqlite,
                        data_dir: data_dir.clone(),
                    },
                ),
            }),
            (
                PersistenceDialect::Postgres,
                PersistenceTopology::ExternalPrimary,
                TenantRoutingConfig::SchemaPerTenant {
                    metadata_schema,
                    tenant_schema_prefix,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::ConnectionString(connection_string) =
                    &self.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "Postgres tenant persistence requires a connection string".to_string(),
                    ));
                };
                Ok(ServiceBootstrapPlan {
                    service_data_dir: control_data_dir.clone(),
                    control_plane: ControlPlaneBootstrapPlan::EmbeddedRedb {
                        data_dir: control_data_dir.clone(),
                    },
                    tenant_provider: TenantProviderBootstrapPlan::Postgres(
                        PostgresTenantBootstrapPlan {
                            connection_string: connection_string.clone(),
                            metadata_schema: metadata_schema.clone(),
                            tenant_schema_prefix: tenant_schema_prefix.clone(),
                            pool: self.tenant_provider.pool.clone(),
                        },
                    ),
                })
            }
            (
                PersistenceDialect::Sqlite,
                PersistenceTopology::ExternalPrimaryWithReplicas,
                TenantRoutingConfig::NamespacePerTenant {
                    metadata_namespace,
                    tenant_namespace_prefix,
                    replica_cache_dir,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::LibsqlReplica {
                    primary_url,
                    auth_token,
                    admin_api_url,
                    admin_auth_header,
                } = &self.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "Replica-connected SQLite tenant persistence requires a primary URL, optional primary auth token, and admin API configuration".to_string(),
                    ));
                };
                Ok(ServiceBootstrapPlan {
                    service_data_dir: control_data_dir.clone(),
                    control_plane: ControlPlaneBootstrapPlan::EmbeddedRedb {
                        data_dir: control_data_dir.clone(),
                    },
                    tenant_provider: TenantProviderBootstrapPlan::LibsqlReplica(
                        LibsqlReplicaTenantBootstrapPlan {
                            primary_url: primary_url.clone(),
                            auth_token: auth_token.clone(),
                            admin_api_url: admin_api_url.clone(),
                            admin_auth_header: admin_auth_header.clone(),
                            metadata_namespace: metadata_namespace.clone(),
                            tenant_namespace_prefix: tenant_namespace_prefix.clone(),
                            replica_cache_dir: replica_cache_dir.clone(),
                        },
                    ),
                })
            }
            (
                PersistenceDialect::MySql,
                PersistenceTopology::ExternalPrimary,
                TenantRoutingConfig::DatabasePerTenant {
                    metadata_database,
                    tenant_database_prefix,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::ConnectionString(connection_string) =
                    &self.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "MySQL tenant persistence requires a connection string".to_string(),
                    ));
                };
                Ok(ServiceBootstrapPlan {
                    service_data_dir: control_data_dir.clone(),
                    control_plane: ControlPlaneBootstrapPlan::EmbeddedRedb {
                        data_dir: control_data_dir.clone(),
                    },
                    tenant_provider: TenantProviderBootstrapPlan::MySql(MySqlTenantBootstrapPlan {
                        connection_string: connection_string.clone(),
                        metadata_database: metadata_database.clone(),
                        tenant_database_prefix: tenant_database_prefix.clone(),
                        pool: self.tenant_provider.pool.clone(),
                    }),
                })
            }
            _ => Err(Error::InvalidInput(
                "unsupported persistence config combination".to_string(),
            )),
        }
    }

    /// Returns which local persistence families are eligible for encryption
    /// based on the current provider configuration.
    pub fn encryptable_families(&self) -> Vec<LocalPersistenceFamily> {
        let mut families = vec![LocalPersistenceFamily::ControlPlaneRedb];

        match self.tenant_provider.dialect {
            PersistenceDialect::Sqlite => {
                match self.tenant_provider.topology {
                    PersistenceTopology::EmbeddedStandalone => {
                        families.push(LocalPersistenceFamily::EmbeddedSqlite);
                    }
                    PersistenceTopology::ExternalPrimaryWithReplicas => {
                        // libsql replica: local cache files are encryptable
                        families.push(LocalPersistenceFamily::LibsqlReplicaCache);
                    }
                    _ => {}
                }
            }
            PersistenceDialect::Redb => {
                if matches!(
                    self.tenant_provider.topology,
                    PersistenceTopology::EmbeddedStandalone
                ) {
                    families.push(LocalPersistenceFamily::EmbeddedRedb);
                }
            }
            PersistenceDialect::Postgres | PersistenceDialect::MySql => {
                // External providers: only control plane is encryptable locally
            }
        }

        families
    }

    /// Validates the encryption config against the provider configuration.
    ///
    /// Returns an error if encryption is requested for a provider path that
    /// is not supported.
    pub fn validate_encryption(&self) -> Result<(), EncryptionValidationError> {
        if !self.local_encryption.is_enabled() {
            return Ok(());
        }

        // Encryption is enabled, validate the key provider config
        let key_provider = self.local_encryption.key_provider().unwrap();
        match key_provider {
            LocalKeyProviderConfig::MasterKeyFile(config) => {
                if config.path.as_os_str().is_empty() {
                    return Err(EncryptionValidationError::EmptyKeyPath {
                        provider: "master-key-file".to_string(),
                    });
                }
            }
            LocalKeyProviderConfig::KeyDirectory(config) => {
                if config.path.as_os_str().is_empty() {
                    return Err(EncryptionValidationError::EmptyKeyPath {
                        provider: "key-dir".to_string(),
                    });
                }
            }
            LocalKeyProviderConfig::AwsKms(config) => {
                if config.key_id.is_empty() {
                    return Err(EncryptionValidationError::MissingAwsKmsKeyId);
                }
            }
        }

        Ok(())
    }
}

/// Errors that can occur during encryption config validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncryptionValidationError {
    /// A key path is empty.
    EmptyKeyPath { provider: String },
    /// AWS KMS key ID is missing.
    MissingAwsKmsKeyId,
}

impl std::fmt::Display for EncryptionValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKeyPath { provider } => {
                write!(f, "{provider} key path cannot be empty")
            }
            Self::MissingAwsKmsKeyId => {
                write!(f, "AWS KMS key ID is required")
            }
        }
    }
}

impl std::error::Error for EncryptionValidationError {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_bootstrap_plan_preserves_tenant_and_control_plane_dirs() {
        let config = ServicePersistenceConfig {
            tenant_provider: TenantProviderConfig::embedded(
                PathBuf::from("./tenant-data"),
                EmbeddedProviderKind::Sqlite,
            ),
            control_plane: ControlPlaneConfig::embedded_redb("./control-data"),
            local_encryption: LocalEncryptionConfig::Disabled,
        };

        let plan = config
            .bootstrap_plan()
            .expect("embedded config should map to a bootstrap plan");
        assert_eq!(plan.service_data_dir, PathBuf::from("./tenant-data"));
        assert_eq!(
            plan.control_plane,
            ControlPlaneBootstrapPlan::EmbeddedRedb {
                data_dir: PathBuf::from("./control-data"),
            }
        );
        assert_eq!(
            plan.tenant_provider,
            TenantProviderBootstrapPlan::Embedded(EmbeddedTenantBootstrapPlan {
                provider_kind: EmbeddedProviderKind::Sqlite,
                data_dir: PathBuf::from("./tenant-data"),
            })
        );
    }

    #[test]
    fn libsql_replica_bootstrap_plan_captures_routing_and_control_plane() {
        let config = ServicePersistenceConfig {
            tenant_provider: TenantProviderConfig {
                dialect: PersistenceDialect::Sqlite,
                topology: PersistenceTopology::ExternalPrimaryWithReplicas,
                routing: TenantRoutingConfig::NamespacePerTenant {
                    metadata_namespace: "meta_ns".to_string(),
                    tenant_namespace_prefix: "tenant_ns_".to_string(),
                    replica_cache_dir: PathBuf::from("./replica-cache"),
                },
                pool: PoolConfig::default(),
                credentials: ProviderCredentials::LibsqlReplica {
                    primary_url: "libsql://primary".to_string(),
                    auth_token: Some("token".to_string()),
                    admin_api_url: "https://admin.example.com".to_string(),
                    admin_auth_header: Some("Bearer token".to_string()),
                },
            },
            control_plane: ControlPlaneConfig::embedded_redb("./control-data"),
            local_encryption: LocalEncryptionConfig::Disabled,
        };

        let plan = config
            .bootstrap_plan()
            .expect("libsql replica config should map to a bootstrap plan");
        assert_eq!(plan.service_data_dir, PathBuf::from("./control-data"));
        assert_eq!(
            plan.control_plane,
            ControlPlaneBootstrapPlan::EmbeddedRedb {
                data_dir: PathBuf::from("./control-data"),
            }
        );
        assert_eq!(
            plan.tenant_provider,
            TenantProviderBootstrapPlan::LibsqlReplica(LibsqlReplicaTenantBootstrapPlan {
                primary_url: "libsql://primary".to_string(),
                auth_token: Some("token".to_string()),
                admin_api_url: "https://admin.example.com".to_string(),
                admin_auth_header: Some("Bearer token".to_string()),
                metadata_namespace: "meta_ns".to_string(),
                tenant_namespace_prefix: "tenant_ns_".to_string(),
                replica_cache_dir: PathBuf::from("./replica-cache"),
            })
        );
    }
}
