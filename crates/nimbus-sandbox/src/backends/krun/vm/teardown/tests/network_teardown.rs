//! Real Krun host-managed attachment teardown proofs.

use std::net::TcpListener;
use std::sync::mpsc;

use nimbus_network::{
    NetworkAttachmentReservationState, NetworkCapabilitySourceDigest, NetworkResourcePhase,
    NetworkSegmentReleaseOutcome, PortLeasePhase,
};

use super::*;
use crate::backends::KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY;
use crate::backends::oci::network::{
    AttachmentAttachAuthority, FixedOciEgressPinProvider, HostManagedAttachmentDetachPhase,
    HostManagedAttachmentReleasePhase,
};
use crate::provider_command::{
    ProviderCommandLockTestProbe, with_provider_command_lock_test_probe,
};
use crate::{
    ProviderCommandClaim, SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownObservation, SandboxNetworkTeardownOperation, SandboxPortBinding,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkContenderRole {
    Execute,
    Adopt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptedAdoptionAllocatorCut {
    Reserved,
    Adopted,
    ReservationCleanupPending,
    Absent,
}

#[path = "network_teardown/fresh_process.rs"]
mod fresh_process;

struct NetworkTeardownFixture {
    root: tempfile::TempDir,
    config: KrunSandboxBackendConfig,
    backend: KrunSandboxBackend,
    runtime: Arc<ScriptedRuntime>,
    id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
}

impl NetworkTeardownFixture {
    fn attached(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let published_reservation =
            TcpListener::bind("127.0.0.1:0").expect("published-port probe should bind");
        let published_port = published_reservation
            .local_addr()
            .expect("published-port probe should report its address")
            .port();
        let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP-port probe should bind");
        let pep_port = pep_reservation
            .local_addr()
            .expect("PEP-port probe should report its address")
            .port();
        let mut config = KrunSandboxBackendConfig::under_root(root.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.netavark_path = PathBuf::from("/usr/bin/true");
        let base = KrunSandboxBackend::new(config.clone())
            .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
        let runtime = Arc::new(ScriptedRuntime::live(base.clone(), 100));
        let backend = base.with_teardown_runtime_provider(runtime.clone());
        let id = SandboxId::new(format!("krun-network-teardown-{label}"));
        let tenant_id = TenantId::new(format!("krun-network-teardown-{label}"))
            .expect("fixture tenant should validate");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::service(format!("workload-{label}")),
            SandboxBackendKind::Krun,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
            SandboxProcessSpec::new(["/usr/bin/service"]),
        )
        .with_port_binding(SandboxPortBinding::tcp("api", published_port, 8_080));
        let execution_attempt_id = SandboxExecutionAttemptId::new(format!("wea-network-{label}"))
            .expect("fixture execution attempt should validate");
        let plan = crate::provision::test_support::sandbox_provision_network_plan_fixture(
            &spec, &id, label,
        );
        drop(published_reservation);
        drop(pep_reservation);

        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan)
            .expect("network teardown fixture should reserve its exact plan");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("network teardown fixture should prepare its workload");

        let mut manifest = backend
            .read_manifest(&id)
            .expect("prepared manifest should read")
            .expect("prepared manifest should exist");
        let reservation_claim = manifest
            .require_reserved_claim()
            .expect("prepared manifest should retain its reservation claim")
            .clone();
        backend
            .mark_attachment_adopting(&mut manifest)
            .expect("fixture should enter attachment adoption intent");
        backend
            .persist_effect_barrier(&manifest, "test Krun network adoption intent")
            .expect("attachment adoption intent should persist");
        let network_config = manifest
            .require_network_config()
            .expect("prepared manifest should retain network config")
            .clone();
        backend
            .segment_allocator
            .adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &network_config.attachment_id,
                &reservation_claim,
            )
            .expect("fixture should adopt its exact segment association");
        manifest
            .mark_adopted()
            .expect("fixture should retain adopted launch authority");
        backend
            .persist_effect_barrier(&manifest, "test Krun network adoption result")
            .expect("adopted attachment authority should persist");
        {
            let ports = backend.port_lease_coordinator();
            let hostname = super::super::super::start::hostname_for(&manifest.spec);
            backend
                .non_routable_attachment_adapter(&manifest, &network_config, &hostname)
                .attach_with_test_host(
                    &backend.attachment_lifecycle(&ports),
                    AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
                    |_| {
                        backend.egress_pin_provider.apply(
                            &manifest.network_layout,
                            manifest
                                .egress_proxy
                                .as_ref()
                                .expect("planned PEP assignment should persist"),
                        )
                    },
                )
                .expect("fixture should realize the exact private attachment");
        }
        backend
            .start_planned_provision_pep(&manifest, &reservation_claim)
            .expect("fixture should start its compiler-planned PEP");

        let mut attached = backend
            .read_manifest(&id)
            .expect("attached manifest should read")
            .expect("attached manifest should exist");
        settle_separate_publication_without_effect(&backend, &attached, &reservation_claim);
        attached.launch_authority = KrunLaunchAuthority::ProviderOwned;
        attached.creator_handoff = KrunCreatorHandoffState::RuntimeObserved {
            receipt: CreatorAttemptReceipt::for_test(format!("creator-network-{label}")),
        };
        attached.status = SandboxStatus::Ready;
        attached.handle.status = SandboxStatus::Ready;
        backend
            .write_manifest(&attached)
            .expect("running attachment fixture should persist");
        assert!(attached.network_layout.netns_path.is_file());
        assert!(attached.network_layout.status_path.is_file());
        assert!(!attached.port_leases.is_empty());
        assert!(attached.egress_proxy.is_some());

        Self {
            root,
            config,
            backend,
            runtime,
            id,
            execution_attempt_id,
        }
    }

    fn interrupted_adoption(label: &str, cut: InterruptedAdoptionAllocatorCut) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let published_reservation =
            TcpListener::bind("127.0.0.1:0").expect("published-port probe should bind");
        let published_port = published_reservation
            .local_addr()
            .expect("published-port probe should report its address")
            .port();
        let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP-port probe should bind");
        let pep_port = pep_reservation
            .local_addr()
            .expect("PEP-port probe should report its address")
            .port();
        let mut config = KrunSandboxBackendConfig::under_root(root.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.netavark_path = PathBuf::from("/usr/bin/true");
        let base = KrunSandboxBackend::new(config.clone())
            .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
        let runtime = Arc::new(ScriptedRuntime::live(base.clone(), 100));
        let backend = base.with_teardown_runtime_provider(runtime.clone());
        let id = SandboxId::new(format!("krun-network-adopting-{label}"));
        let tenant_id = TenantId::new(format!("krun-network-adopting-{label}"))
            .expect("fixture tenant should validate");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::service(format!("workload-{label}")),
            SandboxBackendKind::Krun,
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
            SandboxProcessSpec::new(["/usr/bin/service"]),
        )
        .with_port_binding(SandboxPortBinding::tcp("api", published_port, 8_080));
        let execution_attempt_id = SandboxExecutionAttemptId::new(format!("wea-adopting-{label}"))
            .expect("fixture execution attempt should validate");
        let plan = crate::provision::test_support::sandbox_provision_network_plan_fixture(
            &spec, &id, label,
        );
        drop(published_reservation);
        drop(pep_reservation);

        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan)
            .expect("adopting fixture should reserve its exact plan");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("adopting fixture should prepare its workload");
        let mut manifest = backend
            .read_manifest(&id)
            .expect("adopting manifest should read")
            .expect("adopting manifest should exist");
        let reservation_claim = backend
            .mark_attachment_adopting(&mut manifest)
            .expect("fixture should enter attachment adoption intent");
        backend
            .persist_effect_barrier(&manifest, "test interrupted Krun adoption intent")
            .expect("interrupted adoption intent should persist");
        let network_config = manifest
            .require_network_config()
            .expect("adopting fixture should retain network config");
        match cut {
            InterruptedAdoptionAllocatorCut::Reserved => {
                settle_separate_publication_without_effect(&backend, &manifest, &reservation_claim);
            }
            InterruptedAdoptionAllocatorCut::Adopted => {
                backend
                    .segment_allocator
                    .adopt_reserved_attachment(
                        &manifest.spec.tenant_id,
                        &network_config.attachment_id,
                        &reservation_claim,
                    )
                    .expect("fixture allocator adoption should persist");
                settle_separate_publication_without_effect(&backend, &manifest, &reservation_claim);
            }
            InterruptedAdoptionAllocatorCut::ReservationCleanupPending => {
                backend
                    .port_lease_coordinator()
                    .release_never_bound_launch_claim(&reservation_claim)
                    .expect("fixture port cleanup should precede segment cleanup intent");
                assert!(matches!(
                    backend
                        .segment_allocator
                        .release_reserved_attachment_without_effect(
                            &manifest.spec.tenant_id,
                            &network_config.attachment_id,
                            &reservation_claim,
                        )
                        .expect("fixture reserved cleanup intent should persist"),
                    NetworkSegmentReleaseOutcome::AttachmentCleanupPending
                ));
            }
            InterruptedAdoptionAllocatorCut::Absent => {
                backend
                    .release_reserved_launch(&manifest)
                    .expect("fixture reserved cleanup should reach terminal absence");
            }
        }

        Self {
            root,
            config,
            backend,
            runtime,
            id,
            execution_attempt_id,
        }
    }

    fn execution_command(
        &self,
        operation: SandboxExecutionTeardownOperation,
        attempt: &str,
        epoch: u64,
    ) -> SandboxExecutionTeardownCommand {
        let manifest = self.manifest();
        let plan = manifest
            .provision_network_plan
            .as_ref()
            .expect("fixture has an exact provision plan");
        let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: "authority-krun-network-teardown".to_owned(),
            effect_subject: format!("{{\"sandbox\":\"{}\"}}", self.id),
            source_attempt_id: None,
            attempt_id: attempt.to_owned(),
            dispatch_epoch: epoch,
            workload_generation: plan.generation().as_u64(),
            restart_ordinal: 0,
            desired_digest: "1".repeat(64),
            source_digest: "2".repeat(64),
            network_plan_digest: plan.network_plan().digest().to_string(),
            provider_target_digest: "3".repeat(64),
            operation: operation.provider_operation(),
        })
        .expect("fixture execution claim should validate");
        SandboxExecutionTeardownCommand::new(
            manifest.spec.tenant_id,
            self.id.clone(),
            self.execution_attempt_id.clone(),
            "nimbus-sandbox.krun-execution",
            operation,
            claim,
        )
        .expect("fixture execution command should validate")
    }

    fn stop_execution(&self, attempt: &str) -> SandboxExecutionTeardownCommand {
        let drain = self.execution_command(SandboxExecutionTeardownOperation::Drain, attempt, 1);
        let drain_observation = self.backend.execute_execution_teardown(&drain);
        assert!(
            matches!(
                drain_observation,
                SandboxExecutionTeardownObservation::Succeeded { .. }
            ),
            "{drain_observation:?}"
        );
        self.runtime.set_terminal(true);
        let stop = self.execution_command(SandboxExecutionTeardownOperation::Stop, attempt, 1);
        let stop_observation = self.backend.execute_execution_teardown(&stop);
        assert!(
            matches!(
                stop_observation,
                SandboxExecutionTeardownObservation::Succeeded { .. }
            ),
            "{stop_observation:?}"
        );
        assert!(matches!(
            self.manifest().execution_teardown.stop(),
            KrunStopProgress::ExecutionStopped { fence, .. }
                if fence == stop.provider_claim()
        ));
        let mut stopped = self.manifest();
        match stopped.creator_handoff.clone() {
            KrunCreatorHandoffState::RuntimeObserved { receipt } => {
                stopped.creator_handoff = KrunCreatorHandoffState::Quiesced {
                    proof: CreatorQuiescenceProof::dead_contained(receipt),
                };
            }
            KrunCreatorHandoffState::NotSpawned => {}
            state => panic!(
                "network fixture must have a terminal creator handoff after stop, got {state:?}"
            ),
        }
        stopped.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
        stopped.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
                stopped.handle.id
            ),
        ]);
        self.backend
            .write_manifest(&stopped)
            .expect("stopped network fixture should persist exact provider absence");
        stop
    }

    fn network_command(
        &self,
        stop: &SandboxExecutionTeardownCommand,
        operation: SandboxNetworkTeardownOperation,
        epoch: u64,
    ) -> SandboxNetworkTeardownCommand {
        let manifest = self.manifest();
        let plan = manifest
            .provision_network_plan
            .as_ref()
            .expect("attached fixture has a compiled network plan");
        let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
            tenant_id: manifest.spec.tenant_id,
            sandbox_id: self.id.clone(),
            execution_attempt_id: self.execution_attempt_id.clone(),
            attachment_id: plan.attachment_id().clone(),
            network_plan: plan.network_plan().clone(),
            provider_registration_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
            provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
        })
        .expect("network identity should validate");
        let stop_claim = stop.provider_claim();
        let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
            authority_id: stop_claim.authority_id().to_owned(),
            effect_subject: identity.provider_effect_subject(),
            source_attempt_id: stop_claim.source_attempt_id().map(str::to_owned),
            attempt_id: stop_claim.attempt_id().to_owned(),
            dispatch_epoch: epoch,
            workload_generation: stop_claim.workload_generation(),
            restart_ordinal: stop_claim.restart_ordinal(),
            desired_digest: stop_claim.desired_digest().to_owned(),
            source_digest: stop_claim.source_digest().to_owned(),
            network_plan_digest: stop_claim.network_plan_digest().to_owned(),
            provider_target_digest: identity.provider_target_digest(),
            operation: operation.provider_operation(),
        })
        .expect("network provider claim should validate");
        SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
            identity,
            operation,
            provider_claim: claim,
        })
        .expect("network command should validate")
    }

    fn manifest(&self) -> KrunSandboxManifest {
        self.backend
            .read_manifest(&self.id)
            .expect("fixture manifest should read")
            .expect("fixture manifest should exist")
    }

    fn retained_authority(&self) -> Vec<u8> {
        let manifest = self.manifest();
        serde_json::to_vec(&(
            &manifest.provision_network_plan,
            &manifest.network_config,
            &manifest.port_leases,
            &manifest.egress_proxy,
            &manifest.network_layout,
            &manifest.launch_authority,
        ))
        .expect("retained network authority should encode")
    }
}

fn settle_separate_publication_without_effect(
    backend: &KrunSandboxBackend,
    manifest: &KrunSandboxManifest,
    reservation_claim: &nimbus_network::NetworkReservationClaim,
) {
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("fixture should retain its exact provision plan");
    let complete_plan = manifest
        .egress_proxy
        .as_ref()
        .map(|assignment| assignment.compiled_plan_members(plan))
        .unwrap_or_else(|| plan.port_leases());
    backend
        .port_lease_coordinator()
        .release_never_bound_plan_members(&complete_plan, &manifest.port_leases, reservation_claim)
        .expect("separate publication owner should publish terminal no-effect authority");
}

fn retain_stale_runtime_artifacts(manifest: &KrunSandboxManifest) -> [PathBuf; 3] {
    let paths = [
        manifest.conmon_layout.pidfile.clone(),
        manifest.conmon_layout.conmon_pidfile.clone(),
        manifest.conmon_layout.exit_status_file.clone(),
    ];
    for path in &paths {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("runtime artifact parent should create");
        }
    }
    fs::write(&paths[0], format!("{}\n", i32::MAX)).expect("stale runtime pidfile should persist");
    fs::write(&paths[1], format!("{}\n", i32::MAX)).expect("dead conmon receipt should persist");
    fs::write(&paths[2], b"0\n").expect("exit-status receipt should persist");
    paths
}

#[test]
fn interrupted_adopting_attachment_converges_through_exact_teardown() {
    for (label, cut) in [
        ("reserved", InterruptedAdoptionAllocatorCut::Reserved),
        ("adopted", InterruptedAdoptionAllocatorCut::Adopted),
        (
            "reservation-cleanup-pending",
            InterruptedAdoptionAllocatorCut::ReservationCleanupPending,
        ),
        ("absent", InterruptedAdoptionAllocatorCut::Absent),
    ] {
        let fixture = NetworkTeardownFixture::interrupted_adoption(label, cut);
        let stop = fixture.stop_execution(label);
        let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);
        let detached = execute_network(&fixture.backend, &detach);
        assert_eq!(
            detached.kind(),
            ProviderCommandObservationKind::Succeeded,
            "allocator cut {cut:?}; observation={detached:?}",
        );
        let release = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Release, 1);
        assert_eq!(
            execute_network(&fixture.backend, &release).kind(),
            ProviderCommandObservationKind::Succeeded
        );
        let terminal = fixture.manifest();
        assert_eq!(terminal.launch_authority, KrunLaunchAuthority::Released);
        assert_eq!(
            terminal.network_teardown.release_phase(),
            HostManagedAttachmentReleasePhase::Released
        );
        let config = terminal
            .network_config
            .as_ref()
            .expect("terminal manifest retains identity evidence");
        assert_eq!(
            fixture
                .backend
                .segment_allocator
                .inspect_attachment_reservation(
                    &terminal.spec.tenant_id,
                    &config.attachment_id,
                    &config.reservation_claim,
                )
                .expect("terminal allocator state should inspect")
                .state(),
            NetworkAttachmentReservationState::Absent
        );
    }
}

fn execute_network(
    backend: &KrunSandboxBackend,
    command: &SandboxNetworkTeardownCommand,
) -> crate::ProviderCommandObservation {
    let journal = backend
        .attempt_idempotency_journal()
        .expect("network journal should open");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("network command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh network command must receive effect authority")
        }
    };
    backend
        .execute_network_teardown_with_claim(command, execution)
        .expect("network command should publish its result")
}

fn contend_network(
    fixture: &NetworkTeardownFixture,
    command: &SandboxNetworkTeardownCommand,
) -> Vec<(NetworkContenderRole, ProviderCommandObservationKind)> {
    let backend = Arc::new(fixture.backend.clone());
    let journal = Arc::new(
        fixture
            .backend
            .attempt_idempotency_journal()
            .expect("one network journal should open"),
    );
    let command = Arc::new(command.clone());
    let start = Arc::new(Barrier::new(3));
    let claims_complete = Arc::new(Barrier::new(2));
    let outcomes = std::thread::scope(|scope| {
        let mut contenders = Vec::new();
        for _ in 0..2 {
            let backend = Arc::clone(&backend);
            let journal = Arc::clone(&journal);
            let command = Arc::clone(&command);
            let start = Arc::clone(&start);
            let claims_complete = Arc::clone(&claims_complete);
            contenders.push(scope.spawn(move || {
                start.wait();
                let decision = journal.claim_dispatch_epoch(command.provider_claim());
                claims_complete.wait();
                let decision = decision.expect("network contender should reach the one journal");
                match decision {
                    ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
                        let observation = backend
                            .execute_network_teardown_with_claim(&command, execution)
                            .expect("winning network contender should publish");
                        (NetworkContenderRole::Execute, observation.kind())
                    }
                    ProviderCommandClaimDecision::AdoptExactAttempt(observation) => {
                        (NetworkContenderRole::Adopt, observation.kind())
                    }
                }
            }));
        }
        start.wait();
        contenders
            .into_iter()
            .map(|contender| contender.join().expect("network contender should join"))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|(role, _)| *role == NetworkContenderRole::Execute)
            .count(),
        1,
        "one exact network contender must own provider execution: {outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|(role, _)| *role == NetworkContenderRole::Adopt)
            .count(),
        1,
        "the other exact network contender must adopt durable authority: {outcomes:?}"
    );
    assert_eq!(
        journal
            .adopt_exact_attempt(command.provider_claim())
            .expect("terminal network authority should read")
            .expect("terminal network authority should exist")
            .kind(),
        ProviderCommandObservationKind::Succeeded
    );
    outcomes
}

fn runtime_authority(manifest: &KrunSandboxManifest) -> Vec<u8> {
    serde_json::to_vec(&(
        &manifest.execution_attempt_id,
        &manifest.launch_artifact,
        &manifest.creator_handoff,
        &manifest.conmon_launch,
        manifest.last_exit_code,
        manifest.shutdown_requested,
        &manifest.execution_teardown,
        &manifest.status,
        &manifest.handle.status,
    ))
    .expect("runtime authority should encode")
}

#[test]
fn krun_network_detach_requires_exact_execution_stop_without_runtime_effects() {
    let fixture = NetworkTeardownFixture::attached("requires-stop");
    let stop = fixture.execution_command(SandboxExecutionTeardownOperation::Stop, "order", 1);
    let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);
    let authority_before = fixture.retained_authority();
    let terminal_checks_before = fixture.runtime.terminal_checks.load(Ordering::Acquire);
    let signals_before = fixture.runtime.signals();

    let rejected = execute_network(&fixture.backend, &detach);

    assert_eq!(
        rejected.kind(),
        ProviderCommandObservationKind::DefiniteFailure
    );
    assert_eq!(
        rejected.failure_code(),
        Some("sandbox_teardown_order_invalid")
    );
    assert_eq!(fixture.retained_authority(), authority_before);
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks_before
    );
    assert_eq!(fixture.runtime.signals(), signals_before);
    let manifest = fixture.manifest();
    assert!(matches!(
        manifest.network_teardown.detach_phase(),
        HostManagedAttachmentDetachPhase::NotStarted
    ));
    assert!(manifest.network_layout.netns_path.is_file());
    assert!(manifest.network_layout.status_path.is_file());
}

#[test]
fn krun_network_teardown_detaches_retained_then_releases_in_order() {
    let fixture = NetworkTeardownFixture::attached("detach-release");
    let stop = fixture.stop_execution("network");
    let before = fixture.manifest();
    let retained_authority = fixture.retained_authority();
    let terminal_checks = fixture.runtime.terminal_checks.load(Ordering::Acquire);
    let signals = fixture.runtime.signals();
    let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);

    let detached = execute_network(&fixture.backend, &detach);

    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "provider={detached:?}; state={:?}",
        fixture.manifest().network_teardown,
    );
    let retained = fixture.manifest();
    assert_eq!(
        retained.network_teardown.detach_phase(),
        HostManagedAttachmentDetachPhase::Detached
    );
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(!retained.network_layout.netns_path.exists());
    assert!(!retained.network_layout.status_path.exists());
    assert_eq!(fixture.retained_authority(), retained_authority);
    assert_eq!(retained.network_config, before.network_config);
    assert_eq!(retained.port_leases, before.port_leases);
    assert_eq!(retained.egress_proxy, before.egress_proxy);

    let ports = fixture.backend.port_lease_coordinator();
    let listener_records = ports
        .port_lease_records_snapshot(&retained.port_leases, "terminal separate-owner listeners")
        .expect("separately owned listener evidence should inspect");
    assert!(
        listener_records.iter().all(|record| {
            record.phase() == PortLeasePhase::Released
                && record.binding().is_none()
                && record.active_lifetime().is_none()
        }),
        "attachment detach must consume, not retain or rewrite, terminal separate-owner publication"
    );
    let pep = retained
        .egress_proxy
        .as_ref()
        .expect("detached manifest retains its PEP assignment");
    let pep_record = ports
        .port_lease_records_snapshot(std::slice::from_ref(&pep.port_lease), "retained Krun PEP")
        .expect("retained PEP authority should inspect")
        .pop()
        .expect("one retained PEP record should exist");
    assert_eq!(pep_record.phase(), PortLeasePhase::Reserved);
    assert!(pep_record.binding().is_none());
    assert!(pep_record.active_lifetime().is_none());
    assert!(pep_record.confirmed_stopped_binding().is_some());

    let network_config = retained
        .network_config
        .as_ref()
        .expect("detached manifest retains network config");
    let attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(&retained.spec.tenant_id, &network_config.attachment_id)
        .expect("portable attachment should inspect")
        .expect("portable attachment should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Deleting
    );
    let segment = fixture
        .backend
        .segment_allocator
        .inspect_attachment_reservation(
            &retained.spec.tenant_id,
            &network_config.attachment_id,
            &network_config.reservation_claim,
        )
        .expect("quarantined segment should inspect");
    assert_eq!(
        segment.state(),
        NetworkAttachmentReservationState::ProviderCleanupPending
    );
    assert!(matches!(
        fixture
            .backend
            .attempt_idempotency_journal()
            .expect("network journal should reopen")
            .claim_dispatch_epoch(detach.provider_claim())
            .expect("exact detach should replay"),
        ProviderCommandClaimDecision::AdoptExactAttempt(observation)
            if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks
    );
    assert_eq!(fixture.runtime.signals(), signals);

    let runtime_artifacts = retain_stale_runtime_artifacts(&retained);
    let release = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Release, 1);
    let released = execute_network(&fixture.backend, &release);

    assert_eq!(released.kind(), ProviderCommandObservationKind::Succeeded);
    let terminal = fixture.manifest();
    assert_eq!(
        terminal.network_teardown.release_phase(),
        HostManagedAttachmentReleasePhase::Released
    );
    let terminal_config = terminal
        .network_config
        .as_ref()
        .expect("released manifest retains exact identity evidence");
    let terminal_attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(&terminal.spec.tenant_id, &terminal_config.attachment_id)
        .expect("released attachment should inspect")
        .expect("released attachment tombstone should remain durable");
    assert_eq!(
        terminal_attachment.resource().phase(),
        NetworkResourcePhase::Released
    );
    let terminal_segment = fixture
        .backend
        .segment_allocator
        .inspect_attachment_reservation(
            &terminal.spec.tenant_id,
            &terminal_config.attachment_id,
            &terminal_config.reservation_claim,
        )
        .expect("released segment should inspect");
    assert_eq!(
        terminal_segment.state(),
        NetworkAttachmentReservationState::Absent
    );
    let terminal_listeners = fixture
        .backend
        .port_lease_coordinator()
        .port_lease_records_snapshot(&terminal.port_leases, "released Krun listeners")
        .expect("released listener authority should inspect");
    assert!(
        terminal_listeners
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks
    );
    assert_eq!(fixture.runtime.signals(), signals);
    assert!(matches!(
        terminal.execution_teardown.stop(),
        KrunStopProgress::ExecutionStopped { fence, .. }
            if fence == stop.provider_claim()
    ));
    assert_eq!(terminal.status, SandboxStatus::Stopped);
    assert_eq!(terminal.handle.status, SandboxStatus::Stopped);
    assert!(terminal.shutdown_requested);
    assert!(terminal.launch_artifact.is_none());
    assert_eq!(terminal.launch_authority, KrunLaunchAuthority::Released);
    assert!(runtime_artifacts.iter().all(|path| !path.exists()));
    assert!(
        terminal.has_terminal_network_finality(),
        "ReleaseNetwork must not report success before provider-local artifact cleanup and terminal manifest publication"
    );
}

#[test]
fn krun_network_teardown_reopens_durable_owner_death_state() {
    let fixture = NetworkTeardownFixture::attached("owner-death");
    let stop = fixture.stop_execution("owner-death");
    let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);
    let release = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Release, 1);
    let attached = fixture.manifest();
    let pep = attached
        .egress_proxy
        .as_ref()
        .expect("attached manifest should retain its PEP assignment");
    let active_pep = fixture
        .backend
        .port_lease_coordinator()
        .port_lease_records_snapshot(std::slice::from_ref(&pep.port_lease), "active Krun PEP")
        .expect("active PEP authority should inspect")
        .pop()
        .expect("one active PEP record should exist");
    assert_eq!(active_pep.phase(), PortLeasePhase::Active);
    assert!(active_pep.active_lifetime().is_some());

    let NetworkTeardownFixture {
        root,
        config,
        backend,
        runtime,
        ..
    } = fixture;
    drop(runtime);
    drop(backend);

    let reopened = KrunSandboxBackend::new(config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let detached = execute_network(&reopened, &detach);
    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "a backend reopened from the durable roots must recover the dead PEP owner"
    );
    let retained = reopened
        .read_manifest(detach.sandbox_id())
        .expect("reopened manifest should read")
        .expect("reopened manifest should remain durable");
    assert!(retained.network_teardown.detached_proof().is_some());
    let retained_pep = retained
        .egress_proxy
        .as_ref()
        .expect("reopened detach retains the PEP assignment");
    let retained_record = reopened
        .port_lease_coordinator()
        .port_lease_records_snapshot(
            std::slice::from_ref(&retained_pep.port_lease),
            "owner-death retained Krun PEP",
        )
        .expect("owner-death retained PEP should inspect")
        .pop()
        .expect("one retained PEP record should exist");
    assert_eq!(retained_record.phase(), PortLeasePhase::Reserved);
    assert!(retained_record.binding().is_none());
    assert!(retained_record.active_lifetime().is_none());
    assert!(retained_record.confirmed_stopped_binding().is_some());

    let released = execute_network(&reopened, &release);
    assert_eq!(released.kind(), ProviderCommandObservationKind::Succeeded);
    drop(reopened);

    let recovered = KrunSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let journal = recovered
        .attempt_idempotency_journal()
        .expect("fresh backend should reopen the durable provider journal");
    for command in [&detach, &release] {
        assert!(matches!(
            journal
                .claim_dispatch_epoch(command.provider_claim())
                .expect("terminal network result should replay after reopen"),
            ProviderCommandClaimDecision::AdoptExactAttempt(observation)
                if observation.kind() == ProviderCommandObservationKind::Succeeded
        ));
    }
    let terminal = recovered
        .read_manifest(release.sandbox_id())
        .expect("twice-reopened manifest should read")
        .expect("twice-reopened manifest should remain durable");
    assert_eq!(
        terminal.network_teardown.release_phase(),
        HostManagedAttachmentReleasePhase::Released
    );
    drop(root);
}

#[test]
fn krun_network_two_thread_contenders_have_one_detach_and_release_winner() {
    let fixture = NetworkTeardownFixture::attached("thread-contenders");
    let stop = fixture.stop_execution("thread-contenders");
    let stopped_runtime = runtime_authority(&fixture.manifest());
    let terminal_checks = fixture.runtime.terminal_checks.load(Ordering::Acquire);
    let signals = fixture.runtime.signals();

    let detach = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1);
    let detach_outcomes = contend_network(&fixture, &detach);
    assert!(detach_outcomes.iter().all(|(role, kind)| matches!(
        (role, kind),
        (
            NetworkContenderRole::Execute,
            ProviderCommandObservationKind::Succeeded
        ) | (
            NetworkContenderRole::Adopt,
            ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::Succeeded
        )
    )));
    let retained = fixture.manifest();
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(!retained.network_layout.netns_path.exists());
    assert!(!retained.network_layout.status_path.exists());
    assert_eq!(runtime_authority(&retained), stopped_runtime);
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks
    );
    assert_eq!(fixture.runtime.signals(), signals);

    let mut expected_released_runtime = retained.clone();
    expected_released_runtime.launch_artifact = None;
    expected_released_runtime.shutdown_requested = true;
    expected_released_runtime.status = SandboxStatus::Stopped;
    expected_released_runtime.handle.status = SandboxStatus::Stopped;
    let release = fixture.network_command(&stop, SandboxNetworkTeardownOperation::Release, 1);
    let release_outcomes = contend_network(&fixture, &release);
    assert!(release_outcomes.iter().all(|(role, kind)| matches!(
        (role, kind),
        (
            NetworkContenderRole::Execute,
            ProviderCommandObservationKind::Succeeded
        ) | (
            NetworkContenderRole::Adopt,
            ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::Succeeded
        )
    )));
    let released = fixture.manifest();
    assert_eq!(
        released.network_teardown.release_phase(),
        HostManagedAttachmentReleasePhase::Released
    );
    assert_eq!(
        runtime_authority(&released),
        runtime_authority(&expected_released_runtime)
    );
    assert_eq!(released.launch_authority, KrunLaunchAuthority::Released);
    assert!(released.has_terminal_network_finality());
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks
    );
    assert_eq!(fixture.runtime.signals(), signals);
}

#[test]
fn krun_network_inspect_is_byte_stable_and_cannot_cross_older_execute() {
    let fixture = NetworkTeardownFixture::attached("inspect-order");
    let stop = fixture.stop_execution("inspect-order");
    let stopped_runtime = runtime_authority(&fixture.manifest());
    let terminal_checks = fixture.runtime.terminal_checks.load(Ordering::Acquire);
    let signals = fixture.runtime.signals();
    let detach =
        Arc::new(fixture.network_command(&stop, SandboxNetworkTeardownOperation::Detach, 1));
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("one network journal should open");
    let execution = match journal
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("detach should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh detach must receive execution authority")
        }
    };
    let claimed = execution.observation().clone();

    let before_inspection = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture
            .backend
            .inspect_network_teardown_with_observation(&detach, &claimed),
        SandboxNetworkTeardownObservation::InProgress { .. }
    ));
    assert_eq!(
        snapshot_files(fixture.root.path()),
        before_inspection,
        "exact network Inspect must not change a durable byte"
    );

    let execute_probe = KrunLifecycleLockTestProbe::new(Duration::from_secs(2));
    let inspect_probe = KrunLifecycleLockTestProbe::new(Duration::from_millis(100));
    let execute_backend = fixture
        .backend
        .clone()
        .with_lifecycle_lock_test_probe(execute_probe.clone());
    let inspect_backend = fixture
        .backend
        .clone()
        .with_lifecycle_lock_test_probe(inspect_probe.clone());
    let lifecycle = fixture
        .backend
        .lock_launch_lifecycle(&fixture.manifest())
        .expect("test should hold the production Execute lifecycle lock");

    let execute_command = Arc::clone(&detach);
    let executor = std::thread::spawn(move || {
        execute_backend.execute_network_teardown_with_claim(&execute_command, execution)
    });
    assert!(
        execute_probe.wait_until_contended(),
        "network Execute must hold its journal stream before waiting for lifecycle authority"
    );

    let inspect_command = Arc::clone(&detach);
    let provider_lock_probe = ProviderCommandLockTestProbe::new(Duration::from_secs(1));
    let inspector_lock_probe = provider_lock_probe.clone();
    let (inspection_tx, inspection_rx) = mpsc::channel();
    let inspector = std::thread::spawn(move || {
        let observation = with_provider_command_lock_test_probe(inspector_lock_probe, || {
            inspect_backend.inspect_network_teardown_with_observation(&inspect_command, &claimed)
        });
        inspection_tx
            .send(observation)
            .expect("inspection result should send");
    });
    assert!(
        provider_lock_probe.wait_until_contended(),
        "Inspect must attempt the exact live provider stream lock"
    );
    assert!(
        !inspect_probe.wait_until_contended(),
        "Inspect must wait at the journal before it can contend for the lifecycle lock"
    );
    assert!(
        matches!(inspection_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "Inspect must not report incomplete progress while the older Execute can publish"
    );

    drop(lifecycle);
    let executed = executor
        .join()
        .expect("network executor should join")
        .expect("network executor should publish");
    assert_eq!(executed.kind(), ProviderCommandObservationKind::Succeeded);
    let inspected = inspection_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("inspection should finish after the older Execute publishes");
    inspector.join().expect("network inspector should join");
    assert!(
        matches!(
            inspected,
            SandboxNetworkTeardownObservation::Ambiguous { .. }
        ),
        "the stale claimed observation must not report NotCompleted after terminal publication: {inspected:?}"
    );
    let terminal = fixture.manifest();
    assert!(terminal.network_teardown.detached_proof().is_some());
    assert_eq!(runtime_authority(&terminal), stopped_runtime);
    assert_eq!(
        fixture.runtime.terminal_checks.load(Ordering::Acquire),
        terminal_checks
    );
    assert_eq!(fixture.runtime.signals(), signals);
}
