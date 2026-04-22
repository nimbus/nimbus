//! Key provider initialization for local encryption.

use std::sync::Arc;

use neovex_core::{Error, Result};
#[cfg(feature = "aws-kms")]
use neovex_storage::AwsKmsKeyProvider;
use neovex_storage::{
    KeyDirectoryProvider, LocalKeyProvider, LocalKeyProviderError, MasterKeyFileProvider,
};

use crate::persistence_config::{
    AwsKmsConfig, KeyDirectoryConfig, LocalKeyProviderConfig, MasterKeyFileConfig,
};

/// Initialized key provider ready for manifest-backed DEK unwraps.
#[derive(Clone)]
pub struct InitializedKeyProvider {
    provider: Arc<dyn LocalKeyProvider>,
}

impl InitializedKeyProvider {
    /// Initializes a key provider from configuration.
    pub fn from_config(config: &LocalKeyProviderConfig) -> Result<Self> {
        let provider = match config {
            LocalKeyProviderConfig::MasterKeyFile(config) => {
                Arc::new(Self::from_master_key_file(config)?) as Arc<dyn LocalKeyProvider>
            }
            LocalKeyProviderConfig::KeyDirectory(config) => {
                Arc::new(Self::from_key_directory(config)?) as Arc<dyn LocalKeyProvider>
            }
            LocalKeyProviderConfig::AwsKms(config) => Self::from_aws_kms(config)?,
        };
        Ok(Self { provider })
    }

    fn from_master_key_file(config: &MasterKeyFileConfig) -> Result<MasterKeyFileProvider> {
        MasterKeyFileProvider::new(config.path.clone()).map_err(map_local_key_provider_error)
    }

    fn from_key_directory(config: &KeyDirectoryConfig) -> Result<KeyDirectoryProvider> {
        KeyDirectoryProvider::new(config.path.clone()).map_err(map_local_key_provider_error)
    }

    #[cfg(feature = "aws-kms")]
    fn from_aws_kms(config: &AwsKmsConfig) -> Result<Arc<dyn LocalKeyProvider>> {
        Ok(Arc::new(
            AwsKmsKeyProvider::new(
                config.key_id.clone(),
                config.region.clone(),
                config.endpoint_url.clone(),
            )
            .map_err(map_local_key_provider_error)?,
        ) as Arc<dyn LocalKeyProvider>)
    }

    #[cfg(not(feature = "aws-kms"))]
    fn from_aws_kms(_config: &AwsKmsConfig) -> Result<Arc<dyn LocalKeyProvider>> {
        Err(Error::InvalidInput(
            "aws-kms support is not enabled in this build; rebuild with the aws-kms feature"
                .to_string(),
        ))
    }

    pub fn provider(&self) -> Arc<dyn LocalKeyProvider> {
        self.provider.clone()
    }
}

fn map_local_key_provider_error(error: LocalKeyProviderError) -> Error {
    match error {
        LocalKeyProviderError::MasterKeyReadError { path, source } => {
            if source.kind() == std::io::ErrorKind::NotFound {
                Error::InvalidInput(format!("master key file not found: {}", path.display()))
            } else if source.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied(format!(
                    "cannot read master key file (check permissions): {}",
                    path.display()
                ))
            } else {
                Error::Internal(format!(
                    "failed to read master key file {}: {source}",
                    path.display()
                ))
            }
        }
        LocalKeyProviderError::InvalidMasterKeySize { path, actual } => {
            Error::InvalidInput(format!(
                "master key file {} must contain exactly 32 bytes, found {actual}",
                path.display()
            ))
        }
        LocalKeyProviderError::KeyDirectoryReadError { path, source } => {
            if source.kind() == std::io::ErrorKind::NotFound {
                Error::InvalidInput(format!("key directory not found: {}", path.display()))
            } else if source.kind() == std::io::ErrorKind::PermissionDenied {
                Error::PermissionDenied(format!(
                    "cannot read key directory (check permissions): {}",
                    path.display()
                ))
            } else {
                Error::Internal(format!(
                    "failed to read key directory {}: {source}",
                    path.display()
                ))
            }
        }
        LocalKeyProviderError::KeyFileNotFound { path } => {
            Error::InvalidInput(format!("key file not found: {}", path.display()))
        }
        LocalKeyProviderError::InvalidKeyFileSize { path, actual } => Error::InvalidInput(format!(
            "key file {} must contain exactly 32 bytes, found {actual}",
            path.display()
        )),
        LocalKeyProviderError::AwsKmsKeyNotFound { key_id } => {
            Error::InvalidInput(format!("aws kms key not found: {key_id}"))
        }
        LocalKeyProviderError::AwsKmsConfigurationError { message } => {
            Error::InvalidInput(format!("aws kms configuration error: {message}"))
        }
        LocalKeyProviderError::AwsKmsAuthError { message }
        | LocalKeyProviderError::AwsKmsPermissionDenied { message, .. } => {
            Error::PermissionDenied(message)
        }
        LocalKeyProviderError::AwsKmsNetworkError { operation, message } => Error::Internal(
            format!("aws kms network error during {operation}: {message}"),
        ),
        LocalKeyProviderError::AwsKmsOperationError { operation, message } => {
            Error::Internal(format!("aws kms {operation} failed: {message}"))
        }
        LocalKeyProviderError::UnwrapError { message } => Error::PermissionDenied(message),
        LocalKeyProviderError::WrapError { message }
        | LocalKeyProviderError::UnsupportedCipher { cipher: message }
        | LocalKeyProviderError::RandomError { message } => Error::Internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_root_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn test_from_master_key_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("master.key");
        std::fs::write(&path, test_root_key()).unwrap();

        let config = MasterKeyFileConfig { path };
        let provider = InitializedKeyProvider::from_master_key_file(&config).unwrap();

        assert_eq!(
            provider.kind().to_string(),
            format!("master-key-file:{}", config.path.display())
        );
    }

    #[test]
    fn test_from_key_directory() {
        let dir = TempDir::new().unwrap();
        let config = KeyDirectoryConfig {
            path: dir.path().to_path_buf(),
        };

        let provider = InitializedKeyProvider::from_key_directory(&config).unwrap();
        assert_eq!(
            provider.kind().to_string(),
            format!("key-dir:{}", config.path.display())
        );
    }

    #[test]
    fn test_missing_master_key_file_reports_not_found() {
        let dir = TempDir::new().unwrap();
        let config = MasterKeyFileConfig {
            path: dir.path().join("missing.key"),
        };

        let result = InitializedKeyProvider::from_master_key_file(&config);
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error for missing key file"),
        };
        assert!(error.to_string().contains("not found"));
    }

    #[cfg(not(feature = "aws-kms"))]
    #[test]
    fn test_aws_kms_requires_feature_when_disabled() {
        let config = AwsKmsConfig {
            key_id: "alias/neovex-test".to_string(),
            region: Some("us-east-1".to_string()),
            endpoint_url: None,
        };

        let result = InitializedKeyProvider::from_config(&LocalKeyProviderConfig::AwsKms(config));
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error for unimplemented aws-kms"),
        };
        assert!(error.to_string().contains("aws-kms support is not enabled"));
    }
}
