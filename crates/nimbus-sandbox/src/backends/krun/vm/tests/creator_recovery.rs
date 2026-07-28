//! Krun composition proofs for the shared creator recovery seam.

use std::time::Duration;

use crate::backends::conmon::creator::{CreatorQuiescenceProof, OwnedConmonCreator};
use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::conmon::OciConmonLayout;
use crate::backends::oci::network::OciNetworkLayout;

use super::support::*;

fn krun_creator_fixture(
    root: &std::path::Path,
    id: &str,
) -> (KrunSandboxBackend, KrunSandboxManifest) {
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.to_path_buf()));
    let spec = sample_spec_for_tenant("krun-creator-recovery", id);
    let sandbox_id = SandboxId::new(id);
    let mut manifest = sample_manifest(spec.clone(), KrunStartMode::Execute);
    manifest.handle.id = sandbox_id.clone();
    manifest.conmon_layout =
        OciConmonLayout::new_for_tenant(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
    manifest.network_layout =
        OciNetworkLayout::new(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
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

#[test]
fn krun_live_creator_and_intent_without_birth_remain_distinct_fences() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) =
        krun_creator_fixture(temp_dir.path(), "krun-creator-live-recovery");
    let mut creator = OwnedConmonCreator::spawn(
        &CommandSpec::new("/bin/sh").args(["-c".to_owned(), "exec sleep 60".to_owned()]),
    )
    .expect("creator should spawn");
    let receipt = creator
        .attempt_receipt("krun-live-creator-attempt")
        .expect("creator receipt should capture");
    manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: receipt.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("pending krun creator should persist");

    let live = backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect_err("live exact creator must remain fenced");
    assert!(
        live.to_string().contains("remains live")
            && live.to_string().contains("cleanup remains fenced"),
        "live creator outcome must be explicit: {live}"
    );
    assert_eq!(
        manifest.creator_handoff,
        KrunCreatorHandoffState::Pending { receipt }
    );

    manifest.creator_handoff = KrunCreatorHandoffState::SpawnIntent {
        attempt_id: "krun-intent-only-attempt".to_owned(),
    };
    backend
        .write_manifest(&manifest)
        .expect("intent-only krun creator should persist");
    backend
        .reconcile_pending_creator_before_cleanup(&mut manifest)
        .expect("intent-only attempt cannot cross its unreleased launch gate");
    assert_eq!(
        manifest.creator_handoff,
        KrunCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::launch_gate_never_released("krun-intent-only-attempt",),
        }
    );

    creator
        .cancel_containment_and_reap()
        .expect("test creator should be contained");
}

#[test]
fn krun_dead_contained_creator_composes_exact_absent_or_present_runtime_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (absent_backend, mut absent_manifest) =
        krun_creator_fixture(temp_dir.path(), "krun-creator-dead-absent");
    let mut absent_creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator should spawn");
    let absent_receipt = absent_creator
        .attempt_receipt("krun-dead-absent-attempt")
        .expect("creator receipt should capture");
    absent_creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator should reap with absent containment");
    absent_manifest.conmon_launch.state_command =
        explicit_absence_command(&absent_manifest.handle.id);
    absent_manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: absent_receipt.clone(),
    };
    absent_backend
        .write_manifest(&absent_manifest)
        .expect("pending absent fixture should persist");
    std::fs::write(
        &absent_manifest.conmon_layout.conmon_pidfile,
        format!("{}\n", i32::MAX),
    )
    .expect("dead conmon receipt should persist");

    absent_backend
        .reconcile_pending_creator_before_cleanup(&mut absent_manifest)
        .expect("dead-contained exact-absent krun attempt should quiesce");
    assert_eq!(
        absent_manifest.creator_handoff,
        KrunCreatorHandoffState::Quiesced {
            proof: CreatorQuiescenceProof::dead_contained(absent_receipt),
        }
    );

    let (present_backend, mut present_manifest) =
        krun_creator_fixture(temp_dir.path(), "krun-creator-dead-present");
    let mut present_creator = OwnedConmonCreator::spawn(&CommandSpec::new("/usr/bin/true"))
        .expect("creator should spawn");
    let present_receipt = present_creator
        .attempt_receipt("krun-dead-present-attempt")
        .expect("creator receipt should capture");
    present_creator
        .reap_after_runtime_observed(Duration::from_secs(1))
        .expect("creator should reap with absent containment");
    present_manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\",\
             \"annotations\":{{\"com.nimbus.creator-attempt\":\"{}\"}}}}'",
            present_manifest.handle.id,
            present_receipt.attempt_id(),
        ),
    ]);
    present_manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: present_receipt.clone(),
    };
    present_backend
        .write_manifest(&present_manifest)
        .expect("pending present fixture should persist");

    present_backend
        .reconcile_pending_creator_before_cleanup(&mut present_manifest)
        .expect("exact runtime identity should complete krun handoff");
    assert_eq!(
        present_manifest.creator_handoff,
        KrunCreatorHandoffState::RuntimeObserved {
            receipt: present_receipt,
        }
    );
}
