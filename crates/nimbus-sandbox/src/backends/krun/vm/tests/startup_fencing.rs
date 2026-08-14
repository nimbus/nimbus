//! Startup reconciliation fences admission while preserving exact cleanup.

use super::support::*;

use std::sync::Arc;

use nimbus_network::{
    NetworkProviderHandle, NetworkResourcePhase, NetworkSegmentAllocator, NetworkStateTransition,
    NetworkTransitionEvidence,
};

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::oci::network::{
    AttachmentBackendKind, OciNetavarkOperation, OciSegmentAllocator, RecordingSegmentAllocator,
    ReservedNetworkLaunchAuthority, ReservedNetworkLaunchIdentity, default_network_attachment_id,
    oci_attachment_plan, release_reserved_network_launch_after_ports_with_terminal_publication,
    setup_host_managed_network_for_test, terminal_container_ipam_release_is_absent_for_test,
};

fn inject_startup_reconciliation_failure(backend: &mut KrunSandboxBackend) {
    backend.startup_network_reconciliation_error = Some(Arc::<str>::from(
        "injected retained krun startup reconciliation failure",
    ));
}

#[test]
fn nnc8_3_krun_exact_quarantined_orphan_converges_before_capacity_reuse() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = "/usr/bin/true".into();
    let backend = KrunSandboxBackend::new(config.clone());
    let spec = sample_spec_for_tenant("krun-nnc8-3", "orphan");
    let sandbox_id = SandboxId::new("krun-nnc8-3-quarantined-orphan");
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("planning should persist exact pre-effect authority")
        .manifest;
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("execute plan should contain exact network cleanup context")
        .clone();
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("planned generation should retain its exact launch claim")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &spec.tenant_id,
            &network_config.attachment_id,
            &reservation_claim,
        )
        .expect("the provider setup cut should adopt the exact segment hold");

    let attachments = backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should be open");
    let association = backend
        .segment_allocator
        .inspect_attachment_reservation(
            &spec.tenant_id,
            &network_config.attachment_id,
            &reservation_claim,
        )
        .expect("adopted segment association should inspect")
        .association()
        .expect("adopted segment association should remain present")
        .clone();
    attachments
        .reserve(
            &spec.tenant_id,
            host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Krun),
            &oci_attachment_plan(&spec.tenant_id, &sandbox_id, AttachmentBackendKind::Krun),
            network_config.attachment_id.clone(),
            association,
        )
        .expect("portable attachment desire should be durable before provider effects");
    let reserved = attachments
        .get(&spec.tenant_id, &network_config.attachment_id)
        .expect("portable authority should inspect")
        .expect("planned attachment should be durable");
    let (_, provisioning) = attachments
        .apply_transition(
            &spec.tenant_id,
            &NetworkStateTransition::new(
                reserved.resource().version().clone(),
                NetworkResourcePhase::Provisioning,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("portable attachment should enter provisioning");
    let provider_id = host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Krun);
    let stable_handle = NetworkProviderHandle::new(
        provider_id,
        format!(
            "attachment:{}:{}",
            provisioning.resource().version().plan_id(),
            network_config.attachment_id
        ),
    )
    .expect("stable attachment handle should validate");
    let (_, with_handle) = attachments
        .record_provider_handle(
            &spec.tenant_id,
            provisioning.resource().version(),
            stable_handle,
        )
        .expect("provider handle should become durable before readiness");
    attachments
        .apply_transition(
            &spec.tenant_id,
            &NetworkStateTransition::new(
                with_handle.resource().version().clone(),
                NetworkResourcePhase::Ready,
                NetworkTransitionEvidence::Progress,
            ),
        )
        .expect("portable attachment should record provider readiness");

    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: reservation_claim.clone(),
    };
    fs::write(&manifest.network_layout.netns_path, b"provider-netns")
        .expect("provider namespace witness should exist before setup acknowledgement");
    let operation = OciNetavarkOperation::new(
        &manifest.network_layout,
        &network_config,
        &sandbox_id,
        spec.display_name(),
        "krun-nnc8-3-orphan",
        &spec.port_bindings,
        None,
    );
    setup_host_managed_network_for_test(&backend.ipam_authority, &operation)
        .expect("substituted provider setup should publish exact ready evidence");
    backend
        .segment_allocator
        .quarantine(
            &spec.tenant_id,
            &network_config.attachment_id,
            Some(&reservation_claim),
        )
        .expect("the crash cut should durably fence the exact adopted hold before teardown");
    backend
        .write_manifest(&manifest)
        .expect("exact backend cleanup context should be durable");

    drop(backend);

    let recovered = KrunSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_network_reconciliation_error.is_none(),
        "a fresh Krun owner should converge the exact quarantined generation: {:?}",
        recovered.startup_network_reconciliation_error
    );
    let attachment = recovered
        .attachment_authority
        .as_ref()
        .expect("recovered portable authority should be open")
        .get(&spec.tenant_id, &network_config.attachment_id)
        .expect("released portable authority should inspect")
        .expect("terminal attachment evidence should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Released
    );
    assert_eq!(
        recovered
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .expect("released segment authority should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::Absent,
        "provider and namespace absence must precede exact hold removal"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "provider projection and persistent namespace must both be absent"
    );
    let persisted = recovered
        .read_manifest(&sandbox_id)
        .expect("converged manifest should inspect")
        .expect("workload manifest remains backend-owned evidence");
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    recovered.write_manifest(&manifest).expect(
        "crash cut should restore the pre-publication Krun manifest after network finality",
    );
    drop(recovered);

    let publication_recovery = KrunSandboxBackend::new(config.clone());
    assert!(
        publication_recovery
            .startup_network_reconciliation_error
            .is_none(),
        "terminal Krun network authority must repair a crash before manifest publication: {:?}",
        publication_recovery.startup_network_reconciliation_error
    );
    assert_eq!(
        publication_recovery
            .read_manifest(&sandbox_id)
            .expect("republished Krun manifest should inspect")
            .expect("republished workload evidence should remain")
            .launch_authority,
        KrunLaunchAuthority::Released
    );
    drop(publication_recovery);

    let replay = KrunSandboxBackend::new(config);
    assert!(
        replay.startup_network_reconciliation_error.is_none(),
        "terminal Krun replay must be admission-safe and effect-free: {:?}",
        replay.startup_network_reconciliation_error
    );
    assert_eq!(
        replay
            .attachment_authority
            .as_ref()
            .expect("replayed portable authority should be open")
            .get(&spec.tenant_id, &network_config.attachment_id)
            .expect("terminal attachment should reinspect")
            .expect("terminal attachment witness should remain")
            .resource()
            .phase(),
        NetworkResourcePhase::Released
    );
}

#[test]
fn nnc8_3_krun_dead_never_effected_launch_resumes_reverse_order_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    let backend = KrunSandboxBackend::new(config.clone());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("krun-nnc8-3-never-effected-orphan");
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("planning should reserve exact no-effect authority")
        .manifest;
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("placed launch should retain exact network config")
        .clone();
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("planned launch should retain its exact claim")
        .clone();
    manifest.shutdown_requested = true;
    manifest.status = SandboxStatus::Stopping;
    manifest.handle.status = SandboxStatus::Stopping;
    backend
        .write_manifest(&manifest)
        .expect("dead-owner cleanup intent should be durable first");
    backend
        .segment_allocator
        .release_reserved_attachment_without_effect(
            &spec.tenant_id,
            &network_config.attachment_id,
            &reservation_claim,
        )
        .expect("crash cut should retain exact reservation cleanup authority");
    assert_eq!(
        backend
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .expect("cleanup-pending reservation should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::ReservationCleanupPending
    );
    drop(backend);

    let recovered = KrunSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_network_reconciliation_error.is_none(),
        "fresh Krun startup should resume the already fenced no-effect saga: {:?}",
        recovered.startup_network_reconciliation_error
    );
    assert_eq!(
        recovered
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .expect("released reservation should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::Absent
    );
    let port_authority =
        nimbus_network::LocalPortLeaseAuthority::open(&recovered.config.network_state_root)
            .expect("portable port authority should reopen");
    for request in manifest.port_leases.iter().chain(
        manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| &assignment.port_lease),
    ) {
        assert_eq!(
            port_authority
                .inspect(request.lease_id())
                .expect("released port lease should inspect")
                .expect("terminal lease evidence should remain")
                .phase(),
            nimbus_network::PortLeasePhase::Released,
            "every never-bound listener must settle before IPAM and segment reuse"
        );
    }
    let persisted = recovered
        .read_manifest(&sandbox_id)
        .expect("converged manifest should inspect")
        .expect("workload evidence should remain durable");
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    drop(recovered);

    let replay = KrunSandboxBackend::new(config);
    assert!(
        replay.startup_network_reconciliation_error.is_none(),
        "Krun no-effect terminal replay must remain idempotent: {:?}",
        replay.startup_network_reconciliation_error
    );
}

#[test]
fn nnc8_3_krun_no_effect_terminal_publication_resumes_before_ipam_retirement() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    let backend = KrunSandboxBackend::new(config.clone());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("krun-nnc8-3-no-effect-publication-cut");
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("planning should reserve exact no-effect authority")
        .manifest;
    manifest.shutdown_requested = true;
    manifest.status = SandboxStatus::Stopping;
    manifest.handle.status = SandboxStatus::Stopping;
    backend
        .write_manifest(&manifest)
        .expect("dead-owner cleanup intent should be durable first");
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("placed launch should retain exact network config")
        .clone();
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("planned launch should retain its exact claim")
        .clone();
    let ports = backend.port_lease_coordinator();
    let cut = release_reserved_network_launch_after_ports_with_terminal_publication(
        ReservedNetworkLaunchAuthority::new(
            backend.segment_allocator.as_ref(),
            &backend.ipam_authority,
            ReservedNetworkLaunchIdentity::new(
                &manifest.network_layout,
                &spec.tenant_id,
                &sandbox_id,
                &network_config.attachment_id,
                &reservation_claim,
            ),
            network_config.provider_kind(),
        ),
        ports.release_never_bound_launch_claim(&reservation_claim),
        || {
            Err(crate::error::SandboxError::OperationFailed {
                message: "injected crash before Krun no-effect manifest publication".to_owned(),
            })
        },
    )
    .expect_err("the crash cut should retain terminal IPAM publication evidence");
    assert!(
        cut.to_string()
            .contains("injected crash before Krun no-effect manifest publication")
    );
    assert_eq!(
        backend
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .expect("terminal segment authority should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::Absent
    );
    drop(backend);

    let recovered = KrunSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_network_reconciliation_error.is_none(),
        "startup should use the retained terminal IPAM witness to publish exact Krun finality: {:?}",
        recovered.startup_network_reconciliation_error
    );
    let republished = recovered
        .read_manifest(&sandbox_id)
        .expect("repaired no-effect Krun manifest should inspect")
        .expect("repaired no-effect Krun manifest should remain");
    assert_eq!(republished.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        terminal_container_ipam_release_is_absent_for_test(
            &recovered.ipam_authority,
            &republished.network_layout,
            republished
                .network_config
                .as_ref()
                .expect("repaired Krun manifest should retain exact network identity"),
            &sandbox_id,
        )
        .expect("terminal Krun IPAM retry evidence should inspect"),
        "the Krun recovery process must retire terminal IPAM retry evidence after publication"
    );
    drop(recovered);

    let replay = KrunSandboxBackend::new(config);
    assert!(
        replay.startup_network_reconciliation_error.is_none(),
        "retired IPAM evidence and terminal Krun publication must replay without a fence: {:?}",
        replay.startup_network_reconciliation_error
    );
}

#[test]
fn startup_reconciliation_failure_fences_direct_initial_launch_before_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-launch-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.78.0.0/24",
        78,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-launch-fence"),
            None,
            None,
        )
        .expect("initial planning should reserve exact launch authority")
        .manifest;
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before = fs::read(&authority_path).expect("reserved authority should be durable");
    let operations_before = recorder.operations();
    let manifest_before =
        serde_json::to_vec(&manifest).expect("unstarted manifest should serialize");
    inject_startup_reconciliation_failure(&mut backend);

    let error = backend
        .launch_manifest(&mut manifest, true)
        .expect_err("retained startup failure must fence direct initial launch");

    assert!(
        error
            .to_string()
            .contains("refuses new network work because startup reconciliation did not complete"),
        "the retained startup diagnostic must remain primary: {error}"
    );
    assert_eq!(
        serde_json::to_vec(&manifest).expect("fenced manifest should serialize"),
        manifest_before,
        "the launch fence must precede in-memory lifecycle mutation"
    );
    assert_eq!(
        fs::read(&authority_path).expect("reserved authority should remain readable"),
        authority_before,
        "the launch fence must not mutate portable network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "the launch fence must precede allocator or provider effects"
    );
    assert!(!manifest.network_layout.netns_path.exists());
    assert!(!manifest.network_layout.status_path.exists());
}

#[test]
fn nnc5_2d_krun_startup_durably_fences_unmatched_no_hold_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    let unmatched = config
        .workload_state_root
        .join("tenants")
        .join("tenant-unmatched-krun")
        .join("networks")
        .join("netns")
        .join("orphan-without-hold");
    fs::create_dir_all(
        unmatched
            .parent()
            .expect("unmatched netns parent should exist"),
    )
    .expect("unmatched netns parent should create");
    fs::write(&unmatched, b"persistent-netns").expect("unmatched durable evidence should write");

    for attempt in 0..2 {
        let backend = KrunSandboxBackend::new(config.clone());
        let error = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new(format!("krun-admission-{attempt}")),
                None,
                None,
            )
            .expect_err("unmatched no-hold evidence must fence every fresh backend");
        let message = error.to_string();
        assert!(
            message.contains("startup reconciliation did not complete")
                && message.contains("unmatched artifact"),
            "the durable admission fence must name the retained unmatched evidence: {message}"
        );
        assert!(
            unmatched.is_file(),
            "startup quarantine must preserve unmatched evidence for later cleanup convergence"
        );
    }
}

#[test]
fn startup_reconciliation_failure_allows_exact_unstarted_launch_compensation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-stop-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.79.0.0/24",
        79,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-stop-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should persist");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        fs::read(&authority_path).expect("launch authority should remain durable");
    inject_startup_reconciliation_failure(&mut backend);

    let error = backend.persist_unstarted_launch_failure(
        &mut manifest,
        crate::error::SandboxError::OperationFailed {
            message: "injected fenced launch cancellation".to_owned(),
        },
    );
    assert!(error.to_string().contains("fenced launch cancellation"));
    assert_ne!(
        fs::read(&authority_path).expect("launch authority should remain readable"),
        authority_before,
        "successful stop must durably release exact launch authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > 1,
        "exact stop must invoke authenticated segment cleanup"
    );
}

#[test]
fn startup_reconciliation_failure_keeps_natural_exit_read_only_until_owned_compensation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-exit-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.80.0.0/24",
        80,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let mut backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-exit-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("initial launch should retain its reservation claim")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model adopted attachment authority");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("natural-exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        fs::read(&authority_path).expect("active authority should remain durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("natural-exit inspection should retain exact authority")
        .expect("exited workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 17 }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(
        fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "natural-exit inspection must not release exact network authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("retained manifest should inspect")
        .expect("retained manifest should remain durable");
    assert_eq!(persisted, manifest);
    assert_eq!(
        recorder.operations(),
        operations_before,
        "inspection must not invoke allocator cleanup"
    );

    let error = backend.persist_provider_launch_failure(
        &mut manifest,
        crate::error::SandboxError::OperationFailed {
            message: "injected provider cleanup after startup fence".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider cleanup after startup fence")
    );
    assert_ne!(
        fs::read(&authority_path).expect("released authority should remain readable"),
        authority_before,
        "explicit stop must durably release exact network authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > operations_before.len(),
        "explicit stop must invoke authenticated segment convergence"
    );
}

#[test]
fn startup_reconciliation_failure_keeps_restart_eligible_inspection_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-restart-fence", "api")
        .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 });
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.81.0.0/24",
        81,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-restart-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("initial launch should retain its reservation claim")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model adopted attachment authority");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("restart-eligible exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let manifest_before = fs::read(&manifest.conmon_layout.manifest_path)
        .expect("canonical manifest bytes should read");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before = fs::read(&authority_path).expect("active authority should be durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("startup-fenced inspection should remain available")
        .expect("restart-eligible workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 17 }
    );
    assert_eq!(
        inspected.restart,
        SandboxRestartAssessment::Candidate {
            exit_code: 17,
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical manifest bytes should remain readable"),
        manifest_before,
        "startup-fenced inspection must not publish a restart decision"
    );
    assert_eq!(
        fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "startup-fenced inspection must not mutate network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "startup-fenced inspection must not invoke provider or segment effects"
    );
}
