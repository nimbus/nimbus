//! Durable-store acknowledgement, artifact, and lock-contention proofs.

use super::*;

struct FailStoreCheckpoint(MachinePortEvidenceStoreCheckpoint);

impl MachinePortEvidenceStoreObserver for FailStoreCheckpoint {
    fn checkpoint(&mut self, checkpoint: MachinePortEvidenceStoreCheckpoint) -> Result<()> {
        if checkpoint == self.0 {
            return Err(SandboxError::OperationFailed {
                message: format!("scripted store acknowledgement loss at {checkpoint:?}"),
            });
        }
        Ok(())
    }
}

#[test]
fn nnc5_4a_stage_and_rename_acknowledgement_loss_reconcile_exact_bytes() {
    for checkpoint in [
        MachinePortEvidenceStoreCheckpoint::StageDurable,
        MachinePortEvidenceStoreCheckpoint::CanonicalRenamed,
    ] {
        let fixture = PublicationFixture::new(bindings());
        let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
        let record = fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        );
        let mut observer = FailStoreCheckpoint(checkpoint);
        let error = publish_record_with_observer(
            &fixture.manifest.runner_config.workload_state_root,
            state_dir,
            record.clone(),
            &mut observer,
        )
        .expect_err("scripted store acknowledgement loss should surface");
        assert!(
            error.to_string().contains(&format!("{checkpoint:?}")),
            "the exact store boundary should remain diagnostic: {error}"
        );
        assert!(
            !state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE).exists(),
            "same-process error handling must clean a regular stage"
        );

        match checkpoint {
            MachinePortEvidenceStoreCheckpoint::StageDurable => {
                assert!(
                    !state_dir.join(MACHINE_PORT_EVIDENCE_FILE).exists(),
                    "a pre-rename acknowledgement loss must not fabricate canonical evidence"
                );
                publish_record(
                    &fixture.manifest.runner_config.workload_state_root,
                    state_dir,
                    record.clone(),
                )
                .expect("retry should publish the exact staged record");
            }
            MachinePortEvidenceStoreCheckpoint::CanonicalRenamed => {
                assert_eq!(
                    read_record(state_dir).expect("renamed canonical record should reopen"),
                    record,
                    "ambiguous directory-sync acknowledgement is resolved by canonical reopen"
                );
            }
        }

        let before = fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE))
            .expect("canonical bytes should read");
        publish_record(
            &fixture.manifest.runner_config.workload_state_root,
            state_dir,
            record.clone(),
        )
        .expect("exact replay should remain publishable");
        assert_eq!(
            fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE))
                .expect("replayed canonical bytes should read"),
            before,
            "reconciliation must preserve the canonical envelope bytes"
        );
    }
}

#[test]
fn nnc5_4a_envelope_rejects_unknown_fields_without_rewriting_authority() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        ),
    )
    .expect("strict canonical envelope should publish");
    let path = state_dir.join(MACHINE_PORT_EVIDENCE_FILE);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("canonical envelope should read"))
            .expect("canonical envelope should decode");
    envelope
        .as_object_mut()
        .expect("canonical envelope must be an object")
        .insert("legacy_extension".to_owned(), serde_json::json!(true));
    let substituted =
        serde_json::to_vec_pretty(&envelope).expect("substituted envelope should encode");
    fs::write(&path, &substituted).expect("substituted envelope should write");

    let error = read_record(state_dir)
        .expect_err("an unknown envelope field must not become compatibility input");
    assert!(
        error
            .to_string()
            .contains("unknown field `legacy_extension`"),
        "strict parsing should retain the exact unknown-field diagnostic: {error}"
    );
    assert_eq!(
        fs::read(&path).expect("rejected envelope should remain inspectable"),
        substituted,
        "strict rejection must not rewrite durable authority"
    );
}

#[test]
fn nnc5_4a_lock_contention_returns_typed_timeout_without_changing_canonical_bytes() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        fixture.record(
            MachinePortPublicationPhase::Exposed,
            fixture.exposed_receipts(),
        ),
    )
    .expect("canonical record should publish before contention");
    let before =
        fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE)).expect("canonical bytes should read");
    let guard = lock_publication_for_test(state_dir).expect("first contender should own the lock");
    let contender_dir = state_dir.to_path_buf();
    let contender = std::thread::spawn(move || lock_publication_for_test(&contender_dir));
    let error = contender
        .join()
        .expect("bounded lock contender should not panic")
        .expect_err("second contender should receive the typed timeout");
    assert!(
        matches!(error, MachinePortEvidenceLockError::Timeout { .. }),
        "lock contention must remain typed, got {error:?}"
    );
    drop(guard);
    assert_eq!(
        fs::read(state_dir.join(MACHINE_PORT_EVIDENCE_FILE))
            .expect("canonical bytes should remain readable"),
        before,
        "timed-out contention must not change canonical evidence"
    );
    lock_publication_for_test(state_dir)
        .expect("the lock must be immediately reusable after the winning owner exits");
}

#[test]
fn nnc5_4a_regular_stale_stage_reconciles_and_non_regular_artifacts_fail_closed() {
    let fixture = PublicationFixture::new(bindings());
    let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
    fs::create_dir_all(state_dir).expect("state directory should exist");
    fs::write(
        state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE),
        b"stale partial stage",
    )
    .expect("stale regular stage should write");
    let record = fixture.record(
        MachinePortPublicationPhase::Exposed,
        fixture.exposed_receipts(),
    );
    publish_record(
        &fixture.manifest.runner_config.workload_state_root,
        state_dir,
        record.clone(),
    )
    .expect("a regular stale stage should be removed before exact publication");
    assert_eq!(
        read_record(state_dir).expect("reconciled record should reopen"),
        record
    );

    for artifact in [
        MACHINE_PORT_EVIDENCE_STAGE_FILE,
        MACHINE_PORT_EVIDENCE_LOCK_FILE,
        MACHINE_PORT_EVIDENCE_FILE,
    ] {
        let fixture = PublicationFixture::new(bindings());
        let state_dir = &fixture.manifest.conmon_layout.container_state_dir;
        fs::create_dir_all(state_dir).expect("state directory should exist");
        fs::create_dir(state_dir.join(artifact)).expect("non-regular artifact should exist");
        let error = match artifact {
            MACHINE_PORT_EVIDENCE_FILE => {
                read_record(state_dir).expect_err("non-regular canonical entry must fail")
            }
            MACHINE_PORT_EVIDENCE_LOCK_FILE => lock_publication_for_test(state_dir)
                .expect_err("non-regular lock entry must fail")
                .into_sandbox_error(),
            MACHINE_PORT_EVIDENCE_STAGE_FILE => publish_record(
                &fixture.manifest.runner_config.workload_state_root,
                state_dir,
                fixture.record(
                    MachinePortPublicationPhase::Exposed,
                    fixture.exposed_receipts(),
                ),
            )
            .expect_err("non-regular stage entry must fail"),
            _ => unreachable!(),
        };
        assert!(
            error.to_string().contains("not a regular file"),
            "{artifact} must fail with the exact artifact diagnostic: {error}"
        );
    }
}
