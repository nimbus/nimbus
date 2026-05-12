//! Service-level encryption management.
//!
//! This module provides key provider initialization, manifest-backed runtime
//! wiring, and encryption diagnostics for the service layer.

mod key_provider;

pub use key_provider::InitializedKeyProvider;

use nimbus_core::{Error, Result};

use crate::persistence_config::{
    EncryptionConfigDescriptor, LocalEncryptionConfig, LocalPersistenceFamily,
    ServicePersistenceConfig,
};

/// Validates encryption config at startup and returns an initialized key provider
/// if encryption is enabled.
///
/// All local persistence families are fully supported:
/// - Embedded SQLite via SQLCipher
/// - Embedded redb via EncryptedFileBackend (AES-256-GCM-SIV per page)
/// - Control plane redb via EncryptedFileBackend (AES-256-GCM-SIV per page)
/// - libsql replica cache via SQLCipher
pub fn initialize_encryption(
    config: &ServicePersistenceConfig,
) -> Result<Option<InitializedKeyProvider>> {
    // First, validate the encryption config itself
    config
        .validate_encryption()
        .map_err(|error| Error::InvalidInput(error.to_string()))?;

    let Some(key_provider_config) = config.local_encryption.key_provider() else {
        // Encryption disabled - return None
        return Ok(None);
    };

    // Check for unsupported encryption paths and fail fast.
    // Each provider family requires specific wiring through the startup path.
    // Families that are not yet fully wired must fail fast to avoid silently
    // starting with encryption disabled.
    let encryptable = config.encryptable_families();
    for family in &encryptable {
        match family {
            LocalPersistenceFamily::EmbeddedRedb => {
                // Fully supported via EncryptedFileBackend with AES-256-GCM-SIV per page.
            }
            LocalPersistenceFamily::EmbeddedSqlite => {
                // Fully supported via SQLCipher through the config-based startup path.
            }
            LocalPersistenceFamily::ControlPlaneRedb => {
                // Fully supported via EncryptedFileBackend with AES-256-GCM-SIV per page.
            }
            LocalPersistenceFamily::LibsqlReplicaCache => {
                // Fully supported via SQLCipher through the config-based startup path.
            }
        }
    }

    // Initialize the key provider
    let provider = InitializedKeyProvider::from_config(key_provider_config)?;
    Ok(Some(provider))
}

/// Returns a diagnostics-safe descriptor for the service's encryption state.
#[allow(dead_code)]
pub fn encryption_descriptor(config: &LocalEncryptionConfig) -> EncryptionConfigDescriptor {
    config.descriptor()
}

/// Encryption state for service status reporting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EncryptionStatus {
    /// Whether local encryption is enabled.
    pub enabled: bool,
    /// Which local persistence families have encryption enabled.
    pub encrypted_families: Vec<LocalPersistenceFamily>,
    /// Diagnostics-safe descriptor of the encryption configuration.
    pub descriptor: EncryptionConfigDescriptor,
}

impl EncryptionStatus {
    /// Creates the encryption status from service config.
    pub fn from_config(config: &ServicePersistenceConfig) -> Self {
        let enabled = config.local_encryption.is_enabled();
        let encrypted_families = if enabled {
            config.encryptable_families()
        } else {
            Vec::new()
        };
        let descriptor = config.local_encryption.descriptor();

        Self {
            enabled,
            encrypted_families,
            descriptor,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::persistence_config::{
        LocalKeyProviderConfig, MasterKeyFileConfig, PersistenceDialect, PersistenceTopology,
        PoolConfig, ProviderCredentials, TenantProviderConfig, TenantRoutingConfig,
    };
    use tempfile::TempDir;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    fn sqlite_config_with_encryption(
        data_dir: PathBuf,
        encryption: LocalEncryptionConfig,
    ) -> ServicePersistenceConfig {
        ServicePersistenceConfig {
            tenant_provider: TenantProviderConfig {
                dialect: PersistenceDialect::Sqlite,
                topology: PersistenceTopology::EmbeddedStandalone,
                routing: TenantRoutingConfig::DirectoryPerTenant {
                    data_dir: data_dir.clone(),
                },
                pool: PoolConfig::default(),
                credentials: ProviderCredentials::None,
            },
            control_plane: crate::persistence_config::ControlPlaneConfig::EmbeddedRedb { data_dir },
            local_encryption: encryption,
        }
    }

    fn redb_config_with_encryption(
        data_dir: PathBuf,
        encryption: LocalEncryptionConfig,
    ) -> ServicePersistenceConfig {
        ServicePersistenceConfig {
            tenant_provider: TenantProviderConfig {
                dialect: PersistenceDialect::Redb,
                topology: PersistenceTopology::EmbeddedStandalone,
                routing: TenantRoutingConfig::DirectoryPerTenant {
                    data_dir: data_dir.clone(),
                },
                pool: PoolConfig::default(),
                credentials: ProviderCredentials::None,
            },
            control_plane: crate::persistence_config::ControlPlaneConfig::EmbeddedRedb { data_dir },
            local_encryption: encryption,
        }
    }

    #[test]
    fn test_initialize_encryption_disabled() {
        let dir = TempDir::new().unwrap();
        let config = sqlite_config_with_encryption(
            dir.path().to_path_buf(),
            LocalEncryptionConfig::Disabled,
        );

        let provider = initialize_encryption(&config).unwrap();
        assert!(provider.is_none());
    }

    #[test]
    fn test_initialize_encryption_enabled_sqlite() {
        // Embedded SQLite encryption is fully supported via SQLCipher.
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");
        std::fs::write(&key_path, test_key()).unwrap();

        let config = sqlite_config_with_encryption(
            dir.path().to_path_buf(),
            LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
                MasterKeyFileConfig { path: key_path },
            )),
        );

        let provider = initialize_encryption(&config).unwrap();
        assert!(provider.is_some());
    }

    #[test]
    fn test_initialize_encryption_redb_succeeds() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");
        std::fs::write(&key_path, test_key()).unwrap();

        let config = redb_config_with_encryption(
            dir.path().to_path_buf(),
            LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
                MasterKeyFileConfig { path: key_path },
            )),
        );

        let provider = initialize_encryption(&config).unwrap();
        assert!(provider.is_some());
    }

    fn libsql_replica_config_with_encryption(
        control_data_dir: PathBuf,
        replica_cache_dir: PathBuf,
        encryption: LocalEncryptionConfig,
    ) -> ServicePersistenceConfig {
        ServicePersistenceConfig {
            tenant_provider: TenantProviderConfig {
                dialect: PersistenceDialect::Sqlite,
                topology: PersistenceTopology::ExternalPrimaryWithReplicas,
                routing: TenantRoutingConfig::NamespacePerTenant {
                    metadata_namespace: "test_provider".to_string(),
                    tenant_namespace_prefix: "test_tenant_".to_string(),
                    replica_cache_dir,
                },
                pool: PoolConfig::default(),
                credentials: ProviderCredentials::LibsqlReplica {
                    primary_url: "http://localhost:8080".to_string(),
                    auth_token: None,
                    admin_api_url: "http://localhost:8081".to_string(),
                    admin_auth_header: None,
                },
            },
            control_plane: crate::persistence_config::ControlPlaneConfig::EmbeddedRedb {
                data_dir: control_data_dir,
            },
            local_encryption: encryption,
        }
    }

    #[test]
    fn test_initialize_encryption_missing_key_file() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("missing.key");
        let cache_dir = dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Use libsql replica config since it's the only fully-wired path
        let config = libsql_replica_config_with_encryption(
            dir.path().to_path_buf(),
            cache_dir,
            LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
                MasterKeyFileConfig { path: key_path },
            )),
        );

        let result = initialize_encryption(&config);
        match result {
            Err(error) => assert!(error.to_string().contains("not found")),
            Ok(_) => panic!("expected error for missing key file"),
        }
    }

    #[test]
    fn test_initialize_encryption_enabled_libsql_replica() {
        // libsql replica is the fully-wired path for local encryption
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");
        std::fs::write(&key_path, test_key()).unwrap();
        let cache_dir = dir.path().join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let config = libsql_replica_config_with_encryption(
            dir.path().to_path_buf(),
            cache_dir,
            LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
                MasterKeyFileConfig { path: key_path },
            )),
        );

        let provider = initialize_encryption(&config).unwrap();
        assert!(provider.is_some());
    }

    #[test]
    fn test_encryption_status_disabled() {
        let dir = TempDir::new().unwrap();
        let config = sqlite_config_with_encryption(
            dir.path().to_path_buf(),
            LocalEncryptionConfig::Disabled,
        );

        let status = EncryptionStatus::from_config(&config);
        assert!(!status.enabled);
        assert!(status.encrypted_families.is_empty());
        assert_eq!(status.descriptor, EncryptionConfigDescriptor::Disabled);
    }

    #[test]
    fn test_encryption_status_enabled() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");

        let config = sqlite_config_with_encryption(
            dir.path().to_path_buf(),
            LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
                MasterKeyFileConfig {
                    path: key_path.clone(),
                },
            )),
        );

        let status = EncryptionStatus::from_config(&config);
        assert!(status.enabled);
        assert!(
            status
                .encrypted_families
                .contains(&LocalPersistenceFamily::EmbeddedSqlite)
        );
        assert!(
            status
                .encrypted_families
                .contains(&LocalPersistenceFamily::ControlPlaneRedb)
        );
    }
}
