use std::collections::BTreeMap;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nimbus_process_harness::PortWindow;

use super::support::*;
use super::teardown::state::ContainerDrainProgress;

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

struct ExecuteProvisionFixture {
    root: tempfile::TempDir,
    backend: ContainerSandboxBackend,
    id: SandboxId,
    spec: SandboxSpec,
    attempt: crate::SandboxExecutionAttemptId,
    plan: crate::SandboxProvisionNetworkPlan,
    /// Held until a test is ready for the attachment to bind the PEP port.
    pep_reservation: Option<TcpListener>,
    /// Owns the tripwire port above, so releasing the tripwire hands the port
    /// to the egress proxy and to nothing else.
    _port_window: PortWindow,
}

impl ExecuteProvisionFixture {
    fn reserved(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let port_window = PortWindow::claim();
        let pep_port = port_window.port(0);
        let pep_reservation =
            TcpListener::bind((Ipv4Addr::LOCALHOST, pep_port)).expect("PEP tripwire should bind");
        let mut config = ContainerSandboxBackendConfig::under_root(root.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        let backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(Arc::new(
            crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
        ));
        let id = SandboxId::new(format!("provision-barrier-{label}"));
        let spec = sample_spec_for_tenant(
            &format!("provision-barrier-{label}"),
            &format!("workload-{label}"),
        );
        let attempt = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        backend
            .reserve_provision_network(spec.clone(), id.clone(), attempt.clone(), plan.clone())
            .expect("provision fixture should reserve");
        Self {
            root,
            backend,
            id,
            spec,
            attempt,
            plan,
            pep_reservation: Some(pep_reservation),
            _port_window: port_window,
        }
    }

    fn prepare(&self) {
        self.backend
            .prepare_provision_workload(&self.id, &self.attempt)
            .expect("provision fixture should prepare");
    }

    fn manifest(&self) -> ContainerSandboxManifest {
        self.backend
            .read_manifest(&self.id)
            .expect("fixture manifest should read")
            .expect("fixture manifest should exist")
    }
}

struct MachinePublicationFixture {
    root: tempfile::TempDir,
    backend: ContainerSandboxBackend,
    id: SandboxId,
    attempt: crate::SandboxExecutionAttemptId,
    plan: crate::SandboxProvisionNetworkPlan,
    forwarder: crate::backends::oci::network::OciMachinePortForwarderConfig,
    _published_reservation: TcpListener,
    _forwarder_listener: TcpListener,
    _port_window: PortWindow,
}

impl MachinePublicationFixture {
    fn attached(label: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        // Offset 0 is the published binding, offset 1 the PEP range, offset 2
        // the forwarder endpoint. The published and forwarder tripwires stay
        // live for the whole fixture, which is what proves a closed drain
        // published no listener.
        let port_window = PortWindow::claim();
        let published_port = port_window.port(0);
        let pep_port = port_window.port(1);
        let forwarder_port = port_window.port(2);
        let published_reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, published_port))
            .expect("published tripwire should bind");
        let pep_reservation =
            TcpListener::bind((Ipv4Addr::LOCALHOST, pep_port)).expect("PEP tripwire should bind");
        let forwarder_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forwarder_port))
            .expect("forwarder tripwire should bind");
        let forwarder = sample_forwarder(forwarder_port);
        let mut config = ContainerSandboxBackendConfig::under_root(root.path());
        config.start_mode = ContainerStartMode::PlanOnly;
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.machine_port_forwarder = Some(forwarder.clone());
        let backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(Arc::new(
            crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
        ));
        let id = SandboxId::new(format!("machine-publication-barrier-{label}"));
        let spec = sample_spec_for_tenant(
            &format!("machine-publication-barrier-{label}"),
            &format!("machine-{label}"),
        )
        .with_port_binding(SandboxPortBinding::tcp("api", published_port, 8080));
        let attempt = sample_execution_attempt_id(&id);
        let plan = sample_provision_network_plan(&spec, &id, label);
        backend
            .reserve_provision_network(spec, id.clone(), attempt.clone(), plan.clone())
            .expect("machine fixture should reserve");
        backend
            .prepare_provision_workload(&id, &attempt)
            .expect("machine fixture should prepare");
        drop(pep_reservation);
        backend
            .attach_provision_network_with_test_host(&id, &attempt)
            .expect("machine fixture should attach");
        Self {
            root,
            backend,
            id,
            attempt,
            plan,
            forwarder,
            _published_reservation: published_reservation,
            _forwarder_listener: forwarder_listener,
            _port_window: port_window,
        }
    }
}

#[test]
fn closed_drain_barrier_rejects_all_four_provision_producers_without_mutation() {
    let reservation = ExecuteProvisionFixture::reserved("closed-reservation");
    close_execution_admission(&reservation.backend, &reservation.id);
    let before = snapshot_files(reservation.root.path());
    assert_drain_closed(reservation.backend.reserve_provision_network(
        reservation.spec.clone(),
        reservation.id.clone(),
        reservation.attempt.clone(),
        reservation.plan.clone(),
    ));
    assert_eq!(snapshot_files(reservation.root.path()), before);

    let preparation = ExecuteProvisionFixture::reserved("closed-preparation");
    close_execution_admission(&preparation.backend, &preparation.id);
    let before = snapshot_files(preparation.root.path());
    assert_drain_closed(
        preparation
            .backend
            .prepare_provision_workload(&preparation.id, &preparation.attempt),
    );
    assert_eq!(snapshot_files(preparation.root.path()), before);

    let mut attachment = ExecuteProvisionFixture::reserved("closed-attachment");
    attachment.prepare();
    drop(attachment.pep_reservation.take());
    close_execution_admission(&attachment.backend, &attachment.id);
    let before = snapshot_files(attachment.root.path());
    assert_drain_closed(
        attachment
            .backend
            .attach_provision_network_with_test_host(&attachment.id, &attachment.attempt),
    );
    assert_eq!(snapshot_files(attachment.root.path()), before);

    let publication = MachinePublicationFixture::attached("closed-publication");
    close_execution_admission(&publication.backend, &publication.id);
    let before = snapshot_files(publication.root.path());
    assert_drain_closed(
        publication
            .backend
            .publish_provision_machine_ingress_with_test_provider(
                &publication.id,
                &publication.attempt,
                &publication.plan,
                publication.forwarder.provider_instance(),
                publication.forwarder.provider_generation(),
            ),
    );
    assert_eq!(snapshot_files(publication.root.path()), before);
}

#[test]
fn new_reservation_finishes_before_a_waiting_drain_can_close_admission() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let base = ContainerSandboxBackend::new(config.clone());
    let id = SandboxId::new("provision-barrier-new-reservation-race");
    let spec = sample_spec_for_tenant("provision-barrier-new-reservation-race", "reservation");
    let attempt = sample_execution_attempt_id(&id);
    let plan = sample_provision_network_plan(&spec, &id, "new-reservation-race");
    let admission_probe = ProvisionAdmissionTestProbe::new(TEST_TIMEOUT);
    let producer = base
        .clone()
        .with_provision_admission_test_probe(admission_probe.clone());
    let producer_id = id.clone();
    let producer_attempt = attempt.clone();
    let producer_plan = plan.clone();
    let producer_spec = spec.clone();
    let producer_thread = std::thread::spawn(move || {
        producer.reserve_provision_network(
            producer_spec,
            producer_id,
            producer_attempt,
            producer_plan,
        )
    });
    assert!(admission_probe.wait_until_entered());

    let lock_probe = RunnerLifecycleLockTestProbe::new(TEST_TIMEOUT);
    let closer = ContainerSandboxBackend::new(config)
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let closer_id = id.clone();
    let closer_tenant = spec.tenant_id.clone();
    let closer_thread = std::thread::spawn(move || {
        let layout = OciConmonLayout::new_for_tenant(
            &closer.config.workload_state_root,
            &closer_tenant,
            &closer_id,
        );
        let _lifecycle = super::runner::lock_new_provision_lifecycle_for_backend(
            &closer,
            &layout.container_state_dir,
        )?;
        let mut manifest =
            closer
                .read_manifest(&closer_id)?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "reservation did not publish before drain acquired lifecycle"
                        .to_owned(),
                })?;
        set_execution_admission_closed(&mut manifest);
        closer.write_existing_workload_manifest(&manifest)
    });
    assert!(
        lock_probe.wait_until_contended(),
        "drain must wait for the admitted reservation lifecycle"
    );
    assert!(
        base.read_manifest(&id)
            .expect("manifest lookup should work")
            .is_none()
    );
    admission_probe.release();
    producer_thread
        .join()
        .expect("reservation producer should join")
        .expect("admitted reservation should finish");
    closer_thread
        .join()
        .expect("drain closer should join")
        .expect("drain should close after reservation settles");
    assert!(
        !base
            .read_manifest(&id)
            .unwrap()
            .unwrap()
            .execution_teardown
            .admission_is_open()
    );
    let before = snapshot_files(root.path());
    assert_drain_closed(base.reserve_provision_network(spec, id, attempt, plan));
    assert_eq!(snapshot_files(root.path()), before);
}

#[test]
fn admitted_preparation_finishes_before_a_waiting_drain_and_replay_is_fenced() {
    let fixture = ExecuteProvisionFixture::reserved("preparation-race");
    let admission_probe = ProvisionAdmissionTestProbe::new(TEST_TIMEOUT);
    let producer = fixture
        .backend
        .clone()
        .with_provision_admission_test_probe(admission_probe.clone());
    let producer_id = fixture.id.clone();
    let producer_attempt = fixture.attempt.clone();
    let producer_thread = std::thread::spawn(move || {
        producer.prepare_provision_workload(&producer_id, &producer_attempt)
    });
    assert!(admission_probe.wait_until_entered());

    let lock_probe = RunnerLifecycleLockTestProbe::new(TEST_TIMEOUT);
    let closer = fixture
        .backend
        .clone()
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let closer_id = fixture.id.clone();
    let closer_thread = std::thread::spawn(move || close_execution_admission(&closer, &closer_id));
    assert!(
        lock_probe.wait_until_contended(),
        "drain must wait for admitted preparation"
    );
    assert!(!fixture.manifest().provision_prepared);
    admission_probe.release();
    producer_thread
        .join()
        .expect("preparation producer should join")
        .expect("admitted preparation should finish");
    closer_thread.join().expect("drain closer should join");
    assert!(fixture.manifest().provision_prepared);
    let before = snapshot_files(fixture.root.path());
    assert_drain_closed(
        fixture
            .backend
            .prepare_provision_workload(&fixture.id, &fixture.attempt),
    );
    assert_eq!(snapshot_files(fixture.root.path()), before);
}

fn close_execution_admission(backend: &ContainerSandboxBackend, id: &SandboxId) {
    let snapshot = backend
        .read_manifest(id)
        .expect("manifest should read")
        .expect("manifest should exist");
    let (_lifecycle, mut manifest) =
        super::runner::lock_current_provision_lifecycle_for_backend(backend, &snapshot)
            .expect("drain should acquire lifecycle authority");
    set_execution_admission_closed(&mut manifest);
    backend
        .write_existing_workload_manifest(&manifest)
        .expect("drain barrier should persist");
}

fn set_execution_admission_closed(manifest: &mut ContainerSandboxManifest) {
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("fixture should retain its exact plan");
    let claim = crate::ProviderCommandClaim::new(crate::ProviderCommandClaimInput {
        authority_id: "authority-provision-drain-barrier".to_owned(),
        effect_subject: format!("{{\"sandbox\":\"{}\"}}", manifest.handle.id),
        source_attempt_id: None,
        attempt_id: "provision-drain-barrier".to_owned(),
        dispatch_epoch: 1,
        workload_generation: plan.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: plan.network_plan().digest().to_string(),
        provider_target_digest: "3".repeat(64),
        operation: crate::ProviderCommandOperation::DrainExecution,
    })
    .expect("drain barrier claim should validate");
    manifest
        .execution_teardown
        .set_drain(ContainerDrainProgress::BarrierPersisted { fence: claim });
}

fn assert_drain_closed<T: std::fmt::Debug>(result: crate::Result<T>) {
    let error = result.expect_err("a closed drain barrier must reject provision work");
    assert!(
        error
            .to_string()
            .contains("after the durable execution drain barrier"),
        "unexpected provision rejection: {error}"
    );
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = std::fs::read_dir(path)
            .expect("snapshot directory should read")
            .map(|entry| entry.expect("snapshot entry should read"))
            .collect::<Vec<_>>();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().expect("snapshot metadata should read");
            if metadata.is_dir() {
                visit(root, &path, out);
            } else if metadata.is_file() && path.extension().is_none_or(|ext| ext != "lock") {
                out.insert(
                    path.strip_prefix(root)
                        .expect("snapshot path should remain below root")
                        .to_path_buf(),
                    std::fs::read(&path).expect("snapshot file should read"),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}
