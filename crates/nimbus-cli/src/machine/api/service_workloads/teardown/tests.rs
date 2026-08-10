use nimbus_sandbox::{ProviderCommandClaimInput, ProviderCommandOperation};

use super::*;

#[path = "tests/acceptance.rs"]
mod acceptance;
#[path = "tests/attachment.rs"]
mod attachment;

fn claim() -> ProviderCommandClaim {
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "wsg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        effect_subject: "{\"execution\":\"fixture\"}".to_owned(),
        source_attempt_id: None,
        attempt_id: "wtd_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        dispatch_epoch: 0,
        workload_generation: 7,
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: "3".repeat(64),
        provider_target_digest: "4".repeat(64),
        operation: ProviderCommandOperation::DrainExecution,
    })
    .expect("fixture provider claim should validate")
}

#[test]
fn guest_workload_teardown_join_evidence_is_deterministic_and_child_sensitive() {
    let claim = claim();
    let systemd = ChildObservationEvidence {
        owner: "systemd",
        kind: ChildObservationKind::Succeeded,
        failure_code: None,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256("systemd"),
    };
    let container = ChildObservationEvidence {
        owner: "container",
        kind: ChildObservationKind::Succeeded,
        failure_code: None,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256("container"),
    };
    let first = composite_evidence(
        "command-a",
        WorkloadTeardownStep::DrainExecution,
        &claim,
        &systemd,
        Some(&container),
    )
    .expect("fixture composite evidence should encode");
    let replay = composite_evidence(
        "command-a",
        WorkloadTeardownStep::DrainExecution,
        &claim,
        &systemd,
        Some(&container),
    )
    .expect("fixture replay evidence should encode");
    assert_eq!(first, replay);

    let crossed_container = ChildObservationEvidence {
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256("other-container"),
        ..container
    };
    let crossed = composite_evidence(
        "command-a",
        WorkloadTeardownStep::DrainExecution,
        &claim,
        &systemd,
        Some(&crossed_container),
    )
    .expect("fixture crossed evidence should encode");
    assert_ne!(first, crossed, "each child receipt must bind the join");
}

struct FailingEvidence;

impl serde::Serialize for FailingEvidence {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "injected guest teardown evidence failure",
        ))
    }
}

#[test]
fn guest_workload_teardown_encoding_failure_never_fabricates_terminal_evidence() {
    assert!(
        child_evidence(
            "systemd",
            ChildObservationKind::Succeeded,
            None,
            &FailingEvidence,
        )
        .is_err(),
        "a child encoding failure must remain explicit"
    );

    let failed = serde_json::to_vec(&FailingEvidence);
    let result = composite_execute_result(
        ProviderCommandObservationKind::Succeeded,
        Some("must-not-survive".to_owned()),
        failed,
    );
    assert_eq!(result.kind, ProviderCommandObservationKind::Ambiguous);
    assert_eq!(result.failure_code, None);
    assert!(!result.evidence.is_empty());
}

#[test]
fn guest_workload_teardown_container_projection_preserves_every_closed_kind() {
    let cases = [
        (
            SandboxExecutionTeardownObservation::Succeeded {
                evidence: b"succeeded".to_vec(),
            },
            ChildObservationKind::Succeeded,
        ),
        (
            SandboxExecutionTeardownObservation::DefiniteFailure {
                code: "sandbox_teardown_test_failure".to_owned(),
                evidence: b"failed".to_vec(),
            },
            ChildObservationKind::DefiniteFailure,
        ),
        (
            SandboxExecutionTeardownObservation::Absent {
                evidence: b"absent".to_vec(),
            },
            ChildObservationKind::Absent,
        ),
        (
            SandboxExecutionTeardownObservation::RetryAuthorized {
                evidence: b"retry".to_vec(),
            },
            ChildObservationKind::RetryAuthorized,
        ),
        (
            SandboxExecutionTeardownObservation::InProgress {
                evidence: b"in-progress".to_vec(),
            },
            ChildObservationKind::InProgress,
        ),
        (
            SandboxExecutionTeardownObservation::Ambiguous {
                evidence: b"ambiguous".to_vec(),
            },
            ChildObservationKind::Ambiguous,
        ),
    ];

    for (observation, expected) in cases {
        let projected = sandbox_evidence(&observation);
        assert_eq!(
            std::mem::discriminant(&projected.kind),
            std::mem::discriminant(&expected)
        );
        assert_eq!(projected.owner, "container");
        assert_eq!(
            projected.evidence_sha256,
            WorkloadOwnerEvidenceDigest::sha256(observation.evidence())
        );
    }
}
