use std::path::{Path, PathBuf};

use nimbus_blob::ErasureConfig;
use nimbus_core::{Error, Result};
use nimbus_storage::{
    ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
    PlacementPolicy,
};

pub(crate) const MODE_ENV: &str = "NIMBUS_OBJECT_STORAGE_MODE";
pub(crate) const PROVIDER_ENV: &str = "NIMBUS_OBJECT_STORAGE_PROVIDER";
pub(crate) const BUCKET_ENV: &str = "NIMBUS_OBJECT_STORAGE_BUCKET";
pub(crate) const REGION_ENV: &str = "NIMBUS_OBJECT_STORAGE_REGION";
pub(crate) const ENDPOINT_ENV: &str = "NIMBUS_OBJECT_STORAGE_ENDPOINT";
pub(crate) const PREFIX_ENV: &str = "NIMBUS_OBJECT_STORAGE_PREFIX";
pub(crate) const CREDENTIALS_ENV: &str = "NIMBUS_OBJECT_STORAGE_CREDENTIALS";
pub(crate) const SECRET_REF_ENV: &str = "NIMBUS_OBJECT_STORAGE_SECRET_REF";
pub(crate) const REQUIRE_ACK_ENV: &str = "NIMBUS_OBJECT_STORAGE_REQUIRE_ACK";
pub(crate) const MASTER_KEY_FILE_ENV: &str = "NIMBUS_OBJECT_STORAGE_MASTER_KEY_FILE";
pub(crate) const LOCAL_LEG_ENV: &str = "NIMBUS_OBJECT_STORAGE_LOCAL_LEG";
pub(crate) const ERASURE_DRIVES_ENV: &str = "NIMBUS_OBJECT_STORAGE_ERASURE_DRIVES";
pub(crate) const ERASURE_DATA_ENV: &str = "NIMBUS_OBJECT_STORAGE_ERASURE_DATA";
pub(crate) const ERASURE_PARITY_ENV: &str = "NIMBUS_OBJECT_STORAGE_ERASURE_PARITY";
pub(crate) const ERASURE_STRIPE_ENV: &str = "NIMBUS_OBJECT_STORAGE_ERASURE_STRIPE";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocalLeg {
    #[default]
    Pack,
    Erasure(ErasureLegConfig),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureLegConfig {
    pub drives: Vec<PathBuf>,
    pub data_shards: usize,
    pub parity_shards: usize,
    pub stripe_width: usize,
}

/// Object-storage defaults resolved from programmatic config, env, or local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectStorageConfig {
    default_policy: PlacementPolicy,
    master_key_file: Option<PathBuf>,
    local_leg: LocalLeg,
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
            master_key_file: None,
            local_leg: LocalLeg::Pack,
        }
    }

    /// Creates a config with an explicit server-default placement policy.
    pub fn new(default_policy: PlacementPolicy) -> Self {
        Self {
            default_policy,
            master_key_file: None,
            local_leg: LocalLeg::Pack,
        }
    }

    /// Returns the default placement policy used when a tenant has no override.
    pub fn default_policy(&self) -> &PlacementPolicy {
        &self.default_policy
    }

    /// Returns the configured object-storage master-key file, if explicitly set.
    pub fn master_key_file(&self) -> Option<&Path> {
        self.master_key_file.as_deref()
    }

    pub fn local_leg(&self) -> &LocalLeg {
        &self.local_leg
    }

    pub fn with_local_leg(mut self, local_leg: LocalLeg) -> Self {
        self.local_leg = local_leg;
        self
    }

    /// Uses an explicit object-storage master-key file.
    pub fn with_master_key_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.master_key_file = Some(path.into());
        self
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
        let default_policy = if let Some(policy) = programmatic_default {
            policy
        } else if let Some(policy) = policy_from_env(env)? {
            policy
        } else {
            PlacementPolicy::LocalOnly
        };
        Ok(Self {
            default_policy,
            master_key_file: env.get(MASTER_KEY_FILE_ENV).map(PathBuf::from),
            local_leg: local_leg_from_env(env)?,
        })
    }
}

fn local_leg_from_env(env: &dyn ObjectStorageEnv) -> Result<LocalLeg> {
    let mode = env.get(LOCAL_LEG_ENV).unwrap_or_else(|| "pack".to_string());
    match normalize(&mode).as_str() {
        "pack" => Ok(LocalLeg::Pack),
        "erasure" => {
            let drives_raw = env.get(ERASURE_DRIVES_ENV).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{ERASURE_DRIVES_ENV} is required when {LOCAL_LEG_ENV}=erasure"
                ))
            })?;
            let drives = drives_raw
                .split(',')
                .map(str::trim)
                .map(|drive| {
                    if drive.is_empty() {
                        return Err(Error::InvalidInput(format!(
                            "{ERASURE_DRIVES_ENV} must contain non-empty absolute paths"
                        )));
                    }
                    let path = PathBuf::from(drive);
                    if !path.is_absolute() {
                        return Err(Error::InvalidInput(format!(
                            "{ERASURE_DRIVES_ENV} path must be absolute: {}",
                            path.display()
                        )));
                    }
                    Ok(path)
                })
                .collect::<Result<Vec<_>>>()?;
            let data_shards = parse_usize_env(env, ERASURE_DATA_ENV, 4)?;
            let parity_shards = parse_usize_env(env, ERASURE_PARITY_ENV, 2)?;
            let stripe_width = parse_usize_env(env, ERASURE_STRIPE_ENV, 1_048_576)?;
            ErasureConfig::new(
                "object-storage-config-validation",
                drives.clone(),
                data_shards,
                parity_shards,
                stripe_width,
            )?;
            Ok(LocalLeg::Erasure(ErasureLegConfig {
                drives,
                data_shards,
                parity_shards,
                stripe_width,
            }))
        }
        other => Err(Error::InvalidInput(format!(
            "{LOCAL_LEG_ENV} must be pack or erasure; got {other}"
        ))),
    }
}

fn parse_usize_env(env: &dyn ObjectStorageEnv, key: &str, default: usize) -> Result<usize> {
    let Some(raw) = env.get(key) else {
        return Ok(default);
    };
    raw.trim().parse::<usize>().map_err(|error| {
        Error::InvalidInput(format!(
            "{key} must be an unsigned integer; got {raw}: {error}"
        ))
    })
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
