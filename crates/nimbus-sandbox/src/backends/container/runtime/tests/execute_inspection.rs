//! Execute inspection lifecycle-lock proofs.

use super::*;
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxObservationUnknownReason,
    SandboxRestartAssessment, SandboxRestartIneligibility,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::Instant;

fn snapshot_inspection_tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(base: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        let mut entries = match std::fs::read_dir(current) {
            Ok(entries) => entries
                .map(|entry| entry.expect("test artifact entry should inspect"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("test artifact directory should inspect: {error}"),
        };
        entries.sort_by_key(std::fs::DirEntry::file_name);
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
                    Some(std::fs::read(&path).expect("test artifact should read")),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[test]
fn missing_container_manifest_inspection_creates_no_state() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let workload_root = temp_dir.path().join("missing-container-root");
    let backend = sample_plan_only_backend(&workload_root);
    let id = SandboxId::new("missing-container-inspection");

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
fn container_inspection_requires_an_existing_lock_without_recreating_it() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("container-missing-inspection-lock"),
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
        .join(super::super::runner::RUNNER_HANDOFF_LOCK_FILE);
    std::fs::remove_file(&lock_path).expect("fixture lock should be removable");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("manifest bytes should be readable");

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("a manifest without synchronization authority must fail closed");

    assert!(
        error
            .to_string()
            .contains("failed to open existing container inspection lock")
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
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "missing-lock failure must preserve the durable snapshot"
    );
}

#[test]
fn container_inspection_lock_timeout_is_bounded_and_byte_stable() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("container-inspection-lock-timeout"),
            None,
            None,
        )
        .expect("inspection fixture should plan")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should persist");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("manifest bytes should be readable");
    let lifecycle = super::super::runner::converge_runner_lifecycle_lock_with_timeout_for_test(
        &backend,
        &manifest,
        Duration::from_secs(1),
    )
    .expect("fixture should own the exclusive lifecycle lock");
    let started = Instant::now();

    let error =
        match super::super::runner::lock_current_inspection_for_backend_with_timeout_for_test(
            &backend,
            &manifest,
            Duration::from_millis(30),
        ) {
            Ok(_) => panic!("contended inspection must reach its bounded deadline"),
            Err(error) => error,
        };

    assert!(
        error
            .to_string()
            .contains("timed out acquiring existing container inspection lock")
            && error.to_string().contains("observation remains unknown"),
        "lock ambiguity must fail closed with a named diagnostic: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the injected inspection deadline must remain bounded"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "a timed-out query must not alter the durable snapshot"
    );
    drop(lifecycle);
}

#[test]
fn concurrent_and_fresh_container_inspectors_return_exact_equal_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("container-concurrent-inspection"),
            None,
            None,
        )
        .expect("inspection fixture should plan")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("inspection fixture should persist");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("manifest bytes should be readable");
    let barrier = Arc::new(Barrier::new(3));
    let mut inspectors = Vec::new();
    for _ in 0..2 {
        let inspect_backend = backend.clone();
        let inspect_id = manifest.handle.id.clone();
        let inspect_barrier = Arc::clone(&barrier);
        inspectors.push(std::thread::spawn(move || {
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
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "concurrent read locks must not publish a manifest"
    );

    let fresh_backend = sample_plan_only_backend(temp_dir.path());
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
fn execute_inspection_waits_for_lifecycle_owner_and_observes_current_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let backend = sample_plan_only_backend(temp_dir.path())
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-inspection-lifecycle-lock"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("coordinator should own the shared lifecycle lock");
    let inspect_backend = backend.clone();
    let inspect_id = manifest.handle.id.clone();
    let inspect_thread = std::thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));

    if !lock_probe.wait_until_contended() {
        drop(lifecycle);
        let inspect_result = inspect_thread
            .join()
            .expect("unfenced inspection thread should still join");
        panic!(
            "Execute inspection must acquire the shared lifecycle lock before any mutable \
             fallthrough; inspection completed without contention as {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest.clone();
    withdrawn.shutdown_requested = true;
    withdrawn.last_exit_code = Some(0);
    withdrawn.launch_reservation_claim = None;
    withdrawn.launch_artifact = None;
    withdrawn.network_cleanup_complete = true;
    synchronize_handle_status(&mut withdrawn, SandboxStatus::Stopped);
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist under the lifecycle lock");
    drop(lifecycle);

    let inspected = inspect_thread
        .join()
        .expect("inspection thread should join")
        .expect("inspection should reread the changed canonical manifest")
        .expect("terminal manifest should remain visible");
    assert_eq!(inspected.handle, withdrawn.handle);
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 0 }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Finalized);
    assert_eq!(
        backend
            .read_manifest(&withdrawn.handle.id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        withdrawn,
        "inspection must not overwrite the coordinator's terminal state"
    );
}

#[test]
fn container_runtime_state_matrix_is_read_only_and_nonpublishing() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("api", 18116, 8080)),
            &SandboxId::new("container-inspection-state-matrix"),
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
    let pending_manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("pending manifest bytes should read");
    let pending_authority_before =
        std::fs::read(&authority_path).expect("pending network authority should read");
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
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("pending manifest bytes should remain readable"),
        pending_manifest_before,
        "pending inspection must not publish or rewrite launch state"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("pending network authority should remain readable"),
        pending_authority_before,
        "pending inspection must not adopt or release reserved network authority"
    );

    manifest.launch_reservation_claim = None;

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
        let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should read");
        let authority_before =
            std::fs::read(&authority_path).expect("network authority should read");

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
            std::fs::read(&manifest.conmon_layout.manifest_path)
                .expect("manifest bytes should remain readable"),
            manifest_before,
            "{provider_status}: inspection must not persist its projection"
        );
        assert_eq!(
            std::fs::read(&authority_path).expect("network authority should remain readable"),
            authority_before,
            "{provider_status}: inspection must not mutate network authority"
        );
    }
}

#[test]
fn container_inspection_version_commits_to_exact_external_runtime_and_exit_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("container-inspection-version-evidence"),
            None,
            None,
        )
        .expect("version fixture should plan")
        .manifest;
    manifest.launch_reservation_claim = None;
    let provider_state = temp_dir.path().join("provider-state.json");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!("cat '{}'", provider_state.display()),
    ]);
    backend
        .write_manifest(&manifest)
        .expect("version fixture should persist");

    std::fs::write(
        &provider_state,
        format!(
            "{{\"id\":\"{}\",\"status\":\"created\"}}\n",
            manifest.handle.id
        ),
    )
    .expect("created provider state should persist");
    let created = backend
        .inspect_sync(&manifest.handle.id)
        .expect("created state should inspect")
        .expect("manifest should remain visible");
    std::fs::write(
        &provider_state,
        format!(
            "{{ \"id\": \"{}\", \"status\": \"created\" }}\n",
            manifest.handle.id
        ),
    )
    .expect("equivalent provider state bytes should persist");
    let alternate_created = backend
        .inspect_sync(&manifest.handle.id)
        .expect("alternate created state should inspect")
        .expect("manifest should remain visible");

    assert_eq!(created.handle, alternate_created.handle);
    assert_eq!(created.execution, alternate_created.execution);
    assert_eq!(created.restart, alternate_created.restart);
    assert_eq!(created.cleanup, alternate_created.cleanup);
    assert_ne!(
        created.version, alternate_created.version,
        "raw runtime-state substitution must change the comparison token even when the normalized provider state is identical"
    );

    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("first exit receipt should persist");
    let canonical_exit = backend
        .inspect_sync(&manifest.handle.id)
        .expect("canonical exit should inspect")
        .expect("manifest should remain visible");
    std::fs::write(&manifest.conmon_layout.exit_status_file, "042\n")
        .expect("alternate equal exit receipt should persist");
    let alternate_exit = backend
        .inspect_sync(&manifest.handle.id)
        .expect("alternate exit should inspect")
        .expect("manifest should remain visible");

    assert_eq!(canonical_exit.handle, alternate_exit.handle);
    assert_eq!(canonical_exit.execution, alternate_exit.execution);
    assert_eq!(canonical_exit.restart, alternate_exit.restart);
    assert_eq!(canonical_exit.cleanup, alternate_exit.cleanup);
    assert_ne!(
        canonical_exit.version, alternate_exit.version,
        "raw exit receipt substitution must change the comparison token"
    );
}

#[test]
fn container_terminal_inspection_versions_commit_to_exact_runner_handoff_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("container-terminal-handoff-version"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    let handoff_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let pretty_handoff = std::fs::read(&handoff_path).expect("published handoff bytes should read");
    let handoff_value: serde_json::Value =
        serde_json::from_slice(&pretty_handoff).expect("published handoff should parse");
    let compact_handoff =
        serde_json::to_vec(&handoff_value).expect("equivalent compact handoff should serialize");
    assert_ne!(
        pretty_handoff, compact_handoff,
        "the fixture requires byte-distinct equivalent handoff evidence"
    );

    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit receipt should persist");
    let pretty_exit = backend
        .inspect_sync(&manifest.handle.id)
        .expect("pretty handoff exit should inspect")
        .expect("manifest should remain visible");
    std::fs::write(&handoff_path, &compact_handoff)
        .expect("equivalent compact handoff should persist");
    let compact_exit = backend
        .inspect_sync(&manifest.handle.id)
        .expect("compact handoff exit should inspect")
        .expect("manifest should remain visible");

    assert_eq!(pretty_exit.handle, compact_exit.handle);
    assert_eq!(pretty_exit.execution, compact_exit.execution);
    assert_eq!(pretty_exit.restart, compact_exit.restart);
    assert_eq!(pretty_exit.cleanup, compact_exit.cleanup);
    assert_ne!(
        pretty_exit.version, compact_exit.version,
        "exit comparison versions must commit to exact runner handoff bytes"
    );

    std::fs::remove_file(&manifest.conmon_layout.exit_status_file)
        .expect("exit receipt should be removable");
    manifest.shutdown_requested = true;
    manifest.last_exit_code = Some(0);
    manifest.network_cleanup_complete = true;
    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
    assert!(
        manifest.has_terminal_network_finality(),
        "the finalized branch must be selected"
    );
    backend
        .write_manifest(&manifest)
        .expect("terminal manifest should persist");

    let compact_final = backend
        .inspect_sync(&manifest.handle.id)
        .expect("compact handoff final state should inspect")
        .expect("manifest should remain visible");
    std::fs::write(&handoff_path, &pretty_handoff)
        .expect("equivalent pretty handoff should persist");
    let pretty_final = backend
        .inspect_sync(&manifest.handle.id)
        .expect("pretty handoff final state should inspect")
        .expect("manifest should remain visible");

    assert_eq!(compact_final.handle, pretty_final.handle);
    assert_eq!(compact_final.execution, pretty_final.execution);
    assert_eq!(compact_final.restart, pretty_final.restart);
    assert_eq!(compact_final.cleanup, pretty_final.cleanup);
    assert_ne!(
        compact_final.version, pretty_final.version,
        "finalized comparison versions must commit to exact runner handoff bytes"
    );
}

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}
