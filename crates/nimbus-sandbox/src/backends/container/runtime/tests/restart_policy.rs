//! Container restart-policy decisions and inspection boundaries.

use super::*;
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxRestartAssessment,
    SandboxRestartIneligibility,
};

/// NNC0.6a regression for NNCF20. Inspection races a durable withdrawal and
/// must return the coordinator's current retained snapshot without entering
/// the provider-launch authority that the historical fail-before exposed.
#[test]
fn nnc0_6a_container_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let mut backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_restart_launch_test_probe(restart_probe.clone());
    // NNC5.6 characterizes the inspection edge itself. Host startup
    // reconciliation is a separate admission fence and must not short-circuit
    // this semantic regression fixture.
    backend.startup_reconciliation_error = None;
    let sandbox_id = SandboxId::new("nnc0-6a-container");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute manifest should plan")
        .manifest;
    let reservation_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("restart fixture should begin with exact reserved authority")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &reservation_claim,
        )
        .expect("restart fixture should adopt its exact attachment");
    backend
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("restart fixture should authenticate its port authority")
        .release_never_bound_launch_claim(&reservation_claim)
        .expect("fixture without provider listeners should release never-bound authority");
    manifest.port_leases.clear();
    manifest.egress_proxy = None;
    manifest.launch_reservation_claim = None;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");
    let manifest_before =
        std::fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should read");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("network authority should remain durable");

    let inspected = backend
        .inspect_sync(&sandbox_id)
        .expect("restart-eligible inspection should succeed")
        .expect("manifest should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 42 }
    );
    assert_eq!(
        inspected.restart,
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            blocker: None,
        }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    let repeated = backend
        .inspect_sync(&sandbox_id)
        .expect("repeated inspection should succeed")
        .expect("manifest should remain inspectable");
    assert_eq!(repeated, inspected);
    std::fs::write(&manifest.conmon_layout.exit_status_file, "43\n")
        .expect("substitute exit evidence should persist");
    let substituted = backend
        .inspect_sync(&sandbox_id)
        .expect("substituted inspection should succeed")
        .expect("manifest should remain inspectable");
    assert_eq!(
        substituted.execution,
        SandboxExecutionObservation::Exited { exit_code: 43 }
    );
    assert_ne!(
        substituted.version, inspected.version,
        "changing only provider evidence must change the comparison version"
    );
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("original exit evidence should restore");
    let restored = backend
        .inspect_sync(&sandbox_id)
        .expect("restored inspection should succeed")
        .expect("manifest should remain inspectable");
    assert_eq!(
        restored, inspected,
        "restoring provider evidence must restore byte-stable inspection evidence"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("network authority should remain readable"),
        authority_before
    );
    assert_eq!(restart_probe.effect_count(), 0);

    let mut withdrawn = manifest;
    withdrawn.shutdown_requested = true;
    withdrawn.status = SandboxStatus::Stopping;
    withdrawn.handle.status = SandboxStatus::Stopping;
    withdrawn.handle.published_endpoints.clear();
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist");

    let withdrawn_inspection = backend
        .inspect_sync(&sandbox_id)
        .expect("withdrawn inspection should succeed")
        .expect("manifest should remain inspectable");
    assert_eq!(withdrawn_inspection.handle.status, SandboxStatus::Stopping);
    assert_eq!(
        withdrawn_inspection.restart,
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::ShutdownRequested,
        }
    );

    assert_eq!(
        restart_probe.effect_count(),
        0,
        "NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before \
         release must veto the stale container restart provider effect"
    );
}
