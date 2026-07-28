//! Exact runtime-absence projection proofs.

use super::*;

#[test]
fn explicitly_absent_container_runtime_without_receipts_withdraws_ready_projection() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let sandbox_id = SandboxId::new("container-absent-without-receipt");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute planning should reserve exact network authority")
        .manifest;
    manifest.launch_reservation_claim = None;
    manifest.creator_handoff = ContainerCreatorHandoffState::RuntimeObserved {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "runtime-observed-fixture",
        ),
    };
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    let _ = std::fs::remove_file(&manifest.conmon_layout.pidfile);
    let _ = std::fs::remove_file(&manifest.conmon_layout.exit_status_file);
    backend
        .write_manifest(&manifest)
        .expect("ready provider-owned fixture should persist");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        std::fs::read(&authority_path).expect("retained network authority should be durable");

    let observed = backend
        .inspect_sync(&sandbox_id)
        .expect("explicit absence should remain inspectable")
        .expect("the durable sandbox should remain visible");
    assert_eq!(
        observed.status,
        SandboxStatus::Stopping,
        "authenticated runtime absence must withdraw a false Ready projection"
    );
    assert!(
        observed.published_endpoints.is_empty(),
        "a sandbox without a runtime must not retain visible endpoints"
    );
    let fenced = backend
        .read_manifest(&sandbox_id)
        .expect("fenced manifest should inspect")
        .expect("fenced manifest should remain durable");
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert_eq!(fenced.handle.status, SandboxStatus::Stopping);
    assert!(
        !fenced.shutdown_requested,
        "runtime absence alone must not invent a final-stop decision"
    );
    assert!(
        std::fs::read(&authority_path).expect("retained authority should remain readable")
            == authority_before,
        "projection withdrawal must not release or rewrite network authority"
    );
}
