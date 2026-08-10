use std::io::Write as _;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use super::*;

const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE_ATTEMPT: &str = "wpa_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TARGET_ATTEMPT: &str = "wpa_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NEXT_ATTEMPT: &str = "wpa_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PROCESS_ROOT_ENV: &str = "NIMBUS_NNC64_PROVIDER_JOURNAL_ROOT";
const PROCESS_ROLE_ENV: &str = "NIMBUS_NNC64_PROVIDER_JOURNAL_ROLE";
const PROCESS_CHILD_TEST: &str = "provider_command::tests::provider_claim_child";

fn claim(epoch: u64) -> ProviderCommandClaim {
    command_claim(ProviderCommandOperation::ActivateWorkload, 0, epoch)
}

fn publish_claim(epoch: u64) -> ProviderCommandClaim {
    command_claim(ProviderCommandOperation::PublishIngress, 0, epoch)
}

fn stop_claim(epoch: u64) -> ProviderCommandClaim {
    command_claim(ProviderCommandOperation::StopExecution, 0, epoch)
}

fn restart_claim(
    operation: ProviderCommandOperation,
    restart_ordinal: u64,
    epoch: u64,
) -> ProviderCommandClaim {
    command_claim(operation, restart_ordinal, epoch)
}

fn command_claim(
    operation: ProviderCommandOperation,
    restart_ordinal: u64,
    epoch: u64,
) -> ProviderCommandClaim {
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "wsg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        effect_subject: r#"{"kind":"execution","id":"wex_alpha"}"#.to_owned(),
        source_attempt_id: operation.is_restart().then(|| SOURCE_ATTEMPT.to_owned()),
        attempt_id: TARGET_ATTEMPT.to_owned(),
        dispatch_epoch: epoch,
        workload_generation: 7,
        restart_ordinal,
        desired_digest: DIGEST_A.to_owned(),
        source_digest: DIGEST_B.to_owned(),
        network_plan_digest: DIGEST_A.to_owned(),
        provider_target_digest: DIGEST_B.to_owned(),
        operation,
    })
    .expect("fixture claim should be valid")
}

fn next_restart_claim(operation: ProviderCommandOperation, epoch: u64) -> ProviderCommandClaim {
    let mut claim = restart_claim(operation, 2, epoch);
    claim.source_attempt_id = Some(TARGET_ATTEMPT.to_owned());
    claim.attempt_id = NEXT_ATTEMPT.to_owned();
    claim
}

fn journal(root: &Path) -> ProviderCommandAttemptJournal {
    ProviderCommandAttemptJournal::open(root, "container-runtime")
        .expect("fixture journal should open")
}

#[test]
fn provision_and_restart_operations_require_their_exact_ordinal_domain() {
    let restart_operations = [
        ProviderCommandOperation::WithdrawPublication,
        ProviderCommandOperation::ResetWorkloadForRestart,
        ProviderCommandOperation::PrepareRestartAttempt,
        ProviderCommandOperation::AttachRetainedNetwork,
        ProviderCommandOperation::InspectRestartActivationPrerequisites,
        ProviderCommandOperation::ActivateRestartedWorkload,
        ProviderCommandOperation::InspectRestartReadiness,
        ProviderCommandOperation::PublishRestartIngress,
        ProviderCommandOperation::ObserveRestartPublication,
    ];
    for operation in restart_operations {
        let claim = restart_claim(operation, 1, 0);
        assert_eq!(claim.operation(), operation);
        assert_eq!(claim.workload_generation(), 7);
        assert_eq!(claim.restart_ordinal(), 1);
    }

    let mut provision_with_restart_ordinal = claim(0);
    provision_with_restart_ordinal.restart_ordinal = 1;
    provision_with_restart_ordinal.source_attempt_id = Some(SOURCE_ATTEMPT.to_owned());
    assert!(matches!(
        provision_with_restart_ordinal.validate(),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));

    let mut restart_without_ordinal =
        restart_claim(ProviderCommandOperation::ResetWorkloadForRestart, 1, 0);
    restart_without_ordinal.restart_ordinal = 0;
    assert!(matches!(
        restart_without_ordinal.validate(),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));
}

#[test]
fn exact_replay_adopts_without_second_execute_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);

    assert!(matches!(
        journal
            .claim_dispatch_epoch(&claim)
            .expect("first claim should persist"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let replay = journal
        .claim_dispatch_epoch(&claim)
        .expect("exact replay should inspect");
    assert!(matches!(
        replay,
        ProviderCommandClaimDecision::AdoptExactAttempt(ProviderCommandObservation {
            kind: ProviderCommandObservationKind::Claimed,
            ..
        })
    ));
}

#[test]
fn second_same_generation_restart_requires_a_distinct_monotonic_ordinal() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first restart attempt should claim");
    journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::Succeeded,
            b"first restart completed",
        )
        .expect("first restart result should persist");

    let crossed_chain = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 2, 0);
    assert_eq!(
        journal
            .claim_dispatch_epoch(&crossed_chain)
            .expect_err("the next restart must name the prior target as its source"),
        ProviderCommandJournalError::CrossedClaim
    );
    let second = next_restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 0);
    assert!(matches!(
        journal.claim_dispatch_epoch(&second),
        Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
    ));
}

#[test]
fn exact_next_restart_ordinal_requires_a_terminal_prior_observation() {
    let cases = [
        ("claimed", ProviderCommandObservationKind::Claimed, false),
        (
            "in-progress",
            ProviderCommandObservationKind::InProgress,
            false,
        ),
        (
            "ambiguous",
            ProviderCommandObservationKind::Ambiguous,
            false,
        ),
        ("succeeded", ProviderCommandObservationKind::Succeeded, true),
        (
            "definite-failure",
            ProviderCommandObservationKind::DefiniteFailure,
            true,
        ),
        ("absent", ProviderCommandObservationKind::Absent, true),
    ];
    for (name, kind, terminal) in cases {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let journal = journal(&root.path().join(name));
        let first = restart_claim(ProviderCommandOperation::PrepareRestartAttempt, 1, 0);
        journal
            .claim_dispatch_epoch(&first)
            .expect("first restart ordinal should claim");
        if kind != ProviderCommandObservationKind::Claimed {
            journal
                .record_observation(&first, kind, name.as_bytes())
                .expect("prior observation should persist");
        }

        let second = next_restart_claim(ProviderCommandOperation::PrepareRestartAttempt, 0);
        let decision = journal.claim_dispatch_epoch(&second);
        if terminal {
            assert!(matches!(
                decision,
                Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
            ));
        } else {
            assert_eq!(
                decision.expect_err("an unresolved prior ordinal must fail before effects"),
                ProviderCommandJournalError::PriorEffectUnresolved
            );
        }
    }
}

#[test]
fn restart_ordinal_stale_skipped_and_crossed_claims_fail_before_effects() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let command_journal = journal(root.path());
    let first = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);
    command_journal
        .claim_dispatch_epoch(&first)
        .expect("first restart ordinal should claim");
    command_journal
        .record_observation(&first, ProviderCommandObservationKind::Succeeded, b"ready")
        .expect("terminal prior observation should persist");
    let second = next_restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 0);
    command_journal
        .claim_dispatch_epoch(&second)
        .expect("exact next restart ordinal should claim");

    assert_eq!(
        command_journal
            .claim_dispatch_epoch(&first)
            .expect_err("an old restart ordinal must remain fenced"),
        ProviderCommandJournalError::StaleRestartOrdinal {
            current: 2,
            candidate: 1,
        }
    );
    let skipped = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 4, 0);
    assert_eq!(
        command_journal
            .claim_dispatch_epoch(&skipped)
            .expect_err("a skipped restart ordinal must fail"),
        ProviderCommandJournalError::SkippedRestartOrdinal {
            current: 2,
            candidate: 4,
        }
    );
    let mut crossed = second.clone();
    crossed.desired_digest = DIGEST_B.to_owned();
    assert_eq!(
        command_journal
            .claim_dispatch_epoch(&crossed)
            .expect_err("crossed fences at one ordinal must fail"),
        ProviderCommandJournalError::CrossedClaim
    );

    let fresh_root = tempfile::tempdir().expect("temporary root should exist");
    let fresh_journal = journal(fresh_root.path());
    assert_eq!(
        fresh_journal
            .claim_dispatch_epoch(&restart_claim(
                ProviderCommandOperation::ActivateRestartedWorkload,
                2,
                0,
            ))
            .expect_err("a first restart command cannot skip ordinal one"),
        ProviderCommandJournalError::SkippedRestartOrdinal {
            current: 0,
            candidate: 2,
        }
    );
}

#[test]
fn concurrent_adjacent_restart_ordinal_claims_wait_for_terminal_prior_state() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let first = restart_claim(ProviderCommandOperation::AttachRetainedNetwork, 1, 0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first restart ordinal should claim");
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let journal = Arc::clone(&journal);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                journal.claim_dispatch_epoch(&next_restart_claim(
                    ProviderCommandOperation::AttachRetainedNetwork,
                    0,
                ))
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(
            handle
                .join()
                .expect("claim thread should finish")
                .expect_err(
                    "an adjacent ordinal cannot execute while its prior ordinal is unresolved"
                ),
            ProviderCommandJournalError::PriorEffectUnresolved
        );
    }
}

#[test]
fn exact_absence_is_the_only_authority_for_next_epoch() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let epoch_zero = claim(0);
    journal
        .claim_dispatch_epoch(&epoch_zero)
        .expect("first claim should persist");

    let without_absence = journal
        .claim_dispatch_epoch(&claim(1))
        .expect_err("retry without absence must fail");
    assert_eq!(
        without_absence,
        ProviderCommandJournalError::RetryWithoutAuthority
    );

    journal
        .record_observation(
            &epoch_zero,
            ProviderCommandObservationKind::Absent,
            b"runtime and manifest absent",
        )
        .expect("exact absence should persist");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&claim(1))
            .expect("exact next epoch should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
}

#[test]
fn exact_retry_receipts_survive_multiple_claim_only_crashes() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = stop_claim(1);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first stop claim should persist");
    journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::RetryAuthorized,
            b"exact live process reached the KILL redelivery deadline",
        )
        .expect("safe redelivery authority should persist");

    let second = stop_claim(2);
    let second_execution = match journal
        .claim_dispatch_epoch(&second)
        .expect("first adjacent retry should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("a new retry epoch must receive execute authority")
        }
    };
    assert!(
        second_execution
            .observation()
            .authenticates_retry_progress(&first)
    );
    journal
        .record_observation(
            &second,
            ProviderCommandObservationKind::RetryAuthorized,
            b"the same exact process remains live after another reconciliation deadline",
        )
        .expect("the second safe redelivery authority should persist");

    let third = stop_claim(3);
    let third_execution = match journal
        .claim_dispatch_epoch(&third)
        .expect("second adjacent retry should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("a second new retry epoch must receive execute authority")
        }
    };
    assert!(
        third_execution
            .observation()
            .authenticates_retry_progress(&first)
    );
    assert!(
        third_execution
            .observation()
            .authenticates_retry_progress(&second)
    );
    assert!(
        !third_execution
            .observation()
            .authenticates_retry_progress(&stop_claim(0))
    );
}

#[test]
fn stale_execution_claim_cannot_start_after_retry_authority_advances() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = stop_claim(1);
    let stale_execution = match journal
        .claim_dispatch_epoch(&first)
        .expect("the first stop epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first stop epoch must receive execute authority")
        }
    };
    journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::RetryAuthorized,
            b"inspection authorized the exact adjacent stop epoch",
        )
        .expect("retry authority should become durable");
    let second = stop_claim(2);
    let _current_execution = match journal
        .claim_dispatch_epoch(&second)
        .expect("the adjacent stop epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the adjacent stop epoch must receive execute authority")
        }
    };

    let mut effects = 0_u64;
    assert_eq!(
        journal
            .execute_current_claim(stale_execution, |_| {
                effects += 1;
                (
                    (),
                    ProviderCommandObservationKind::Succeeded,
                    None,
                    b"stale effect must not publish".to_vec(),
                )
            })
            .expect_err("an older live claimant must fail before its provider effect"),
        ProviderCommandJournalError::StaleDispatchEpoch {
            current: 2,
            candidate: 1,
        }
    );
    assert_eq!(effects, 0, "the stale claimant must not start an effect");
}

#[test]
fn execution_publishes_its_result_before_releasing_the_live_claim_lock() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = stop_claim(1);
    let execution = match journal
        .claim_dispatch_epoch(&claim)
        .expect("the stop epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first stop epoch must receive execute authority")
        }
    };
    let effect_started = Arc::new(Barrier::new(2));
    let release_effect = Arc::new(Barrier::new(2));
    let executor_journal = Arc::clone(&journal);
    let executor_started = Arc::clone(&effect_started);
    let executor_release = Arc::clone(&release_effect);
    let executor = std::thread::spawn(move || {
        executor_journal.execute_current_claim(execution, |_| {
            executor_started.wait();
            executor_release.wait();
            (
                (),
                ProviderCommandObservationKind::InProgress,
                None,
                b"the exact TERM effect is in progress".to_vec(),
            )
        })
    });

    effect_started.wait();
    let inspector_journal = Arc::clone(&journal);
    let inspector_claim = claim.clone();
    let inspector = std::thread::spawn(move || {
        inspector_journal.record_observation(
            &inspector_claim,
            ProviderCommandObservationKind::RetryAuthorized,
            b"inspection authenticated the adjacent retry",
        )
    });
    release_effect.wait();

    let (_, executed) = executor
        .join()
        .expect("executor thread should join")
        .expect("the live claimant should publish its exact result");
    assert_eq!(executed.kind(), ProviderCommandObservationKind::InProgress);
    let inspected = inspector
        .join()
        .expect("inspector thread should join")
        .expect("inspection may advance only after the effect result is durable");
    assert_eq!(
        inspected.kind(),
        ProviderCommandObservationKind::RetryAuthorized
    );
}

#[test]
fn retry_authority_is_stop_only_and_retry_lineage_corruption_fails_closed() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let activation_journal = journal(root.path());
    let activation = claim(0);
    activation_journal
        .claim_dispatch_epoch(&activation)
        .expect("activation claim should persist");
    assert!(matches!(
        activation_journal.record_observation(
            &activation,
            ProviderCommandObservationKind::RetryAuthorized,
            b"invalid retry authority",
        ),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));

    let lineage_root = root.path().join("lineage");
    let lineage_journal = journal(&lineage_root);
    let first = stop_claim(1);
    lineage_journal
        .claim_dispatch_epoch(&first)
        .expect("first stop claim should persist");
    lineage_journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::RetryAuthorized,
            b"first redelivery authority",
        )
        .expect("first retry authority should persist");
    let second = stop_claim(2);
    lineage_journal
        .claim_dispatch_epoch(&second)
        .expect("second stop claim should persist");
    lineage_journal
        .record_observation(
            &second,
            ProviderCommandObservationKind::RetryAuthorized,
            b"second redelivery authority",
        )
        .expect("second retry authority should persist");
    let third = stop_claim(3);
    lineage_journal
        .claim_dispatch_epoch(&third)
        .expect("third stop claim should persist");

    let paths = lineage_journal.paths(&third);
    let bytes = fs::read(&paths.record).expect("record should be readable");
    let mut envelope: JournalEnvelope =
        serde_json::from_slice(&bytes).expect("record should be valid JSON");
    envelope.observation.retry_lineage[1].claim.dispatch_epoch = 7;
    envelope.observation_sha256 = observation_sha256(&envelope.observation)
        .expect("the semantically corrupt observation should encode");
    fs::write(
        &paths.record,
        serde_json::to_vec_pretty(&envelope).expect("corrupt envelope should encode"),
    )
    .expect("test should publish checksum-valid semantic corruption");

    assert!(matches!(
        lineage_journal.adopt_exact_attempt(&third),
        Err(ProviderCommandJournalError::Corrupt { .. })
    ));
}

#[test]
fn process_bound_publish_success_reconciles_to_exact_absence_before_retry() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = publish_claim(0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("publish claim should persist");
    journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::Succeeded,
            b"listener was active before process death",
        )
        .expect("publish success should persist");

    let reconciled = journal
        .record_reconciled_absence(
            &first,
            b"dead process lifetime proves the listener is absent",
        )
        .expect("provider-proven process absence should supersede success");
    assert_eq!(reconciled.kind(), ProviderCommandObservationKind::Absent);
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&publish_claim(1))
            .expect("exact next publish epoch should receive authority"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
}

#[test]
fn reconciled_absence_rejects_non_publish_and_definite_failure() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let activation = claim(0);
    journal
        .claim_dispatch_epoch(&activation)
        .expect("activation claim should persist");
    assert!(matches!(
        journal.record_reconciled_absence(&activation, b"invalid"),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));

    let publish = publish_claim(0);
    let publish_journal =
        ProviderCommandAttemptJournal::open(root.path().join("publish"), "container-runtime")
            .expect("publish journal should open");
    publish_journal
        .claim_dispatch_epoch(&publish)
        .expect("publish claim should persist");
    publish_journal
        .record_observation(
            &publish,
            ProviderCommandObservationKind::DefiniteFailure,
            b"provider rejected the exact request",
        )
        .expect("definite failure should persist");
    assert_eq!(
        publish_journal
            .record_reconciled_absence(&publish, b"invalid overwrite")
            .expect_err("definite failure must remain terminal"),
        ProviderCommandJournalError::CrossedClaim
    );
}

#[test]
fn live_restart_operations_may_reconcile_provider_proven_absence() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    for operation in [
        ProviderCommandOperation::AttachRetainedNetwork,
        ProviderCommandOperation::ActivateRestartedWorkload,
        ProviderCommandOperation::PublishRestartIngress,
        ProviderCommandOperation::ObserveRestartPublication,
    ] {
        let journal = journal(&root.path().join(operation.as_str()));
        let claim = restart_claim(operation, 1, 0);
        journal
            .claim_dispatch_epoch(&claim)
            .expect("live restart command should claim");
        journal
            .record_observation(
                &claim,
                ProviderCommandObservationKind::Succeeded,
                b"live resource observed",
            )
            .expect("success should persist");
        assert_eq!(
            journal
                .record_reconciled_absence(&claim, b"resource is conclusively absent")
                .expect("approved live state may reconcile to absence")
                .kind(),
            ProviderCommandObservationKind::Absent
        );
    }
}

#[test]
fn stale_skipped_and_crossed_claims_fail_before_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = claim(2);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first claim should persist");
    journal
        .record_observation(&first, ProviderCommandObservationKind::Absent, b"absent")
        .expect("absence should persist");

    assert_eq!(
        journal
            .claim_dispatch_epoch(&claim(1))
            .expect_err("stale epoch must fail"),
        ProviderCommandJournalError::StaleDispatchEpoch {
            current: 2,
            candidate: 1,
        }
    );
    assert_eq!(
        journal
            .claim_dispatch_epoch(&claim(4))
            .expect_err("skipped epoch must fail"),
        ProviderCommandJournalError::SkippedDispatchEpoch {
            current: 2,
            candidate: 4,
        }
    );
    let mut crossed = claim(2);
    crossed.effect_subject = "crossed-subject".to_owned();
    assert_eq!(
        journal
            .claim_dispatch_epoch(&crossed)
            .expect_err("crossed claim must fail"),
        ProviderCommandJournalError::CrossedClaim
    );

    assert_eq!(
        journal
            .adopt_exact_attempt(&first)
            .expect("original observation should remain")
            .expect("original observation should exist")
            .kind(),
        ProviderCommandObservationKind::Absent
    );
}

#[test]
fn concurrent_equal_claims_grant_one_execute_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let barrier = Arc::new(Barrier::new(16));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let journal = Arc::clone(&journal);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                journal
                    .claim_dispatch_epoch(&claim(0))
                    .expect("contending claim should resolve")
            })
        })
        .collect();

    let decisions: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread should finish"))
        .collect();
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(decision, ProviderCommandClaimDecision::ExecuteClaimed(_)))
            .count(),
        1,
        "only one contender may receive effect authority"
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(
                decision,
                ProviderCommandClaimDecision::AdoptExactAttempt(_)
            ))
            .count(),
        15,
        "every losing contender must adopt the durable claim"
    );
}

#[test]
#[ignore = "spawned only by the NNC6.4 provider-journal process parent"]
fn provider_claim_child() {
    let root = PathBuf::from(
        std::env::var_os(PROCESS_ROOT_ENV).expect("child process root must be supplied"),
    );
    let role = std::env::var(PROCESS_ROLE_ENV).expect("child process role must be supplied");
    let ready = root.join(format!("ready-{role}"));
    File::create(&ready)
        .and_then(|file| file.sync_all())
        .expect("child readiness marker should become durable");
    let gate = root.join("go");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.exists() {
        assert!(
            Instant::now() < deadline,
            "child timed out waiting for process contention gate"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    match journal(&root)
        .claim_dispatch_epoch(&claim(0))
        .expect("child claim should resolve")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(_) => {
            let effect_path = root.join("external-effect");
            let mut effect = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&effect_path)
                .expect("only the execute winner may create the external-effect marker");
            effect
                .write_all(role.as_bytes())
                .and_then(|()| effect.sync_all())
                .expect("external-effect marker should become durable");
            println!("NNC64_PROVIDER_DECISION:execute");
        }
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            println!("NNC64_PROVIDER_DECISION:adopt");
        }
    }
}

#[test]
fn concurrent_processes_produce_one_external_effect() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut children: Vec<_> = (0..8)
        .map(|role| spawn_provider_child(root.path(), role))
        .collect();
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ready = (0..8)
            .filter(|role| root.path().join(format!("ready-{role}")).is_file())
            .count();
        if ready == 8 {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "only {ready}/8 provider children reached the contention gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    File::create(root.path().join("go"))
        .and_then(|file| file.sync_all())
        .expect("parent gate should become durable");

    let outputs: Vec<_> = children.iter_mut().map(wait_for_provider_child).collect();
    let execute_count = outputs
        .iter()
        .filter(|output| output.contains("NNC64_PROVIDER_DECISION:execute"))
        .count();
    let adopt_count = outputs
        .iter()
        .filter(|output| output.contains("NNC64_PROVIDER_DECISION:adopt"))
        .count();
    assert_eq!(execute_count, 1, "one process must own the effect");
    assert_eq!(adopt_count, 7, "every losing process must adopt");
    assert!(
        root.path().join("external-effect").is_file(),
        "the sole execute owner must leave the external-effect witness"
    );
}

#[test]
fn authenticated_record_rejects_tampering() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);
    journal
        .claim_dispatch_epoch(&claim)
        .expect("claim should persist");
    let paths = journal.paths(&claim);
    let bytes = fs::read(&paths.record).expect("record should be readable");
    let tampered = String::from_utf8(bytes)
        .expect("record should be UTF-8")
        .replace("\"restartOrdinal\": 1", "\"restartOrdinal\": 2");
    fs::write(&paths.record, tampered).expect("test should tamper record");

    let error = journal
        .adopt_exact_attempt(&claim)
        .expect_err("tampering must fail closed");
    assert!(matches!(error, ProviderCommandJournalError::Corrupt { .. }));
}

#[test]
fn teardown_failure_code_is_required_durable_and_exactly_replayed() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let command_journal = journal(root.path());
    let claim = stop_claim(0);
    command_journal
        .claim_dispatch_epoch(&claim)
        .expect("stop claim should persist");

    assert!(matches!(
        command_journal.record_observation(
            &claim,
            ProviderCommandObservationKind::DefiniteFailure,
            b"crossed teardown command",
        ),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));

    let persisted = command_journal
        .record_observation_with_failure_code(
            &claim,
            ProviderCommandObservationKind::DefiniteFailure,
            Some("sandbox_teardown_command_crossed"),
            b"crossed teardown command",
        )
        .expect("coded teardown failure should persist");
    assert_eq!(
        persisted.failure_code(),
        Some("sandbox_teardown_command_crossed")
    );

    let reopened = journal(root.path())
        .adopt_exact_attempt(&claim)
        .expect("reopened journal should read")
        .expect("reopened journal should retain the observation");
    assert_eq!(reopened, persisted);
    assert_eq!(
        command_journal
            .record_observation_with_failure_code(
                &claim,
                ProviderCommandObservationKind::DefiniteFailure,
                Some("sandbox_teardown_command_crossed"),
                b"crossed teardown command",
            )
            .expect("an exact replay should adopt"),
        persisted
    );
    assert_eq!(
        command_journal
            .record_observation_with_failure_code(
                &claim,
                ProviderCommandObservationKind::DefiniteFailure,
                Some("sandbox_teardown_provider_mismatch"),
                b"crossed teardown command",
            )
            .expect_err("a different durable failure code must cross the claim"),
        ProviderCommandJournalError::CrossedClaim
    );
}

#[test]
fn failure_code_schema_and_semantics_fail_closed() {
    let root = tempfile::tempdir().expect("temporary root should exist");

    let missing_root = root.path().join("missing");
    let missing_journal = journal(&missing_root);
    let missing_claim = stop_claim(0);
    missing_journal
        .claim_dispatch_epoch(&missing_claim)
        .expect("missing-field claim should persist");
    let missing_paths = missing_journal.paths(&missing_claim);
    let mut value: serde_json::Value = serde_json::from_slice(
        &fs::read(&missing_paths.record).expect("missing-field record should read"),
    )
    .expect("missing-field record should decode as JSON");
    value["observation"]
        .as_object_mut()
        .expect("observation should be an object")
        .remove("failureCode");
    fs::write(
        &missing_paths.record,
        serde_json::to_vec_pretty(&value).expect("missing-field record should encode"),
    )
    .expect("test should remove failureCode");
    assert!(matches!(
        missing_journal.adopt_exact_attempt(&missing_claim),
        Err(ProviderCommandJournalError::Corrupt { .. })
    ));

    let invalid_root = root.path().join("invalid");
    let invalid_journal = journal(&invalid_root);
    let invalid_claim = stop_claim(0);
    invalid_journal
        .claim_dispatch_epoch(&invalid_claim)
        .expect("invalid-code claim should persist");
    invalid_journal
        .record_observation_with_failure_code(
            &invalid_claim,
            ProviderCommandObservationKind::DefiniteFailure,
            Some("valid_failure"),
            b"invalid code fixture",
        )
        .expect("valid fixture should persist");
    let invalid_paths = invalid_journal.paths(&invalid_claim);
    let mut invalid_envelope: JournalEnvelope = serde_json::from_slice(
        &fs::read(&invalid_paths.record).expect("invalid-code record should read"),
    )
    .expect("invalid-code envelope should decode");
    invalid_envelope.observation.failure_code = Some("not portable".to_owned());
    invalid_envelope.observation_sha256 = observation_sha256(&invalid_envelope.observation)
        .expect("invalid-code observation should encode");
    fs::write(
        &invalid_paths.record,
        serde_json::to_vec_pretty(&invalid_envelope).expect("invalid envelope should encode"),
    )
    .expect("test should publish checksum-valid invalid code");
    assert!(matches!(
        invalid_journal.adopt_exact_attempt(&invalid_claim),
        Err(ProviderCommandJournalError::Corrupt { .. })
    ));

    let misplaced_root = root.path().join("misplaced");
    let misplaced_journal = journal(&misplaced_root);
    let misplaced_claim = stop_claim(0);
    misplaced_journal
        .claim_dispatch_epoch(&misplaced_claim)
        .expect("misplaced-code claim should persist");
    misplaced_journal
        .record_observation(
            &misplaced_claim,
            ProviderCommandObservationKind::Succeeded,
            b"successful stop",
        )
        .expect("success fixture should persist");
    let misplaced_paths = misplaced_journal.paths(&misplaced_claim);
    let mut misplaced_envelope: JournalEnvelope = serde_json::from_slice(
        &fs::read(&misplaced_paths.record).expect("misplaced-code record should read"),
    )
    .expect("misplaced-code envelope should decode");
    misplaced_envelope.observation.failure_code = Some("misplaced_failure".to_owned());
    misplaced_envelope.observation_sha256 = observation_sha256(&misplaced_envelope.observation)
        .expect("misplaced-code observation should encode");
    fs::write(
        &misplaced_paths.record,
        serde_json::to_vec_pretty(&misplaced_envelope).expect("misplaced envelope should encode"),
    )
    .expect("test should publish checksum-valid misplaced code");
    assert!(matches!(
        misplaced_journal.adopt_exact_attempt(&misplaced_claim),
        Err(ProviderCommandJournalError::Corrupt { .. })
    ));
}

#[test]
fn higher_generation_requires_resolved_prior_effect_and_fences_stale_generation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = claim(0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first claim should persist");

    let mut next = claim(0);
    next.workload_generation = 8;
    next.attempt_id =
        "wpa_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();
    next.effect_subject = r#"{"kind":"execution","id":"wex_beta"}"#.to_owned();
    assert_eq!(
        journal
            .claim_dispatch_epoch(&next)
            .expect_err("unresolved prior effect must fence replacement"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );

    journal
        .record_observation(
            &first,
            ProviderCommandObservationKind::Absent,
            b"provider and manifest absent",
        )
        .expect("absence should persist");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&next)
            .expect("resolved prior generation permits successor"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    assert_eq!(
        journal
            .claim_dispatch_epoch(&first)
            .expect_err("old generation must stay fenced"),
        ProviderCommandJournalError::StaleWorkloadGeneration {
            current: 8,
            candidate: 7,
        }
    );
}

fn spawn_provider_child(root: &Path, role: usize) -> Child {
    Command::new(std::env::current_exe().expect("sandbox test executable should resolve"))
        .args(["--exact", PROCESS_CHILD_TEST, "--ignored", "--nocapture"])
        .env(PROCESS_ROOT_ENV, root)
        .env(PROCESS_ROLE_ENV, role.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("provider journal child should start")
}

fn wait_for_provider_child(child: &mut Child) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("provider journal child exceeded 15 seconds");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed to inspect provider journal child: {error}");
            }
        }
    };
    let mut stdout = String::new();
    std::io::Read::read_to_string(
        child.stdout.as_mut().expect("child stdout should be piped"),
        &mut stdout,
    )
    .expect("child stdout should be readable");
    let mut stderr = String::new();
    std::io::Read::read_to_string(
        child.stderr.as_mut().expect("child stderr should be piped"),
        &mut stderr,
    )
    .expect("child stderr should be readable");
    assert!(status.success(), "provider child failed: {stderr}");
    stdout
}
