use std::path::{Path, PathBuf};
use std::sync::Arc;

use deno_core::ModuleSpecifier;
use sha2::{Digest, Sha256};

use crate::error::{NeovexRuntimeError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    entrypoint: PathBuf,
    expected_sha256: Option<String>,
}

impl RuntimeBundleIdentity {
    pub fn entrypoint(&self) -> &Path {
        &self.entrypoint
    }

    pub fn expected_sha256(&self) -> Option<&str> {
        self.expected_sha256.as_deref()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeBundleShared {
    entrypoint: PathBuf,
    canonical_entrypoint: Option<PathBuf>,
    expected_sha256: Option<String>,
    identity: RuntimeBundleIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundle {
    shared: Arc<RuntimeBundleShared>,
}

impl RuntimeBundle {
    pub fn new(entrypoint: impl AsRef<Path>) -> Self {
        Self::from_parts(entrypoint.as_ref().to_path_buf(), None)
    }

    pub fn with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            Some(normalize_sha256(expected_sha256.as_ref())?),
        ))
    }

    pub fn entrypoint(&self) -> &Path {
        &self.shared.entrypoint
    }

    pub fn canonical_entrypoint(&self) -> Option<&Path> {
        self.shared.canonical_entrypoint.as_deref()
    }

    pub fn bundle_identity(&self) -> &RuntimeBundleIdentity {
        &self.shared.identity
    }

    pub fn compute_sha256_for_path(path: impl AsRef<Path>) -> Result<String> {
        let bytes = std::fs::read(path)?;
        Ok(compute_sha256_hex(&bytes))
    }

    pub(crate) fn module_specifier(&self) -> Result<ModuleSpecifier> {
        ModuleSpecifier::from_file_path(self.entrypoint()).map_err(|_| {
            NeovexRuntimeError::Contract(format!(
                "bundle entrypoint is not a valid file URL: {}",
                self.entrypoint().display()
            ))
        })
    }

    pub(crate) fn module_root(&self) -> Result<PathBuf> {
        self.entrypoint()
            .parent()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract(format!(
                    "bundle entrypoint does not have a parent directory: {}",
                    self.entrypoint().display()
                ))
            })?
            .canonicalize()
            .map_err(NeovexRuntimeError::from)
    }

    pub(crate) fn verify_integrity(&self) -> Result<()> {
        // Stable bundle identity is only for pooling, metrics, and provenance bookkeeping.
        // Path-backed bundles remain mutable, so every invocation must re-hash bundle contents.
        let Some(expected_sha256) = &self.shared.expected_sha256 else {
            return Ok(());
        };
        let actual_sha256 = Self::compute_sha256_for_path(self.entrypoint())?;
        if &actual_sha256 == expected_sha256 {
            return Ok(());
        }
        Err(NeovexRuntimeError::BundleIntegrityMismatch(format!(
            "{} (expected {}, got {})",
            self.entrypoint().display(),
            expected_sha256,
            actual_sha256
        )))
    }

    fn from_parts(entrypoint: PathBuf, expected_sha256: Option<String>) -> Self {
        let canonical_entrypoint = entrypoint.canonicalize().ok();
        let identity = RuntimeBundleIdentity {
            entrypoint: canonical_entrypoint
                .clone()
                .unwrap_or_else(|| entrypoint.clone()),
            expected_sha256: expected_sha256.clone(),
        };
        Self {
            shared: Arc::new(RuntimeBundleShared {
                entrypoint,
                canonical_entrypoint,
                expected_sha256,
                identity,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

fn normalize_sha256(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NeovexRuntimeError::Contract(format!(
            "bundle sha256 must be a 64-character hex string, got {trimmed:?}"
        )));
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
