//! Real Container host-managed attachment teardown proofs.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::{Barrier, mpsc};
use std::time::Duration;

use nimbus_network::{NetworkCapabilitySourceDigest, NetworkResourcePhase, PortLeasePhase};

use super::*;
use crate::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY;
use crate::backends::conmon::creator::{CreatorAttemptReceipt, CreatorQuiescenceProof};
use crate::backends::container::runtime::machine_port_publication::{
    MachinePortPublicationAction, MachinePortPublicationCheckpoint, MachinePortPublicationObserver,
};
use crate::backends::container::runtime::support::sample_forwarder;
use crate::backends::container::runtime::{
    ContainerLifecycleCoordinator, ContainerNetworkPublicationMode, ContainerSandboxBackendConfig,
};
use crate::provider_command::{
    ProviderCommandLockTestProbe, with_provider_command_lock_test_probe,
};
use crate::{
    ProviderCommandClaim, ProviderCommandObservation, SandboxNetworkTeardownCommand,
    SandboxNetworkTeardownCommandInput, SandboxNetworkTeardownIdentity,
    SandboxNetworkTeardownIdentityInput, SandboxNetworkTeardownObservation,
    SandboxNetworkTeardownOperation, SandboxStatus,
};

#[path = "network_teardown/fresh_process.rs"]
mod fresh_process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkContenderRole {
    Execute,
    Adopt,
}

struct ForwardedNetworkFixture {
    fixture: TeardownFixture,
    forwarder: crate::backends::oci::network::OciMachinePortForwarderConfig,
    forwarder_listener: Option<TcpListener>,
}

impl ForwardedNetworkFixture {
    fn attached(label: &str, with_listener: bool) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let published_reservation = with_listener.then(|| {
            TcpListener::bind("127.0.0.1:0").expect("published-port tripwire should bind")
        });
        let published_port = published_reservation.as_ref().map(|listener| {
            listener
                .local_addr()
                .expect("published-port tripwire should report its address")
                .port()
        });
        let pep_reservation =
            TcpListener::bind("127.0.0.1:0").expect("PEP-port tripwire should bind");
        let pep_port = pep_reservation
            .local_addr()
            .expect("PEP-port tripwire should report its address")
            .port();
        let forwarder_listener =
            TcpListener::bind("127.0.0.1:0").expect("forwarder fixture should bind");
        let forwarder = sample_forwarder(
            forwarder_listener
                .local_addr()
                .expect("forwarder fixture should report its address")
                .port(),
        );
        let mut config = ContainerSandboxBackendConfig::under_root(root.path());
        config.start_mode = ContainerStartMode::PlanOnly;
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.netavark_path = PathBuf::from("/usr/bin/true");
        config.machine_port_forwarder = Some(forwarder.clone());
        let backend = ContainerSandboxBackend::new(config)
            .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
        let id = crate::SandboxId::new(format!("container-forwarded-teardown-{label}"));
        let mut spec = sample_spec_for_tenant(
            &format!("container-forwarded-teardown-{label}"),
            &format!("workload-{label}"),
        );
        if let Some(port) = published_port {
            spec = spec.with_port_binding(crate::SandboxPortBinding::tcp("api", port, 8080));
        }
        let execution_attempt_id = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        drop(pep_reservation);
        backend
            .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), plan.clone())
            .expect("forwarded teardown fixture should reserve its exact plan");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("forwarded teardown fixture should prepare its runner handoff");
        backend
            .attach_provision_network_with_test_host(&id, &execution_attempt_id)
            .expect("forwarded teardown fixture should attach its private network");
        drop(published_reservation);
        backend
            .publish_provision_machine_ingress_with_test_provider(
                &id,
                &execution_attempt_id,
                &plan,
                forwarder.provider_instance(),
                forwarder.provider_generation(),
            )
            .expect("forwarded teardown fixture should publish exact machine ingress");
        let manifest = backend
            .read_manifest(&id)
            .expect("forwarded manifest should read")
            .expect("forwarded manifest should exist");
        assert_eq!(manifest.start_mode, ContainerStartMode::PlanOnly);
        assert_eq!(
            manifest.lifecycle_coordinator,
            ContainerLifecycleCoordinator::PreparedServiceRunner
        );
        assert_eq!(
            manifest.runner_config.network_publication_mode,
            ContainerNetworkPublicationMode::MachineForwarded
        );
        Self {
            fixture: TeardownFixture {
                root,
                backend,
                id,
                execution_attempt_id,
            },
            forwarder,
            forwarder_listener: Some(forwarder_listener),
        }
    }

    fn persist_composite_stop(
        &self,
    ) -> (SandboxExecutionTeardownCommand, ProviderCommandObservation) {
        let stop = self.fixture.command(
            SandboxExecutionTeardownOperation::Stop,
            "forwarded-composite-stop",
            1,
        );
        let child_evidence = b"exact Container child terminal evidence".to_vec();
        let mut manifest = self.fixture.manifest();
        manifest.execution_teardown.set_stop(
            super::super::state::ContainerStopProgress::ExecutionStopped {
                fence: stop.provider_claim().clone(),
                evidence: child_evidence,
            },
        );
        self.fixture
            .backend
            .write_existing_workload_manifest(&manifest)
            .expect("forwarded child stop evidence should persist");
        let journal = self
            .fixture
            .backend
            .attempt_idempotency_journal()
            .expect("forwarded journal should open");
        let execution = match journal
            .claim_dispatch_epoch(stop.provider_claim())
            .expect("composite stop should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("fresh composite stop must own execution")
            }
        };
        let composite_evidence = b"exact Systemd plus Container composite stop evidence".to_vec();
        let (_, observation) = journal
            .execute_current_claim(execution, |_| {
                (
                    (),
                    ProviderCommandObservationKind::Succeeded,
                    None,
                    composite_evidence,
                )
            })
            .expect("composite stop should publish exact success");
        (stop, observation)
    }

    fn spawn_withdraw_provider(&mut self) -> std::thread::JoinHandle<Vec<String>> {
        let listener = self
            .forwarder_listener
            .take()
            .expect("withdraw provider should start once");
        let binding = self
            .fixture
            .manifest()
            .spec
            .port_bindings
            .into_iter()
            .next()
            .expect("withdraw server requires one binding");
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            for step in 0..3 {
                let (mut stream, _) = listener.accept().expect("forwarder request should connect");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("forwarder read timeout should configure");
                let request = read_http_request(&mut stream);
                let request_line = request.lines().next().unwrap_or_default().to_owned();
                requests.push(request_line.clone());
                let body = match step {
                    0 => {
                        assert!(request_line.contains("GET /services/forwarder/all "));
                        serde_json::to_vec(&vec![serde_json::json!({
                            "local": binding.host_socket_addr().to_string(),
                            "remote": format!(":{}", binding.host_port),
                            "protocol": "tcp",
                        })])
                        .expect("forwarder exposed response should encode")
                    }
                    1 => {
                        assert!(request_line.contains("POST /services/forwarder/unexpose "));
                        Vec::new()
                    }
                    2 => {
                        assert!(request_line.contains("GET /services/forwarder/all "));
                        b"[]".to_vec()
                    }
                    _ => unreachable!(),
                };
                let response = format!(
                    "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .and_then(|()| stream.write_all(&body))
                    .expect("forwarder response should write");
                stream
                    .shutdown(Shutdown::Write)
                    .expect("forwarder response should terminate");
            }
            requests
        })
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .expect("forwarder request should read");
        assert_ne!(read, 0, "forwarder request should contain complete headers");
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(request).expect("forwarder request should be UTF-8")
}

fn network_command(
    fixture: &TeardownFixture,
    stop: &SandboxExecutionTeardownCommand,
    operation: SandboxNetworkTeardownOperation,
    epoch: u64,
) -> SandboxNetworkTeardownCommand {
    let manifest = fixture.manifest();
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("attached fixture has a compiled network plan");
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: manifest.spec.tenant_id.clone(),
        sandbox_id: fixture.id.clone(),
        execution_attempt_id: fixture.execution_attempt_id.clone(),
        attachment_id: plan.attachment_id().clone(),
        network_plan: plan.network_plan().clone(),
        provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
    })
    .expect("network identity should validate");
    let stop_claim = stop.provider_claim();
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: stop_claim.authority_id().to_owned(),
        effect_subject: identity.provider_effect_subject(),
        source_attempt_id: None,
        attempt_id: stop_claim.attempt_id().to_owned(),
        dispatch_epoch: epoch,
        workload_generation: stop_claim.workload_generation(),
        restart_ordinal: 0,
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

fn execute_network(
    fixture: &TeardownFixture,
    command: &SandboxNetworkTeardownCommand,
) -> crate::ProviderCommandObservation {
    execute_network_with_backend(&fixture.backend, command)
}

fn execute_network_with_backend(
    backend: &ContainerSandboxBackend,
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

fn execute_forwarded_network(
    backend: &ContainerSandboxBackend,
    command: &SandboxNetworkTeardownCommand,
    prior_observation: &ProviderCommandObservation,
    forwarder: &crate::backends::oci::network::OciMachinePortForwarderConfig,
) -> ProviderCommandObservation {
    let journal = backend
        .attempt_idempotency_journal()
        .expect("forwarded network journal should open");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("forwarded network command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh forwarded network command must receive effect authority")
        }
    };
    let (sandbox_observation, provider_observation) = journal
        .execute_current_claim(execution, |current| {
            let observation = backend.execute_forwarded_network_teardown_substep(
                command,
                current,
                prior_observation,
                forwarder,
            );
            let kind = match &observation {
                SandboxNetworkTeardownObservation::Succeeded { .. } => {
                    ProviderCommandObservationKind::Succeeded
                }
                SandboxNetworkTeardownObservation::DefiniteFailure { .. } => {
                    ProviderCommandObservationKind::DefiniteFailure
                }
                SandboxNetworkTeardownObservation::InProgress { .. } => {
                    ProviderCommandObservationKind::InProgress
                }
                SandboxNetworkTeardownObservation::Absent { .. }
                | SandboxNetworkTeardownObservation::RetryAuthorized { .. }
                | SandboxNetworkTeardownObservation::Ambiguous { .. } => {
                    ProviderCommandObservationKind::Ambiguous
                }
            };
            let failure_code = observation.failure_code().map(str::to_owned);
            let evidence = observation.evidence().to_vec();
            (observation, kind, failure_code, evidence)
        })
        .expect("forwarded network command should publish its exact result");
    assert!(
        matches!(
            sandbox_observation,
            SandboxNetworkTeardownObservation::Succeeded { .. }
        ),
        "forwarded Sandbox observation should succeed: {sandbox_observation:?}"
    );
    provider_observation
}

fn claim_forwarded_detach_for_inspection(
    forwarded: &ForwardedNetworkFixture,
    label: &str,
) -> (
    ProviderCommandObservation,
    SandboxNetworkTeardownCommand,
    ProviderCommandObservation,
) {
    let (stop, composite_stop) = forwarded.persist_composite_stop();
    let detach = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    );
    let journal = forwarded
        .fixture
        .backend
        .attempt_idempotency_journal()
        .expect("forwarded inspection journal should open");
    let execution = match journal
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("forwarded detach should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh forwarded detach {label} must own execution")
        }
    };
    let claimed = execution.observation().clone();
    drop(execution);
    (composite_stop, detach, claimed)
}

fn persist_forwarded_detach_phase(
    forwarded: &ForwardedNetworkFixture,
    command: &SandboxNetworkTeardownCommand,
    target: crate::backends::oci::network::HostManagedAttachmentDetachPhase,
) {
    use crate::backends::oci::network::HostManagedAttachmentDetachPhase;

    let mut manifest = forwarded.fixture.manifest();
    for phase in [
        HostManagedAttachmentDetachPhase::AttachmentDeleting,
        HostManagedAttachmentDetachPhase::SegmentQuarantined,
        HostManagedAttachmentDetachPhase::PepStopMayExist,
        HostManagedAttachmentDetachPhase::PepRetained,
        HostManagedAttachmentDetachPhase::ListenerStopMayExist,
        HostManagedAttachmentDetachPhase::ProviderDeleteMayExist,
    ] {
        if phase > target {
            break;
        }
        assert!(
            manifest
                .network_teardown
                .record_detach_phase(command.provider_claim(), phase)
                .expect("forwarded detach checkpoint should advance")
        );
    }
    forwarded
        .fixture
        .backend
        .write_existing_workload_manifest(&manifest)
        .expect("forwarded detach checkpoint should persist");
}

struct FailAfterWithdrawalPrepared;

impl MachinePortPublicationObserver for FailAfterWithdrawalPrepared {
    fn checkpoint(&mut self, checkpoint: MachinePortPublicationCheckpoint) -> crate::Result<()> {
        assert!(matches!(
            checkpoint,
            MachinePortPublicationCheckpoint::BatchPrepared {
                action: MachinePortPublicationAction::Withdraw,
                ..
            }
        ));
        Err(crate::SandboxError::OperationFailed {
            message: "test stops after durable partial machine withdrawal".to_owned(),
        })
    }
}

fn contend_network(
    fixture: &TeardownFixture,
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
    let outcomes = std::thread::scope(|scope| {
        let mut contenders = Vec::new();
        for _ in 0..2 {
            let backend = Arc::clone(&backend);
            let journal = Arc::clone(&journal);
            let command = Arc::clone(&command);
            let start = Arc::clone(&start);
            contenders.push(scope.spawn(move || {
                start.wait();
                match journal
                    .claim_dispatch_epoch(command.provider_claim())
                    .expect("network contender should reach the one journal")
                {
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

fn runtime_authority(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    serde_json::to_vec(&(
        &manifest.execution_attempt_id,
        &manifest.conmon_launch,
        manifest.last_exit_code,
        manifest.shutdown_requested,
        &manifest.execution_teardown,
    ))
    .expect("runtime authority should serialize")
}

fn retain_stale_runtime_artifacts(
    fixture: &TeardownFixture,
    manifest: &mut ContainerSandboxManifest,
    label: &str,
) -> [PathBuf; 3] {
    manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced {
        proof: CreatorQuiescenceProof::dead_contained(CreatorAttemptReceipt::for_test(format!(
            "container-network-{label}"
        ))),
    };
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    fixture
        .backend
        .write_existing_workload_manifest(manifest)
        .expect("Container release fixture should persist exact provider absence");

    let paths = [
        manifest.conmon_layout.pidfile.clone(),
        manifest.conmon_layout.conmon_pidfile.clone(),
        manifest.conmon_layout.exit_status_file.clone(),
    ];
    for path in &paths {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("runtime artifact parent should create");
        }
    }
    std::fs::write(&paths[0], format!("{}\n", i32::MAX))
        .expect("stale runtime pidfile should persist");
    std::fs::write(&paths[1], format!("{}\n", i32::MAX))
        .expect("dead conmon receipt should persist");
    std::fs::write(&paths[2], b"0\n").expect("exit-status receipt should persist");
    paths
}

#[test]
fn forwarded_container_attachment_teardown_accepts_composite_stop_then_releases_machine_ports() {
    let mut forwarded = ForwardedNetworkFixture::attached("composite-stop", true);
    let (stop, composite_stop) = forwarded.persist_composite_stop();
    let detach = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    );
    forwarded
        .fixture
        .backend
        .preflight_forwarded_network_teardown_substep(
            &detach,
            &composite_stop,
            &forwarded.forwarder,
        )
        .expect("exact composite Stop should authorize forwarded detach");

    let withdrawal = forwarded.spawn_withdraw_provider();
    let detached = execute_forwarded_network(
        &forwarded.fixture.backend,
        &detach,
        &composite_stop,
        &forwarded.forwarder,
    );
    let requests = withdrawal
        .join()
        .expect("forwarder withdrawal server should join");
    assert_eq!(requests.len(), 3);

    let retained = forwarded.fixture.manifest();
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(
        forwarded
            .fixture
            .backend
            .exact_absent_machine_port_witness(&retained)
            .expect("machine absence witness should inspect")
            .is_some()
    );
    let retained_ports = forwarded
        .fixture
        .backend
        .port_lease_coordinator_for_manifest(&retained)
        .expect("retained machine port authority should open")
        .port_lease_records_snapshot(&retained.port_leases, "forwarded retained listeners")
        .expect("retained machine listeners should inspect");
    assert!(retained_ports.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.confirmed_stopped_binding().is_some()
    }));

    let reopened = ContainerSandboxBackend::new(forwarded.fixture.backend.config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let release = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Release,
        1,
    );
    reopened
        .preflight_forwarded_network_teardown_substep(&release, &detached, &forwarded.forwarder)
        .expect("fresh process should authenticate prior Detach and exact publication absence");
    let released = execute_forwarded_network(&reopened, &release, &detached, &forwarded.forwarder);
    assert_eq!(released.kind(), ProviderCommandObservationKind::Succeeded);

    let terminal = reopened
        .read_manifest(&forwarded.fixture.id)
        .expect("terminal forwarded manifest should read")
        .expect("terminal forwarded manifest should exist");
    assert_eq!(
        terminal.network_teardown.release_phase(),
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
    let released_ports = reopened
        .port_lease_coordinator_for_manifest(&terminal)
        .expect("released machine port authority should open")
        .port_lease_records_snapshot(&terminal.port_leases, "forwarded released listeners")
        .expect("released machine listeners should inspect");
    assert!(
        released_ports
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released)
    );
}

#[test]
fn forwarded_container_attachment_teardown_zero_listener_still_detaches_and_releases() {
    let forwarded = ForwardedNetworkFixture::attached("zero-listener", false);
    let (stop, composite_stop) = forwarded.persist_composite_stop();
    let detach = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    );
    let detached = execute_forwarded_network(
        &forwarded.fixture.backend,
        &detach,
        &composite_stop,
        &forwarded.forwarder,
    );
    let release = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Release,
        1,
    );
    let released = execute_forwarded_network(
        &forwarded.fixture.backend,
        &release,
        &detached,
        &forwarded.forwarder,
    );
    assert_eq!(released.kind(), ProviderCommandObservationKind::Succeeded);
    let terminal = forwarded.fixture.manifest();
    assert!(terminal.port_leases.is_empty());
    assert_eq!(
        terminal.network_teardown.release_phase(),
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
}

#[test]
fn forwarded_container_attachment_teardown_claimed_inspection_is_read_only_before_detach() {
    let forwarded = ForwardedNetworkFixture::attached("claimed-inspection", false);
    let (stop, composite_stop) = forwarded.persist_composite_stop();
    let detach = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    );
    let journal = forwarded
        .fixture
        .backend
        .attempt_idempotency_journal()
        .expect("forwarded inspection journal should open");
    let execution = match journal
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("forwarded detach should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh forwarded detach must own execution")
        }
    };
    let before = snapshot_files(forwarded.fixture.root.path());

    let observation = forwarded
        .fixture
        .backend
        .inspect_forwarded_network_teardown_substep(
            &detach,
            execution.observation(),
            &composite_stop,
            &forwarded.forwarder,
        );

    assert!(matches!(
        observation,
        SandboxNetworkTeardownObservation::InProgress { .. }
    ));
    assert_eq!(
        snapshot_files(forwarded.fixture.root.path()),
        before,
        "claimed inspection before the first detach effect must not create publication evidence or mutate durable bytes"
    );
}

#[test]
fn forwarded_container_attachment_teardown_early_present_inspection_is_in_progress_and_read_only() {
    use crate::backends::oci::network::HostManagedAttachmentDetachPhase;

    let forwarded = ForwardedNetworkFixture::attached("early-present-inspection", true);
    let (composite_stop, detach, claimed) =
        claim_forwarded_detach_for_inspection(&forwarded, "early-present");
    persist_forwarded_detach_phase(
        &forwarded,
        &detach,
        HostManagedAttachmentDetachPhase::AttachmentDeleting,
    );
    let before = snapshot_files(forwarded.fixture.root.path());

    let observation = forwarded
        .fixture
        .backend
        .inspect_forwarded_network_teardown_substep(
            &detach,
            &claimed,
            &composite_stop,
            &forwarded.forwarder,
        );

    assert!(matches!(
        observation,
        SandboxNetworkTeardownObservation::InProgress { .. }
    ));
    assert_eq!(snapshot_files(forwarded.fixture.root.path()), before);
}

#[test]
fn forwarded_container_attachment_teardown_late_present_inspection_is_ambiguous_and_read_only() {
    use crate::backends::oci::network::HostManagedAttachmentDetachPhase;

    let forwarded = ForwardedNetworkFixture::attached("late-present-inspection", true);
    let (composite_stop, detach, claimed) =
        claim_forwarded_detach_for_inspection(&forwarded, "late-present");
    persist_forwarded_detach_phase(
        &forwarded,
        &detach,
        HostManagedAttachmentDetachPhase::ProviderDeleteMayExist,
    );
    let before = snapshot_files(forwarded.fixture.root.path());

    let observation = forwarded
        .fixture
        .backend
        .inspect_forwarded_network_teardown_substep(
            &detach,
            &claimed,
            &composite_stop,
            &forwarded.forwarder,
        );

    assert!(matches!(
        observation,
        SandboxNetworkTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(forwarded.fixture.root.path()), before);
}

#[test]
fn forwarded_container_attachment_teardown_partial_publication_inspection_is_ambiguous_and_read_only()
 {
    use crate::backends::oci::network::HostManagedAttachmentDetachPhase;

    let forwarded = ForwardedNetworkFixture::attached("partial-publication-inspection", true);
    let (composite_stop, detach, claimed) =
        claim_forwarded_detach_for_inspection(&forwarded, "partial-publication");
    let manifest = forwarded.fixture.manifest();
    forwarded
        .fixture
        .backend
        .prepare_machine_port_publication_withdrawal_for_test_with_observer(
            &manifest,
            &mut FailAfterWithdrawalPrepared,
        )
        .expect_err("partial withdrawal fixture must stop after durable preparation");
    persist_forwarded_detach_phase(
        &forwarded,
        &detach,
        HostManagedAttachmentDetachPhase::AttachmentDeleting,
    );
    let before = snapshot_files(forwarded.fixture.root.path());

    let observation = forwarded
        .fixture
        .backend
        .inspect_forwarded_network_teardown_substep(
            &detach,
            &claimed,
            &composite_stop,
            &forwarded.forwarder,
        );

    assert!(matches!(
        observation,
        SandboxNetworkTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(snapshot_files(forwarded.fixture.root.path()), before);
}

#[test]
fn forwarded_container_release_inspection_requires_local_detach_journal_and_is_read_only() {
    let forwarded = ForwardedNetworkFixture::attached("release-inspect-local-detach", false);
    let (stop, composite_stop) = forwarded.persist_composite_stop();
    let detach = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    );
    let detached = execute_forwarded_network(
        &forwarded.fixture.backend,
        &detach,
        &composite_stop,
        &forwarded.forwarder,
    );
    let release = network_command(
        &forwarded.fixture,
        &stop,
        SandboxNetworkTeardownOperation::Release,
        1,
    );
    let journal = forwarded
        .fixture
        .backend
        .attempt_idempotency_journal()
        .expect("forwarded Release inspection journal should open");
    let execution = match journal
        .claim_dispatch_epoch(release.provider_claim())
        .expect("forwarded Release inspection should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh forwarded Release inspection must own execution")
        }
    };
    let claimed = execution.observation().clone();
    drop(execution);

    let detached_claim =
        serde_json::to_value(detached.claim()).expect("prior DetachNetwork claim should encode");
    let detach_records = snapshot_files(forwarded.fixture.root.path())
        .into_iter()
        .filter(|(path, bytes)| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && serde_json::from_slice::<serde_json::Value>(bytes)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("observation")
                            .and_then(|observation| observation.get("claim"))
                            .cloned()
                    })
                    .as_ref()
                    == Some(&detached_claim)
        })
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    assert_eq!(
        detach_records.len(),
        1,
        "fixture must contain one exact prior DetachNetwork journal record"
    );
    std::fs::remove_file(forwarded.fixture.root.path().join(&detach_records[0]))
        .expect("test should remove the exact prior DetachNetwork journal record");
    let before = snapshot_files(forwarded.fixture.root.path());

    let observation = forwarded
        .fixture
        .backend
        .inspect_forwarded_network_teardown_substep(
            &release,
            &claimed,
            &detached,
            &forwarded.forwarder,
        );

    assert!(matches!(
        observation,
        SandboxNetworkTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(
        snapshot_files(forwarded.fixture.root.path()),
        before,
        "Release inspection must not recreate missing prior journal authority or mutate durable bytes"
    );
}

#[test]
fn container_network_teardown_detaches_retained_then_releases_in_order() {
    let fixture = TeardownFixture::attached("network-detach-release");
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "network", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "network", 1);
    fixture.runtime_for_terminal_stop(&stop);

    let before = fixture.manifest();
    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
    let detached = execute_network(&fixture, &detach);
    let detached_inspection = fixture
        .backend
        .inspect_network_teardown_with_observation(&detach, &detached);
    let failed_manifest = fixture.manifest();
    let pep = failed_manifest
        .egress_proxy
        .as_ref()
        .expect("attached fixture retains its PEP assignment");
    let pep_record = fixture
        .backend
        .port_lease_coordinator_for_manifest(&failed_manifest)
        .expect("PEP port authority should open")
        .port_lease_records_snapshot(std::slice::from_ref(&pep.port_lease), "failed detach PEP")
        .expect("PEP record should inspect");
    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "provider={detached:?}; inspection={detached_inspection:?}; state={:?}; pep={pep:?}; record={pep_record:?}",
        failed_manifest.network_teardown
    );

    let retained = fixture.manifest();
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(!retained.network_layout.netns_path.exists());
    assert!(!retained.network_layout.status_path.exists());
    let ports = fixture
        .backend
        .port_lease_coordinator_for_manifest(&retained)
        .expect("retained port authority should open")
        .port_lease_records_snapshot(&retained.port_leases, "retained test listeners")
        .expect("retained listener records should inspect");
    assert!(ports.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.active_lifetime().is_none()
    }));
    let attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(
            &retained.spec.tenant_id,
            &retained
                .network_config
                .as_ref()
                .expect("network config remains retained")
                .attachment_id,
        )
        .expect("portable attachment should inspect")
        .expect("portable attachment should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Deleting
    );
    assert_eq!(before.network_config, retained.network_config);

    let replay = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("network journal should reopen")
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("exact detach should replay");
    assert!(matches!(
        replay,
        ProviderCommandClaimDecision::AdoptExactAttempt(observation)
            if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));

    let mut retained = fixture.manifest();
    let runtime_artifacts =
        retain_stale_runtime_artifacts(&fixture, &mut retained, "direct-release");
    let release = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Release, 1);
    let released = execute_network(&fixture, &release);
    assert_eq!(
        released.kind(),
        ProviderCommandObservationKind::Succeeded,
        "provider={released:?}; state={:?}",
        fixture.manifest().network_teardown
    );
    let terminal = fixture.manifest();
    assert_eq!(
        terminal.network_teardown.release_phase(),
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
    let attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(
            &terminal.spec.tenant_id,
            &terminal
                .network_config
                .as_ref()
                .expect("released manifest retains identity evidence")
                .attachment_id,
        )
        .expect("released attachment should inspect")
        .expect("released attachment tombstone should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Released
    );
    assert_eq!(terminal.status, SandboxStatus::Stopped);
    assert_eq!(terminal.handle.status, SandboxStatus::Stopped);
    assert!(terminal.shutdown_requested);
    assert!(terminal.launch_artifact.is_none());
    assert!(terminal.launch_reservation_claim.is_none());
    assert!(terminal.network_cleanup_complete);
    assert!(runtime_artifacts.iter().all(|path| !path.exists()));
    assert!(
        terminal.has_terminal_network_finality(),
        "ReleaseNetwork must not report success before provider-local artifact cleanup and terminal manifest publication"
    );
}

#[test]
fn container_network_detach_recovers_pep_after_process_owner_death() {
    let fixture = TeardownFixture::attached("network-detach-pep-owner-death");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-owner-death",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-owner-death",
        1,
    );
    fixture.runtime_for_terminal_stop(&stop);
    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
    let config = fixture.backend.config.clone();
    let root = fixture.root;
    drop(fixture.backend);

    let reopened = ContainerSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let detached = execute_network_with_backend(&reopened, &detach);
    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "fresh backend must recover the dead process-bound PEP from durable lifetime authority"
    );
    let manifest = reopened
        .read_manifest(detach.sandbox_id())
        .expect("reopened manifest should read")
        .expect("reopened manifest should remain durable");
    assert!(manifest.network_teardown.detached_proof().is_some());
    let assignment = manifest
        .egress_proxy
        .as_ref()
        .expect("detached manifest retains the PEP assignment");
    let record = reopened
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("reopened port authority should compile")
        .port_lease_records_snapshot(
            std::slice::from_ref(&assignment.port_lease),
            "owner-death retained PEP",
        )
        .expect("retained PEP should inspect")
        .pop()
        .expect("one retained PEP record should exist");
    assert_eq!(record.phase(), PortLeasePhase::Reserved);
    assert!(record.binding().is_none());
    assert!(record.active_lifetime().is_none());
    assert!(record.confirmed_stopped_binding().is_some());
    assert!(
        !crate::backends::oci::egress::egress_trust_anchor_path(
            &crate::backends::oci::egress::egress_trust_anchor_root(
                &reopened.config.network_state_root,
            ),
            &manifest.spec.tenant_id,
            &manifest.handle.id,
        )
        .exists(),
        "dead-owner recovery must remove the exact trust anchor before retained settlement"
    );
    drop(root);
}

#[test]
fn container_network_two_thread_contenders_have_one_detach_and_release_winner() {
    let fixture = TeardownFixture::attached("network-thread-contenders");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-thread-contenders",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-thread-contenders",
        1,
    );
    let runtime = fixture.runtime_for_terminal_stop(&stop);
    let stopped_runtime = runtime_authority(&fixture.manifest());

    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
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
    assert!(runtime.signals().is_empty());

    let release = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Release, 1);
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
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
    assert_eq!(runtime_authority(&released), stopped_runtime);
    assert!(runtime.signals().is_empty());
}

#[test]
fn container_network_inspect_is_byte_stable_and_cannot_cross_older_execute() {
    let fixture = TeardownFixture::attached("network-inspect-order");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-inspect-order",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-inspect-order",
        1,
    );
    let runtime = fixture.runtime_for_terminal_stop(&stop);
    let stopped_runtime = runtime_authority(&fixture.manifest());
    let detach = Arc::new(network_command(
        &fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    ));
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

    let execute_probe =
        super::super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let inspect_probe =
        super::super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_millis(100));
    let execute_backend = fixture
        .backend
        .clone()
        .with_runner_lifecycle_lock_test_probe(execute_probe.clone());
    let inspect_backend = fixture
        .backend
        .clone()
        .with_runner_lifecycle_lock_test_probe(inspect_probe.clone());
    let lifecycle = super::super::super::runner::lock_execute_lifecycle(&fixture.manifest())
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
    assert!(runtime.signals().is_empty());
}

impl TeardownFixture {
    fn runtime_for_terminal_stop(&self, stop: &SandboxExecutionTeardownCommand) -> ScriptedRuntime {
        let journal = self
            .backend
            .attempt_idempotency_journal()
            .expect("execution journal should open");
        let execution = claim_teardown_execution(&journal, stop);
        let runtime = ScriptedRuntime::live(self.backend.clone(), 100);
        runtime.terminal.store(true, Ordering::Release);
        let observation = self
            .backend
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                stop,
                &runtime,
                Some(execution.observation()),
            )
            .expect("scripted terminal stop should complete");
        assert!(matches!(
            observation,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        persist_teardown_observation(&journal, stop, &observation);
        runtime
    }
}
