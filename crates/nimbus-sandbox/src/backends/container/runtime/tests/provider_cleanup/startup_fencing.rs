//! Startup-reconciliation admission fences preserve existing-workload cleanup.

use super::*;
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxRestartAssessment,
    SandboxRestartBlocker,
};
use nimbus_network::{
    NetworkProviderHandle, NetworkResourcePhase, NetworkStateTransition, NetworkTransitionEvidence,
};

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::oci::network::{
    AttachmentBackendKind, OciNetavarkOperation, ReservedNetworkLaunchAuthority,
    ReservedNetworkLaunchIdentity, begin_host_managed_teardown_without_ack_for_test,
    oci_attachment_plan, release_reserved_network_launch_after_ports_with_terminal_publication,
    setup_host_managed_network_for_test, terminal_container_ipam_release_is_absent_for_test,
};

fn inject_startup_reconciliation_failure(backend: &mut ContainerSandboxBackend) {
    backend.startup_reconciliation_error = Some(Arc::<str>::from(
        "injected retained startup reconciliation failure",
    ));
}

#[test]
fn nnc8_3_exact_quarantined_orphan_converges_before_capacity_reuse() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("nnc8-3-quarantined-orphan");
    let manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("planning should persist exact pre-effect authority")
        .manifest;
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("execute plan should contain exact network cleanup context");
    let reservation_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("planned generation should retain its exact launch claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &spec.tenant_id,
            &network_config.attachment_id,
            reservation_claim,
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
            reservation_claim,
        )
        .expect("adopted segment association should inspect")
        .association()
        .expect("adopted segment association should remain present")
        .clone();
    attachments
        .reserve(
            &spec.tenant_id,
            host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container),
            &oci_attachment_plan(
                &spec.tenant_id,
                &sandbox_id,
                AttachmentBackendKind::Container,
            ),
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
    let provider_id =
        host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container);
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

    std::fs::write(&manifest.network_layout.netns_path, b"provider-netns")
        .expect("provider namespace witness should exist before setup acknowledgement");
    let operation = OciNetavarkOperation::new(
        &manifest.network_layout,
        network_config,
        &sandbox_id,
        spec.display_name(),
        "nnc8-3-orphan",
        &spec.port_bindings,
        None,
    );
    setup_host_managed_network_for_test(&backend.ipam_authority, &operation)
        .expect("substituted provider setup should publish exact ready evidence");
    backend
        .write_manifest(&manifest)
        .expect("exact backend cleanup context should be durable");
    let exact_manifest_bytes = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("exact backend cleanup context should read");

    let response_loss =
        begin_host_managed_teardown_without_ack_for_test(&backend.ipam_authority, &operation)
            .expect_err("the crash cut should lose the provider teardown response");
    assert!(
        response_loss
            .to_string()
            .contains("injected lost Netavark teardown response"),
        "the response-loss cut must occur after durable delete intent: {response_loss}"
    );
    std::fs::remove_file(&manifest.network_layout.netns_path)
        .expect("the lost response should leave exact external provider absence");
    drop(backend);

    std::fs::write(&manifest.conmon_layout.manifest_path, b"{corrupt")
        .expect("corrupt-context cut should replace only the workload manifest");
    let corrupt_context = ContainerSandboxBackend::new(config.clone());
    let corrupt_error = corrupt_context
        .startup_reconciliation_error
        .as_ref()
        .expect("corrupt cleanup context must retain the startup fence");
    assert!(
        corrupt_error.contains("failed to parse exact Container orphan-cleanup manifest"),
        "the retained diagnostic must name the untrusted cleanup context: {corrupt_error}"
    );
    assert_eq!(
        corrupt_context
            .attachment_authority
            .as_ref()
            .expect("quarantined portable authority should remain open")
            .get(&spec.tenant_id, &network_config.attachment_id)
            .expect("quarantined attachment should inspect")
            .expect("quarantined attachment should remain durable")
            .resource()
            .phase(),
        NetworkResourcePhase::CleanupPending,
        "corrupt context must not prevent the exact durable quarantine"
    );
    assert_eq!(
        corrupt_context
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                reservation_claim,
            )
            .expect("quarantined segment authority should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::ProviderCleanupPending
    );
    drop(corrupt_context);

    std::fs::write(&manifest.conmon_layout.manifest_path, &exact_manifest_bytes)
        .expect("exact cleanup context should be restorable after the retained fence");
    std::fs::write(&manifest.conmon_layout.pidfile, b"424242\n")
        .expect("runtime-ownership cut should retain one exact receipt");
    let runtime_owned = ContainerSandboxBackend::new(config.clone());
    let runtime_error = runtime_owned
        .startup_reconciliation_error
        .as_ref()
        .expect("runtime ownership must retain the startup fence");
    assert!(
        runtime_error.contains("runtime pidfile"),
        "the retained diagnostic must name the live-owner witness: {runtime_error}"
    );
    assert_eq!(
        runtime_owned
            .segment_allocator
            .inspect_attachment_reservation(
                &spec.tenant_id,
                &network_config.attachment_id,
                reservation_claim,
            )
            .expect("runtime-fenced segment authority should inspect")
            .state(),
        nimbus_network::NetworkAttachmentReservationState::ProviderCleanupPending,
        "live runtime evidence must not release reusable authority"
    );
    drop(runtime_owned);
    std::fs::remove_file(&manifest.conmon_layout.pidfile)
        .expect("runtime-absence evidence should replace the retained receipt");

    let recovered = ContainerSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_reconciliation_error.is_none(),
        "a fresh owner should inspect the exact quarantined generation, settle provider and \
         namespace absence, and release its retained authority: {:?}",
        recovered.startup_reconciliation_error
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
                reservation_claim,
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
    assert!(
        persisted.network_cleanup_complete && persisted.launch_reservation_claim.is_none(),
        "backend manifest should record terminal network context without becoming authority"
    );
    recovered
        .write_existing_workload_manifest(&manifest)
        .expect("crash cut should restore the pre-publication manifest after network finality");
    drop(recovered);

    let publication_recovery = ContainerSandboxBackend::new(config.clone());
    assert!(
        publication_recovery.startup_reconciliation_error.is_none(),
        "terminal network authority must repair a crash before manifest publication: {:?}",
        publication_recovery.startup_reconciliation_error
    );
    let republished = publication_recovery
        .read_manifest(&sandbox_id)
        .expect("republished manifest should inspect")
        .expect("republished workload evidence should remain");
    assert!(republished.network_cleanup_complete);
    assert!(republished.launch_reservation_claim.is_none());
    drop(publication_recovery);

    let replay = ContainerSandboxBackend::new(config);
    assert!(
        replay.startup_reconciliation_error.is_none(),
        "terminal replay must be admission-safe and effect-free: {:?}",
        replay.startup_reconciliation_error
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
fn nnc8_3_dead_never_effected_launch_resumes_reverse_order_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("nnc8-3-never-effected-orphan");
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
        .launch_reservation_claim
        .as_ref()
        .expect("planned launch should retain its exact claim")
        .clone();
    manifest.shutdown_requested = true;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
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

    let recovered = ContainerSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_reconciliation_error.is_none(),
        "fresh startup should resume the already fenced no-effect saga: {:?}",
        recovered.startup_reconciliation_error
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
    assert!(
        persisted.network_cleanup_complete && persisted.launch_reservation_claim.is_none(),
        "manifest should publish terminal network cleanup after authority release"
    );
    drop(recovered);

    let replay = ContainerSandboxBackend::new(config);
    assert!(
        replay.startup_reconciliation_error.is_none(),
        "no-effect terminal replay must remain idempotent: {:?}",
        replay.startup_reconciliation_error
    );
}

#[test]
fn nnc8_3_no_effect_terminal_publication_resumes_before_ipam_retirement() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("nnc8-3-no-effect-publication-cut");
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("planning should reserve exact no-effect authority")
        .manifest;
    manifest.shutdown_requested = true;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
    backend
        .write_manifest(&manifest)
        .expect("dead-owner cleanup intent should be durable first");
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("placed launch should retain exact network config")
        .clone();
    let reservation_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("planned launch should retain its exact claim")
        .clone();
    let ports = backend
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("exact port coordinator should open");
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
                message: "injected crash before no-effect manifest publication".to_owned(),
            })
        },
    )
    .expect_err("the crash cut should retain terminal IPAM publication evidence");
    assert!(
        cut.to_string()
            .contains("injected crash before no-effect manifest publication")
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

    let recovered = ContainerSandboxBackend::new(config.clone());
    assert!(
        recovered.startup_reconciliation_error.is_none(),
        "startup should use the retained terminal IPAM witness to publish exact manifest finality: {:?}",
        recovered.startup_reconciliation_error
    );
    let republished = recovered
        .read_manifest(&sandbox_id)
        .expect("repaired no-effect manifest should inspect")
        .expect("repaired no-effect manifest should remain");
    assert!(republished.network_cleanup_complete);
    assert!(republished.launch_reservation_claim.is_none());
    assert!(
        terminal_container_ipam_release_is_absent_for_test(
            &recovered.ipam_authority,
            &republished.network_layout,
            republished
                .network_config
                .as_ref()
                .expect("repaired manifest should retain exact network identity"),
            &sandbox_id,
        )
        .expect("terminal IPAM retry evidence should inspect"),
        "the recovery process must retire terminal IPAM retry evidence after publication"
    );
    drop(recovered);

    let replay = ContainerSandboxBackend::new(config);
    assert!(
        replay.startup_reconciliation_error.is_none(),
        "retired IPAM evidence and terminal publication must replay without a fence: {:?}",
        replay.startup_reconciliation_error
    );
}

#[test]
fn startup_reconciliation_failure_fences_direct_initial_launch_before_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec();
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        73,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(&spec, &SandboxId::new("launch-startup-fence"), None, None)
        .expect("initial planning should reserve exact launch authority")
        .manifest;
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("reserved authority should be durable");
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
            .contains("refuses new durable work because startup reconciliation did not complete"),
        "the retained startup diagnostic must remain primary: {error}"
    );
    assert_eq!(
        serde_json::to_vec(&manifest).expect("fenced manifest should serialize"),
        manifest_before,
        "the launch fence must precede in-memory lifecycle mutation"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("reserved authority should remain readable"),
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
fn startup_reconciliation_failure_allows_exact_plan_only_status_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut backend = sample_plan_only_backend(temp_dir.path());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("plan-only-startup-fence");
    let artifact_path = crate::artifact_paths::rootfs_root(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    )
    .join(sandbox_id.as_str());
    let rootfs_path = artifact_path.join("rootfs");
    std::fs::create_dir_all(&rootfs_path).expect("rootfs fixture should create");
    let sentinel = rootfs_path.join("sentinel");
    std::fs::write(&sentinel, b"owned-rootfs").expect("rootfs sentinel should persist");
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &sandbox_id,
            None,
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("plan-only manifest should lower")
        .manifest;
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should persist");
    inject_startup_reconciliation_failure(&mut backend);

    let stopped = backend
        .update_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Stopped)
        .expect("an existing workload must retain exact cleanup authority")
        .expect("the stopped workload should remain inspectable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(
        !sentinel.exists(),
        "exact status cleanup should remove its owned launch artifact"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert!(
        persisted.has_terminal_network_finality(),
        "cleanup must durably publish exact terminal network finality"
    );
    assert!(persisted.launch_artifact.is_none());
}

#[test]
fn startup_reconciliation_failure_keeps_natural_exit_inspection_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec();
    let id = SandboxId::new("natural-exit-startup-fence");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        74,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder;
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let mut backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("execute manifest should reserve exact authority")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    let launch_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("execute launch should retain exact reservation authority")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture provider must adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own its exact listener");
    manifest.launch_reservation_claim = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("natural-exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let readiness_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("baseline readiness should inspect")
        .expect("baseline PEP should remain registered");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("active authority should be durable");
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&id)
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
        std::fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "natural-exit inspection must not release exact network authority"
    );
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("post-inspection readiness should inspect")
            .is_some(),
        "natural-exit inspection must not remove the exact PEP registration"
    );
    assert!(
        readiness_before.is_ready(),
        "the fixture must begin with a live PEP before cleanup"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("retained manifest should inspect")
        .expect("retained manifest should remain durable");
    assert_eq!(persisted, manifest);
}

#[test]
fn startup_reconciliation_failure_keeps_restart_eligible_inspection_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec()
        .with_restart_policy(crate::spec::SandboxRestartPolicy::OnFailure { max_restarts: 1 });
    let id = SandboxId::new("restart-startup-fence");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        75,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let mut backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("execute manifest should reserve exact authority")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    let launch_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("execute launch should retain exact reservation authority")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture provider must adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own its exact listener");
    manifest.launch_reservation_claim = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("restart-eligible exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("canonical manifest bytes should read");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("active authority should be durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&id)
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
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical manifest bytes should remain readable"),
        manifest_before,
        "startup-fenced inspection must not publish a restart decision"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "startup-fenced inspection must not mutate network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "startup-fenced inspection must not invoke provider or segment effects"
    );
}
