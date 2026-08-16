use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandJournalError, ProviderCommandObservationKind,
    ProviderCommandOperation,
};
use nimbus_workloads::{
    WorkloadTeardownCommandMode, WorkloadTeardownDispatchAuthorization, WorkloadTeardownStep,
};

use super::{
    ConfirmedTeardownProviderCommand, ConfirmedTeardownProviderJournal, provider_operation,
};

const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn command(
    operation: ProviderCommandOperation,
    mode: WorkloadTeardownCommandMode,
) -> ConfirmedTeardownProviderCommand {
    ConfirmedTeardownProviderCommand {
        mode,
        authorization: WorkloadTeardownDispatchAuthorization::Initial,
        claim: ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "wsg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            effect_subject: r#"{"kind":"teardown","id":"subject-alpha"}"#.to_owned(),
            source_attempt_id: None,
            attempt_id: "wta_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_owned(),
            dispatch_epoch: 0,
            workload_generation: 7,
            restart_ordinal: 0,
            desired_digest: DIGEST_A.to_owned(),
            source_digest: DIGEST_B.to_owned(),
            network_plan_digest: DIGEST_A.to_owned(),
            provider_target_digest: DIGEST_B.to_owned(),
            operation,
        })
        .expect("teardown provider command fixture should validate"),
    }
}

#[test]
fn every_teardown_step_maps_to_one_exact_provider_operation() {
    let cases = [
        (
            WorkloadTeardownStep::WithdrawPublication,
            ProviderCommandOperation::WithdrawFinalPublication,
        ),
        (
            WorkloadTeardownStep::DrainExecution,
            ProviderCommandOperation::DrainExecution,
        ),
        (
            WorkloadTeardownStep::StopExecution,
            ProviderCommandOperation::StopExecution,
        ),
        (
            WorkloadTeardownStep::DetachNetwork,
            ProviderCommandOperation::DetachNetwork,
        ),
        (
            WorkloadTeardownStep::ReleaseNetwork,
            ProviderCommandOperation::ReleaseNetwork,
        ),
    ];

    for (step, expected) in cases {
        assert_eq!(provider_operation(step), expected);
    }
}

#[test]
fn confirmed_teardown_journal_receives_an_existing_provider_journal() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "teardown-seam-test")
        .expect("provider journal should open");

    let _seam = ConfirmedTeardownProviderJournal::new(journal);
}

#[test]
fn journal_mode_fence_rejects_before_any_durable_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let state_root = root.path().join("provider-journal");
    let journal = ProviderCommandAttemptJournal::open(&state_root, "teardown-mode-test")
        .expect("provider journal should open without creating state");
    let seam = ConfirmedTeardownProviderJournal::new(journal);

    let inspect = command(
        ProviderCommandOperation::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    );
    assert!(matches!(
        seam.claim_execute(&inspect),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));
    assert!(!state_root.exists());

    let execute = command(
        ProviderCommandOperation::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    assert!(matches!(
        seam.adopt_inspect(&execute),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));
    assert!(!state_root.exists());
}

#[test]
fn one_injected_journal_keeps_five_teardown_streams_independent() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "five-teardown-streams")
        .expect("provider journal should open");
    let seam = ConfirmedTeardownProviderJournal::new(journal.clone());
    let operations = [
        ProviderCommandOperation::WithdrawFinalPublication,
        ProviderCommandOperation::DrainExecution,
        ProviderCommandOperation::StopExecution,
        ProviderCommandOperation::DetachNetwork,
        ProviderCommandOperation::ReleaseNetwork,
    ];

    for operation in operations {
        let command = command(operation, WorkloadTeardownCommandMode::Execute);
        assert!(matches!(
            seam.claim_execute(&command)
                .expect("each teardown operation should claim an independent stream"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        let terminal = seam
            .record_observation_with_failure_code(
                &command,
                ProviderCommandObservationKind::Succeeded,
                None,
                b"exact provider result",
            )
            .expect("each exact teardown result should persist");
        assert_eq!(terminal.claim(), command.claim());

        let reopened = ConfirmedTeardownProviderJournal::new(
            ProviderCommandAttemptJournal::open(root.path(), "five-teardown-streams")
                .expect("fresh provider journal handle should open"),
        );
        assert!(matches!(
            reopened
                .claim_execute(&command)
                .expect("fresh handle should adopt the exact terminal result"),
            ProviderCommandClaimDecision::AdoptExactAttempt(observation)
                if observation == terminal
        ));
    }
}

#[test]
fn journal_observation_cannot_cross_a_confirmed_teardown_command() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "crossed-teardown-stream")
        .expect("provider journal should open");
    let seam = ConfirmedTeardownProviderJournal::new(journal);
    let drain = command(
        ProviderCommandOperation::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let stop = command(
        ProviderCommandOperation::StopExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let observation = match seam
        .claim_execute(&drain)
        .expect("drain should claim its exact stream")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution.observation().clone(),
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("first drain must receive execute authority")
        }
    };
    let inspections = AtomicUsize::new(0);

    assert_eq!(
        seam.inspect_current_claim(&stop, &observation, |_| {
            inspections.fetch_add(1, Ordering::SeqCst);
        })
        .expect_err("a crossed observation must fail before inspection"),
        ProviderCommandJournalError::CrossedClaim
    );
    assert_eq!(inspections.load(Ordering::SeqCst), 0);
}

#[test]
fn final_withdraw_requires_coded_failure_and_accepts_exact_retry_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "final-withdraw-policy")
        .expect("provider journal should open");
    let seam = ConfirmedTeardownProviderJournal::new(journal);
    let command = command(
        ProviderCommandOperation::WithdrawFinalPublication,
        WorkloadTeardownCommandMode::Execute,
    );
    seam.claim_execute(&command)
        .expect("final withdrawal should claim its teardown stream");

    assert!(matches!(
        seam.record_observation_with_failure_code(
            &command,
            ProviderCommandObservationKind::DefiniteFailure,
            None,
            b"missing stable code",
        ),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));
    let terminal = seam
        .record_observation_with_failure_code(
            &command,
            ProviderCommandObservationKind::DefiniteFailure,
            Some("final_withdraw_failed"),
            b"stable final withdrawal failure",
        )
        .expect("coded teardown failure should persist");
    assert_eq!(terminal.failure_code(), Some("final_withdraw_failed"));

    let retry_journal = ProviderCommandAttemptJournal::open(root.path(), "final-withdraw-retry")
        .expect("provider retry journal should open");
    let retry_seam = ConfirmedTeardownProviderJournal::new(retry_journal);
    retry_seam
        .claim_execute(&command)
        .expect("final withdrawal retry fixture should claim its stream");
    let retry = retry_seam
        .record_observation_with_failure_code(
            &command,
            ProviderCommandObservationKind::RetryAuthorized,
            None,
            b"fresh complete forwarding inspection authorizes the adjacent retry",
        )
        .expect("exact final withdrawal retry authority should persist");
    assert_eq!(
        retry.kind(),
        ProviderCommandObservationKind::RetryAuthorized
    );
    assert_eq!(retry.failure_code(), None);
}
