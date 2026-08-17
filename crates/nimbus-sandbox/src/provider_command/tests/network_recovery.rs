use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::time::Duration;

use super::*;

fn network_claim(operation: ProviderCommandOperation, epoch: u64) -> ProviderCommandClaim {
    assert!(matches!(
        operation,
        ProviderCommandOperation::DetachNetwork | ProviderCommandOperation::ReleaseNetwork
    ));
    command_claim(operation, 0, epoch)
}

#[test]
fn teardown_retry_authority_accepts_only_exact_recovery_operations() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let allowed = [
        ProviderCommandOperation::WithdrawFinalPublication,
        ProviderCommandOperation::StopExecution,
        ProviderCommandOperation::DetachNetwork,
        ProviderCommandOperation::ReleaseNetwork,
    ];
    for operation in allowed {
        let operation_root = root.path().join(operation.as_str());
        let journal = journal(&operation_root);
        let first = command_claim(operation, 0, 4);
        journal
            .claim_dispatch_epoch(&first)
            .expect("the first recoverable teardown epoch should claim");
        journal
            .record_observation(
                &first,
                ProviderCommandObservationKind::RetryAuthorized,
                b"exact recovery inspection authorized the adjacent epoch",
            )
            .expect("recoverable teardown retry authority should persist");
        let adjacent = command_claim(operation, 0, 5);
        let execution = match journal
            .claim_dispatch_epoch(&adjacent)
            .expect("the adjacent recoverable teardown epoch should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("a new adjacent epoch must receive execute authority")
            }
        };
        assert!(
            execution.observation().authenticates_retry_progress(&first),
            "the adjacent epoch must authenticate the exact prior recovery receipt"
        );
    }

    let disallowed = [
        ProviderCommandOperation::ReserveNetwork,
        ProviderCommandOperation::PrepareWorkload,
        ProviderCommandOperation::AttachNetwork,
        ProviderCommandOperation::InspectActivationPrerequisites,
        ProviderCommandOperation::ActivateWorkload,
        ProviderCommandOperation::InspectWorkloadReadiness,
        ProviderCommandOperation::PublishIngress,
        ProviderCommandOperation::ObserveIngress,
        ProviderCommandOperation::WithdrawPublication,
        ProviderCommandOperation::ResetWorkloadForRestart,
        ProviderCommandOperation::PrepareRestartAttempt,
        ProviderCommandOperation::AttachRetainedNetwork,
        ProviderCommandOperation::InspectRestartActivationPrerequisites,
        ProviderCommandOperation::ActivateRestartedWorkload,
        ProviderCommandOperation::InspectRestartReadiness,
        ProviderCommandOperation::PublishRestartIngress,
        ProviderCommandOperation::ObserveRestartPublication,
        ProviderCommandOperation::DrainExecution,
    ];
    for operation in disallowed {
        let operation_root = root.path().join(format!("rejected-{}", operation.as_str()));
        let journal = journal(&operation_root);
        let restart_ordinal = u64::from(operation.is_restart());
        let claim = command_claim(operation, restart_ordinal, 0);
        journal
            .claim_dispatch_epoch(&claim)
            .expect("the disallowed operation fixture should claim");
        assert!(matches!(
            journal.record_observation(
                &claim,
                ProviderCommandObservationKind::RetryAuthorized,
                b"invalid retry authority",
            ),
            Err(ProviderCommandJournalError::InvalidClaim { .. })
        ));
    }
}

#[test]
fn network_retry_lineage_requires_exact_adjacent_epoch() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    for operation in [
        ProviderCommandOperation::DetachNetwork,
        ProviderCommandOperation::ReleaseNetwork,
    ] {
        let journal = journal(&root.path().join(operation.as_str()));
        let first = network_claim(operation, 4);
        journal
            .claim_dispatch_epoch(&first)
            .expect("the first network recovery epoch should claim");
        journal
            .record_observation(
                &first,
                ProviderCommandObservationKind::RetryAuthorized,
                b"network inspection authorized exact recovery",
            )
            .expect("network retry authority should persist");

        assert_eq!(
            journal
                .claim_dispatch_epoch(&network_claim(operation, 6))
                .expect_err("a skipped network recovery epoch must fail"),
            ProviderCommandJournalError::SkippedDispatchEpoch {
                current: 4,
                candidate: 6,
            }
        );

        let adjacent = network_claim(operation, 5);
        let adjacent_execution = match journal
            .claim_dispatch_epoch(&adjacent)
            .expect("the exact adjacent network epoch should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("the new adjacent network epoch must receive execute authority")
            }
        };
        assert!(
            adjacent_execution
                .observation()
                .authenticates_retry_progress(&first)
        );
        assert!(matches!(
            journal
                .claim_dispatch_epoch(&adjacent)
                .expect("the exact adjacent claim should replay"),
            ProviderCommandClaimDecision::AdoptExactAttempt(ref observation)
                if observation.kind() == ProviderCommandObservationKind::Claimed
        ));
        assert_eq!(
            journal
                .claim_dispatch_epoch(&first)
                .expect_err("the prior network epoch must remain stale"),
            ProviderCommandJournalError::StaleDispatchEpoch {
                current: 5,
                candidate: 4,
            }
        );

        journal
            .record_observation(
                &adjacent,
                ProviderCommandObservationKind::Succeeded,
                b"network recovery completed",
            )
            .expect("terminal network recovery should persist");
        assert!(matches!(
            journal
                .claim_dispatch_epoch(&adjacent)
                .expect("terminal network recovery should replay"),
            ProviderCommandClaimDecision::AdoptExactAttempt(ref observation)
                if observation.kind() == ProviderCommandObservationKind::Succeeded
        ));
        assert_eq!(
            journal
                .claim_dispatch_epoch(&network_claim(operation, 6))
                .expect_err("terminal success must not authorize another network retry"),
            ProviderCommandJournalError::RetryWithoutAuthority
        );
    }
}

#[test]
fn network_inspect_current_claim_blocks_live_execute_without_writes() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = network_claim(ProviderCommandOperation::DetachNetwork, 3);
    let execution = match journal
        .claim_dispatch_epoch(&claim)
        .expect("the network command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first network command must receive execute authority")
        }
    };
    let claimed = execution.observation().clone();
    let paths = journal.paths(&claim);

    let before_claimed_inspection = fs::read(&paths.record).expect("journal record should read");
    journal
        .inspect_current_claim(&claimed, |current| {
            assert_eq!(current, &claimed);
        })
        .expect("exact claimed inspection should succeed");
    assert_eq!(
        fs::read(&paths.record).expect("journal record should remain readable"),
        before_claimed_inspection,
        "claimed inspection must not change a journal byte"
    );

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
                b"network provider effect is in progress".to_vec(),
            )
        })
    });

    effect_started.wait();
    let inspector_journal = Arc::clone(&journal);
    let stale_expected = claimed.clone();
    let lock_probe = ProviderCommandLockTestProbe::new(Duration::from_secs(1));
    let inspector_lock_probe = lock_probe.clone();
    let (callback_tx, callback_rx) = mpsc::channel();
    let inspector = std::thread::spawn(move || {
        with_provider_command_lock_test_probe(inspector_lock_probe, || {
            inspector_journal.inspect_current_claim(&stale_expected, |_| {
                callback_tx
                    .send(())
                    .expect("inspection callback should become observable");
            })
        })
    });
    assert!(
        lock_probe.wait_until_contended(),
        "inspection must attempt the exact live provider stream lock"
    );
    assert!(
        matches!(callback_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "inspection must not cross a live provider execution"
    );

    release_effect.wait();
    let (_, in_progress) = executor
        .join()
        .expect("executor thread should join")
        .expect("network execution should publish its result");
    assert_eq!(
        inspector
            .join()
            .expect("inspector thread should join")
            .expect_err("the stale inspection must fail after execution publishes"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
    assert!(
        callback_rx.try_recv().is_err(),
        "a stale inspection callback must not run"
    );

    let before_in_progress_inspection =
        fs::read(&paths.record).expect("in-progress journal record should read");
    journal
        .inspect_current_claim(&in_progress, |current| {
            assert_eq!(current, &in_progress);
        })
        .expect("exact in-progress inspection should succeed");
    assert_eq!(
        fs::read(&paths.record).expect("journal record should remain readable"),
        before_in_progress_inspection,
        "in-progress inspection must not change a journal byte"
    );
}

#[test]
fn resumed_current_claim_tokens_allow_one_execution_winner() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = network_claim(ProviderCommandOperation::ReleaseNetwork, 7);
    let original = match journal
        .claim_dispatch_epoch(&claim)
        .expect("the release command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first release command must receive execute authority")
        }
    };
    let claimed = original.observation().clone();
    let paths = journal.paths(&claim);
    let before_resume = fs::read(&paths.record).expect("claimed journal record should read");
    let first = journal
        .resume_current_claim(&claimed)
        .expect("the first owner-death token should resume");
    let second = journal
        .resume_current_claim(&claimed)
        .expect("the second owner-death token should resume");
    assert_eq!(
        fs::read(&paths.record).expect("claimed journal record should remain readable"),
        before_resume,
        "execution recovery must not change a journal byte"
    );

    let start = Arc::new(Barrier::new(3));
    let effects = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = [first, second]
        .into_iter()
        .map(|execution| {
            let journal = Arc::clone(&journal);
            let start = Arc::clone(&start);
            let effects = Arc::clone(&effects);
            std::thread::spawn(move || {
                start.wait();
                journal.execute_current_claim(execution, |_| {
                    effects.fetch_add(1, Ordering::SeqCst);
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        b"release recovery completed".to_vec(),
                    )
                })
            })
        })
        .collect();
    start.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().expect("recovery worker should join"))
        .collect();
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "one recovered token must publish the result"
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(ProviderCommandJournalError::PriorEffectUnresolved)
                )
            })
            .count(),
        1,
        "the losing recovered token must fail before its callback"
    );
    assert_eq!(
        effects.load(Ordering::SeqCst),
        1,
        "two recovered tokens must produce one provider effect"
    );
    let terminal = journal
        .adopt_exact_attempt(&claim)
        .expect("terminal release result should read")
        .expect("terminal release result should exist");
    assert!(matches!(
        journal.resume_current_claim(&terminal),
        Err(ProviderCommandJournalError::InvalidClaim { .. })
    ));
    assert!(matches!(
        journal.resume_current_claim(&claimed),
        Err(ProviderCommandJournalError::PriorEffectUnresolved)
    ));

    let mut stale = claimed.clone();
    stale.claim.dispatch_epoch = 6;
    assert_eq!(
        journal
            .resume_current_claim(&stale)
            .expect_err("a stale recovery observation must fail"),
        ProviderCommandJournalError::StaleDispatchEpoch {
            current: 7,
            candidate: 6,
        }
    );
    let mut crossed = claimed;
    crossed.claim.effect_subject = r#"{"kind":"attachment","id":"crossed"}"#.to_owned();
    assert_eq!(
        journal
            .resume_current_claim(&crossed)
            .expect_err("a crossed recovery observation must fail"),
        ProviderCommandJournalError::CrossedClaim
    );
}
