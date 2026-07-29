//! Machine configuration record and its image-source / resource / volume parts.

use std::fmt::{self, Display, Formatter};
use std::path::{Component, Path, PathBuf};

use nimbus_core::Error;
use nimbus_network::NetworkProviderHandle;
use serde::{Deserialize, Serialize};

use crate::provider::MachineProvider;
use crate::roots::MachineRootLayout;

// The machine config schema (`config.json`) is at its first version. Like the
// state schema it starts at 1 pre-launch -- there is no shipped older version to
// account for, so the dev-era 1 -> 2 -> 3 history collapses to a single
// canonical v1. Unlike the state schema, the config loader is *strict*: a
// version mismatch is a hard error, never a silent rebuild. config.json is the
// operator's declared configuration (provider, resources, image source,
// volumes) -- durable user data -- so rebuilding it from defaults would
// silently invent intent that exists nowhere else. To keep that strictness
// non-destructive, the loader first copies the rejected file aside to a
// `config.json.v{N}.bak` sibling and then directs the operator to recreate the
// machine, so their declared settings survive for reference instead of being
// destroyed by the recovery. That asymmetry with `CURRENT_MACHINE_STATE_VERSION`
// (rebuildable runtime data) is deliberate; do not make config self-heal. The
// first post-launch schema change bumps to 2.
pub const CURRENT_MACHINE_CONFIG_VERSION: u32 = 1;

/// Persisted provenance for the parent OS-node network authority that owns one
/// machine's gvproxy effects.
///
/// This record carries only serialized intent and identity. Resolving aliases,
/// authenticating the live authority, and performing provider effects remain
/// responsibilities of the parent composition layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "MachineNetworkAuthorityRecordWire",
    into = "MachineNetworkAuthorityRecordWire"
)]
pub struct MachineNetworkAuthorityRecord {
    authority_path: PathBuf,
    provider_instance: NetworkProviderHandle,
}

impl MachineNetworkAuthorityRecord {
    /// Record an already-canonicalized absolute authority path and a
    /// parent-issued opaque gvproxy provider instance.
    ///
    /// This validation is lexical and performs no filesystem I/O.
    pub fn new(
        authority_path: impl Into<PathBuf>,
        provider_instance: NetworkProviderHandle,
    ) -> Result<Self, MachineNetworkAuthorityRecordError> {
        let authority_path = authority_path.into();
        if authority_path.as_os_str().is_empty() {
            return Err(MachineNetworkAuthorityRecordError::EmptyAuthorityPath);
        }
        if !authority_path.is_absolute() {
            return Err(MachineNetworkAuthorityRecordError::RelativeAuthorityPath);
        }
        if authority_path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return Err(MachineNetworkAuthorityRecordError::NonCanonicalAuthorityPath);
        }
        Ok(Self {
            authority_path,
            provider_instance,
        })
    }

    /// Canonical parent authority provenance. This is not an artifact root.
    pub fn authority_path(&self) -> &Path {
        &self.authority_path
    }

    /// Parent-issued gvproxy provider identity for this machine configuration.
    pub fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }
}

/// Stable validation failures for serialized machine network provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineNetworkAuthorityRecordError {
    EmptyAuthorityPath,
    RelativeAuthorityPath,
    NonCanonicalAuthorityPath,
}

impl Display for MachineNetworkAuthorityRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAuthorityPath => {
                formatter.write_str("machine network authority path must not be empty")
            }
            Self::RelativeAuthorityPath => {
                formatter.write_str("machine network authority path must be absolute")
            }
            Self::NonCanonicalAuthorityPath => formatter.write_str(
                "machine network authority path must not contain '.' or '..' components",
            ),
        }
    }
}

impl std::error::Error for MachineNetworkAuthorityRecordError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineNetworkAuthorityRecordWire {
    authority_path: PathBuf,
    provider_instance: NetworkProviderHandle,
}

impl TryFrom<MachineNetworkAuthorityRecordWire> for MachineNetworkAuthorityRecord {
    type Error = MachineNetworkAuthorityRecordError;

    fn try_from(value: MachineNetworkAuthorityRecordWire) -> Result<Self, Self::Error> {
        Self::new(value.authority_path, value.provider_instance)
    }
}

impl From<MachineNetworkAuthorityRecord> for MachineNetworkAuthorityRecordWire {
    fn from(value: MachineNetworkAuthorityRecord) -> Self {
        Self {
            authority_path: value.authority_path,
            provider_instance: value.provider_instance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineConfigRecord {
    pub version: u32,
    pub name: String,
    pub provider: MachineProvider,
    pub guest: MachineGuestConfig,
    pub resources: MachineResources,
    pub volumes: Vec<MachineVolume>,
    pub roots: MachineRootLayout,
    pub network_authority: MachineNetworkAuthorityRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineGuestConfig {
    pub image_source: MachineImageSource,
    pub provisioning: MachineGuestProvisioning,
    pub ssh_user: String,
    pub ssh_identity_path: Option<PathBuf>,
    pub ignition_file_path: Option<PathBuf>,
    pub efi_variable_store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineGuestProvisioning {
    Ignition,
    BootcMachineConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MachineImageSource {
    OciReference { reference: String },
    HttpUrl { url: String, sha256: String },
    LocalDisk { path: PathBuf },
}

impl MachineImageSource {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        if value.is_empty() {
            return Err(Error::InvalidInput(
                "machine image source cannot be empty".to_owned(),
            ));
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            return parse_http_image_source(value);
        }

        if value.starts_with("docker://") {
            return Ok(Self::OciReference {
                reference: value.to_owned(),
            });
        }

        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(Self::LocalDisk { path });
        }

        Ok(Self::OciReference {
            reference: format!("docker://{value}"),
        })
    }

    /// Render this image source as its canonical single-string form: the OCI
    /// reference verbatim, an HTTP URL with its `#sha256=` integrity suffix, or
    /// the local disk path. This is the inverse presentation of [`parse`] used
    /// by control-plane projections and machine status rendering.
    ///
    /// [`parse`]: Self::parse
    pub fn as_source_string(&self) -> String {
        match self {
            Self::OciReference { reference } => reference.clone(),
            Self::HttpUrl { url, sha256 } => format!("{url}#sha256={sha256}"),
            Self::LocalDisk { path } => path.display().to_string(),
        }
    }
}

fn parse_http_image_source(value: &str) -> Result<MachineImageSource, Error> {
    let (url, fragment) = value.rsplit_once('#').ok_or_else(|| {
        Error::InvalidInput(
            "HTTP machine image sources must include an integrity suffix: #sha256=<64 hex>"
                .to_owned(),
        )
    })?;
    if url.is_empty() {
        return Err(Error::InvalidInput(
            "HTTP machine image source URL cannot be empty".to_owned(),
        ));
    }
    let digest = fragment.strip_prefix("sha256=").ok_or_else(|| {
        Error::InvalidInput(
            "HTTP machine image source integrity suffix must be #sha256=<64 hex>".to_owned(),
        )
    })?;
    let sha256 = normalize_sha256_hex(digest)?;
    Ok(MachineImageSource::HttpUrl {
        url: url.to_owned(),
        sha256,
    })
}

fn normalize_sha256_hex(value: &str) -> Result<String, Error> {
    if value.len() != 64 || !value.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(Error::InvalidInput(format!(
            "HTTP machine image sha256 must be exactly 64 hex characters, got {value:?}"
        )));
    }
    Ok(value.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineResources {
    pub cpus: u8,
    pub memory_mib: u32,
    pub disk_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineVolume {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl MachineVolume {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let (source, target) = value.split_once(':').ok_or_else(|| {
            Error::InvalidInput(format!(
                "invalid machine volume '{value}'; expected <source>:<target>"
            ))
        })?;
        if source.is_empty() || target.is_empty() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; expected non-empty <source>:<target>"
            )));
        }
        let source = PathBuf::from(source);
        let target = PathBuf::from(target);
        if !source.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; source path must be absolute"
            )));
        }
        if !target.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "invalid machine volume '{value}'; target path must be absolute"
            )));
        }
        Ok(Self { source, target })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nimbus_network::{NetworkProviderHandle, NetworkProviderId};

    use super::{MachineImageSource, MachineNetworkAuthorityRecord, MachineVolume};

    fn provider_instance(value: &str) -> NetworkProviderHandle {
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("machine-gvproxy"),
            value,
        )
        .expect("provider fixture should validate")
    }

    fn absolute_authority_path() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(r"C:\ProgramData\Nimbus\control")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/var/lib/nimbus/control")
        }
    }

    fn noncanonical_absolute_authority_path() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(r"C:\ProgramData\Nimbus\..\control")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/var/lib/nimbus/../control")
        }
    }

    #[test]
    fn machine_network_authority_is_absolute_strict_and_round_trips() {
        let authority = MachineNetworkAuthorityRecord::new(
            absolute_authority_path(),
            provider_instance("machine-config-01"),
        )
        .expect("absolute authority path should validate");

        assert_eq!(authority.authority_path(), absolute_authority_path());
        assert_eq!(
            authority.provider_instance(),
            &provider_instance("machine-config-01")
        );

        let value = serde_json::to_value(&authority).expect("authority should serialize");
        assert_eq!(
            serde_json::from_value::<MachineNetworkAuthorityRecord>(value.clone())
                .expect("authority should deserialize"),
            authority
        );

        let mut unknown = value.clone();
        unknown
            .as_object_mut()
            .expect("authority wire should be an object")
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<MachineNetworkAuthorityRecord>(unknown).is_err(),
            "unknown authority fields must fail closed"
        );

        let mut missing = value;
        missing
            .as_object_mut()
            .expect("authority wire should be an object")
            .remove("provider_instance");
        assert!(
            serde_json::from_value::<MachineNetworkAuthorityRecord>(missing).is_err(),
            "missing authority fields must fail closed"
        );
    }

    #[test]
    fn machine_network_authority_rejects_noncanonical_paths_without_io() {
        for path in [
            PathBuf::new(),
            PathBuf::from("relative/control"),
            noncanonical_absolute_authority_path(),
        ] {
            assert!(
                MachineNetworkAuthorityRecord::new(
                    path.clone(),
                    provider_instance("machine-config-01"),
                )
                .is_err(),
                "authority path {path:?} must be rejected"
            );
        }
    }

    #[test]
    fn machine_image_source_parse_classifies_supported_sources() {
        let digest = "A".repeat(64);
        assert_eq!(
            MachineImageSource::parse(&format!("https://example.com/disk.raw#sha256={digest}"))
                .expect("http source should parse"),
            MachineImageSource::HttpUrl {
                url: "https://example.com/disk.raw".to_owned(),
                sha256: digest.to_ascii_lowercase(),
            }
        );
        assert_eq!(
            MachineImageSource::parse("docker://registry.example.com/nimbus/machine:latest")
                .expect("explicit docker source should parse"),
            MachineImageSource::OciReference {
                reference: "docker://registry.example.com/nimbus/machine:latest".to_owned(),
            }
        );
        let local_disk = std::env::temp_dir().join("nimbus-machine.raw");
        assert_eq!(
            MachineImageSource::parse(local_disk.to_str().expect("temp path should be utf-8"))
                .expect("absolute disk path should parse"),
            MachineImageSource::LocalDisk { path: local_disk }
        );
        assert_eq!(
            MachineImageSource::parse("registry.example.com/nimbus/machine:latest")
                .expect("implicit docker source should parse"),
            MachineImageSource::OciReference {
                reference: "docker://registry.example.com/nimbus/machine:latest".to_owned(),
            }
        );
    }

    #[test]
    fn machine_image_source_parse_rejects_empty_or_unverified_http_sources() {
        assert_invalid_image_source("", "cannot be empty");
        assert_invalid_image_source("https://example.com/disk.raw", "integrity suffix");
        assert_invalid_image_source(
            "https://example.com/disk.raw#md5=abc",
            "must be #sha256=<64 hex>",
        );
        assert_invalid_image_source("https://example.com/disk.raw#sha256=abc", "exactly 64 hex");
    }

    #[test]
    fn machine_image_source_as_source_string_round_trips_each_variant() {
        assert_eq!(
            MachineImageSource::OciReference {
                reference: "docker://registry.example.com/nimbus/machine:latest".to_owned(),
            }
            .as_source_string(),
            "docker://registry.example.com/nimbus/machine:latest",
        );
        let digest = "a".repeat(64);
        assert_eq!(
            MachineImageSource::HttpUrl {
                url: "https://example.com/disk.raw".to_owned(),
                sha256: digest.clone(),
            }
            .as_source_string(),
            format!("https://example.com/disk.raw#sha256={digest}"),
        );
        assert_eq!(
            MachineImageSource::LocalDisk {
                path: PathBuf::from("/var/lib/nimbus/disk.raw"),
            }
            .as_source_string(),
            "/var/lib/nimbus/disk.raw",
        );
    }

    #[test]
    fn machine_volume_parse_accepts_absolute_source_and_target() {
        assert_eq!(
            MachineVolume::parse("/host/data:/guest/data").expect("volume should parse"),
            MachineVolume {
                source: PathBuf::from("/host/data"),
                target: PathBuf::from("/guest/data"),
            }
        );
    }

    #[test]
    fn machine_volume_parse_rejects_missing_or_relative_paths() {
        assert_volume_error("missing-separator", "expected <source>:<target>");
        assert_volume_error(":/guest", "expected non-empty");
        assert_volume_error("/host:", "expected non-empty");
        assert_volume_error("relative:/guest", "source path must be absolute");
        assert_volume_error("/host:relative", "target path must be absolute");
    }

    fn assert_invalid_image_source(value: &str, expected: &str) {
        let error = MachineImageSource::parse(value).expect_err("image source should be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in {error}"
        );
    }

    fn assert_volume_error(value: &str, expected: &str) {
        let error = MachineVolume::parse(value).expect_err("volume should be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in {error}"
        );
    }
}
