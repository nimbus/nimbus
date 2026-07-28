//! Creator birth/containment and provider-observation composition proofs.

use std::time::Duration;

use tempfile::TempDir;

use crate::backends::conmon::creator::{
    CreatorContainmentObservation, CreatorQuiescenceProof, OwnedConmonCreator,
    observe_creator_containment,
};
use crate::backends::conmon::lifecycle::wait_for_path;
use crate::backends::oci::command::CommandSpec;

use super::support::sample_spec;
use super::*;

#[cfg(unix)]
#[path = "creator_recovery/fresh_process.rs"]
mod fresh_process;

fn creator_recovery_fixture(
    root: &std::path::Path,
    id: &str,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let backend = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root));
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new(id), None, None)
        .expect("creator recovery fixture should plan")
        .manifest;
    (backend, manifest)
}

fn explicit_absence_command(id: &SandboxId) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            id
        ),
    ])
}

fn exact_present_creator_command(id: &SandboxId, attempt_id: &str) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{id}\",\"status\":\"running\",\
             \"annotations\":{{\"com.nimbus.creator-attempt\":\"{attempt_id}\"}}}}'"
        ),
    ])
}

#[test]
fn live_exact_creator_remains_pending_and_fences_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-live");
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-live-recovery-attempt")
        .expect("creator receipt should capture");
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: receipt.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("pending creator should persist");

    let error = backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect_err("a live exact creator must retain cleanup authority");
    assert!(
        error.to_string().contains("remains live")
            && error.to_string().contains("cleanup remains fenced"),
        "live-owner diagnostic must be explicit: {error}"
    );
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Pending { receipt }
    );

    creator
        .cancel_containment_and_reap()
        .expect("test creator should be contained");
}

#[test]
fn dead_contained_creator_and_explicit_runtime_absence_publish_quiesced_once() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-dead-contained");
    let mut creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-dead-contained-recovery-attempt")
        .expect("creator receipt should capture");
    creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator should reap with absent containment");
    std::fs::write(
        &manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");
    manifest.conmon_launch.state_command = explicit_absence_command(&manifest.handle.id);
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: receipt.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("pending creator should persist");

    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("dead-contained attempt should quiesce");
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::dead_contained(receipt),
        }
    );
    let first_bytes = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("quiesced manifest should remain durable");
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("quiesced replay should be idempotent");
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("replayed manifest should remain durable"),
        first_bytes,
        "quiesced replay must not rewrite canonical state"
    );
}

#[test]
fn dead_contained_creator_and_exact_runtime_identity_publish_runtime_observed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-runtime-observed");
    let mut creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("creator-runtime-observed-recovery-attempt")
        .expect("creator receipt should capture");
    creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator should reap with absent containment");
    manifest.conmon_launch.state_command =
        exact_present_creator_command(&manifest.handle.id, receipt.attempt_id());
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: receipt.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("pending creator should persist");

    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("exact runtime identity should complete handoff");
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::RuntimeObserved { receipt }
    );
}

#[cfg(unix)]
#[test]
fn escaped_creator_group_and_substituted_birth_are_distinct_fail_closed_outcomes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let descendant_receipt = temp_dir.path().join("creator-descendant.pid");
    let command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "sleep 60 & descendant=$!; printf '%s' \"$descendant\" > {}; exit 0",
            shell_words::quote(&descendant_receipt.to_string_lossy())
        ),
    ]);
    let mut escaped_creator =
        OwnedConmonCreator::spawn(&command).expect("escaped creator fixture should spawn");
    let escaped_receipt = escaped_creator
        .attempt_receipt("creator-escaped-recovery-attempt")
        .expect("creator receipt should capture");
    assert!(
        wait_for_path(&descendant_receipt, Duration::from_secs(2)),
        "creator descendant receipt should appear"
    );
    escaped_creator
        .reap_after_runtime_observed(Duration::from_millis(40))
        .expect_err("live descendant should retain containment");
    assert!(matches!(
        observe_creator_containment(&escaped_receipt),
        CreatorContainmentObservation::Escaped { .. }
    ));

    let (backend, mut manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-escaped");
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: escaped_receipt.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("escaped creator should persist");
    let escaped_error = backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect_err("escaped containment must remain fenced");
    assert!(
        escaped_error.to_string().contains("escaped")
            && escaped_error.to_string().contains("cleanup remains fenced"),
        "escaped outcome must remain distinct: {escaped_error}"
    );

    let (unknown_backend, mut unknown_manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-unknown");
    let mut live_creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("unknown creator fixture should spawn");
    let unknown_receipt = live_creator
        .attempt_receipt("creator-unknown-recovery-attempt")
        .expect("creator receipt should capture")
        .with_substituted_birth_for_test();
    unknown_manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: unknown_receipt,
    };
    unknown_backend
        .write_manifest(&unknown_manifest)
        .expect("unknown creator should persist");
    let unknown_error = unknown_backend
        .reconcile_pending_creator_before_cleanup(&mut unknown_manifest)
        .expect_err("substituted birth must remain unknown");
    assert!(
        unknown_error
            .to_string()
            .contains("cannot be authenticated")
            && unknown_error
                .to_string()
                .contains("different process birth"),
        "unknown outcome must remain distinct: {unknown_error}"
    );

    let escaped_group = i32::try_from(escaped_receipt.process().process_group())
        .expect("test process group should fit i32");
    // SAFETY: the signal targets the exact test group captured above.
    let _ = unsafe { libc::kill(-escaped_group, libc::SIGKILL) };
    live_creator
        .cancel_containment_and_reap()
        .expect("live test creator should be contained");
}

#[test]
fn durable_spawn_intent_proves_the_launch_gate_was_never_released() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        creator_recovery_fixture(temp_dir.path(), "creator-recovery-intent-only");
    manifest.creator_handoff = ContainerCreatorHandoffState::SpawnIntent {
        attempt_id: "creator-intent-only-attempt".to_owned(),
    };
    backend
        .write_manifest(&manifest)
        .expect("intent-only creator should persist");

    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("intent-only attempt cannot have crossed the pre-effect launch gate");
    assert_eq!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::launch_gate_never_released(
                "creator-intent-only-attempt",
            ),
        }
    );
    assert!(
        manifest.creator_handoff.authorizes_runtime_cleanup(),
        "durable unreleased-gate quiescence should authorize later runtime cleanup"
    );
}
