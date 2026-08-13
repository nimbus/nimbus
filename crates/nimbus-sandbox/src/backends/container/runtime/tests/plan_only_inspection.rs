use super::*;
use crate::inspection::SandboxCleanupObservation;

fn mark_prepared_service_runner(manifest: &mut ContainerSandboxManifest) {
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
}

#[test]
fn durably_cancelled_plan_only_workload_inspects_without_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18112, 8080)),
            &SandboxId::new("runner-cancel-inspection"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("cancellation should durably converge");

    let canonical_manifest = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let canonical_decision =
        std::fs::read(&decision_path).expect("cancel decision bytes should read");
    let stage_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::manifest::MANIFEST_PUBLICATION_STAGE_FILE);
    std::fs::create_dir(&stage_path)
        .expect("directory-shaped publication stage should fence any accidental write");

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("authenticated terminal cancellation should inspect")
        .expect("terminal workload should remain visible");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopped);
    assert!(
        inspected.handle.published_endpoints.is_empty(),
        "terminal plan-only evidence must not republish preview endpoints"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("terminal manifest bytes should reread"),
        canonical_manifest,
        "terminal cancellation inspection must not publish a manifest"
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("cancel decision bytes should reread"),
        canonical_decision,
        "terminal cancellation inspection must not rewrite decision authority"
    );
    assert!(
        stage_path.is_dir(),
        "read-only inspection must leave the publication tripwire untouched"
    );
    backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("terminal cancellation replay should remain idempotent");
}

#[test]
fn nonfinal_plan_only_terminal_or_shutdown_projection_is_retained_and_nonpublishing() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());

    for (suffix, status, shutdown_requested) in [
        ("stopped", SandboxStatus::Stopped, false),
        ("failed", SandboxStatus::Failed, false),
        ("shutdown-ready", SandboxStatus::Ready, true),
    ] {
        let mut manifest = backend
            .plan_start_with_id(
                &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18120, 8080)),
                &SandboxId::new(format!("plan-only-retained-{suffix}")),
                None,
                None,
            )
            .expect("retained plan-only fixture should plan")
            .manifest;
        backend
            .write_manifest(&manifest)
            .expect("valid pre-crash plan-only fixture should persist");
        manifest.status = status;
        manifest.handle.status = status;
        manifest.shutdown_requested = shutdown_requested;
        std::fs::write(
            &manifest.conmon_layout.manifest_path,
            serde_json::to_vec(&manifest)
                .expect("contradictory legacy plan-only fixture should serialize"),
        )
        .expect("contradictory legacy plan-only fixture should replace durable bytes");

        let inspection = backend
            .inspect_sync(&manifest.handle.id)
            .expect("retained plan-only fixture should inspect")
            .expect("retained plan-only manifest should remain visible");

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
fn nonterminal_cancel_decision_remains_fenced_from_inspection() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18113, 8080)),
            &SandboxId::new("runner-nonterminal-cancel-inspection"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    drop(
        super::super::runner::lock_plan_only_status_update(&manifest, true)
            .expect("the test should publish a durable Cancel decision"),
    );
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("prepared manifest bytes should read");

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("a nonterminal Cancel decision must remain fenced");
    assert!(
        error.to_string().contains("Cancel") || error.to_string().contains("cancel"),
        "the diagnostic must identify the incomplete cancellation: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("prepared manifest bytes should reread"),
        before,
        "fenced inspection must not release or rewrite prepared authority"
    );
    backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("the authorized stop path should converge the durable Cancel decision");
}

#[test]
fn terminal_cancel_inspection_rejects_substituted_identity() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18114, 8080)),
            &SandboxId::new("runner-cancel-identity-substitution"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("cancellation should durably converge");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let mut decision: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&decision_path).expect("cancel decision bytes should read"),
    )
    .expect("cancel decision should parse");
    decision["execution_identity_sha256"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(
        &decision_path,
        serde_json::to_vec_pretty(&decision).expect("substituted decision should serialize"),
    )
    .expect("substituted decision should persist");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("substituted cancellation identity must fail closed");
    assert!(
        error.to_string().contains("does not authenticate")
            || error.to_string().contains("does not match"),
        "the diagnostic must identify decision authentication failure: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("terminal manifest bytes should reread"),
        before,
        "identity rejection must not mutate the terminal manifest"
    );
}

#[test]
fn malformed_plan_only_decision_fences_inspection_without_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18115, 8080)),
            &SandboxId::new("runner-malformed-inspection-decision"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    std::fs::write(&decision_path, b"{not-json\n")
        .expect("malformed decision fixture should persist");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("prepared manifest bytes should read");

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("malformed decision authority must fence inspection");
    assert!(
        error.to_string().contains("failed to parse durable"),
        "inspection must identify malformed durable authority: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("prepared manifest bytes should reread"),
        before,
        "malformed decision rejection must not publish a manifest"
    );
}
