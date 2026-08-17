//! Pre-spawn container creator-intent publication and readback proofs.

use std::cell::{Cell, RefCell};

use crate::backends::conmon::creator::CreatorQuiescenceProof;

use super::*;

fn creator_persistence_fixture(
    root: &std::path::Path,
    id: &str,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root));
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new(id), None, None)
        .expect("creator persistence fixture should plan")
        .manifest;
    (backend, manifest)
}

#[test]
fn creator_pending_precommit_failure_durably_quiesces_before_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_persistence_fixture(temp_dir.path(), "creator-pending-precommit-failure");
    let persist_calls = Cell::new(0_u32);
    let durable = RefCell::new(None::<ContainerSandboxManifest>);

    let error = backend
        .persist_creator_intent_before_spawn_for_test(
            &mut manifest,
            "creator-precommit-attempt",
            |candidate| {
                let call = persist_calls.get();
                persist_calls.set(call + 1);
                if call == 0 {
                    Err(SandboxError::OperationFailed {
                        message: "injected creator-intent precommit failure".to_owned(),
                    })
                } else {
                    durable.replace(Some(candidate.clone()));
                    Ok(())
                }
            },
            |_| -> Result<Option<ContainerSandboxManifest>> {
                panic!("confirmed quiescence publication must not require readback")
            },
        )
        .expect_err("the original creator-intent failure must remain primary");

    assert!(
        error
            .to_string()
            .contains("creator-intent precommit failure"),
        "the original publication failure must remain visible: {error}"
    );
    assert_eq!(persist_calls.get(), 2);
    assert!(matches!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::NeverSpawned { ref attempt_id },
        } if attempt_id == "creator-precommit-attempt"
    ));
    assert_eq!(
        durable.borrow().as_ref(),
        Some(&manifest),
        "cleanup authorization must match the exact durable quiesced manifest"
    );
    assert!(manifest.creator_handoff.authorizes_runtime_cleanup());
}

#[test]
fn creator_pending_rename_ack_loss_confirms_quiesced_readback_without_spawning() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_persistence_fixture(temp_dir.path(), "creator-pending-ack-loss");
    let persist_calls = Cell::new(0_u32);
    let durable = RefCell::new(None::<ContainerSandboxManifest>);

    let error = backend
        .persist_creator_intent_before_spawn_for_test(
            &mut manifest,
            "creator-ack-loss-attempt",
            |candidate| {
                let call = persist_calls.get();
                persist_calls.set(call + 1);
                durable.replace(Some(candidate.clone()));
                Err(SandboxError::OperationFailed {
                    message: if call == 0 {
                        "injected creator-intent rename acknowledgement loss".to_owned()
                    } else {
                        "injected quiescence rename acknowledgement loss".to_owned()
                    },
                })
            },
            |_| Ok(durable.borrow().clone()),
        )
        .expect_err("the original acknowledgement loss must remain visible");

    assert!(
        error
            .to_string()
            .contains("creator-intent rename acknowledgement loss")
            && !error
                .to_string()
                .contains("quiescence rename acknowledgement loss"),
        "exact readback must confirm the quiesced commit while preserving the primary error: \
         {error}"
    );
    assert_eq!(persist_calls.get(), 2);
    assert_eq!(
        durable.borrow().as_ref(),
        Some(&manifest),
        "in-memory cleanup authority must be the byte-equivalent durable manifest"
    );
    assert!(matches!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::NeverSpawned { ref attempt_id },
        } if attempt_id == "creator-ack-loss-attempt"
    ));
    assert!(manifest.creator_handoff.authorizes_runtime_cleanup());
}

#[test]
fn creator_quiescence_failure_retains_exact_pending_fence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_persistence_fixture(temp_dir.path(), "creator-quiescence-failure");
    let persist_calls = Cell::new(0_u32);
    let durable = RefCell::new(None::<ContainerSandboxManifest>);

    let error = backend
        .persist_creator_intent_before_spawn_for_test(
            &mut manifest,
            "creator-fenced-attempt",
            |candidate| {
                let call = persist_calls.get();
                persist_calls.set(call + 1);
                if call == 0 {
                    durable.replace(Some(candidate.clone()));
                    Err(SandboxError::OperationFailed {
                        message: "injected creator-intent acknowledgement loss".to_owned(),
                    })
                } else {
                    Err(SandboxError::OperationFailed {
                        message: "injected quiescence precommit failure".to_owned(),
                    })
                }
            },
            |_| Ok(durable.borrow().clone()),
        )
        .expect_err("unconfirmed quiescence must retain the pending fence");

    assert!(
        error
            .to_string()
            .contains("creator-intent acknowledgement loss")
            && error.to_string().contains("quiescence precommit failure"),
        "both persistence diagnostics must remain actionable: {error}"
    );
    assert!(matches!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::SpawnIntent { ref attempt_id }
            if attempt_id == "creator-fenced-attempt"
    ));
    assert!(
        !manifest.creator_handoff.authorizes_runtime_cleanup(),
        "cleanup must remain fenced when the durable state still says Pending"
    );
}

#[test]
fn pending_creator_manifest_persists_exact_birth_and_containment_receipt() {
    let state = ContainerCreatorHandoffState::Pending {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "creator-birth-receipt-attempt",
        ),
    };
    let encoded = serde_json::to_value(state).expect("creator handoff should serialize");

    assert!(
        encoded
            .get("receipt")
            .and_then(|receipt| receipt.get("process"))
            .and_then(|process| process.get("birth"))
            .is_some(),
        "a pending creator must durably identify its exact OS process birth before provider \
         effects can race owner death; current state was {encoded}"
    );
    assert!(
        encoded
            .get("receipt")
            .and_then(|receipt| receipt.get("process"))
            .and_then(|process| process.get("process_group"))
            .is_some(),
        "a pending creator must durably identify its containment group; current state was \
         {encoded}"
    );
}
