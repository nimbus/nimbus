//! Krun lifecycle-lock and inspection-side-effect proofs.

use super::support::*;
use crate::backends::oci::conmon::OciConmonLayout;
use crate::backends::oci::network::{OciNetworkLayout, default_network_attachment_id};

const ASYNC_COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn fresh_krun_lifecycle_lock_bootstraps_only_its_private_parent() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let spec = sample_spec_for_tenant("krun-lock-bootstrap", "api");
    let sandbox_id = SandboxId::new("krun-lock-bootstrap");
    let conmon_layout =
        OciConmonLayout::new_for_tenant(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
    let network_layout =
        OciNetworkLayout::new(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
    assert!(!conmon_layout.container_state_dir.exists());

    let lifecycle = backend
        .lock_launch_lifecycle_for(&spec.tenant_id, &sandbox_id)
        .expect("fresh lifecycle lock should bootstrap its own parent");

    assert!(conmon_layout.container_state_dir.is_dir());
    assert!(
        conmon_layout
            .container_state_dir
            .join(".nimbus-krun-lifecycle.lock")
            .is_file()
    );
    assert!(
        !network_layout.network_root.exists()
            && !conmon_layout.manifest_path.exists()
            && !conmon_layout.exit_dir.exists()
            && !conmon_layout.persist_dir.exists(),
        "lock bootstrap must not perform network, manifest, exit, or persistence effects"
    );
    drop(lifecycle);
}

#[test]
fn execute_planning_waits_for_lifecycle_lock_before_network_or_manifest_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe = KrunLifecycleLockTestProbe::new(Duration::from_secs(1));
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ))
    .with_lifecycle_lock_test_probe(lock_probe.clone());
    let spec = sample_spec_for_tenant("krun-planning-lock", "api");
    let sandbox_id = SandboxId::new("krun-planning-lock");
    let lifecycle = backend
        .lock_launch_lifecycle_for(&spec.tenant_id, &sandbox_id)
        .expect("test owner should acquire the lifecycle lock");
    let network_layout =
        OciNetworkLayout::new(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
    let conmon_layout =
        OciConmonLayout::new_for_tenant(&backend.config.state_root, &spec.tenant_id, &sandbox_id);
    let planning_backend = backend.clone();
    let planning_spec = spec.clone();
    let planning_id = sandbox_id.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel();
    let planner = thread::spawn(move || {
        completed_tx
            .send(planning_backend.plan_start_with_id(&planning_spec, &planning_id, None, None))
            .expect("planning result should be observable");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "planning must reach the actual contended lifecycle-lock boundary"
    );
    assert!(
        !network_layout.network_root.exists()
            && !conmon_layout.manifest_path.exists()
            && !conmon_layout.exit_dir.exists()
            && !conmon_layout.persist_dir.exists(),
        "no launch effect may escape while another lifecycle owner holds the lock"
    );

    drop(lifecycle);
    completed_rx
        .recv_timeout(ASYNC_COMPLETION_TIMEOUT)
        .expect("planning should finish after lock release")
        .expect("planning should succeed");
    planner.join().expect("planning thread should join");
}

#[test]
fn inspect_rereads_only_after_acquiring_the_krun_lifecycle_lock() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.start_mode = KrunStartMode::PlanOnly;
    let lock_probe = KrunLifecycleLockTestProbe::new(Duration::from_secs(1));
    let backend =
        KrunSandboxBackend::new(config).with_lifecycle_lock_test_probe(lock_probe.clone());
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-inspect-lifecycle-lock"),
            None,
            None,
        )
        .expect("plan-only manifest should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should be durable");
    let lifecycle = backend
        .lock_launch_lifecycle(&manifest)
        .expect("test owner should acquire the lifecycle lock");

    let inspect_backend = backend.clone();
    let inspect_id = manifest.handle.id.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel();
    let inspector = thread::spawn(move || {
        let result = inspect_backend.inspect_sync(&inspect_id);
        completed_tx
            .send(result)
            .expect("inspection result should be observed");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "inspection must reach the actual contended lifecycle-lock boundary"
    );
    let mut changed = manifest.clone();
    changed.shutdown_requested = true;
    changed.status = SandboxStatus::Stopped;
    changed.handle.status = SandboxStatus::Stopped;
    backend
        .write_manifest(&changed)
        .expect("coordinator update should persist while inspection is fenced");

    drop(lifecycle);
    let inspected = completed_rx
        .recv_timeout(ASYNC_COMPLETION_TIMEOUT)
        .expect("inspection should finish after the lifecycle owner releases")
        .expect("inspection should succeed")
        .expect("manifest should remain visible");
    inspector.join().expect("inspection thread should join");
    assert_eq!(
        inspected, changed.handle,
        "inspection must reread the durable manifest after acquiring the lock"
    );
}

#[test]
fn explicitly_absent_runtime_without_exit_receipt_is_fenced_nonterminal() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let sandbox_id = SandboxId::new("krun-absent-without-receipt");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-absent-without-receipt", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute planning should reserve launch authority")
        .manifest;
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.shutdown_requested = false;
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    let _ = fs::remove_file(&manifest.conmon_layout.pidfile);
    let _ = fs::remove_file(&manifest.conmon_layout.exit_status_file);
    backend
        .write_manifest(&manifest)
        .expect("provider-owned fixture should persist");

    let observed = backend
        .inspect_sync(&sandbox_id)
        .expect("absence observation should remain inspectable")
        .expect("manifest should remain durable");
    assert_eq!(observed.status, SandboxStatus::Stopping);
    assert!(observed.published_endpoints.is_empty());
    let fenced = backend
        .read_manifest(&sandbox_id)
        .expect("fenced manifest should inspect")
        .expect("fenced manifest should remain durable");
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert_eq!(fenced.handle.status, SandboxStatus::Stopping);
    assert_eq!(fenced.launch_authority, KrunLaunchAuthority::ProviderOwned);
    assert!(
        !fenced.shutdown_requested,
        "unexpected absence must not invent a final-stop decision"
    );
}

#[test]
fn explicitly_absent_runtime_with_inaccessible_pidfile_fails_closed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let sandbox_id = SandboxId::new("krun-absent-inaccessible-pidfile");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-absent-inaccessible-pidfile", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute planning should reserve launch authority")
        .manifest;
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.shutdown_requested = false;
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    let _ = fs::remove_file(&manifest.conmon_layout.exit_status_file);
    let non_directory = manifest
        .conmon_layout
        .container_state_dir
        .join("pidfile-parent");
    fs::write(&non_directory, b"not a directory").expect("non-directory parent should create");
    manifest.conmon_layout.pidfile = non_directory.join("pid");
    backend
        .write_manifest(&manifest)
        .expect("provider-owned fixture should persist");
    let durable_before = backend
        .read_manifest(&sandbox_id)
        .expect("fixture should inspect")
        .expect("fixture should remain durable");

    let error = backend
        .inspect_sync(&sandbox_id)
        .expect_err("inaccessible creator evidence must fail inspection closed");
    assert!(
        error
            .to_string()
            .contains("failed to inspect sandbox pidfile")
            && error
                .to_string()
                .contains(&manifest.conmon_layout.pidfile.display().to_string()),
        "the inaccessible evidence path must remain explicit: {error}"
    );
    assert_eq!(
        backend
            .read_manifest(&sandbox_id)
            .expect("fenced manifest should inspect")
            .expect("fenced manifest should remain durable"),
        durable_before,
        "failed evidence inspection must not mutate provider-owned authority or projection"
    );
}

/// NNC0.6a fail-before baseline for NNCF20. The barrier is inside the actual
/// krun provider-launch entry selected by inspect restart policy. Withdrawal
/// persists while inspection holds a stale manifest; release proves inspection
/// still performs and republishes the restart side effect.
#[test]
#[ignore = "NNC0.6a expected red until NNC5.6/NNC6.4a make inspect side-effect-free and fence restart"]
fn nnc0_6a_krun_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ))
    .with_restart_launch_test_probe(restart_probe.clone());
    let sandbox_id = SandboxId::new("nnc0-6a-krun");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-nnc0-6a", "restart-race")
                .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute manifest should plan")
        .manifest;
    let reservation_claim = manifest
        .require_reserved_claim()
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
        .port_manager()
        .release_never_bound_launch_claim(&reservation_claim)
        .expect("fixture without a PEP effect should release never-bound port authority");
    manifest.port_leases.clear();
    manifest.egress_proxy = None;
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.next_restart_at_millis = Some(0);
    fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");

    let inspect_backend = backend.clone();
    let inspect_id = sandbox_id.clone();
    let inspect_thread = thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));
    if !restart_probe.wait_until_entered() {
        let inspect_result = inspect_thread
            .join()
            .expect("inspect thread should join after a missing barrier");
        panic!(
            "inspect must reach the provider-launch barrier through restart policy; \
             inspect completed instead with {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest;
    withdrawn.shutdown_requested = true;
    withdrawn.next_restart_at_millis = None;
    withdrawn.status = SandboxStatus::Stopped;
    withdrawn.handle.status = SandboxStatus::Stopped;
    withdrawn.handle.published_endpoints.clear();
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist before launch release");

    restart_probe.release();
    let inspected = inspect_thread
        .join()
        .expect("inspect thread should join")
        .expect("current inspect restart should complete through the test provider")
        .expect("manifest should remain inspectable");
    assert_eq!(
        inspected.status,
        SandboxStatus::Starting,
        "precondition: stale inspection currently reactivates the withdrawn manifest"
    );

    assert_eq!(
        restart_probe.effect_count(),
        0,
        "NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before \
         release must veto the stale krun restart provider effect"
    );
}
