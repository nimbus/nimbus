//! Durable sandbox-owned locator for OCI provider evidence.
//!
//! Stable attachment identity remains in `nimbus-network`. This locator only
//! maps one authenticated provider attempt back to the process-injected
//! workload artifact realm. Paths and artifacts never become desired state.

use std::path::Path;

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use nimbus_core::TenantId;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

const ARTIFACT_REALM_DOMAIN: &[u8] = b"nimbus.sandbox.oci.artifact-realm.v2\0";
const ARTIFACT_REALM_PREFIX: &str = "oci-artifact-realm-v2-sha256:";

/// OCI attachment provider family that owns the located artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OciAttachmentProviderKind {
    Container,
    Krun,
}

impl OciAttachmentProviderKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Krun => "krun",
        }
    }
}

/// Stable process-mappable identity of one canonical workload artifact root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OciArtifactRealmId(String);

impl OciArtifactRealmId {
    pub(super) fn for_workload_root(workload_state_root: &Path) -> Result<Self> {
        let directory =
            Dir::open_ambient_dir(workload_state_root, ambient_authority()).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to open OCI workload artifact root {}: {error}",
                        workload_state_root.display()
                    ),
                }
            })?;
        Self::for_open_directory(&directory)
    }

    pub(super) fn for_open_directory(directory: &Dir) -> Result<Self> {
        let file = directory
            .try_clone()
            .map(Dir::into_std_file)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to retain OCI workload artifact root capability for identity: {error}"
                ),
            })?;
        let metadata = file
            .metadata()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to inspect OCI workload artifact root capability: {error}"
                ),
            })?;
        if !metadata.is_dir() {
            return Err(SandboxError::OperationFailed {
                message: "OCI workload artifact root capability is not a directory".to_owned(),
            });
        }
        let mut digest = Sha256::new();
        digest.update(ARTIFACT_REALM_DOMAIN);
        update_directory_identity(&mut digest, &metadata)?;
        Ok(Self(format!(
            "{ARTIFACT_REALM_PREFIX}{}",
            digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )))
    }

    pub(super) fn authenticates_workload_root(&self, workload_state_root: &Path) -> Result<bool> {
        Self::for_workload_root(workload_state_root).map(|candidate| candidate == *self)
    }

    pub(super) fn authenticates_open_directory(&self, directory: &Dir) -> Result<bool> {
        Self::for_open_directory(directory).map(|candidate| candidate == *self)
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for OciArtifactRealmId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OciArtifactRealmId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let Some(digest) = value.strip_prefix(ARTIFACT_REALM_PREFIX) else {
            return Err(serde::de::Error::custom(
                "OCI artifact realm is missing its versioned SHA-256 prefix",
            ));
        };
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom(
                "OCI artifact realm must contain exactly 64 lowercase hexadecimal digits",
            ));
        }
        Ok(Self(value))
    }
}

/// Reversible provider-adjacent evidence stored with one exact IPAM attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OciAttachmentProviderLocator {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    provider_kind: OciAttachmentProviderKind,
    artifact_realm_id: OciArtifactRealmId,
}

impl OciAttachmentProviderLocator {
    pub(super) fn new(
        workload_state_root: &Path,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        provider_kind: OciAttachmentProviderKind,
    ) -> Result<Self> {
        let locator = Self {
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            provider_kind,
            artifact_realm_id: OciArtifactRealmId::for_workload_root(workload_state_root)?,
        };
        locator.validate()?;
        Ok(locator)
    }

    pub(super) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(super) fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub(super) fn provider_kind(&self) -> OciAttachmentProviderKind {
        self.provider_kind
    }

    pub(super) fn artifact_realm_id(&self) -> &OciArtifactRealmId {
        &self.artifact_realm_id
    }

    pub(super) fn authenticates_workload_root(&self, workload_state_root: &Path) -> Result<bool> {
        self.artifact_realm_id
            .authenticates_workload_root(workload_state_root)
    }

    pub(super) fn authenticates_open_directory(&self, directory: &Dir) -> Result<bool> {
        self.artifact_realm_id
            .authenticates_open_directory(directory)
    }

    pub(super) fn validate(&self) -> Result<()> {
        let value = self.sandbox_id.as_str();
        if value.is_empty()
            || value == "."
            || value == ".."
            || value.contains('/')
            || value.contains('\\')
            || value.contains('\0')
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "OCI provider locator contains unsafe sandbox path component {value:?}"
                ),
            });
        }
        Ok(())
    }
}

fn update_directory_identity(digest: &mut Sha256, metadata: &std::fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        digest.update(b"unix\0");
        digest.update(metadata.dev().to_be_bytes());
        digest.update(metadata.ino().to_be_bytes());
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        let volume =
            metadata
                .volume_serial_number()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "OCI workload artifact root has no stable Windows volume identity"
                        .to_owned(),
                })?;
        let file = metadata
            .file_index()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "OCI workload artifact root has no stable Windows file identity"
                    .to_owned(),
            })?;
        digest.update(b"windows\0");
        digest.update(volume.to_be_bytes());
        digest.update(file.to_be_bytes());
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (digest, metadata);
        Err(SandboxError::OperationFailed {
            message:
                "this platform does not expose a stable OCI workload artifact directory identity"
                    .to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn realm_is_bound_to_the_opened_directory_identity() {
        let root = tempdir().expect("root should exist");
        let direct = OciArtifactRealmId::for_workload_root(root.path()).expect("direct realm");
        assert!(
            direct
                .authenticates_workload_root(root.path())
                .expect("same root should authenticate")
        );

        let other = tempdir().expect("other root should exist");
        assert!(
            !direct
                .authenticates_workload_root(other.path())
                .expect("other root should compare"),
            "a different canonical artifact root must not reuse the realm"
        );
    }

    #[test]
    fn realm_wire_rejects_unversioned_uppercase_and_wrong_length_digests() {
        for invalid in [
            "not-a-realm",
            "oci-artifact-realm-v2-sha256:ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789",
            "oci-artifact-realm-v2-sha256:abcd",
        ] {
            let error = serde_json::from_value::<OciArtifactRealmId>(serde_json::Value::String(
                invalid.to_owned(),
            ))
            .expect_err("malformed artifact realms must fail schema validation");
            assert!(
                error.to_string().contains("artifact realm"),
                "malformed realm {invalid:?} should produce a named error: {error}"
            );
        }
    }

    #[test]
    fn locator_rejects_unsafe_sandbox_components_before_persistence() {
        let root = tempdir().expect("root should exist");
        let tenant_id = TenantId::new("tenant-safe").expect("tenant should validate");
        for unsafe_sandbox in ["", ".", "..", "../foreign", "nested/path", "windows\\path"] {
            let error = OciAttachmentProviderLocator::new(
                root.path(),
                &tenant_id,
                &SandboxId::new(unsafe_sandbox),
                OciAttachmentProviderKind::Container,
            )
            .expect_err("unsafe sandbox locators must fail before persistence");
            assert!(
                error.to_string().contains("unsafe sandbox path component"),
                "unsafe sandbox {unsafe_sandbox:?} should produce a named error: {error}"
            );
        }
    }
}
