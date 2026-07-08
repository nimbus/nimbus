//! Machine configuration record and its image-source / resource / volume parts.

use std::path::PathBuf;

use nimbus_core::Error;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineConfigRecord {
    pub version: u32,
    pub name: String,
    pub provider: MachineProvider,
    pub guest: MachineGuestConfig,
    pub resources: MachineResources,
    pub volumes: Vec<MachineVolume>,
    pub roots: MachineRootLayout,
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

    use super::{MachineImageSource, MachineVolume};

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
