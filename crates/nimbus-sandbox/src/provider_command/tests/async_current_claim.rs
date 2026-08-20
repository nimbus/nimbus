use std::fs;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use super::*;

const ASYNC_PROCESS_ROOT_ENV: &str = "NIMBUS_NNC65D4_ASYNC_JOURNAL_ROOT";
const ASYNC_PROCESS_ROLE_ENV: &str = "NIMBUS_NNC65D4_ASYNC_JOURNAL_ROLE";
const ASYNC_PROCESS_CHILD: &str =
    "provider_command::tests::async_current_claim::async_current_claim_process_child";

fn execution(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
) -> ProviderCommandExecutionClaim {
    match journal
        .claim_dispatch_epoch(claim)
        .expect("the exact command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first exact command must receive execution authority")
        }
    }
}

#[tokio::test]
async fn async_start_persists_exact_request_before_provider_io() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = stop_claim(12);
    let request = br#"{"request":"exact-forwarded-teardown"}"#.to_vec();
    let expected_request_digest = evidence_sha256(&request);
    let execution = match journal
        .claim_dispatch_epoch_started(&claim, &request)
        .expect("the request and epoch should publish atomically")
    {
        ProviderCommandStartedClaimDecision::ExecuteStarted(execution) => execution,
        ProviderCommandStartedClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first exact request must receive started authority")
        }
    };
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&provider_calls);

    let (_, observation) = journal
        .execute_started_claim_async(execution, move |current| {
            Box::pin(async move {
                assert_eq!(
                    current.observation().kind(),
                    ProviderCommandObservationKind::InProgress
                );
                assert_eq!(
                    current.observation().evidence_sha256(),
                    Some(expected_request_digest.as_str())
                );
                observed_calls.fetch_add(1, Ordering::SeqCst);
                (
                    (),
                    ProviderCommandObservationKind::Ambiguous,
                    None,
                    b"provider response was lost".to_vec(),
                )
            })
        })
        .await
        .expect("the exact request boundary should publish");

    assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.kind(),
        ProviderCommandObservationKind::Ambiguous
    );
}

#[tokio::test]
async fn inspected_absence_invalidates_a_delayed_started_token_before_io() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = stop_claim(13);
    let request = b"exact request that may exist";
    let execution = match journal
        .claim_dispatch_epoch_started(&claim, request)
        .expect("the request and epoch should publish atomically")
    {
        ProviderCommandStartedClaimDecision::ExecuteStarted(execution) => execution,
        ProviderCommandStartedClaimDecision::AdoptExactAttempt(_) => unreachable!(),
    };
    let started = execution.observation().clone();

    let (_, absent) = journal
        .inspect_current_claim_async_and_publish(&started, |_| {
            Box::pin(async move {
                (
                    (),
                    ProviderCommandObservationKind::Absent,
                    None,
                    b"provider proves the request never completed".to_vec(),
                )
            })
        })
        .await
        .expect("inspection should publish exact absence");
    assert_eq!(absent.kind(), ProviderCommandObservationKind::Absent);

    let calls = Arc::new(AtomicUsize::new(0));
    let delayed_calls = Arc::clone(&calls);
    let error = journal
        .execute_started_claim_async(execution, move |_| {
            Box::pin(async move {
                delayed_calls.fetch_add(1, Ordering::SeqCst);
                (
                    (),
                    ProviderCommandObservationKind::Succeeded,
                    None,
                    b"must not publish".to_vec(),
                )
            })
        })
        .await
        .expect_err("the inspected result must invalidate the delayed token");
    assert_eq!(error, ProviderCommandJournalError::PriorEffectUnresolved);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let retry = stop_claim(14);
    let retry_request = b"exact adjacent retry request";
    let retry_execution = match journal
        .claim_dispatch_epoch_after_inspected_absence_started(
            &retry,
            claim.dispatch_epoch(),
            b"provider proves the request never completed",
            retry_request,
        )
        .expect("exact inspected absence should atomically start one adjacent retry")
    {
        ProviderCommandStartedClaimDecision::ExecuteStarted(execution) => execution,
        ProviderCommandStartedClaimDecision::AdoptExactAttempt(_) => unreachable!(),
    };
    assert_eq!(
        retry_execution.observation().kind(),
        ProviderCommandObservationKind::InProgress
    );
    assert_eq!(
        retry_execution.observation().prepared_request(),
        Some(retry_request.as_slice())
    );
    assert_eq!(retry_execution.observation().retry_lineage.len(), 1);
}

#[tokio::test]
async fn claimed_restart_inspection_fences_a_delayed_async_token_before_io() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);
    let execution = match journal
        .claim_dispatch_epoch(&claim)
        .expect("the restart epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => unreachable!(),
    };
    let delayed = journal
        .resume_current_claim(execution.observation())
        .expect("the delayed restart owner should retain its exact token");

    let (_, absent) = journal
        .inspect_claimed_current_async_and_publish(execution, |_| {
            Box::pin(async move {
                (
                    (),
                    ProviderCommandObservationKind::Absent,
                    None,
                    b"guest restart inspection proves exact absence".to_vec(),
                )
            })
        })
        .await
        .expect("the claimed restart inspection should publish");
    assert_eq!(absent.kind(), ProviderCommandObservationKind::Absent);

    let retry = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 1);
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&retry)
            .expect("exact absence should authorize the adjacent restart epoch"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));

    let calls = Arc::new(AtomicUsize::new(0));
    let delayed_calls = Arc::clone(&calls);
    assert!(matches!(
        journal
            .execute_current_claim_async(delayed, move |_| {
                Box::pin(async move {
                    delayed_calls.fetch_add(1, Ordering::SeqCst);
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        b"delayed restart effect must not publish".to_vec(),
                    )
                })
            })
            .await,
        Err(ProviderCommandJournalError::StaleDispatchEpoch {
            current: 1,
            candidate: 0,
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn adopted_claimed_async_inspection_recovers_and_fences_delayed_execution() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = restart_claim(ProviderCommandOperation::ActivateRestartedWorkload, 1, 0);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();

    let (_, absent) = journal
        .inspect_current_claim_async_and_publish(&claimed, |current| {
            Box::pin(async move {
                assert_eq!(current.kind(), ProviderCommandObservationKind::Claimed);
                (
                    (),
                    ProviderCommandObservationKind::Absent,
                    None,
                    b"reopened provider proves the claimed effect absent".to_vec(),
                )
            })
        })
        .await
        .expect("an orphaned claimed interval should be inspected and published");
    assert_eq!(absent.kind(), ProviderCommandObservationKind::Absent);

    let calls = Arc::new(AtomicUsize::new(0));
    let delayed_calls = Arc::clone(&calls);
    assert!(
        journal
            .execute_current_claim_async(execution, move |_| {
                Box::pin(async move {
                    delayed_calls.fetch_add(1, Ordering::SeqCst);
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        b"delayed effect must not publish".to_vec(),
                    )
                })
            })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_remote_inspect_contenders_publish_one_result_winner() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = stop_claim(14);
    let started = match journal
        .claim_dispatch_epoch_started(&claim, b"one remote request")
        .expect("the request and epoch should publish atomically")
    {
        ProviderCommandStartedClaimDecision::ExecuteStarted(execution) => {
            execution.observation().clone()
        }
        ProviderCommandStartedClaimDecision::AdoptExactAttempt(_) => unreachable!(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let first_journal = Arc::clone(&journal);
    let first_started = started.clone();
    let first_calls = Arc::clone(&calls);
    let first = tokio::spawn(async move {
        first_journal
            .inspect_current_claim_async_and_publish(&first_started, move |_| {
                Box::pin(async move {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        b"one correlated remote result".to_vec(),
                    )
                })
            })
            .await
    });
    tokio::task::yield_now().await;
    let second_calls = Arc::clone(&calls);
    let second = journal.inspect_current_claim_async_and_publish(&started, move |_| {
        Box::pin(async move {
            second_calls.fetch_add(1, Ordering::SeqCst);
            (
                (),
                ProviderCommandObservationKind::Succeeded,
                None,
                b"must not become a second result".to_vec(),
            )
        })
    });

    assert_eq!(
        first
            .await
            .expect("first inspection task should join")
            .expect("first inspection should publish")
            .1
            .kind(),
        ProviderCommandObservationKind::Succeeded
    );
    assert_eq!(
        second
            .await
            .expect_err("the stale contender must fail before provider I/O"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_execution_holds_the_stream_lock_through_await_and_publication() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = stop_claim(1);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();
    let effect_started = Arc::new(tokio::sync::Notify::new());
    let release_effect = Arc::new(tokio::sync::Notify::new());
    let callback_ran = Arc::new(AtomicBool::new(false));
    let executor_claimed = claimed.clone();

    let executor_journal = Arc::clone(&journal);
    let executor_started = Arc::clone(&effect_started);
    let executor_release = Arc::clone(&release_effect);
    let executor = tokio::spawn(async move {
        executor_journal
            .execute_current_claim_async(execution, move |current| {
                Box::pin(async move {
                    assert_eq!(current.observation(), &executor_claimed);
                    executor_started.notify_one();
                    executor_release.notified().await;
                    (
                        (),
                        ProviderCommandObservationKind::InProgress,
                        None,
                        b"the exact provider effect remains in progress".to_vec(),
                    )
                })
            })
            .await
    });

    effect_started.notified().await;
    let inspector_journal = Arc::clone(&journal);
    let expected = claimed;
    let inspector_callback_ran = Arc::clone(&callback_ran);
    let lock_probe = ProviderCommandLockTestProbe::new(Duration::from_secs(1));
    let inspector_probe = lock_probe.clone();
    let inspector = std::thread::spawn(move || {
        with_provider_command_lock_test_probe(inspector_probe, || {
            inspector_journal.inspect_current_claim(&expected, |_| {
                inspector_callback_ran.store(true, Ordering::SeqCst);
            })
        })
    });
    assert!(
        lock_probe.wait_until_contended(),
        "inspection must wait for the live async execution lock"
    );
    assert!(!callback_ran.load(Ordering::SeqCst));

    release_effect.notify_one();
    let (_, in_progress) = executor
        .await
        .expect("the async executor task should join")
        .expect("the live execution should publish");
    assert_eq!(
        in_progress.kind(),
        ProviderCommandObservationKind::InProgress
    );
    assert_eq!(
        inspector
            .join()
            .expect("the inspector thread should join")
            .expect_err("the stale inspection must fail after publication"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
    assert!(!callback_ran.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canceling_the_caller_does_not_cancel_the_locked_effect_or_publication() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = stop_claim(2);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();
    let effect_started = Arc::new(tokio::sync::Notify::new());
    let release_effect = Arc::new(tokio::sync::Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));

    let executor_journal = Arc::clone(&journal);
    let executor_started = Arc::clone(&effect_started);
    let executor_release = Arc::clone(&release_effect);
    let executor_effects = Arc::clone(&effects);
    let caller = tokio::spawn(async move {
        executor_journal
            .execute_current_claim_async(execution, move |_| {
                Box::pin(async move {
                    executor_effects.fetch_add(1, Ordering::SeqCst);
                    executor_started.notify_one();
                    executor_release.notified().await;
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        b"the detached worker published the exact result".to_vec(),
                    )
                })
            })
            .await
    });

    effect_started.notified().await;
    caller.abort();
    assert!(
        caller
            .await
            .expect_err("the caller should be canceled")
            .is_cancelled()
    );

    let inspector_journal = Arc::clone(&journal);
    let lock_probe = ProviderCommandLockTestProbe::new(Duration::from_secs(1));
    let inspector_probe = lock_probe.clone();
    let inspector = std::thread::spawn(move || {
        with_provider_command_lock_test_probe(inspector_probe, || {
            inspector_journal.inspect_current_claim(&claimed, |_| ())
        })
    });
    assert!(
        lock_probe.wait_until_contended(),
        "the detached worker must retain the exact stream lock"
    );

    release_effect.notify_one();
    assert_eq!(
        inspector
            .join()
            .expect("the inspector thread should join")
            .expect_err("the old claimed observation must become stale"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
    let terminal =
        wait_for_observation(&journal, &claim, ProviderCommandObservationKind::Succeeded).await;
    assert_eq!(terminal.kind(), ProviderCommandObservationKind::Succeeded);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn async_inspect_refuses_to_poll_while_an_execute_effect_can_still_start() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = stop_claim(3);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();
    let paths = journal.paths(&claim);
    let before = fs::read(&paths.record).expect("claimed record should read");
    let callback_ran = Arc::new(AtomicBool::new(false));
    let inspect_callback_ran = Arc::clone(&callback_ran);

    let result = journal
        .inspect_current_claim_async(&claimed, move |_| {
            Box::pin(async move {
                inspect_callback_ran.store(true, Ordering::SeqCst);
                "not-completed"
            })
        })
        .await
        .expect("claimed inspection should return explicit live authority");

    match result {
        ProviderCommandCurrentInspection::EffectCanStillStart(current) => {
            assert_eq!(*current, claimed);
        }
        ProviderCommandCurrentInspection::Inspected(_) => {
            panic!("a still-startable Execute must not return an inspection result")
        }
    }
    assert!(!callback_ran.load(Ordering::SeqCst));
    assert_eq!(
        fs::read(&paths.record).expect("claimed record should remain readable"),
        before,
        "read-only inspection must not change the journal"
    );
}

#[tokio::test]
async fn async_inspect_is_read_only_for_in_progress_and_ambiguous_evidence() {
    for kind in [
        ProviderCommandObservationKind::InProgress,
        ProviderCommandObservationKind::Ambiguous,
    ] {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let journal = journal(root.path());
        let claim = stop_claim(4);
        journal
            .claim_dispatch_epoch(&claim)
            .expect("the command should claim");
        let current = journal
            .record_observation(&claim, kind, b"provider evidence")
            .expect("nonterminal evidence should persist");
        let paths = journal.paths(&claim);
        let before = fs::read(&paths.record).expect("journal record should read");

        let inspected = journal
            .inspect_current_claim_async(&current, |locked| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    locked.clone()
                })
            })
            .await
            .expect("exact nonterminal inspection should succeed");
        match inspected {
            ProviderCommandCurrentInspection::Inspected(observation) => {
                assert_eq!(observation, current);
            }
            ProviderCommandCurrentInspection::EffectCanStillStart(_) => {
                panic!("settled nonterminal evidence cannot retain an Execute token")
            }
        }
        assert_eq!(
            fs::read(&paths.record).expect("journal record should remain readable"),
            before,
            "read-only inspection must preserve exact durable bytes"
        );
    }
}

#[tokio::test]
async fn stale_async_inspection_fails_before_polling_its_callback() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = stop_claim(5);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();
    journal
        .record_observation(
            &claim,
            ProviderCommandObservationKind::Succeeded,
            b"terminal provider result",
        )
        .expect("terminal result should persist");
    let callback_ran = Arc::new(AtomicBool::new(false));
    let inspect_callback_ran = Arc::clone(&callback_ran);

    assert_eq!(
        journal
            .inspect_current_claim_async(&claimed, move |_| {
                Box::pin(async move {
                    inspect_callback_ran.store(true, Ordering::SeqCst);
                })
            })
            .await
            .expect_err("a stale observation must fail before inspection"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
    assert!(!callback_ran.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "current_thread")]
async fn async_lock_wait_does_not_block_the_current_thread_runtime() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let claim = stop_claim(6);
    let execution = execution(&journal, &claim);
    let claimed = execution.observation().clone();
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
                b"provider effect completed".to_vec(),
            )
        })
    });
    effect_started.wait();

    let mut inspection =
        Box::pin(journal.inspect_current_claim_async(&claimed, |_| Box::pin(async move {})));
    tokio::select! {
        biased;
        result = &mut inspection => panic!("inspection crossed the live lock: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(20)) => {}
    }

    release_effect.wait();
    executor
        .join()
        .expect("the executor thread should join")
        .expect("the executor should publish");
    assert_eq!(
        inspection
            .await
            .expect_err("the old claimed observation must become stale"),
        ProviderCommandJournalError::PriorEffectUnresolved
    );
}

#[test]
fn two_process_async_contenders_publish_one_effect_and_result() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = stop_claim(9);
    let _execution = execution(&journal, &claim);
    let executable = std::env::current_exe().expect("test executable should resolve");
    let children = ["first", "second"].map(|role| {
        Command::new(&executable)
            .arg("--exact")
            .arg(ASYNC_PROCESS_CHILD)
            .arg("--ignored")
            .arg("--nocapture")
            .env(ASYNC_PROCESS_ROOT_ENV, root.path())
            .env(ASYNC_PROCESS_ROLE_ENV, role)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("async contender process should start")
    });
    for role in ["first", "second"] {
        wait_for_path(&root.path().join(format!("ready-{role}")));
    }
    fs::write(root.path().join("start"), b"go").expect("start gate should publish");

    for child in children {
        let output = child
            .wait_with_output()
            .expect("async contender process should finish");
        assert!(
            output.status.success(),
            "async contender failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    let effects = fs::read_to_string(root.path().join("effects"))
        .expect("one process effect should be durable");
    assert_eq!(effects.lines().count(), 1);
    assert_eq!(
        journal
            .adopt_exact_attempt(&claim)
            .expect("terminal result should read")
            .expect("terminal result should exist")
            .kind(),
        ProviderCommandObservationKind::Succeeded
    );
}

#[test]
#[ignore = "subprocess entry point; the NNC6.5d4 parent supplies the durable root"]
fn async_current_claim_process_child() {
    let Some(root) = std::env::var_os(ASYNC_PROCESS_ROOT_ENV) else {
        return;
    };
    let role = std::env::var(ASYNC_PROCESS_ROLE_ENV).expect("child role should exist");
    let root = std::path::PathBuf::from(root);
    let journal = journal(&root);
    let claim = stop_claim(9);
    let claimed = journal
        .adopt_exact_attempt(&claim)
        .expect("claimed result should read")
        .expect("claimed result should exist");
    let execution = journal
        .resume_current_claim(&claimed)
        .expect("owner-death execution authority should resume");
    fs::write(root.join(format!("ready-{role}")), b"ready")
        .expect("child readiness should publish");
    wait_for_path(&root.join("start"));

    let effect_path = root.join("effects");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("child runtime should build");
    let result = runtime.block_on(journal.execute_current_claim_async(execution, move |_| {
        Box::pin(async move {
            let mut effects = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(effect_path)
                .expect("effect witness should open");
            writeln!(effects, "{role}").expect("effect witness should persist");
            effects.sync_all().expect("effect witness should sync");
            (
                (),
                ProviderCommandObservationKind::Succeeded,
                None,
                b"one async process effect completed".to_vec(),
            )
        })
    }));
    assert!(
        matches!(&result, Ok(((), observation)) if observation.kind() == ProviderCommandObservationKind::Succeeded)
            || matches!(
                &result,
                Err(ProviderCommandJournalError::PriorEffectUnresolved)
            ),
        "a process contender must either publish or adopt the winner: {result:?}"
    );
}

async fn wait_for_observation(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    expected: ProviderCommandObservationKind,
) -> ProviderCommandObservation {
    for _ in 0..100 {
        if let Some(observation) = journal
            .adopt_exact_attempt(claim)
            .expect("exact observation should read")
            && observation.kind() == expected
        {
            return observation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the detached provider worker did not publish {expected:?}");
}

fn wait_for_path(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}
