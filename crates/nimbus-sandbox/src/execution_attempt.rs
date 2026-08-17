//! Provider-neutral workload execution-attempt fences.
//!
//! Upper lifecycle owners translate their attempt identities to this opaque
//! value. Sandbox providers persist and echo it without deriving workload
//! semantics or depending on `nimbus-workloads`.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_EXECUTION_ATTEMPT_ID_LEN: usize = 256;

/// Opaque identity for one workload execution incarnation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SandboxExecutionAttemptId(String);

impl SandboxExecutionAttemptId {
    pub fn new(value: impl Into<String>) -> Result<Self, SandboxExecutionAttemptIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SandboxExecutionAttemptIdError::Empty);
        }
        if value.len() > MAX_EXECUTION_ATTEMPT_ID_LEN {
            return Err(SandboxExecutionAttemptIdError::TooLong {
                max: MAX_EXECUTION_ATTEMPT_ID_LEN,
            });
        }
        if value.trim() != value {
            return Err(SandboxExecutionAttemptIdError::SurroundingWhitespace);
        }
        if value.chars().any(char::is_control) {
            return Err(SandboxExecutionAttemptIdError::ControlCharacter);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn provider_initial() -> Self {
        Self(format!(
            "sandbox-initial-{}",
            ulid::Ulid::new().to_string().to_ascii_lowercase()
        ))
    }
}

impl Display for SandboxExecutionAttemptId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for SandboxExecutionAttemptId {
    type Error = SandboxExecutionAttemptIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SandboxExecutionAttemptId> for String {
    fn from(value: SandboxExecutionAttemptId) -> Self {
        value.0
    }
}

/// Exact source and target attempt chain for one restart ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRestartAttemptFence {
    source_attempt_id: SandboxExecutionAttemptId,
    attempt_id: SandboxExecutionAttemptId,
    restart_ordinal: u64,
}

impl SandboxRestartAttemptFence {
    pub fn new(
        source_attempt_id: SandboxExecutionAttemptId,
        attempt_id: SandboxExecutionAttemptId,
        restart_ordinal: u64,
    ) -> Result<Self, SandboxExecutionAttemptIdError> {
        if source_attempt_id == attempt_id {
            return Err(SandboxExecutionAttemptIdError::SameRestartAttempt);
        }
        if restart_ordinal == 0 {
            return Err(SandboxExecutionAttemptIdError::ZeroRestartOrdinal);
        }
        Ok(Self {
            source_attempt_id,
            attempt_id,
            restart_ordinal,
        })
    }

    pub fn source_attempt_id(&self) -> &SandboxExecutionAttemptId {
        &self.source_attempt_id
    }

    pub fn attempt_id(&self) -> &SandboxExecutionAttemptId {
        &self.attempt_id
    }

    pub const fn restart_ordinal(&self) -> u64 {
        self.restart_ordinal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SandboxExecutionAttemptIdError {
    #[error("sandbox execution attempt ID must not be empty")]
    Empty,
    #[error("sandbox execution attempt ID must not exceed {max} bytes")]
    TooLong { max: usize },
    #[error("sandbox execution attempt ID must not contain surrounding whitespace")]
    SurroundingWhitespace,
    #[error("sandbox execution attempt ID must not contain control characters")]
    ControlCharacter,
    #[error("sandbox restart source and target attempt IDs must differ")]
    SameRestartAttempt,
    #[error("sandbox restart ordinal must be nonzero")]
    ZeroRestartOrdinal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_attempt_wire_is_strict_and_bounded() {
        let attempt = SandboxExecutionAttemptId::new("wea_exact").unwrap();
        assert_eq!(serde_json::to_string(&attempt).unwrap(), "\"wea_exact\"");
        assert_eq!(
            serde_json::from_str::<SandboxExecutionAttemptId>("\"wea_exact\"").unwrap(),
            attempt
        );
        for value in ["", " crossed", "crossed ", "crossed\n"] {
            assert!(SandboxExecutionAttemptId::new(value).is_err());
        }
        assert!(SandboxExecutionAttemptId::new("x".repeat(257)).is_err());
    }

    #[test]
    fn restart_attempt_fence_rejects_same_attempt_and_zero_ordinal() {
        let source = SandboxExecutionAttemptId::new("wea_source").unwrap();
        let target = SandboxExecutionAttemptId::new("wea_target").unwrap();
        assert!(SandboxRestartAttemptFence::new(source.clone(), source, 1).is_err());
        assert!(SandboxRestartAttemptFence::new(target.clone(), target, 0).is_err());
    }
}
