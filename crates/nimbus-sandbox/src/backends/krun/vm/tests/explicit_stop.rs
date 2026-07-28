//! Explicit-stop ordering and cleanup-authority proofs.

use super::support::*;

use std::sync::Arc;

use nimbus_network::{LocalPortLeaseAuthority, NetworkSegmentAllocator, PortLeasePhase};

use crate::backends::oci::conmon::OciConmonLayout;
use crate::backends::oci::network::{
    OciNetworkConfig, OciNetworkLayout, OciSegmentAllocator, RecordingSegmentAllocator,
    allocate_container_ips, deallocate_container_ips_after_confirmed_detach,
    default_network_attachment_id, reconcile_terminal_container_ipam_releases,
};
use crate::error::SandboxError;

fn terminal_publication_fixture(
    backend: &KrunSandboxBackend,
    sandbox_id: &SandboxId,
) -> KrunSandboxManifest {
    let spec = sample_spec_for_tenant("krun-terminal-publication", "api");
    let mut manifest = sample_manifest(spec.clone(), KrunStartMode::Execute);
    manifest.handle.id = sandbox_id.clone();
    manifest.network_config = None;
    manifest.port_leases.clear();
    manifest.egress_proxy = None;
    manifest.launch_artifact = None;
    manifest.conmon_layout =
        OciConmonLayout::new_for_tenant(&backend.config.state_root, &spec.tenant_id, sandbox_id);
    manifest.network_layout =
        OciNetworkLayout::new(&backend.config.state_root, &spec.tenant_id, sandbox_id);
    manifest.shutdown_requested = true;
    manifest.status = SandboxStatus::Stopping;
    manifest.handle.status = SandboxStatus::Stopping;
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest
}

#[test]
fn terminal_manifest_write_failure_preserves_replayable_stopping_checkpoint() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let backend = KrunSandboxBackend::new(config.clone());
    let sandbox_id = SandboxId::new("krun-terminal-before-write");
    let stopping = terminal_publication_fixture(&backend, &sandbox_id);
    backend
        .write_manifest(&stopping)
        .expect("stopping checkpoint should persist");

    let failing = KrunSandboxBackend::new(config.clone()).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "explicit krun stop completion",
            KrunEffectBarrierFailureStage::BeforeWrite,
        ),
    );
    let mut terminal = stopping.clone();
    terminal.launch_authority = KrunLaunchAuthority::Released;
    terminal.status = SandboxStatus::Stopped;
    terminal.handle.status = SandboxStatus::Stopped;
    terminal.shutdown_requested = false;
    let false_terminal_error = backend
        .write_manifest(&terminal)
        .expect_err("terminal publication without durable shutdown intent must fail closed");
    assert!(
        false_terminal_error
            .to_string()
            .contains("shutdown_requested=false"),
        "terminal rejection must identify the incomplete cleanup authority: \
         {false_terminal_error}"
    );
    terminal.shutdown_requested = true;
    let error = failing
        .persist_effect_barrier(&terminal, "explicit krun stop completion")
        .expect_err("pre-publication failure must retain the stopping checkpoint");
    assert!(
        error.to_string().contains("was not durably observable"),
        "failure must distinguish an unchanged checkpoint: {error}"
    );
    assert_eq!(
        failing
            .read_manifest(&sandbox_id)
            .expect("checkpoint should inspect")
            .expect("checkpoint should remain durable"),
        stopping
    );

    let reopened = KrunSandboxBackend::new(config);
    reopened
        .persist_effect_barrier(&terminal, "explicit krun stop completion")
        .expect("explicit retry should publish the exact terminal result");
    let once = fs::read(&terminal.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");
    reopened
        .persist_effect_barrier(&terminal, "explicit krun stop completion")
        .expect("terminal publication replay should be idempotent");
    assert_eq!(
        fs::read(&terminal.conmon_layout.manifest_path)
            .expect("replayed terminal manifest bytes should read"),
        once,
        "terminal publication replay must be byte-identical"
    );
}

#[test]
fn terminal_projection_rejects_every_retained_krun_launch_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let sandbox_id = SandboxId::new("krun-terminal-authority-matrix");
    let stopping = terminal_publication_fixture(&backend, &sandbox_id);
    backend
        .write_manifest(&stopping)
        .expect("nonterminal checkpoint should persist");
    let checkpoint = fs::read(&stopping.conmon_layout.manifest_path)
        .expect("nonterminal checkpoint bytes should read");
    let claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("matrix claim should validate");
    let retained = [
        KrunLaunchAuthority::Reserved {
            reservation_claim: claim.clone(),
        },
        KrunLaunchAuthority::Adopting {
            reservation_claim: claim.clone(),
        },
        KrunLaunchAuthority::Adopted {
            reservation_claim: claim,
        },
        KrunLaunchAuthority::ProviderOwned,
    ];

    for launch_authority in retained {
        let mut terminal = stopping.clone();
        terminal.launch_authority = launch_authority.clone();
        terminal.status = SandboxStatus::Stopped;
        terminal.handle.status = SandboxStatus::Stopped;
        let error = backend
            .write_manifest(&terminal)
            .expect_err("retained launch authority must veto terminal projection");
        assert!(
            error
                .to_string()
                .contains(&format!("launch_authority={launch_authority:?}")),
            "diagnostic must name the retained launch authority: {error}"
        );
        assert_eq!(
            fs::read(&terminal.conmon_layout.manifest_path)
                .expect("rejected publication must preserve prior bytes"),
            checkpoint,
            "terminal projection must remain nonterminal for {launch_authority:?}"
        );
    }

    let mut terminal = stopping.clone();
    terminal.launch_authority = KrunLaunchAuthority::Released;
    terminal.provider_failure_cleanup = KrunProviderFailureCleanupState::Requested;
    terminal.status = SandboxStatus::Failed;
    terminal.handle.status = SandboxStatus::Failed;
    let error = backend
        .write_manifest(&terminal)
        .expect_err("active provider-failure cleanup must veto terminal projection");
    assert!(
        error
            .to_string()
            .contains("provider_failure_cleanup=Requested"),
        "diagnostic must name active provider-failure cleanup: {error}"
    );
    assert_eq!(
        fs::read(&terminal.conmon_layout.manifest_path)
            .expect("rejected cleanup publication must preserve prior bytes"),
        checkpoint
    );

    terminal.provider_failure_cleanup = KrunProviderFailureCleanupState::Inactive;
    terminal.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "terminal-authority-matrix",
        ),
    };
    let error = backend
        .write_manifest(&terminal)
        .expect_err("pending creator authority must veto terminal projection");
    assert!(
        error.to_string().contains("creator_handoff=Pending"),
        "diagnostic must name pending creator authority: {error}"
    );
    assert_eq!(
        fs::read(&terminal.conmon_layout.manifest_path)
            .expect("rejected creator publication must preserve prior bytes"),
        checkpoint
    );

    terminal.creator_handoff = KrunCreatorHandoffState::Quiesced {
        proof: crate::backends::conmon::creator::CreatorQuiescenceProof::never_spawned(
            "terminal-authority-matrix",
        ),
    };
    terminal.next_restart_at_millis = Some(1);
    let error = backend
        .write_manifest(&terminal)
        .expect_err("retained restart authority must veto terminal projection");
    assert!(
        error.to_string().contains("next_restart_at_millis=Some(1)"),
        "diagnostic must name retained restart authority: {error}"
    );
    assert_eq!(
        fs::read(&terminal.conmon_layout.manifest_path)
            .expect("rejected restart publication must preserve prior bytes"),
        checkpoint
    );

    terminal.next_restart_at_millis = None;
    backend
        .write_manifest(&terminal)
        .expect("fully released terminal authority should publish");
    assert_eq!(
        backend
            .read_manifest(&sandbox_id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        terminal
    );
}

#[test]
fn terminal_manifest_acknowledgement_loss_is_inspected_and_confirmed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let backend = KrunSandboxBackend::new(config.clone());
    let sandbox_id = SandboxId::new("krun-terminal-ack-loss");
    let mut terminal = terminal_publication_fixture(&backend, &sandbox_id);
    backend
        .write_manifest(&terminal)
        .expect("stopping checkpoint should persist");
    terminal.launch_authority = KrunLaunchAuthority::Released;
    terminal.status = SandboxStatus::Stopped;
    terminal.handle.status = SandboxStatus::Stopped;

    let ambiguous = KrunSandboxBackend::new(config).with_effect_barrier_test_probe(
        KrunEffectBarrierTestProbe::once(
            "explicit krun stop completion",
            KrunEffectBarrierFailureStage::AfterRenameBeforeParentSync,
        ),
    );
    ambiguous
        .persist_effect_barrier(&terminal, "explicit krun stop completion")
        .expect("exact readback should recover acknowledgement loss");
    assert_eq!(
        ambiguous
            .read_manifest(&sandbox_id)
            .expect("terminal result should inspect")
            .expect("terminal result should remain durable"),
        terminal
    );
}

#[test]
fn terminal_ipam_retirement_failure_is_not_manifest_acknowledgement_loss() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let backend = KrunSandboxBackend::new(config.clone());
    let sandbox_id = SandboxId::new("krun-terminal-ipam-retirement-failure");
    let mut terminal = terminal_publication_fixture(&backend, &sandbox_id);
    let network_config = OciNetworkConfig::default();
    terminal
        .network_layout
        .ensure_directories()
        .expect("network layout should exist");
    allocate_container_ips(&terminal.network_layout, &network_config, &sandbox_id)
        .expect("fixture should allocate exact IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &terminal.network_layout,
        &sandbox_id,
        &network_config.reservation_claim,
    )
    .expect("fixture should persist the exact terminal witness");
    terminal.network_config = Some(network_config);
    terminal.launch_authority = KrunLaunchAuthority::Released;
    terminal.status = SandboxStatus::Stopped;
    terminal.handle.status = SandboxStatus::Stopped;

    let failing = KrunSandboxBackend::new(config)
        .with_terminal_ipam_retirement_failure("injected exact retirement failure");
    let result = failing.persist_effect_barrier(&terminal, "explicit krun stop completion");

    let error = result.expect_err(
        "post-publication IPAM retirement failure must not be recovered as manifest ack loss",
    );
    assert!(
        error.to_string().contains("IPAM")
            && error.to_string().contains("retirement")
            && error.to_string().contains("manifest")
            && error.to_string().contains("durable"),
        "diagnostic must distinguish durable manifest publication from pending retirement: {error}"
    );
    assert_eq!(
        failing
            .read_manifest(&sandbox_id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        terminal,
        "terminal desired state should remain durable while witness retirement retries"
    );
    assert_eq!(
        reconcile_terminal_container_ipam_releases(&failing.config.state_root)
            .expect("fresh-process reconciliation should retire the exact witness"),
        1
    );
    assert_eq!(
        reconcile_terminal_container_ipam_releases(&failing.config.state_root)
            .expect("terminal reconciliation replay should be idempotent"),
        0
    );
    KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ))
    .persist_effect_barrier(&terminal, "explicit krun stop completion")
    .expect("terminal publication retry should tolerate an already-retired witness");
}

#[test]
fn pending_creator_fences_provider_and_network_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-pending-creator", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.77.0.0/24",
        77,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(&spec, &SandboxId::new("krun-pending-creator"), None, None)
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("reserved launch should retain its coordinator")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model an adopted attachment");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
    manifest.creator_handoff = KrunCreatorHandoffState::Pending {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "pending-test-attempt",
        ),
    };
    backend
        .write_manifest(&manifest)
        .expect("pending creator crash cut should persist");
    let before = manifest.clone();
    let operations_before = recorder.operations();

    let error = backend.persist_provider_launch_failure(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "forced runtime observation timeout".to_owned(),
        },
    );

    assert!(
        error.to_string().contains("creator handoff")
            && error
                .to_string()
                .contains("may still materialize provider effects"),
        "cleanup refusal must retain the creator ambiguity: {error}"
    );
    assert_eq!(
        manifest, before,
        "cleanup refusal must not mutate the pending authority"
    );
    assert_eq!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("pending manifest should inspect")
            .expect("pending manifest should remain durable"),
        before,
        "durable creator evidence must remain byte-equivalent"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "no segment or provider cleanup may run while a creator is pending"
    );
}

#[test]
fn reserved_stop_releases_only_the_exact_unstarted_launch_batch() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let launch = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-reserved-stop", "api"),
            &SandboxId::new("krun-reserved-stop"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority");
    let manifest = launch.manifest;
    let mut reservations = manifest.port_leases.clone();
    reservations.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute planning should reserve its PEP listener")
            .port_lease
            .clone(),
    );

    backend
        .stop_sync(&manifest.handle.id)
        .expect("reserved launch stop should compensate only its exact claim");

    let stopped = backend
        .read_manifest(&manifest.handle.id)
        .expect("stopped manifest should inspect")
        .expect("stopped manifest should remain durable");
    assert!(stopped.shutdown_requested);
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert_eq!(stopped.handle.status, SandboxStatus::Stopped);
    assert_eq!(stopped.launch_authority, KrunLaunchAuthority::Released);

    let port_authority =
        LocalPortLeaseAuthority::open(&backend.config.state_root).expect("authority should reopen");
    for request in reservations {
        assert_eq!(
            port_authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("released receipt should remain durable")
                .phase(),
            PortLeasePhase::Released,
            "reserved stop must release every exact member of the unstarted launch batch"
        );
    }
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "reserved stop must finalize only the exact unstarted attachment claim"
    );
}

#[test]
fn adopting_stop_releases_exactly_when_allocator_still_proves_reserved() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-adopting-stop", "api"),
            &SandboxId::new("krun-adopting-stop"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("reserved launch should retain its coordinator")
        .clone();
    manifest.launch_authority = KrunLaunchAuthority::Adopting {
        reservation_claim: claim.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("adopting crash-cut fixture should persist");

    backend
        .stop_sync(&manifest.handle.id)
        .expect("exact pre-adoption allocator evidence must authorize compensation");
    let stopped = backend
        .read_manifest(&manifest.handle.id)
        .expect("stopped manifest should inspect")
        .expect("stopped manifest should remain durable");
    assert!(stopped.shutdown_requested);
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert_eq!(stopped.handle.status, SandboxStatus::Stopped);
    assert_eq!(
        stopped.launch_authority,
        KrunLaunchAuthority::Released,
        "the exact reserved outcome must converge to one terminal release"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "pre-adoption compensation must remove only the exact reserved attachment"
    );
    backend
        .stop_sync(&manifest.handle.id)
        .expect("terminal release must replay idempotently");
}

#[test]
fn adopting_stop_promotes_exact_allocator_adoption_before_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let backend = KrunSandboxBackend::new(config.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-adopted-recovery", "api"),
            &SandboxId::new("krun-adopted-recovery"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("reserved launch should retain its coordinator")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should commit allocator adoption before manifest acknowledgement");
    manifest.launch_authority = KrunLaunchAuthority::Adopting {
        reservation_claim: claim.clone(),
    };
    backend
        .write_manifest(&manifest)
        .expect("adopting crash-cut fixture should persist");
    assert_eq!(
        backend
            .segment_allocator
            .inspect_attachment_reservation(
                &manifest.spec.tenant_id,
                &default_network_attachment_id(&manifest.handle.id),
                &claim,
            )
            .expect("exact allocator state should inspect"),
        nimbus_network::NetworkAttachmentReservationState::Adopted
    );

    let recovery = KrunSandboxBackend::new(config);
    recovery
        .stop_sync(&manifest.handle.id)
        .expect("fresh owner should promote exact adoption before cleanup");
    let released = recovery
        .read_manifest(&manifest.handle.id)
        .expect("released manifest should inspect")
        .expect("released manifest should remain durable");
    assert!(released.shutdown_requested);
    assert_eq!(released.status, SandboxStatus::Failed);
    assert_eq!(released.handle.status, SandboxStatus::Failed);
    assert_eq!(released.launch_authority, KrunLaunchAuthority::Released);
    assert_eq!(
        released.provider_failure_cleanup,
        KrunProviderFailureCleanupState::Inactive
    );
    let once =
        fs::read(&released.conmon_layout.manifest_path).expect("released manifest should read");
    recovery
        .stop_sync(&manifest.handle.id)
        .expect("promoted cleanup must replay idempotently");
    assert_eq!(
        fs::read(&released.conmon_layout.manifest_path)
            .expect("replayed released manifest should read"),
        once,
        "terminal replay must not reconstruct never-realized compensation"
    );
}

#[test]
fn explicit_stop_cleanup_failure_remains_stopping_and_cannot_restart() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-explicit-stop-failure", "api")
        .with_restart_policy(SandboxRestartPolicy::Always { max_restarts: 2 });
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "10.79.0.0/24", 79)
            .with_quarantine_failure("forced explicit-stop quarantine failure"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-explicit-stop-failure"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("reserved launch should retain its coordinator")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model an adopted attachment");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim.clone(),
    };
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    backend
        .write_manifest(&manifest)
        .expect("running-shaped fixture should persist");
    fs::write(&manifest.conmon_layout.exit_status_file, b"0\n")
        .expect("provider exit receipt should persist");

    let error = backend
        .stop_sync(&manifest.handle.id)
        .expect_err("failed cleanup must retain nonterminal stop authority");
    assert!(
        error
            .to_string()
            .contains("forced explicit-stop quarantine failure"),
        "the exact cleanup failure must remain observable: {error}"
    );
    let fenced = backend
        .read_manifest(&manifest.handle.id)
        .expect("cleanup checkpoint should inspect")
        .expect("cleanup checkpoint should remain durable");
    assert!(fenced.shutdown_requested);
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert_eq!(fenced.handle.status, SandboxStatus::Stopping);
    assert_eq!(
        fenced.launch_authority,
        KrunLaunchAuthority::Adopted {
            reservation_claim: claim,
        }
    );

    let operations_before_inspect = recorder.operations();
    let observed = backend
        .inspect_sync(&manifest.handle.id)
        .expect("inspection must preserve cleanup-pending evidence")
        .expect("cleanup-pending workload should remain inspectable");
    assert_eq!(
        observed.status,
        SandboxStatus::Stopping,
        "inspection must not publish terminal state or invoke restart while cleanup is fenced"
    );
    assert_eq!(
        recorder.operations(),
        operations_before_inspect,
        "observed projection must not retry or bypass explicit-stop cleanup authority"
    );
}

#[test]
fn stop_effects_are_refused_when_durable_intent_cannot_be_confirmed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-stop-barrier", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.80.0.0/24",
        80,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(&spec, &SandboxId::new("krun-stop-barrier"), None, None)
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("reserved launch should retain its coordinator")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model an adopted attachment");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };

    let blocked_state_dir = temp_dir.path().join("manifest-parent-is-a-file");
    fs::write(&blocked_state_dir, b"not a directory")
        .expect("barrier failure fixture should be a regular file");
    manifest.conmon_layout.container_state_dir = blocked_state_dir.clone();
    manifest.conmon_layout.manifest_path = blocked_state_dir.join("manifest.json");
    manifest.conmon_layout.pidfile = blocked_state_dir.join("missing-pidfile");
    let operations_before_stop = recorder.operations();

    let error = backend
        .execute_stop_for_test(&mut manifest)
        .expect_err("unconfirmed durable intent must reject every later stop effect");
    assert!(
        error.to_string().contains("explicit krun stop intent")
            && error.to_string().contains("refusing subsequent effects"),
        "the barrier failure must be distinguished from a later provider error: {error}"
    );
    assert!(manifest.shutdown_requested);
    assert_eq!(manifest.status, SandboxStatus::Stopping);
    assert_eq!(
        recorder.operations(),
        operations_before_stop,
        "no attachment cleanup may execute before durable stop intent is confirmed"
    );
}
