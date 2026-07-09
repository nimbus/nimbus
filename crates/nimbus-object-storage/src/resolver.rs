use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nimbus_blob::{
    BlobCloudConfig, BlobHash, BlobS3Credentials, BlobStore, EncryptedBlobStore, LocalPackStore,
    LocalPackStoreOptions, ObjectStoreBlobStore, PlacementBlobStore, PlacementMode,
};
use nimbus_core::{Error, Result, StorageErrorKind, TenantId};
use nimbus_crypto::{
    FramedBlobKey, LocalKeySubject, ManifestCipher, MasterKeyFileProvider,
    resolve_subject_encryption_key,
};
use nimbus_engine::Engine;
use nimbus_storage::{
    ObjectPlacement, ObjectStorePlacementTarget, ObjectStoreProviderCredentials,
    ObjectStoreProviderKind, PlacementPolicy,
};
use rand::RngCore;

use crate::config::ObjectStorageConfig;
use crate::credentials::{NoObjectStoreCredentialResolver, ObjectStoreCredentialResolver};

const DEFAULT_MASTER_KEY_FILE: &str = "object-storage.master.key";
const BLOB_KEY_PROTECTED_NAME: &str = "blob-key";

/// Resolves per-tenant placement policy into byte-plane store composition.
#[derive(Clone)]
pub struct ObjectStorageResolver {
    engine: Arc<Engine>,
    config: ObjectStorageConfig,
    credentials: Arc<dyn ObjectStoreCredentialResolver>,
    local_stores: Arc<Mutex<HashMap<TenantId, Arc<dyn BlobStore>>>>,
}

impl ObjectStorageResolver {
    /// Builds a resolver with local-only default placement.
    pub fn new(engine: Arc<Engine>) -> Self {
        Self::with_config(engine, ObjectStorageConfig::default())
    }

    /// Builds a resolver with explicit server-default placement.
    pub fn with_config(engine: Arc<Engine>, config: ObjectStorageConfig) -> Self {
        Self {
            engine,
            config,
            credentials: Arc::new(NoObjectStoreCredentialResolver),
            local_stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a resolver with the supplied secret lookup seam.
    pub fn with_credential_resolver(
        mut self,
        credentials: Arc<dyn ObjectStoreCredentialResolver>,
    ) -> Self {
        self.credentials = credentials;
        self
    }

    /// Returns the policy currently effective for `tenant`.
    pub fn effective_policy(&self, tenant: &TenantId) -> Result<PlacementPolicy> {
        Ok(self
            .engine
            .object_placement(tenant)?
            .map(|placement: ObjectPlacement| placement.policy)
            .unwrap_or_else(|| self.config.default_policy().clone()))
    }

    /// Resolves the tenant's byte-plane store for the current effective policy.
    pub fn blob_store(&self, tenant: &TenantId) -> Result<Arc<dyn BlobStore>> {
        self.blob_store_for_policy(tenant, self.effective_policy(tenant)?)
    }

    /// Resolves a byte-plane store for an explicit policy.
    pub fn blob_store_for_policy(
        &self,
        tenant: &TenantId,
        policy: PlacementPolicy,
    ) -> Result<Arc<dyn BlobStore>> {
        let local = self.local_store(tenant)?;
        match policy {
            PlacementPolicy::LocalOnly => Ok(local),
            PlacementPolicy::Mirror {
                target,
                require_ack,
            } => Ok(Arc::new(PlacementBlobStore::new(
                local,
                PlacementMode::Mirror {
                    mirror: self.remote_store(tenant, &target)?,
                    require_ack,
                },
            ))),
            PlacementPolicy::Tier { target } => Ok(Arc::new(PlacementBlobStore::new(
                local,
                PlacementMode::Tier {
                    cold: self.remote_store(tenant, &target)?,
                },
            ))),
            PlacementPolicy::CloudPrimary { target } => Ok(Arc::new(PlacementBlobStore::new(
                local,
                PlacementMode::CloudPrimary {
                    cloud: self.remote_store(tenant, &target)?,
                },
            ))),
        }
    }

    /// Returns the local pack root used for `tenant`.
    pub fn object_blob_root(&self, tenant: &TenantId) -> PathBuf {
        object_blob_root(self.engine.data_dir(), tenant)
    }

    /// Returns the default or configured master-key file used for object blobs.
    pub fn object_master_key_path(&self) -> PathBuf {
        self.config
            .master_key_file()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| object_master_key_path(self.engine.data_dir()))
    }

    /// Returns the protected sidecar path used to wrap one tenant blob DEK.
    pub fn object_blob_key_path(&self, tenant: &TenantId) -> PathBuf {
        object_blob_key_path(self.engine.data_dir(), tenant)
    }

    fn local_store(&self, tenant: &TenantId) -> Result<Arc<dyn BlobStore>> {
        {
            let stores = self.local_stores.lock().map_err(|_| {
                Error::storage(
                    StorageErrorKind::Other,
                    "object-storage local pack store cache lock poisoned",
                )
            })?;
            if let Some(store) = stores.get(tenant) {
                return Ok(store.clone());
            }
        }

        // Bind the pack root to this tenant: a root provisioned for tenant A
        // refuses to open as tenant B (format-marker identity), and the
        // exclusive root lock makes any second live writable handle Busy.
        let store: Arc<dyn BlobStore> = Arc::new(EncryptedBlobStore::new(
            LocalPackStore::open_with_options(
                self.object_blob_root(tenant),
                LocalPackStoreOptions {
                    identity: Some(tenant_root_identity(tenant)),
                    ..LocalPackStoreOptions::default()
                },
            )?,
            self.tenant_blob_key(tenant)?,
        ));
        let mut stores = self.local_stores.lock().map_err(|_| {
            Error::storage(
                StorageErrorKind::Other,
                "object-storage local pack store cache lock poisoned",
            )
        })?;
        if let Some(existing) = stores.get(tenant) {
            return Ok(existing.clone());
        }
        stores.insert(tenant.clone(), store.clone());
        Ok(store)
    }

    fn remote_store(
        &self,
        tenant: &TenantId,
        target: &ObjectStorePlacementTarget,
    ) -> Result<Arc<dyn BlobStore>> {
        let config = self.blob_cloud_config(target)?;
        let store = ObjectStoreBlobStore::from_cloud_config(config, target.prefix.as_str())?;
        Ok(Arc::new(EncryptedBlobStore::new(
            store,
            self.tenant_blob_key(tenant)?,
        )))
    }

    /// Turns policy config (env vars, secret-ref resolution, data-dir-relative
    /// local roots) into the Nimbus-owned [`BlobCloudConfig`] `nimbus-blob`
    /// accepts. All provider-selection and credential-resolution *policy*
    /// stays here; `nimbus-blob` only owns the mechanical
    /// `object_store::ObjectStore` construction (SR5).
    fn blob_cloud_config(&self, target: &ObjectStorePlacementTarget) -> Result<BlobCloudConfig> {
        match target.provider {
            ObjectStoreProviderKind::Memory => Ok(BlobCloudConfig::Memory),
            ObjectStoreProviderKind::Local => Ok(BlobCloudConfig::Local {
                root: local_object_store_root(self.engine.data_dir(), target),
            }),
            ObjectStoreProviderKind::S3 => {
                let (credentials, secret_ref_token) = self.blob_s3_credentials(target)?;
                Ok(BlobCloudConfig::S3 {
                    bucket: target.bucket.clone(),
                    region: target.region.clone().or_else(env_region),
                    endpoint: target.endpoint.clone().or_else(env_s3_endpoint),
                    credentials,
                    // `AWS_SESSION_TOKEN`, when set, overrides any token a
                    // secret-ref credential carried.
                    session_token: std::env::var("AWS_SESSION_TOKEN").ok().or(secret_ref_token),
                })
            }
            ObjectStoreProviderKind::Gcs | ObjectStoreProviderKind::Azure => {
                Err(Error::InvalidInput(format!(
                    "object placement provider {:?} is not enabled in this build; use s3, local, or memory",
                    target.provider
                )))
            }
        }
    }

    /// Resolves S3 credentials plus any inline session token they carry.
    fn blob_s3_credentials(
        &self,
        target: &ObjectStorePlacementTarget,
    ) -> Result<(BlobS3Credentials, Option<String>)> {
        match &target.credentials {
            ObjectStoreProviderCredentials::Anonymous => Ok((BlobS3Credentials::Anonymous, None)),
            ObjectStoreProviderCredentials::Environment => Ok((
                BlobS3Credentials::Keys {
                    access_key_id: required_env("AWS_ACCESS_KEY_ID", "S3 object placement")?,
                    secret_access_key: required_env(
                        "AWS_SECRET_ACCESS_KEY",
                        "S3 object placement",
                    )?,
                },
                None,
            )),
            ObjectStoreProviderCredentials::SecretRef { id } => {
                let secret = self.credentials.resolve_object_store_secret(id)?;
                Ok((
                    BlobS3Credentials::Keys {
                        access_key_id: secret.access_key_id,
                        secret_access_key: secret.secret_access_key,
                    },
                    secret.session_token,
                ))
            }
        }
    }

    fn tenant_blob_key(&self, tenant: &TenantId) -> Result<FramedBlobKey> {
        let master_key_path = self.object_master_key_path();
        ensure_object_master_key_file(&master_key_path)?;
        let provider = MasterKeyFileProvider::new(master_key_path.clone()).map_err(|error| {
            Error::InvalidInput(format!(
                "open object-storage master key file {}: {error}",
                master_key_path.display()
            ))
        })?;
        let protected_path = self.object_blob_key_path(tenant);
        if let Some(parent) = protected_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!(
                        "create object blob key parent {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
        let subject = LocalKeySubject::object_blob_store(tenant.clone(), BLOB_KEY_PROTECTED_NAME);
        let data_key = resolve_subject_encryption_key(
            &protected_path,
            &provider,
            &subject,
            ManifestCipher::FramedBlobAes256GcmSiv,
        )?;
        Ok(FramedBlobKey::new(data_key))
    }
}

/// Returns the local byte-plane root for one tenant under a deployment data dir.
pub fn object_blob_root(data_dir: &Path, tenant: &TenantId) -> PathBuf {
    data_dir.join("object-blobs").join(tenant.as_str())
}

/// Root identity a tenant's pack root is bound to (BLAKE3 of the tenant id).
pub fn tenant_root_identity(tenant: &TenantId) -> [u8; 32] {
    *BlobHash::of(tenant.as_str().as_bytes()).as_bytes()
}

/// Returns the default object-storage master-key file under a deployment data dir.
pub fn object_master_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keys").join(DEFAULT_MASTER_KEY_FILE)
}

/// Returns the protected path whose sidecar wraps one tenant's blob DEK.
pub fn object_blob_key_path(data_dir: &Path, tenant: &TenantId) -> PathBuf {
    object_blob_root(data_dir, tenant).join(BLOB_KEY_PROTECTED_NAME)
}

/// Computes the local byte-plane root a `Local` placement target resolves to.
/// `nimbus-blob`'s `from_cloud_config` is responsible for creating it.
fn local_object_store_root(data_dir: &Path, target: &ObjectStorePlacementTarget) -> PathBuf {
    target
        .endpoint
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("object-store-targets").join(&target.bucket))
}

fn ensure_object_master_key_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            Error::storage(
                StorageErrorKind::Io,
                format!(
                    "create object-storage master-key parent {}: {error}",
                    parent.display()
                ),
            )
        })?;
        set_private_dir_permissions(parent)?;
    }

    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key).map_err(|error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!(
                        "write object-storage master key {}: {error}",
                        path.display()
                    ),
                )
            })?;
            file.sync_data().map_err(|error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!("sync object-storage master key {}: {error}", path.display()),
                )
            })?;
            set_private_file_permissions(path)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(Error::storage(
            StorageErrorKind::Io,
            format!(
                "create object-storage master key {}: {error}",
                path.display()
            ),
        )),
    }
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    // Off unix the key file relies on the platform's default ACLs; Windows
    // ACL tightening is windows-machine-support-plan territory.
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!(
                        "set object-storage master-key permissions {}: {error}",
                        path.display()
                    ),
                )
            },
        )?;
    }
    Ok(())
}

fn set_private_dir_permissions(path: &Path) -> Result<()> {
    // Off unix the key directory relies on the platform's default ACLs;
    // Windows ACL tightening is windows-machine-support-plan territory.
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                Error::storage(
                    StorageErrorKind::Io,
                    format!(
                        "set object-storage key directory permissions {}: {error}",
                        path.display()
                    ),
                )
            },
        )?;
    }
    Ok(())
}

fn env_region() -> Option<String> {
    std::env::var("AWS_REGION")
        .ok()
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
}

pub(crate) fn env_s3_endpoint() -> Option<String> {
    std::env::var("AWS_ENDPOINT_URL_S3")
        .ok()
        .or_else(|| std::env::var("AWS_ENDPOINT").ok())
}

fn required_env(key: &str, context: &str) -> Result<String> {
    std::env::var(key).map_err(|_| {
        Error::InvalidInput(format!(
            "{} requires {} or secret-ref credentials",
            context, key
        ))
    })
}
