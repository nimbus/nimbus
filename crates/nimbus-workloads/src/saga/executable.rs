//! Strict portable executable content carried by one desired generation.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use super::{WorkloadExecutableContentDigest, WorkloadSagaError};

/// Portable executable-envelope format understood by this crate.
pub const WORKLOAD_EXECUTABLE_FORMAT_VERSION: u32 = 1;

/// Maximum canonical executable content retained in one workload saga.
pub const MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES: usize = 1024 * 1024;

/// Closed encoding interpreted by the compute-owned executable codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadExecutableEncoding {
    SandboxSpecCanonicalJsonV1,
}

/// Complete provider-neutral executable content for one desired generation.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadExecutableIntent {
    format_version: u32,
    encoding: WorkloadExecutableEncoding,
    content: String,
    content_digest: WorkloadExecutableContentDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadExecutableIntentWire {
    format_version: u32,
    encoding: WorkloadExecutableEncoding,
    content: String,
    content_digest: WorkloadExecutableContentDigest,
}

impl WorkloadExecutableIntent {
    /// Build a validated carrier and derive its exact content digest.
    pub fn new(
        encoding: WorkloadExecutableEncoding,
        content: impl Into<String>,
    ) -> Result<Self, WorkloadSagaError> {
        let content = content.into();
        validate_content(&content)?;
        Ok(Self {
            format_version: WORKLOAD_EXECUTABLE_FORMAT_VERSION,
            encoding,
            content_digest: WorkloadExecutableContentDigest::sha256(content.as_bytes()),
            content,
        })
    }

    /// Portable envelope format version.
    pub const fn format_version(&self) -> u32 {
        self.format_version
    }

    /// Closed typed encoding identity.
    pub const fn encoding(&self) -> WorkloadExecutableEncoding {
        self.encoding
    }

    /// Exact canonical UTF-8 content interpreted only by compute.
    pub fn canonical_content(&self) -> &str {
        &self.content
    }

    /// Domain-separated digest of the exact canonical content bytes.
    pub const fn content_digest(&self) -> WorkloadExecutableContentDigest {
        self.content_digest
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.format_version != WORKLOAD_EXECUTABLE_FORMAT_VERSION {
            return Err(WorkloadSagaError::InvalidIntent(
                "unsupported workload executable format version",
            ));
        }
        validate_content(&self.content)?;
        if self.content_digest != WorkloadExecutableContentDigest::sha256(self.content.as_bytes()) {
            return Err(WorkloadSagaError::InvalidDigest(
                "workload executable content digest does not match canonical content",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for WorkloadExecutableIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkloadExecutableIntent")
            .field("format_version", &self.format_version)
            .field("encoding", &self.encoding)
            .field("content_bytes", &self.content.len())
            .field("content_digest", &self.content_digest)
            .finish()
    }
}

impl<'de> Deserialize<'de> for WorkloadExecutableIntent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadExecutableIntentWire::deserialize(deserializer)?;
        let candidate = Self {
            format_version: wire.format_version,
            encoding: wire.encoding,
            content: wire.content,
            content_digest: wire.content_digest,
        };
        candidate.validate().map_err(serde::de::Error::custom)?;
        Ok(candidate)
    }
}

fn validate_content(content: &str) -> Result<(), WorkloadSagaError> {
    if content.is_empty() {
        return Err(WorkloadSagaError::InvalidIntent(
            "workload executable content must not be empty",
        ));
    }
    if content.len() > MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES {
        return Err(WorkloadSagaError::InvalidIntent(
            "workload executable content exceeds the maximum byte length",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "executable/tests.rs"]
mod tests;
