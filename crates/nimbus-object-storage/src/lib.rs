//! Native Nimbus object-storage control-plane resolver.
//!
//! This crate is deliberately not an S3 protocol crate. It turns persisted
//! placement policy plus operator configuration into byte-plane [`BlobStore`]
//! compositions that S3, Convex `_storage`, backup/restore, R2 compatibility,
//! and the future filesystem binder can share.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nimbus_blob::{
    BlobHash, BlobStore, LocalPackStore, ObjectStoreBlobStore, PlacementBlobStore, PlacementMode,
};
use nimbus_core::{Error, Result, StorageErrorKind, TenantId};
use nimbus_engine::Engine;
use nimbus_storage::{
    OBJECT_MANIFEST_TABLE, ObjectBlobLayout, ObjectManifest, ObjectPlacement,
    ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
    PlacementPolicy, PointInTimeRestoreArchive,
};

const MODE_ENV: &str = "NIMBUS_OBJECT_STORAGE_MODE";
const PROVIDER_ENV: &str = "NIMBUS_OBJECT_STORAGE_PROVIDER";
const BUCKET_ENV: &str = "NIMBUS_OBJECT_STORAGE_BUCKET";
const REGION_ENV: &str = "NIMBUS_OBJECT_STORAGE_REGION";
const ENDPOINT_ENV: &str = "NIMBUS_OBJECT_STORAGE_ENDPOINT";
const PREFIX_ENV: &str = "NIMBUS_OBJECT_STORAGE_PREFIX";
const CREDENTIALS_ENV: &str = "NIMBUS_OBJECT_STORAGE_CREDENTIALS";
const SECRET_REF_ENV: &str = "NIMBUS_OBJECT_STORAGE_SECRET_REF";
const REQUIRE_ACK_ENV: &str = "NIMBUS_OBJECT_STORAGE_REQUIRE_ACK";

/// Object-storage defaults resolved from programmatic config, env, or local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectStorageConfig {
    default_policy: PlacementPolicy,
}

impl Default for ObjectStorageConfig {
    fn default() -> Self {
        Self::local_only()
    }
}

impl ObjectStorageConfig {
    /// Creates a config with local packs as the default placement.
    pub fn local_only() -> Self {
        Self {
            default_policy: PlacementPolicy::LocalOnly,
        }
    }

    /// Creates a config with an explicit server-default placement policy.
    pub fn new(default_policy: PlacementPolicy) -> Self {
        Self { default_policy }
    }

    /// Returns the default placement policy used when a tenant has no override.
    pub fn default_policy(&self) -> &PlacementPolicy {
        &self.default_policy
    }

    /// Resolves object-storage config from process env unless a programmatic
    /// policy is supplied. Precedence is programmatic > env > local default.
    pub fn from_env(programmatic_default: Option<PlacementPolicy>) -> Result<Self> {
        Self::from_sources(programmatic_default, &ProcessEnv)
    }

    /// Resolves object-storage config from an injectable env source.
    pub fn from_sources(
        programmatic_default: Option<PlacementPolicy>,
        env: &dyn ObjectStorageEnv,
    ) -> Result<Self> {
        if let Some(policy) = programmatic_default {
            return Ok(Self::new(policy));
        }
        if let Some(policy) = policy_from_env(env)? {
            return Ok(Self::new(policy));
        }
        Ok(Self::local_only())
    }
}

/// Injectable environment source for deterministic config tests.
pub trait ObjectStorageEnv {
    fn get(&self, key: &str) -> Option<String>;
}

struct ProcessEnv;

impl ObjectStorageEnv for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Resolved secret material for an object-store placement target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectStoreSecret {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl ObjectStoreSecret {
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        session_token: Option<String>,
    ) -> Result<Self> {
        let access_key_id = access_key_id.into();
        let secret_access_key = secret_access_key.into();
        if access_key_id.trim().is_empty() {
            return Err(Error::InvalidInput(
                "object-store access key id is required".to_string(),
            ));
        }
        if secret_access_key.trim().is_empty() {
            return Err(Error::InvalidInput(
                "object-store secret access key is required".to_string(),
            ));
        }
        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

/// Secret lookup seam for provider `SecretRef` credentials.
pub trait ObjectStoreCredentialResolver: Send + Sync {
    fn resolve_object_store_secret(&self, id: &str) -> Result<ObjectStoreSecret>;
}

#[derive(Debug, Default)]
struct NoObjectStoreCredentialResolver;

impl ObjectStoreCredentialResolver for NoObjectStoreCredentialResolver {
    fn resolve_object_store_secret(&self, id: &str) -> Result<ObjectStoreSecret> {
        Err(Error::InvalidInput(format!(
            "object-store credential secret ref {id} cannot be resolved: no credential resolver configured"
        )))
    }
}

/// Resolves per-tenant placement policy into byte-plane store composition.
#[derive(Clone)]
pub struct ObjectStorageResolver {
    engine: Arc<Engine>,
    config: ObjectStorageConfig,
    credentials: Arc<dyn ObjectStoreCredentialResolver>,
    local_stores: Arc<Mutex<HashMap<TenantId, Arc<LocalPackStore>>>>,
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
            .unwrap_or_else(|| self.config.default_policy.clone()))
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
                    mirror: self.remote_store(&target)?,
                    require_ack,
                },
            ))),
            PlacementPolicy::Tier { target } => Ok(Arc::new(PlacementBlobStore::new(
                local,
                PlacementMode::Tier {
                    cold: self.remote_store(&target)?,
                },
            ))),
            PlacementPolicy::CloudPrimary { target } => Ok(Arc::new(PlacementBlobStore::new(
                local,
                PlacementMode::CloudPrimary {
                    cloud: self.remote_store(&target)?,
                },
            ))),
        }
    }

    /// Returns the local pack root used for `tenant`.
    pub fn object_blob_root(&self, tenant: &TenantId) -> PathBuf {
        object_blob_root(self.engine.data_dir(), tenant)
    }

    fn local_store(&self, tenant: &TenantId) -> Result<Arc<dyn BlobStore>> {
        let mut stores = self.local_stores.lock().map_err(|_| {
            Error::storage(
                StorageErrorKind::Other,
                "object-storage local pack store cache lock poisoned",
            )
        })?;
        if let Some(store) = stores.get(tenant) {
            let store: Arc<dyn BlobStore> = store.clone();
            return Ok(store);
        }
        let store = Arc::new(LocalPackStore::open(self.object_blob_root(tenant))?);
        stores.insert(tenant.clone(), store.clone());
        let store: Arc<dyn BlobStore> = store;
        Ok(store)
    }

    fn remote_store(&self, target: &ObjectStorePlacementTarget) -> Result<Arc<dyn BlobStore>> {
        let store: Arc<dyn object_store::ObjectStore> = match target.provider {
            ObjectStoreProviderKind::Memory => Arc::new(object_store::memory::InMemory::new()),
            ObjectStoreProviderKind::Local => {
                Arc::new(local_object_store(self.engine.data_dir(), target)?)
            }
            ObjectStoreProviderKind::S3 => Arc::new(self.s3_object_store(target)?),
            ObjectStoreProviderKind::Gcs | ObjectStoreProviderKind::Azure => {
                return Err(Error::InvalidInput(format!(
                    "object placement provider {:?} is not enabled in this build; use s3, local, or memory",
                    target.provider
                )));
            }
        };
        Ok(Arc::new(ObjectStoreBlobStore::new(
            store,
            target.prefix.as_str(),
        )))
    }

    fn s3_object_store(
        &self,
        target: &ObjectStorePlacementTarget,
    ) -> Result<object_store::aws::AmazonS3> {
        let mut builder = object_store::aws::AmazonS3Builder::new()
            .with_bucket_name(target.bucket.clone())
            .with_virtual_hosted_style_request(false);
        if let Some(region) = target.region.clone().or_else(env_region) {
            builder = builder.with_region(region);
        }
        if let Some(endpoint) = target.endpoint.clone().or_else(env_s3_endpoint) {
            builder = builder.with_endpoint(endpoint.clone());
            if endpoint.starts_with("http://") {
                builder = builder.with_allow_http(true);
            }
        }
        builder = match &target.credentials {
            ObjectStoreProviderCredentials::Anonymous => builder.with_skip_signature(true),
            ObjectStoreProviderCredentials::Environment => builder
                .with_access_key_id(required_env("AWS_ACCESS_KEY_ID", "S3 object placement")?)
                .with_secret_access_key(required_env(
                    "AWS_SECRET_ACCESS_KEY",
                    "S3 object placement",
                )?),
            ObjectStoreProviderCredentials::SecretRef { id } => {
                let secret = self.credentials.resolve_object_store_secret(id)?;
                let mut builder = builder
                    .with_access_key_id(secret.access_key_id)
                    .with_secret_access_key(secret.secret_access_key);
                if let Some(token) = secret.session_token {
                    builder = builder.with_token(token);
                }
                builder
            }
        };
        if let Ok(token) = std::env::var("AWS_SESSION_TOKEN") {
            builder = builder.with_token(token);
        }
        builder
            .build()
            .map_err(|error| Error::InvalidInput(format!("build S3 object placement: {error}")))
    }
}

/// Returns the local byte-plane root for one tenant under a deployment data dir.
pub fn object_blob_root(data_dir: &Path, tenant: &TenantId) -> PathBuf {
    data_dir.join("object-blobs").join(tenant.as_str())
}

/// Extracts committed object blob roots from a materialized PITR archive.
pub fn object_backup_roots(archive: &PointInTimeRestoreArchive) -> Result<Vec<BlobHash>> {
    if !archive.journal_tail.is_empty() {
        return Err(Error::InvalidInput(
            "object backup root extraction requires a materialized archive with an empty journal tail"
                .to_string(),
        ));
    }

    let mut roots = BTreeSet::new();
    for document in &archive.base_snapshot.documents {
        if document.table.as_str() != OBJECT_MANIFEST_TABLE {
            continue;
        }
        let manifest = ObjectManifest::from_document(document)?;
        match manifest.blob_layout {
            ObjectBlobLayout::Whole { blob_hash } => {
                roots.insert(BlobHash::from_hex(&blob_hash)?);
            }
            ObjectBlobLayout::Chunked { chunks } => {
                for chunk in chunks {
                    roots.insert(BlobHash::from_hex(&chunk.blob_hash)?);
                }
            }
        }
    }
    Ok(roots.into_iter().collect())
}

fn local_object_store(
    data_dir: &Path,
    target: &ObjectStorePlacementTarget,
) -> Result<object_store::local::LocalFileSystem> {
    let root = target
        .endpoint
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("object-store-targets").join(&target.bucket));
    std::fs::create_dir_all(&root).map_err(|error| {
        Error::storage(
            StorageErrorKind::Io,
            format!("create local object_store root {}: {error}", root.display()),
        )
    })?;
    object_store::local::LocalFileSystem::new_with_prefix(&root).map_err(|error| {
        Error::storage(
            StorageErrorKind::Io,
            format!("open local object_store root {}: {error}", root.display()),
        )
    })
}

fn policy_from_env(env: &dyn ObjectStorageEnv) -> Result<Option<PlacementPolicy>> {
    let Some(mode) = env.get(MODE_ENV) else {
        return Ok(None);
    };
    match normalize(&mode).as_str() {
        "local" | "local-only" => Ok(Some(PlacementPolicy::LocalOnly)),
        "mirror" => Ok(Some(PlacementPolicy::Mirror {
            target: target_from_env(env)?,
            require_ack: env
                .get(REQUIRE_ACK_ENV)
                .as_deref()
                .map(parse_bool)
                .transpose()?
                .unwrap_or(false),
        })),
        "tier" => Ok(Some(PlacementPolicy::Tier {
            target: target_from_env(env)?,
        })),
        "cloud-primary" => Ok(Some(PlacementPolicy::CloudPrimary {
            target: target_from_env(env)?,
        })),
        other => Err(Error::InvalidInput(format!(
            "{MODE_ENV} must be local, mirror, tier, or cloud-primary; got {other}"
        ))),
    }
}

fn target_from_env(env: &dyn ObjectStorageEnv) -> Result<ObjectStorePlacementTarget> {
    let provider =
        match normalize(&env.get(PROVIDER_ENV).unwrap_or_else(|| "s3".to_string())).as_str() {
            "s3" => ObjectStoreProviderKind::S3,
            "local" => ObjectStoreProviderKind::Local,
            "memory" => ObjectStoreProviderKind::Memory,
            "gcs" | "google-cloud-storage" => ObjectStoreProviderKind::Gcs,
            "azure" | "azure-blob" => ObjectStoreProviderKind::Azure,
            other => {
                return Err(Error::InvalidInput(format!(
                    "{PROVIDER_ENV} must be s3, local, memory, gcs, or azure; got {other}"
                )));
            }
        };
    let bucket = env.get(BUCKET_ENV).ok_or_else(|| {
        Error::InvalidInput(format!(
            "{BUCKET_ENV} is required for non-local object placement"
        ))
    })?;
    let mut target = ObjectStorePlacementTarget::new(provider, bucket, credentials_from_env(env)?)?;
    if let Some(region) = env.get(REGION_ENV) {
        target = target.with_region(region);
    }
    if let Some(endpoint) = env.get(ENDPOINT_ENV) {
        target = target.with_endpoint(endpoint);
    }
    if let Some(prefix) = env.get(PREFIX_ENV) {
        target = target.with_prefix(prefix);
    }
    Ok(target)
}

fn credentials_from_env(env: &dyn ObjectStorageEnv) -> Result<ObjectStoreProviderCredentials> {
    match normalize(
        &env.get(CREDENTIALS_ENV)
            .unwrap_or_else(|| "environment".to_string()),
    )
    .as_str()
    {
        "anonymous" => Ok(ObjectStoreProviderCredentials::Anonymous),
        "environment" | "env" => Ok(ObjectStoreProviderCredentials::Environment),
        "secret-ref" => {
            let id = env.get(SECRET_REF_ENV).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{SECRET_REF_ENV} is required when {CREDENTIALS_ENV}=secret-ref"
                ))
            })?;
            Ok(ObjectStoreProviderCredentials::SecretRef { id })
        }
        other => Err(Error::InvalidInput(format!(
            "{CREDENTIALS_ENV} must be anonymous, environment, or secret-ref; got {other}"
        ))),
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match normalize(value).as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(Error::InvalidInput(format!(
            "boolean env value must be true/false/1/0/yes/no; got {other}"
        ))),
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn env_region() -> Option<String> {
    std::env::var("AWS_REGION")
        .ok()
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
}

fn env_s3_endpoint() -> Option<String> {
    std::env::var("AWS_ENDPOINT_URL_S3")
        .ok()
        .or_else(|| std::env::var("AWS_ENDPOINT").ok())
}

fn required_env(key: &str, context: &str) -> Result<String> {
    std::env::var(key).map_err(|_| {
        Error::InvalidInput(format!(
            "{context} requires {key} or secret-ref credentials"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bytes::Bytes;
    use nimbus_storage::{ObjectChunkRef, ObjectManifestAttributes};
    use tempfile::tempdir;

    use super::*;

    struct MapEnv(BTreeMap<String, String>);

    impl ObjectStorageEnv for MapEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    fn tenant() -> TenantId {
        TenantId::new("tenant-a").expect("tenant should parse")
    }

    fn memory_target() -> ObjectStorePlacementTarget {
        ObjectStorePlacementTarget::new(
            ObjectStoreProviderKind::Memory,
            "memory-bucket",
            ObjectStoreProviderCredentials::Anonymous,
        )
        .expect("target should build")
    }

    #[test]
    fn env_default_is_overridden_by_programmatic_config() {
        let env = MapEnv(BTreeMap::from([
            (MODE_ENV.to_string(), "mirror".to_string()),
            (PROVIDER_ENV.to_string(), "memory".to_string()),
            (BUCKET_ENV.to_string(), "env-bucket".to_string()),
        ]));

        let from_env = ObjectStorageConfig::from_sources(None, &env).unwrap();
        assert!(matches!(
            from_env.default_policy(),
            PlacementPolicy::Mirror { .. }
        ));

        let programmatic =
            ObjectStorageConfig::from_sources(Some(PlacementPolicy::LocalOnly), &env).unwrap();
        assert_eq!(programmatic.default_policy(), &PlacementPolicy::LocalOnly);
    }

    #[test]
    fn tenant_placement_override_wins_over_server_default() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(Engine::new(temp.path()).unwrap());
        let tenant = tenant();
        let resolver = ObjectStorageResolver::with_config(
            engine.clone(),
            ObjectStorageConfig::new(PlacementPolicy::Mirror {
                target: memory_target(),
                require_ack: true,
            }),
        );

        assert!(matches!(
            resolver.effective_policy(&tenant).unwrap(),
            PlacementPolicy::Mirror { .. }
        ));

        engine
            .set_object_placement(ObjectPlacement::new(
                tenant.clone(),
                PlacementPolicy::LocalOnly,
                42,
            ))
            .unwrap();

        assert_eq!(
            resolver.effective_policy(&tenant).unwrap(),
            PlacementPolicy::LocalOnly
        );
    }

    #[tokio::test]
    async fn resolver_builds_local_blob_store() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(Engine::new(temp.path()).unwrap());
        let resolver = ObjectStorageResolver::new(engine);

        let store = resolver.blob_store(&tenant()).unwrap();
        let hash = store
            .put(Bytes::from_static(b"native bytes"))
            .await
            .unwrap();

        assert_eq!(
            store.get(&hash).await.unwrap(),
            Bytes::from_static(b"native bytes")
        );
    }

    #[test]
    fn missing_secret_ref_credentials_fail_closed() {
        let temp = tempdir().unwrap();
        let engine = Arc::new(Engine::new(temp.path()).unwrap());
        let target = ObjectStorePlacementTarget::new(
            ObjectStoreProviderKind::S3,
            "bucket",
            ObjectStoreProviderCredentials::SecretRef {
                id: "secret/s3".to_string(),
            },
        )
        .unwrap();
        let resolver = ObjectStorageResolver::with_config(
            engine,
            ObjectStorageConfig::new(PlacementPolicy::CloudPrimary { target }),
        );

        let err = match resolver.blob_store(&tenant()) {
            Ok(_) => panic!("secret-ref placement should fail without a resolver"),
            Err(error) => error,
        };
        assert!(
            err.to_string()
                .contains("no credential resolver configured"),
            "{err}"
        );
    }

    #[test]
    fn backup_roots_are_extracted_from_object_manifest_snapshot() {
        let first = BlobHash::of(b"first");
        let second = BlobHash::of(b"second");
        let third = BlobHash::of(b"third");
        let attrs = ObjectManifestAttributes::new("\"etag\"", 1);
        let manifest = ObjectManifest::chunked(
            "bucket",
            "key",
            11,
            vec![
                ObjectChunkRef {
                    blob_hash: first.to_hex(),
                    offset: 0,
                    len: 5,
                },
                ObjectChunkRef {
                    blob_hash: second.to_hex(),
                    offset: 5,
                    len: 6,
                },
                ObjectChunkRef {
                    blob_hash: third.to_hex(),
                    offset: 11,
                    len: 0,
                },
            ],
            attrs,
        );
        assert!(
            manifest.is_err(),
            "manifest validation rejects zero-length chunks"
        );

        let attrs = ObjectManifestAttributes::new("\"etag\"", 1);
        let manifest = ObjectManifest::chunked(
            "bucket",
            "key",
            11,
            vec![
                ObjectChunkRef {
                    blob_hash: first.to_hex(),
                    offset: 0,
                    len: 5,
                },
                ObjectChunkRef {
                    blob_hash: second.to_hex(),
                    offset: 5,
                    len: 6,
                },
            ],
            attrs,
        )
        .unwrap();
        let mut snapshot = nimbus_storage::MaterializedJournalSnapshot {
            version: 0,
            applied_sequence: nimbus_core::SequenceNumber(0),
            durable_head: nimbus_core::SequenceNumber(0),
            table_identities: Vec::new(),
            schema: nimbus_core::Schema::default(),
            documents: Vec::new(),
            scheduled_execution_ids: Vec::new(),
        };
        snapshot.documents.push(manifest.to_document().unwrap());
        let archive = PointInTimeRestoreArchive {
            version: 1,
            target_sequence: nimbus_core::SequenceNumber(0),
            target_timestamp: nimbus_core::Timestamp(0),
            base_snapshot: snapshot,
            journal_tail: Vec::new(),
            storage_format_version: nimbus_storage::CURRENT_STORAGE_FORMAT_VERSION,
            document_version_storage_format:
                nimbus_storage::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT,
            index_version_storage_format: nimbus_storage::CURRENT_INDEX_VERSION_STORAGE_FORMAT,
            target_fingerprint: String::new(),
        };

        assert_eq!(object_backup_roots(&archive).unwrap(), vec![first, second]);
    }
}
