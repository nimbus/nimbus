//! Krun lifecycle-lock and inspection-side-effect proofs.

use super::support::*;
use crate::backends::oci::conmon::OciConmonLayout;
use crate::backends::oci::network::{OciNetworkLayout, default_network_attachment_id};
use crate::inspection::{SandboxCleanupObservation, SandboxObservationUnknownReason};
use std::sync::{Arc, Barrier};
use std::time::Instant;

const ASYNC_COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

fn snapshot_inspection_tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(base: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        let mut entries = match fs::read_dir(current) {
            Ok(entries) => entries
                .map(|entry| entry.expect("test artifact entry should inspect"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("test artifact directory should inspect: {error}"),
        };
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .expect("test artifact should remain under its fixture root")
                .to_path_buf();
            let metadata = entry
                .metadata()
                .expect("test artifact metadata should inspect");
            if metadata.is_dir() {
                snapshot.insert(relative, None);
                visit(base, &path, snapshot);
            } else {
                snapshot.insert(
                    relative,
                    Some(fs::read(&path).expect("test artifact should read")),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[test]
fn missing_krun_manifest_inspection_creates_no_state() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let workload_root = temp_dir.path().join("missing-krun-root");
    let backend =
        KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(workload_root.clone()));
    let id = SandboxId::new("missing-krun-inspection");

    let before = snapshot_inspection_tree(&workload_root);
    assert!(
        backend
            .inspect_sync(&id)
            .expect("missing manifest inspection should succeed")
            .is_none()
    );
    assert_eq!(
        snapshot_inspection_tree(&workload_root),
        before,
        "a query for a missing manifest must not create a directory, lock, or other artifact"
    );
}

#[test]
fn nonfinal_krun_plan_only_terminal_or_shutdown_projection_is_retained() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.start_mode = KrunStartMode::PlanOnly;
    let backend = KrunSandboxBackend::new(config);

    for (suffix, status, shutdown_requested) in [
        ("stopped", SandboxStatus::Stopped, false),
        ("failed", SandboxStatus::Failed, false),
        ("shutdown-ready", SandboxStatus::Ready, true),
    ] {
        let mut manifest = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new(format!("krun-plan-only-retained-{suffix}")),
                None,
                None,
            )
            .expect("retained krun plan-only fixture should plan")
            .manifest;
        backend
            .write_manifest(&manifest)
            .expect("valid pre-crash krun plan-only fixture should persist");
        manifest.status = status;
        manifest.handle.status = status;
        manifest.shutdown_requested = shutdown_requested;
        std::fs::write(
            &manifest.conmon_layout.manifest_path,
            serde_json::to_vec(&manifest)
                .expect("contradictory legacy krun plan-only fixture should serialize"),
        )
        .expect("contradictory legacy krun plan-only fixture should replace durable bytes");

        let inspection = backend
            .inspect_sync(&manifest.handle.id)
            .expect("retained krun plan-only fixture should inspect")
            .expect("retained krun plan-only manifest should remain visible");

        assert_eq!(
            inspection.handle.status,
            SandboxStatus::Stopping,
            "{suffix}"
        );
        assert_eq!(
            inspection.cleanup,
            SandboxCleanupObservation::Retained,
            "{suffix}"
        );
        assert!(
            inspection.handle.published_endpoints.is_empty(),
            "{suffix}: retained or contradictory PlanOnly evidence cannot publish"
        );
    }
}

#[test]
fn krun_inspection_requires_an_existing_lock_without_recreating_it() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.start_mode = KrunStartMode::PlanOnly;
    let backend = KrunSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-missing-inspection-lock"),
            None,
            None,
        )
        .expect("inspection fixture should plan")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should persist");
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::lifecycle::KRUN_LIFECYCLE_LOCK_FILE);
    fs::remove_file(&lock_path).expect("fixture lock should be removable");
    let manifest_before =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should be readable");

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("a manifest without synchronization authority must fail closed");

    assert!(
        error
            .to_string()
            .contains("failed to open existing krun inspection lock")
            && error
                .to_string()
                .contains("inspection cannot create synchronization state"),
        "missing-lock ambiguity must remain named: {error}"
    );
    assert!(
        !lock_path.exists(),
        "inspection must not recreate a missing lifecycle lock"
    );
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "missing-lock failure must preserve the durable snapshot"
    );
}

#[test]
fn krun_inspection_lock_timeout_is_bounded_and_byte_stable() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.start_mode = KrunStartMode::PlanOnly;
    let backend = KrunSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-inspection-lock-timeout"),
            None,
            None,
        )
        .expect("inspection fixture should plan")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should persist");
    let manifest_before =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should be readable");
    let lifecycle = backend
        .lock_launch_lifecycle(&manifest)
        .expect("fixture should own the exclusive lifecycle lock");
    let started = Instant::now();

    let error = match backend
        .lock_current_inspection_with_timeout_for_test(&manifest, Duration::from_millis(30))
    {
        Ok(_) => panic!("contended inspection must reach its bounded deadline"),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("timed out acquiring existing krun inspection lock")
            && error.to_string().contains("observation remains unknown"),
        "lock ambiguity must fail closed with a named diagnostic: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the injected inspection deadline must remain bounded"
    );
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "a timed-out query must not alter the durable snapshot"
    );
    drop(lifecycle);
}

#[test]
fn concurrent_and_fresh_krun_inspectors_return_exact_equal_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.start_mode = KrunStartMode::PlanOnly;
    let backend = KrunSandboxBackend::new(config.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-concurrent-inspection"),
            None,
            None,
        )
        .expect("inspection fixture should plan")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should persist");
    let manifest_before =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should be readable");
    let barrier = Arc::new(Barrier::new(3));
    let mut inspectors = Vec::new();
    for _ in 0..2 {
        let inspect_backend = backend.clone();
        let inspect_id = manifest.handle.id.clone();
        let inspect_barrier = Arc::clone(&barrier);
        inspectors.push(thread::spawn(move || {
            inspect_barrier.wait();
            inspect_backend.inspect_sync(&inspect_id)
        }));
    }
    barrier.wait();
    let first = inspectors
        .remove(0)
        .join()
        .expect("first inspector should join")
        .expect("first inspection should succeed")
        .expect("manifest should remain present");
    let second = inspectors
        .remove(0)
        .join()
        .expect("second inspector should join")
        .expect("second inspection should succeed")
        .expect("manifest should remain present");
    assert_eq!(second, first);
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "concurrent read locks must not publish a manifest"
    );

    let fresh_backend = KrunSandboxBackend::new(config);
    let fresh = fresh_backend
        .inspect_sync(&manifest.handle.id)
        .expect("fresh backend inspection should succeed")
        .expect("manifest should remain present");
    assert_eq!(
        fresh, first,
        "process-local construction cannot change authenticated evidence"
    );

    manifest.last_exit_code = Some(7);
    backend
        .write_manifest(&manifest)
        .expect("durable evidence substitution should persist");
    let substituted = backend
        .inspect_sync(&manifest.handle.id)
        .expect("substituted inspection should succeed")
        .expect("manifest should remain present");
    assert_eq!(substituted.handle, first.handle);
    assert_ne!(
        substituted.version, first.version,
        "the comparison token must detect a durable snapshot substitution"
    );
}

#[test]
fn fresh_krun_lifecycle_lock_bootstraps_only_its_private_parent() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let spec = sample_spec_for_tenant("krun-lock-bootstrap", "api");
    let sandbox_id = SandboxId::new("krun-lock-bootstrap");
    let conmon_layout = OciConmonLayout::new_for_tenant(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
    let network_layout = OciNetworkLayout::under_root(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
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
    let network_layout = OciNetworkLayout::under_root(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
    let conmon_layout = OciConmonLayout::new_for_tenant(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
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
    assert_eq!(inspected.handle.status, SandboxStatus::Stopped);
    assert!(
        inspected.handle.published_endpoints.is_empty(),
        "terminal observation must not republish stale durable endpoints"
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Finalized);
    assert_eq!(
        backend
            .read_manifest(&changed.handle.id)
            .expect("changed manifest should remain readable")
            .expect("changed manifest should remain present"),
        changed,
        "inspection must reread without changing the durable winner"
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
    assert_eq!(observed.handle.status, SandboxStatus::Stopping);
    assert!(observed.handle.published_endpoints.is_empty());
    assert_eq!(
        observed.execution,
        SandboxExecutionObservation::AbsentWithoutExit
    );
    assert_eq!(
        observed.restart,
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
        }
    );
    assert_eq!(observed.cleanup, SandboxCleanupObservation::Retained);
    let fenced = backend
        .read_manifest(&sandbox_id)
        .expect("fenced manifest should inspect")
        .expect("fenced manifest should remain durable");
    assert_eq!(
        fenced, manifest,
        "inspection must not persist its projection"
    );
    assert_eq!(fenced.launch_authority, KrunLaunchAuthority::ProviderOwned);
    assert!(
        !fenced.shutdown_requested,
        "unexpected absence must not invent a final-stop decision"
    );
}

#[test]
fn krun_runtime_state_matrix_is_read_only_and_nonpublishing() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-inspection-state-matrix", "api")
                .with_port_binding(SandboxPortBinding::tcp("api", 18117, 8080)),
            &SandboxId::new("krun-inspection-state-matrix"),
            None,
            None,
        )
        .expect("state-matrix fixture should plan")
        .manifest;
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    backend
        .write_manifest(&manifest)
        .expect("pending-launch fixture should persist");
    let pending_manifest_before = fs::read(&manifest.conmon_layout.manifest_path)
        .expect("pending manifest bytes should read");
    let pending_authority_before =
        fs::read(&authority_path).expect("pending network authority should read");
    let pending = backend
        .inspect_sync(&manifest.handle.id)
        .expect("pending launch should inspect")
        .expect("pending manifest should remain visible");
    assert_eq!(pending.handle.status, SandboxStatus::Starting);
    assert!(pending.handle.published_endpoints.is_empty());
    assert_eq!(
        pending.execution,
        SandboxExecutionObservation::Unknown {
            reason: SandboxObservationUnknownReason::LaunchHandoffPending,
        }
    );
    assert_eq!(
        pending.restart,
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
        }
    );
    assert_eq!(pending.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("pending manifest bytes should remain readable"),
        pending_manifest_before,
        "pending inspection must not publish or rewrite launch state"
    );
    assert_eq!(
        fs::read(&authority_path).expect("pending network authority should remain readable"),
        pending_authority_before,
        "pending inspection must not adopt or release reserved network authority"
    );

    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;

    for (provider_status, expected_status, expected_restart, expected_cleanup) in [
        (
            "created",
            SandboxStatus::Starting,
            SandboxRestartIneligibility::RuntimePresent,
            SandboxCleanupObservation::NotRequired,
        ),
        (
            "creating",
            SandboxStatus::Starting,
            SandboxRestartIneligibility::RuntimePresent,
            SandboxCleanupObservation::NotRequired,
        ),
        (
            "paused",
            SandboxStatus::Stopping,
            SandboxRestartIneligibility::CleanupPending,
            SandboxCleanupObservation::Retained,
        ),
        (
            "stopped",
            SandboxStatus::Stopping,
            SandboxRestartIneligibility::CleanupPending,
            SandboxCleanupObservation::Retained,
        ),
        (
            "provider-unknown",
            SandboxStatus::Stopping,
            SandboxRestartIneligibility::CleanupPending,
            SandboxCleanupObservation::Retained,
        ),
    ] {
        manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"{provider_status}\"}}'",
                manifest.handle.id
            ),
        ]);
        backend
            .write_manifest(&manifest)
            .expect("state-matrix fixture should persist");
        let manifest_before =
            fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should read");
        let authority_before = fs::read(&authority_path).expect("network authority should read");

        let inspected = backend
            .inspect_sync(&manifest.handle.id)
            .expect("runtime state should inspect")
            .expect("manifest should remain visible");
        assert_eq!(
            inspected.handle.status, expected_status,
            "{provider_status}"
        );
        assert!(
            inspected.handle.published_endpoints.is_empty(),
            "{provider_status}: a non-Ready runtime must not publish endpoints"
        );
        assert_eq!(
            inspected.execution,
            SandboxExecutionObservation::Present,
            "{provider_status}"
        );
        assert_eq!(
            inspected.restart,
            SandboxRestartAssessment::Ineligible {
                reason: expected_restart,
            },
            "{provider_status}"
        );
        assert_eq!(inspected.cleanup, expected_cleanup, "{provider_status}");
        assert_eq!(
            backend
                .inspect_sync(&manifest.handle.id)
                .expect("repeated runtime state should inspect")
                .expect("manifest should remain visible"),
            inspected,
            "{provider_status}: unchanged provider evidence must remain exact"
        );
        assert_eq!(
            fs::read(&manifest.conmon_layout.manifest_path)
                .expect("manifest bytes should remain readable"),
            manifest_before,
            "{provider_status}: inspection must not persist its projection"
        );
        assert_eq!(
            fs::read(&authority_path).expect("network authority should remain readable"),
            authority_before,
            "{provider_status}: inspection must not mutate network authority"
        );
    }
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
    fs::create_dir(&manifest.conmon_layout.pidfile)
        .expect("the exact pidfile path should be unreadable as a file");
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
        error.to_string().contains("failed to read sandbox pidfile")
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

/// NNC0.6a regression for NNCF20. Inspection races a durable withdrawal and
/// must return the coordinator's current retained snapshot without entering
/// the provider-launch authority that the historical fail-before exposed.
#[test]
fn nnc0_6a_krun_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let mut backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ))
    .with_restart_launch_test_probe(restart_probe.clone());
    // NNC5.6 characterizes the inspection edge itself. Host startup
    // reconciliation is a separate admission fence and must not short-circuit
    // this semantic regression fixture.
    backend.startup_network_reconciliation_error = None;
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
        .port_lease_coordinator()
        .release_never_bound_launch_claim(&reservation_claim)
        .expect("fixture without a PEP effect should release never-bound port authority");
    manifest.port_leases.clear();
    manifest.egress_proxy = None;
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");
    let manifest_before =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should read");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        fs::read(&authority_path).expect("network authority should remain durable");

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
    fs::write(&manifest.conmon_layout.exit_status_file, "43\n")
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
    fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
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
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before
    );
    assert_eq!(
        fs::read(&authority_path).expect("network authority should remain readable"),
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
         release must veto the stale krun restart provider effect"
    );
}
