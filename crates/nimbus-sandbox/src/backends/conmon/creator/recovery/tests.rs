use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::backends::conmon::creator::OwnedConmonCreator;
use crate::backends::conmon::lifecycle::wait_for_path;
use crate::backends::oci::command::CommandSpec;

#[test]
fn exact_live_creator_receipt_round_trips_and_observes_live() {
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-live-attempt")
        .expect("live creator receipt should capture");
    let encoded = serde_json::to_value(&receipt).expect("receipt should serialize");
    let decoded: CreatorAttemptReceipt =
        serde_json::from_value(encoded.clone()).expect("receipt should deserialize");

    assert_eq!(decoded, receipt);
    assert_eq!(receipt.attempt_id(), "creator-live-attempt");
    assert_eq!(receipt.process().pid(), creator.child.id());
    assert_eq!(
        encoded["process"]["process_group"],
        serde_json::json!(creator.child.id())
    );
    assert!(
        encoded["process"]["birth"]["kind"].is_string(),
        "birth kind must be explicit in the durable schema: {encoded}"
    );
    assert_eq!(
        observe_creator_containment(&receipt),
        CreatorContainmentObservation::Live
    );

    creator
        .cancel_containment_and_reap()
        .expect("test creator should be contained");
}

#[test]
fn exact_dead_creator_and_absent_group_observe_dead_contained() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let release = temp_dir.path().join("release-creator");
    let command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "while [ ! -e {} ]; do sleep 0.01; done",
            shell_words::quote(&release.to_string_lossy())
        ),
    ]);
    let mut creator = OwnedConmonCreator::spawn(&command).expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-dead-contained-attempt")
        .expect("creator receipt should capture before reap");
    std::fs::write(&release, b"release\n").expect("creator release gate should publish");
    creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("successful creator should reap with absent containment");

    assert_eq!(
        observe_creator_containment(&receipt),
        CreatorContainmentObservation::DeadContained
    );
}

#[cfg(unix)]
#[test]
fn absent_leader_with_live_group_observes_escaped_without_signalling() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let descendant_receipt = temp_dir.path().join("descendant.pid");
    let release = temp_dir.path().join("release-creator");
    let command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "while [ ! -e {} ]; do sleep 0.01; done; \
             sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; exit 0",
            shell_words::quote(&release.to_string_lossy()),
            shell_words::quote(&descendant_receipt.to_string_lossy())
        ),
    ]);
    let mut creator = OwnedConmonCreator::spawn(&command).expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-escaped-attempt")
        .expect("creator receipt should capture");
    std::fs::write(&release, b"release\n").expect("creator release gate should publish");
    assert!(
        wait_for_path(&descendant_receipt, Duration::from_secs(2)),
        "descendant receipt should appear"
    );
    creator
        .reap_after_runtime_observed(Duration::from_millis(40))
        .expect_err("live descendant must prevent containment completion");

    let observation = observe_creator_containment(&receipt);
    assert!(
        matches!(
            &observation,
            CreatorContainmentObservation::Escaped { reason }
                if reason.contains("leader is absent")
                    && reason.contains("process group")
        ),
        "live descendant must be a distinct escaped observation: {observation:?}"
    );
    let group = i32::try_from(receipt.process().process_group())
        .expect("test process group should fit i32");
    // SAFETY: this signal targets the exact test group captured before its
    // leader was reaped; the test has not allowed PID reuse between capture and
    // cleanup.
    let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
}

#[test]
fn substituted_birth_observes_unknown_and_never_matches_by_pid_alone() {
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("creator should spawn");
    let mut receipt = creator
        .attempt_receipt("creator-substituted-birth-attempt")
        .expect("creator receipt should capture");
    match &mut receipt.process.birth {
        CreatorProcessBirth::LinuxProcStartTicks { ticks } => *ticks = ticks.saturating_add(1),
        CreatorProcessBirth::AppleBsdStartTime { microseconds, .. } => {
            *microseconds = microseconds.saturating_add(1)
        }
    }

    let observation = observe_creator_containment(&receipt);
    assert!(
        matches!(
            &observation,
            CreatorContainmentObservation::Unknown { reason }
                if reason.contains("recycled")
                    && reason.contains("different process birth")
        ),
        "a substituted birth token must not authenticate from PID equality: {observation:?}"
    );

    creator
        .cancel_containment_and_reap()
        .expect("test creator should be contained");
}

#[test]
fn linux_proc_stat_parser_handles_parentheses_inside_the_command_name() {
    let identity = parse_linux_process_stat(
        42,
        "42 (creator ) command) S 1 77 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 999 21",
    )
    .expect("complete Linux process stat should parse");

    assert_eq!(identity.pid, 42);
    assert_eq!(identity.process_group, 77);
    assert_eq!(
        identity.birth,
        CreatorProcessBirth::LinuxProcStartTicks { ticks: 999 }
    );
}
