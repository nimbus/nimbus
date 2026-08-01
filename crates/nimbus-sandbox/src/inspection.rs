use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SandboxHandle;

/// Read-only evidence returned by a sandbox backend.
///
/// This value is an observation, never desired state or lifecycle authority.
/// A caller must pair it with its own current desired generation before any
/// command may act on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxInspection {
    pub handle: SandboxHandle,
    pub execution: SandboxExecutionObservation,
    pub restart: SandboxRestartAssessment,
    pub cleanup: SandboxCleanupObservation,
    pub version: SandboxInspectionVersion,
}

impl SandboxInspection {
    pub(crate) fn exact(
        handle: SandboxHandle,
        execution: SandboxExecutionObservation,
        restart: SandboxRestartAssessment,
        cleanup: SandboxCleanupObservation,
        snapshot_parts: &[&[u8]],
    ) -> Self {
        let version = SandboxInspectionVersion::sha256(snapshot_parts);
        Self {
            handle,
            execution,
            restart,
            cleanup,
            version,
        }
    }

    /// Build a fail-closed observation for a provider that reports only a
    /// handle and has no exact lifecycle evidence.
    ///
    /// Real Container, Krun, and forwarded-machine adapters must preserve
    /// exact evidence instead of using this constructor. It exists for narrow
    /// external/test backends whose honest capability is handle projection.
    pub fn provider_reported(handle: SandboxHandle) -> Self {
        let rendered = serde_json::to_vec(&handle)
            .expect("SandboxHandle serialization is infallible for observation evidence");
        Self::exact(
            handle,
            SandboxExecutionObservation::Unknown {
                reason: SandboxObservationUnknownReason::ProviderReportedHandleOnly,
            },
            SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::ObservationUnknown,
            },
            // A handle-only provider cannot authenticate cleanup finality,
            // even when its projected status is terminal.
            SandboxCleanupObservation::Retained,
            &[&rendered],
        )
    }

    /// Combine an authenticated backend snapshot with one read-only outer
    /// provider observation.
    ///
    /// The resulting version commits to both the inner snapshot version and
    /// the projected evidence. This remains comparison evidence only; it does
    /// not grant desired-generation or lifecycle command authority.
    pub fn with_provider_projection(
        self,
        handle: SandboxHandle,
        execution: SandboxExecutionObservation,
        restart: SandboxRestartAssessment,
        cleanup: SandboxCleanupObservation,
    ) -> Self {
        self.with_provider_projection_evidence(handle, execution, restart, cleanup, &[])
    }

    /// Combine an authenticated backend snapshot with exact opaque evidence
    /// from the read-only outer provider.
    ///
    /// The extra bytes are version evidence only. They are never exposed as
    /// authority and cannot change the typed projection without the caller
    /// supplying that projection explicitly.
    pub fn with_provider_projection_evidence(
        self,
        handle: SandboxHandle,
        execution: SandboxExecutionObservation,
        restart: SandboxRestartAssessment,
        cleanup: SandboxCleanupObservation,
        provider_evidence: &[u8],
    ) -> Self {
        let rendered = serde_json::to_vec(&(&handle, execution, restart, cleanup))
            .expect("inspection projection serialization is infallible");
        Self::exact(
            handle,
            execution,
            restart,
            cleanup,
            &[self.version.as_bytes(), &rendered, provider_evidence],
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SandboxExecutionObservation {
    PlanOnly,
    Present,
    Exited {
        exit_code: i32,
    },
    AbsentWithoutExit,
    Unknown {
        reason: SandboxObservationUnknownReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "assessment")]
pub enum SandboxRestartAssessment {
    Ineligible {
        reason: SandboxRestartIneligibility,
    },
    Candidate {
        exit_code: i32,
        completed_restarts: u32,
        retry_delay_millis: u64,
        persisted_not_before_millis: Option<u64>,
        blocker: Option<SandboxRestartBlocker>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRestartIneligibility {
    PlanOnly,
    RuntimePresent,
    ShutdownRequested,
    PolicyNever,
    SuccessfulExitExcluded,
    AttemptsExhausted,
    CleanupPending,
    RuntimeAbsenceUnproven,
    ObservationUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRestartBlocker {
    StartupReconciliationUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxObservationUnknownReason {
    ProviderReportedHandleOnly,
    LaunchHandoffPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxCleanupObservation {
    NotRequired,
    Retained,
    Finalized,
}

/// Opaque comparison token for one authenticated inspection snapshot.
///
/// It is not a workload generation, provider handle, or authorization
/// capability. IP addresses and ports never participate as identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SandboxInspectionVersion([u8; 32]);

impl SandboxInspectionVersion {
    fn sha256(parts: &[&[u8]]) -> Self {
        let mut digest = Sha256::new();
        for part in parts {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part);
        }
        Self(digest.finalize().into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SandboxBackendKind, SandboxId, SandboxStatus};
    use nimbus_core::TenantId;

    fn handle(status: SandboxStatus) -> SandboxHandle {
        SandboxHandle::new(
            TenantId::new("inspection-contract").expect("tenant ID"),
            SandboxId::new("inspection-contract"),
            "api",
            SandboxBackendKind::Container,
            status,
            Vec::new(),
        )
    }

    #[test]
    fn provider_reported_handle_is_explicitly_non_authoritative() {
        let inspection = SandboxInspection::provider_reported(handle(SandboxStatus::Ready));

        assert_eq!(
            inspection.execution,
            SandboxExecutionObservation::Unknown {
                reason: SandboxObservationUnknownReason::ProviderReportedHandleOnly,
            }
        );
        assert_eq!(
            inspection.restart,
            SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::ObservationUnknown,
            }
        );
        assert_eq!(
            inspection.cleanup,
            SandboxCleanupObservation::Retained,
            "handle-only terminal state must not manufacture cleanup finality"
        );
    }

    #[test]
    fn inspection_version_is_stable_and_snapshot_sensitive() {
        let first = SandboxInspectionVersion::sha256(&[b"manifest", b"exit=42"]);
        let repeated = SandboxInspectionVersion::sha256(&[b"manifest", b"exit=42"]);
        let changed = SandboxInspectionVersion::sha256(&[b"manifest", b"exit=43"]);

        assert_eq!(first, repeated);
        assert_ne!(first, changed);
    }

    #[test]
    fn inspection_contract_round_trips_every_evidence_field() {
        let inspection = SandboxInspection::exact(
            handle(SandboxStatus::Stopping),
            SandboxExecutionObservation::Exited { exit_code: 42 },
            SandboxRestartAssessment::Candidate {
                exit_code: 42,
                completed_restarts: 1,
                retry_delay_millis: 2_000,
                persisted_not_before_millis: Some(9_000),
                blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
            },
            SandboxCleanupObservation::Retained,
            &[b"exact-snapshot"],
        );

        let bytes = serde_json::to_vec(&inspection).expect("inspection should serialize");
        let decoded: SandboxInspection =
            serde_json::from_slice(&bytes).expect("inspection should deserialize");
        assert_eq!(decoded, inspection);
    }
}
