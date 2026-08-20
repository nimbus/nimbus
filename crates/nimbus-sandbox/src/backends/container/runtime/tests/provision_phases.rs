use super::support::*;
use crate::backends::oci::network::default_network_attachment_id;
use nimbus_network::{LocalPortLeaseAuthority, NetworkResourceGeneration, PortLeasePhase};
use nimbus_process_harness::PortWindow;
use std::collections::BTreeMap;
use std::io::Read as _;
use std::net::{Ipv4Addr, Shutdown, TcpListener};

#[derive(Clone, Copy, Debug)]
enum ReservationCrashCut {
    ClaimPublished,
    AttachmentReserved,
    PortsReserved,
}

#[test]
fn container_provision_activation_classifies_runtime_state() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("container-activation-state-matrix");
    let execution_attempt_id = sample_execution_attempt_id(&id);
    let spec = sample_spec_for_tenant("container-activation-state", "api");
    let network_plan = sample_provision_network_plan(&spec, &id, "activation-state-matrix");
    backend
        .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), network_plan)
        .expect("activation fixture should reserve");
    let mut manifest = backend
        .read_manifest(&id)
        .expect("activation fixture manifest should read")
        .expect("activation fixture manifest should exist");

    for (state, expected) in [
        ("running", "succeeded"),
        ("creating", "in_progress"),
        ("created", "absent"),
        ("stopped", "ambiguous"),
        ("paused", "ambiguous"),
        ("unknown-provider-state", "ambiguous"),
    ] {
        manifest.conmon_launch.state_command =
            crate::backends::oci::command::CommandSpec::new("/bin/sh").args([
                "-c".to_owned(),
                format!(
                    "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"{state}\"}}'",
                    id.as_str()
                ),
            ]);
        backend
            .write_manifest(&manifest)
            .expect("activation state fixture should persist");
        let observed = backend
            .inspect_provision_workload_activation(&id, &execution_attempt_id)
            .expect("activation state should inspect");
        let actual = match observed {
            crate::SandboxProvisionPhaseObservation::Succeeded { .. } => "succeeded",
            crate::SandboxProvisionPhaseObservation::Absent { .. } => "absent",
            crate::SandboxProvisionPhaseObservation::InProgress { .. } => "in_progress",
            crate::SandboxProvisionPhaseObservation::Ambiguous { .. } => "ambiguous",
        };
        assert_eq!(actual, expected, "runtime state {state} was misclassified");
    }
}

fn seed_partial_reservation(
    backend: &ContainerSandboxBackend,
    spec: &SandboxSpec,
    id: &SandboxId,
    network_plan: &crate::SandboxProvisionNetworkPlan,
    cut: ReservationCrashCut,
) -> ContainerSandboxManifest {
    let mut start = backend
        .plan_start_with_id_with_network_reservation(
            spec,
            id,
            ContainerStartPlanningOptions {
                execution_attempt_id: sample_execution_attempt_id(id),
                launch_defaults: None,
                launch_artifact: None,
                provision_network_plan: Some(network_plan),
                reserve_execute_network: false,
                prepare_bundle: false,
            },
        )
        .expect("partial reservation plan should lower");
    let claim = backend
        .begin_launch_reservation(&mut start.manifest)
        .expect("desired plan and reservation claim should publish atomically");
    if matches!(cut, ReservationCrashCut::ClaimPublished) {
        return start.manifest;
    }
    let config = backend
        .place_sandbox_config(
            &spec.tenant_id,
            &start.manifest.network_layout,
            id,
            network_plan.attachment_id(),
            &claim,
        )
        .expect("partial reservation should place exact attachment authority");
    if matches!(cut, ReservationCrashCut::AttachmentReserved) {
        return start.manifest;
    }
    let internal = crate::backends::oci::egress::egress_listener_reservation(&config)
        .expect("partial reservation should compile its PEP listener");
    backend
        .port_lease_coordinator()
        .reserve_exact_provision_ports(network_plan, Some(internal), &claim)
        .expect("partial reservation should reserve exact plan members")
        .confirm_manifest_published()
        .expect("test crash cut should leave durable reservation records");
    start.manifest
}

fn snapshot_files(root: &std::path::Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &std::path::Path, path: &std::path::Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
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

struct PlanOnlyMachineProvisionFixture {
    _root: tempfile::TempDir,
    /// Owns every host port below. The tripwires are released only when the
    /// product is about to bind the same port, so the window has to outlive
    /// them both.
    _port_window: PortWindow,
    backend: ContainerSandboxBackend,
    id: SandboxId,
    execution_attempt_id: crate::SandboxExecutionAttemptId,
    spec: SandboxSpec,
    network_plan: crate::SandboxProvisionNetworkPlan,
    published_reservation: Option<TcpListener>,
    pep_reservation: Option<TcpListener>,
    forwarder_listener: Option<TcpListener>,
}

impl PlanOnlyMachineProvisionFixture {
    fn prepared(name: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary root should exist");
        // The window partitions this fixture's host ports: offset 0 is the
        // published binding, offset 1 the PEP range, offset 2 the forwarder
        // observer. The tripwires below still hold the first two, which is
        // what proves the product created no listener of its own; the window
        // makes the hand-off deterministic once a tripwire is released.
        let port_window = PortWindow::claim();
        let published_port = port_window.port(0);
        let pep_port = port_window.port(1);
        let forwarder_port = port_window.port(2);
        let published_reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, published_port))
            .expect("published tripwire should bind");
        let pep_reservation =
            TcpListener::bind((Ipv4Addr::LOCALHOST, pep_port)).expect("PEP tripwire should bind");
        let forwarder_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forwarder_port))
            .expect("forwarder observer should bind");
        let mut config = ContainerSandboxBackendConfig::under_root(root.path());
        config.start_mode = ContainerStartMode::PlanOnly;
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = pep_port..=pep_port;
        config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
        let backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(Arc::new(
            crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
        ));
        let id = SandboxId::new(format!("plan-only-machine-{name}"));
        let spec = sample_spec_for_tenant(
            &format!("plan-only-machine-{name}"),
            &format!("machine-{name}"),
        )
        .with_port_binding(SandboxPortBinding::tcp("api", published_port, 8080));
        let network_plan = sample_provision_network_plan(&spec, &id, name);
        let execution_attempt_id = sample_execution_attempt_id(&id);
        backend
            .reserve_provision_network(
                spec.clone(),
                id.clone(),
                execution_attempt_id.clone(),
                network_plan.clone(),
            )
            .expect("PlanOnly reservation should retain exact network authority");
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("PlanOnly preparation should install the runner handoff");
        Self {
            _root: root,
            _port_window: port_window,
            backend,
            id,
            execution_attempt_id,
            spec,
            network_plan,
            published_reservation: Some(published_reservation),
            pep_reservation: Some(pep_reservation),
            forwarder_listener: Some(forwarder_listener),
        }
    }

    fn forwarder(&self) -> crate::backends::oci::network::OciMachinePortForwarderConfig {
        self.backend
            .read_manifest(&self.id)
            .expect("manifest should read")
            .expect("manifest should exist")
            .runner_config
            .machine_port_forwarder
            .expect("machine fixture should retain forwarder authority")
    }

    fn attach(&mut self) {
        drop(self.pep_reservation.take());
        self.backend
            .attach_provision_network_with_test_host(&self.id, &self.execution_attempt_id)
            .expect("deterministic host should realize the private attachment");
    }

    fn publish(&mut self) {
        drop(self.published_reservation.take());
        let forwarder = self.forwarder();
        self.backend
            .publish_provision_machine_ingress_with_test_provider(
                &self.id,
                &self.execution_attempt_id,
                &self.network_plan,
                forwarder.provider_instance(),
                forwarder.provider_generation(),
            )
            .expect("exact publish command should converge deterministic provider evidence");
    }

    fn manifest(&self) -> ContainerSandboxManifest {
        self.backend
            .read_manifest(&self.id)
            .expect("manifest should read")
            .expect("manifest should exist")
    }

    fn publication_registry_len(&self) -> usize {
        self.backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should inspect")
            .len()
    }

    fn evidence_path(&self) -> PathBuf {
        self.manifest()
            .conmon_layout
            .container_state_dir
            .join(".nimbus-machine-port-evidence.json")
    }

    fn reopen_backend_after_process_death(&mut self) {
        let config = self.backend.config.clone();
        let placeholder_root = tempfile::tempdir().expect("placeholder root should exist");
        let placeholder = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(
            placeholder_root.path(),
        ));
        let dead_owner = std::mem::replace(&mut self.backend, placeholder);
        drop(dead_owner);
        self.backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(Arc::new(
            crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
        ));
    }

    fn spawn_exact_inspection(&mut self) -> std::thread::JoinHandle<String> {
        let listener = self
            .forwarder_listener
            .take()
            .expect("exact inspection server starts once");
        let bindings = self.manifest().spec.port_bindings;
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("inspection should connect");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .expect("inspection read timeout should configure");
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).expect("inspection should read");
                assert_ne!(
                    read, 0,
                    "inspection request should include complete headers"
                );
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            let body = serde_json::to_vec(
                &bindings
                    .iter()
                    .map(|binding| {
                        serde_json::json!({
                            "local": binding.host_socket_addr().to_string(),
                            "remote": format!(":{}", binding.host_port),
                            "protocol": "tcp",
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("inspection body should encode");
            let headers = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream
                .write_all(headers.as_bytes())
                .and_then(|()| stream.write_all(&body))
                .expect("inspection response should write");
            stream
                .shutdown(Shutdown::Write)
                .expect("inspection response should terminate");
            String::from_utf8(request).expect("inspection request should be UTF-8")
        })
    }
}

fn assert_published_lease_is_reserved(fixture: &PlanOnlyMachineProvisionFixture) {
    let manifest = fixture.manifest();
    let authority = LocalPortLeaseAuthority::open(&fixture.backend.config.network_state_root)
        .expect("port authority should open");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("published lease should inspect")
        .expect("published lease should exist");
    assert_eq!(record.phase(), PortLeasePhase::Reserved);
    assert!(record.binding().is_none());
}

#[test]
fn reserve_is_durable_and_stays_unprepared_and_unattached() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-reserved-container");
    let spec = sample_spec();
    let network_plan = sample_provision_network_plan(&spec, &id, "container-reserve");

    let handle = backend
        .reserve_provision_network(
            spec.clone(),
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan.clone(),
        )
        .expect("reserve should acquire durable provider resources");
    assert_eq!(handle.id, id);
    let manifest = backend
        .read_manifest(&id)
        .expect("reserved manifest should be readable")
        .expect("reserved manifest should exist");
    assert!(
        manifest.network_config.is_some() && manifest.launch_reservation_claim.is_some(),
        "reservation must persist the exact private-network and lease authority"
    );
    assert!(!manifest.provision_prepared);
    assert!(
        !manifest.bundle_layout.config_path.exists(),
        "reservation must not materialize the workload bundle"
    );
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "reservation must not create the workload network namespace"
    );
    assert_eq!(
        backend
            .inspect_provision_network_reservation(
                &id,
                &sample_execution_attempt_id(&id),
                &network_plan,
            )
            .expect("reservation inspection should succeed")
            .expect("reservation should be observed")
            .id,
        id
    );
    let crossed_plan = sample_provision_network_plan(&spec, &id, "container-reserve-crossed");
    let error = backend
        .inspect_provision_network_reservation(
            &id,
            &sample_execution_attempt_id(&id),
            &crossed_plan,
        )
        .expect_err("crossed reservation plans must fail closed");
    assert!(
        error
            .to_string()
            .contains("exact execution attempt or compiled network plan")
    );
    assert!(
        backend
            .inspect_provision_preparation(&id, &sample_execution_attempt_id(&id))
            .expect("preparation inspection should succeed")
            .is_none(),
        "an unprepared reservation must not be reported as prepared"
    );
    let error = backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect_err("direct replay must inspect rather than replace the reservation");
    assert!(error.to_string().contains("inspect it instead"));
}

#[test]
fn exact_inspection_projects_reserved_attachment_without_unready_endpoints() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-container-portable-inspection");
    let spec = sample_spec();
    let plan = sample_provision_network_plan(&spec, &id, "container-portable-inspection");
    let expected_attachment = plan.attachment_id().clone();
    let expected_generation = plan.generation();
    backend
        .reserve_provision_network(spec, id.clone(), sample_execution_attempt_id(&id), plan)
        .expect("reservation should succeed");

    let inspection = backend
        .inspect_sync(&id)
        .expect("exact inspection should succeed")
        .expect("reserved workload should remain visible");
    let status = inspection
        .network_status
        .expect("exact manifest should project portable status");
    assert_eq!(
        status
            .attachment()
            .expect("reserved attachment should be visible")
            .attachment_id(),
        &expected_attachment
    );
    assert_eq!(status.generation(), Some(expected_generation));
    assert!(
        status.published_endpoints().is_empty(),
        "unready inspection must not publish endpoint handles"
    );
}

#[test]
fn every_partial_reservation_cut_reopens_and_converges_exactly_once() {
    for cut in [
        ReservationCrashCut::ClaimPublished,
        ReservationCrashCut::AttachmentReserved,
        ReservationCrashCut::PortsReserved,
    ] {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let config = ContainerSandboxBackendConfig::under_root(root.path());
        let backend = ContainerSandboxBackend::new(config.clone());
        let id = SandboxId::new(format!("reserve-reopen-{cut:?}"));
        let spec = sample_spec_for_tenant(
            &format!("reserve-reopen-{cut:?}"),
            &format!("reserve-reopen-{cut:?}"),
        )
        .with_port_binding(SandboxPortBinding::tcp("api", 18_180, 8_080));
        let network_plan = sample_provision_network_plan(&spec, &id, &format!("{cut:?}"));
        let partial = seed_partial_reservation(&backend, &spec, &id, &network_plan, cut);
        let claim = partial
            .launch_reservation_claim
            .clone()
            .expect("partial manifest should retain exact claim");
        drop(backend);

        let reopened = ContainerSandboxBackend::new(config);
        reopened
            .reserve_provision_network(
                spec.clone(),
                id.clone(),
                sample_execution_attempt_id(&id),
                network_plan.clone(),
            )
            .unwrap_or_else(|error| panic!("{cut:?} retry should converge: {error}"));
        let complete = reopened
            .read_manifest(&id)
            .expect("manifest should read")
            .expect("manifest should remain durable");
        assert_eq!(
            complete.launch_reservation_claim.as_ref(),
            Some(&claim),
            "{cut:?} retry must preserve the one reservation coordinator"
        );
        assert_eq!(
            complete.provision_network_plan.as_ref(),
            Some(&network_plan),
            "{cut:?} retry must preserve the compiler desired plan"
        );
        let records = LocalPortLeaseAuthority::open(&reopened.config.network_state_root)
            .expect("port authority should reopen")
            .list_plan(network_plan.plan_id())
            .expect("plan records should inspect");
        assert_eq!(
            records.len(),
            network_plan.listeners().len() + 1,
            "{cut:?} retry must not duplicate the published subset or PEP sibling"
        );
        assert!(records.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved && record.reservation_claim() == Some(&claim)
        }));
    }
}

#[test]
fn partial_reservation_crossed_plan_fails_before_any_durable_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let id = SandboxId::new("reserve-crossed-plan");
    let spec = sample_spec_for_tenant("reserve-crossed-plan", "reserve-crossed-plan")
        .with_port_binding(SandboxPortBinding::tcp("api", 18_181, 8_080));
    let network_plan = sample_provision_network_plan(&spec, &id, "original-plan");
    seed_partial_reservation(
        &backend,
        &spec,
        &id,
        &network_plan,
        ReservationCrashCut::AttachmentReserved,
    );
    drop(backend);
    let reopened = ContainerSandboxBackend::new(config);
    let before = snapshot_files(root.path());
    let crossed = sample_provision_network_plan(&spec, &id, "crossed-plan");
    let error = reopened
        .reserve_provision_network(spec, id.clone(), sample_execution_attempt_id(&id), crossed)
        .expect_err("crossed desired plan must fail before resume effects");
    assert!(error.to_string().contains("crossed its exact durable"));
    assert_eq!(
        snapshot_files(root.path()),
        before,
        "crossed retry must preserve manifest, allocator, IPAM, and port authority bytes"
    );
}

#[test]
fn effect_bearing_claim_only_manifest_is_fenced_on_fresh_backend_without_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let id = SandboxId::new("reserve-effect-bearing");
    let spec = sample_spec_for_tenant("reserve-effect-bearing", "reserve-effect-bearing")
        .with_port_binding(SandboxPortBinding::tcp("api", 18_182, 8_080));
    let network_plan = sample_provision_network_plan(&spec, &id, "effect-bearing");
    let manifest = seed_partial_reservation(
        &backend,
        &spec,
        &id,
        &network_plan,
        ReservationCrashCut::ClaimPublished,
    );
    std::fs::create_dir_all(
        manifest
            .bundle_layout
            .config_path
            .parent()
            .expect("bundle config should have a parent"),
    )
    .expect("effect-bearing bundle directory should write");
    std::fs::write(&manifest.bundle_layout.config_path, b"untrusted-effect")
        .expect("effect-bearing bundle artifact should write");
    drop(backend);

    let before = snapshot_files(root.path());
    let reopened = ContainerSandboxBackend::new(config);
    let startup_error = reopened
        .startup_reconciliation_error
        .as_deref()
        .expect("effect-bearing pending manifest should fence startup");
    assert!(
        startup_error.contains("effect-bearing reservation-pending shape"),
        "{startup_error}"
    );
    let error = reopened
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect_err("startup fence must reject effects before retry");
    assert!(error.to_string().contains("refuses new durable work"));
    assert_eq!(snapshot_files(root.path()), before);
}

#[test]
fn corrupt_claim_only_manifest_is_fenced_on_fresh_backend_without_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let backend = ContainerSandboxBackend::new(config.clone());
    let id = SandboxId::new("reserve-corrupt");
    let spec = sample_spec_for_tenant("reserve-corrupt", "reserve-corrupt")
        .with_port_binding(SandboxPortBinding::tcp("api", 18_183, 8_080));
    let network_plan = sample_provision_network_plan(&spec, &id, "corrupt");
    let manifest = seed_partial_reservation(
        &backend,
        &spec,
        &id,
        &network_plan,
        ReservationCrashCut::ClaimPublished,
    );
    std::fs::write(&manifest.conmon_layout.manifest_path, b"{not-valid-json")
        .expect("corrupt manifest should write");
    drop(backend);

    let before = snapshot_files(root.path());
    let reopened = ContainerSandboxBackend::new(config);
    let startup_error = reopened
        .startup_reconciliation_error
        .as_deref()
        .expect("corrupt pending manifest should fence startup");
    assert!(
        startup_error.contains("unmatched artifact")
            && startup_error.contains(&manifest.conmon_layout.manifest_path.display().to_string()),
        "{startup_error}"
    );
    let error = reopened
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect_err("corrupt manifest must reject retry");
    assert!(
        error.to_string().contains("refuses new durable work"),
        "{error}"
    );
    assert_eq!(snapshot_files(root.path()), before);
}

#[test]
fn prepare_requires_reservation_and_stays_unattached() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-prepared-container");
    let execution_attempt_id = sample_execution_attempt_id(&id);
    let spec = sample_spec();
    let network_plan = sample_provision_network_plan(&spec, &id, "container-prepare");
    let missing = backend
        .prepare_provision_workload(&id, &execution_attempt_id)
        .expect_err("preparation cannot invent a reservation");
    assert!(matches!(missing, SandboxError::NotFound { .. }));

    backend
        .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), network_plan)
        .expect("reservation should succeed");
    let handle = backend
        .prepare_provision_workload(&id, &execution_attempt_id)
        .expect("prepare should materialize the reserved workload");
    assert_eq!(handle.id, id);
    let manifest = backend
        .read_manifest(&id)
        .expect("prepared manifest should be readable")
        .expect("prepared manifest should exist");
    assert!(manifest.network_config.is_some());
    assert!(manifest.launch_reservation_claim.is_some());
    assert!(manifest.provision_prepared);
    assert!(manifest.bundle_layout.config_path.is_file());
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "preparation must not create the workload network namespace"
    );
    assert_eq!(
        backend
            .inspect_provision_preparation(&id, &execution_attempt_id)
            .expect("preparation inspection should succeed")
            .expect("preparation should be observed")
            .id,
        id
    );
    assert_eq!(
        backend
            .prepare_provision_workload(&id, &execution_attempt_id)
            .expect("exact direct replay should adopt durable preparation")
            .id,
        id
    );
}

#[test]
fn crossed_attempt_preparation_inspection_preserves_durable_state() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("crossed-attempt-preparation-inspection");
    let execution_attempt_id = sample_execution_attempt_id(&id);
    let spec = sample_spec();
    let network_plan = sample_provision_network_plan(&spec, &id, "crossed-inspection");
    backend
        .reserve_provision_network(spec, id.clone(), execution_attempt_id.clone(), network_plan)
        .expect("reservation should succeed");
    backend
        .prepare_provision_workload(&id, &execution_attempt_id)
        .expect("preparation should succeed");
    let before = snapshot_files(root.path());
    let crossed_attempt_id =
        crate::SandboxExecutionAttemptId::new("crossed-attempt-preparation-inspection:other")
            .expect("crossed attempt should validate");

    let error = backend
        .inspect_provision_preparation(&id, &crossed_attempt_id)
        .expect_err("crossed inspection must fail closed");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert_eq!(
        snapshot_files(root.path()),
        before,
        "crossed inspection must not repair or rewrite durable state"
    );
}

#[test]
fn reservation_preserves_compiler_identities_without_binding_or_routability() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    let network_root = config.network_state_root.clone();
    let backend = ContainerSandboxBackend::new(config);
    let id = SandboxId::new("wex-exact-container-authority");
    let spec = sample_spec().with_port_bindings([
        SandboxPortBinding::tcp("exact", 18_123, 8_080),
        SandboxPortBinding::tcp("assigned", 0, 8_081),
    ]);
    let network_plan = sample_provision_network_plan(&spec, &id, "container-exact-authority");
    let expected_plan = network_plan.network_plan().clone();
    let expected_attachment = network_plan.attachment_id().clone();
    let expected_leases = network_plan.port_leases();
    let expected_dependency = network_plan.dependency_listeners()[0].clone();
    assert_ne!(
        expected_attachment,
        default_network_attachment_id(&id),
        "the proof must not accidentally exercise the legacy sandbox-derived identity"
    );

    backend
        .reserve_provision_network(
            spec.clone(),
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("exact reservation should succeed");
    let manifest = backend
        .read_manifest(&id)
        .expect("manifest should read")
        .expect("manifest should exist");
    let network = manifest
        .network_config
        .as_ref()
        .expect("exact network config should persist");
    assert_eq!(network.attachment_id, expected_attachment);
    assert_eq!(network.network_plan.as_ref(), Some(&expected_plan));
    assert_eq!(manifest.port_leases, expected_leases);
    assert_eq!(manifest.spec.port_bindings[0].host_port, 18_123);
    assert_eq!(manifest.spec.port_bindings[1].host_port, 0);
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "reservation must not make the attachment routable"
    );

    let authority =
        LocalPortLeaseAuthority::open(&network_root).expect("shared port authority should open");
    let records = authority
        .list_plan(expected_plan.plan_id())
        .expect("exact plan leases should inspect");
    assert_eq!(records.len(), expected_leases.len() + 1);
    for expected in &expected_leases {
        let record = records
            .iter()
            .find(|record| record.request().lease_id() == expected.lease_id())
            .expect("every compiler lease must remain present");
        assert_eq!(record.request(), expected);
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(record.binding().is_none());
        assert!(record.reservation_claim().is_some());
        match expected.binding().port() {
            nimbus_network::PortRequestMode::Exact(port) => {
                assert_eq!(record.reserved_port(), Some(*port));
            }
            nimbus_network::PortRequestMode::ProviderAssigned => {
                assert_eq!(record.reserved_port(), None);
            }
            nimbus_network::PortRequestMode::Range(_) => {
                panic!("compiled sandbox fixture must not contain a range request");
            }
        }
    }
    let pep_lease = &manifest
        .egress_proxy
        .as_ref()
        .expect("egress PEP reservation should persist")
        .port_lease;
    assert_eq!(
        pep_lease.lease_id(),
        &nimbus_network::PortLeaseId::for_listener(expected_dependency.listener_id())
    );
    assert_eq!(pep_lease.plan_id(), Some(expected_plan.plan_id()));
    assert_eq!(pep_lease.generation(), expected_plan.generation());
    assert_eq!(
        pep_lease.accounting(),
        nimbus_network::PortLeaseAccounting::HostInternal
    );
    let pep_record = records
        .iter()
        .find(|record| record.request().lease_id() == pep_lease.lease_id())
        .expect("the compiler dependency listener must own the PEP lease");
    assert_eq!(pep_record.phase(), PortLeasePhase::Reserved);
    assert!(pep_record.reserved_port().is_some());
    assert!(pep_record.binding().is_none());
    let claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("reservation claim should persist");
    assert!(
        backend
            .segment_allocator
            .inspect_attachment_reservation(&spec.tenant_id, &expected_attachment, claim)
            .expect("exact allocator reservation should inspect")
            .association()
            .is_some(),
        "segment allocation must use the compiler attachment identity"
    );
}

#[test]
fn plan_only_reserve_and_prepare_publish_only_the_exact_runner_handoff() {
    let fixture = PlanOnlyMachineProvisionFixture::prepared("prepare-handoff");
    let manifest = fixture.manifest();

    assert_eq!(manifest.start_mode, ContainerStartMode::PlanOnly);
    assert_eq!(
        manifest.lifecycle_coordinator,
        ContainerLifecycleCoordinator::PreparedServiceRunner
    );
    assert!(manifest.provision_prepared);
    assert!(manifest.bundle_layout.config_path.is_file());
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "reserve/prepare must not attach or route the workload"
    );
    let pointer = std::fs::read_to_string(
        manifest
            .bundle_layout
            .bundle_dir
            .join(RUNNER_MANIFEST_POINTER_FILE),
    )
    .expect("runner pointer should read");
    assert_eq!(
        PathBuf::from(pointer.trim()),
        manifest.conmon_layout.manifest_path
    );
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
    assert_published_lease_is_reserved(&fixture);

    fixture
        .backend
        .prepare_provision_workload(&fixture.id, &fixture.execution_attempt_id)
        .expect("exact preparation replay should adopt the same handoff");
    assert!(
        fixture
            .backend
            .activate_provision_workload(&fixture.id, &fixture.execution_attempt_id)
            .expect_err("PlanOnly activation must remain node-owned")
            .to_string()
            .contains("guest node provider")
    );
    assert!(
        fixture
            .backend
            .inspect_provision_workload_readiness(&fixture.id, &fixture.execution_attempt_id)
            .expect_err("PlanOnly readiness must remain node-owned")
            .to_string()
            .contains("guest node provider")
    );
}

#[test]
fn plan_only_private_attach_keeps_machine_publication_absent() {
    let mut fixture = PlanOnlyMachineProvisionFixture::prepared("private-attach");
    fixture.attach();

    assert!(matches!(
        fixture
            .backend
            .inspect_provision_network_attachment(&fixture.id, &fixture.execution_attempt_id)
            .expect("private attachment should inspect"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
    assert_published_lease_is_reserved(&fixture);
    assert!(
        fixture.published_reservation.is_some(),
        "the bind tripwire remaining live proves attachment created no host listener"
    );

    let forwarder = fixture.forwarder();
    let before = fixture
        .backend
        .inspect_provision_machine_ingress(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect("effect-free publication inspection should succeed");
    assert!(matches!(
        before,
        crate::SandboxProvisionPhaseObservation::Absent { .. }
    ));
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
}

#[test]
fn crossed_attempt_private_attach_fails_before_provider_effects() {
    let fixture = PlanOnlyMachineProvisionFixture::prepared("crossed-attempt-attach");
    let before = snapshot_files(fixture._root.path());
    let crossed_attempt_id =
        crate::SandboxExecutionAttemptId::new("crossed-attempt-private-attach:other")
            .expect("crossed attempt should validate");

    let error = fixture
        .backend
        .attach_provision_network_with_test_host(&fixture.id, &crossed_attempt_id)
        .expect_err("crossed attachment must fail before provider effects");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(fixture.pep_reservation.is_some());
    assert!(!fixture.manifest().network_layout.netns_path.exists());
    assert_eq!(
        snapshot_files(fixture._root.path()),
        before,
        "crossed attachment must preserve manifest, allocator, IPAM, and lease bytes"
    );
}

#[test]
fn machine_ingress_absence_requires_exact_never_effected_leases() {
    let mut fixture = PlanOnlyMachineProvisionFixture::prepared("ambiguous-bind-claim");
    fixture.attach();
    let manifest = fixture.manifest();
    let request = manifest
        .port_leases
        .first()
        .expect("machine fixture should retain one published lease")
        .clone();
    let plan_members = ContainerSandboxBackend::provision_port_plan_witness(&manifest);
    let reservation_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("machine fixture should retain its reservation claim");
    let coordinator = fixture
        .backend
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("machine fixture coordinator should open");
    let authority = coordinator
        .authority()
        .expect("machine fixture authority should open");
    let (_claim, lifetime) =
        crate::backends::oci::port_lease::claim_bind_plan_member_attempt_with_lifetime(
            authority,
            &plan_members,
            &request,
            crate::backends::oci::port_lease::OciPortProvider::MachinePortProxy,
            reservation_claim,
            nimbus_network::PortLeaseEffectScope::ProviderManaged,
        )
        .expect("ambiguous machine bind fixture should retain claim-before-effect evidence");

    let forwarder = fixture.forwarder();
    let observed = fixture
        .backend
        .inspect_provision_machine_ingress(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect("ambiguous machine publication should inspect");
    assert!(matches!(
        observed,
        crate::SandboxProvisionPhaseObservation::Ambiguous { .. }
    ));
    drop(lifetime);
}

#[test]
fn machine_publish_rejects_missing_prerequisites_without_listener_or_journal_effect() {
    let fixture = PlanOnlyMachineProvisionFixture::prepared("publish-too-early");
    let forwarder = fixture.forwarder();
    let error = fixture
        .backend
        .publish_provision_machine_ingress_with_test_provider(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect_err("publish before private attachment and PEP readiness must fail");

    assert!(
        error
            .to_string()
            .contains("authenticated private attachment")
    );
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
    assert_published_lease_is_reserved(&fixture);
}

#[test]
fn exact_machine_publish_is_first_routable_effect_and_replay_is_idempotent() {
    let mut fixture = PlanOnlyMachineProvisionFixture::prepared("publish-replay");
    fixture.attach();
    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
    let manifest_before_publish = fixture.manifest();
    let pep_lease = &manifest_before_publish
        .egress_proxy
        .as_ref()
        .expect("attached plan should retain its PEP")
        .port_lease;
    let authority = LocalPortLeaseAuthority::open(&fixture.backend.config.network_state_root)
        .expect("port authority should open");
    let pep_before_publish = authority
        .inspect(pep_lease.lease_id())
        .expect("PEP should inspect")
        .expect("PEP should remain durable");
    assert_eq!(pep_before_publish.phase(), PortLeasePhase::Active);

    fixture.publish();
    assert_eq!(fixture.publication_registry_len(), 1);
    assert!(fixture.evidence_path().is_file());
    let first_receipts = fixture
        .backend
        .exposed_machine_port_receipts(&fixture.id)
        .expect("exact exposed receipts should read");
    assert_eq!(first_receipts.len(), 1);
    assert_eq!(
        authority
            .inspect(pep_lease.lease_id())
            .expect("PEP should inspect after publication")
            .expect("PEP should remain durable after publication"),
        pep_before_publish,
        "the machine publication subset must not mutate the independently Active PEP member"
    );

    let forwarder = fixture.forwarder();
    fixture
        .backend
        .publish_provision_machine_ingress_with_test_provider(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect("exact publish replay should adopt the existing provider generation");
    assert_eq!(fixture.publication_registry_len(), 1);
    assert_eq!(
        fixture
            .backend
            .exposed_machine_port_receipts(&fixture.id)
            .expect("replayed receipts should read"),
        first_receipts
    );

    let evidence_before = std::fs::read(fixture.evidence_path()).expect("evidence should read");
    let inspection = fixture.spawn_exact_inspection();
    let observed = fixture
        .backend
        .inspect_provision_machine_ingress(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect("exact current provider evidence should inspect");
    assert!(matches!(
        observed,
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let request = inspection.join().expect("inspection server should join");
    assert!(request.contains("GET /services/forwarder/all "));
    assert!(!request.contains("/expose "));
    assert_eq!(fixture.publication_registry_len(), 1);
    assert_eq!(
        std::fs::read(fixture.evidence_path()).expect("evidence should reread"),
        evidence_before,
        "inspection must not rewrite or repair durable publication evidence"
    );
}

#[test]
fn fresh_backend_inspects_absent_then_recovers_planned_pep_and_machine_listener() {
    let mut fixture = PlanOnlyMachineProvisionFixture::prepared("planned-dead-owner-recovery");
    fixture.attach();
    drop(fixture.published_reservation.take());
    let manifest = fixture.manifest();
    let assigned_ip = fixture
        .backend
        .ready_machine_publication_address(&manifest)
        .expect("private attachment should expose one authenticated route address");
    let plan_members = ContainerSandboxBackend::provision_port_plan_witness(&manifest);
    let reservation_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("planned publication should retain launch authority");
    let error = fixture
        .backend
        .ensure_machine_port_proxies_running_with_publication(
            &fixture.id,
            &[assigned_ip],
            &manifest,
            crate::backends::oci::network::MachinePortPreparationReleaseAuthority::FreshPlannedLaunch {
                reservation_claim,
                plan_members: &plan_members,
            },
            || {
                Err(SandboxError::OperationFailed {
                    message: "simulated lost response before publication journal".to_owned(),
                })
            },
        )
        .expect_err("lost publication response should leave exact local authority for recovery");
    assert!(error.to_string().contains("simulated lost response"));
    let authority = LocalPortLeaseAuthority::open(&fixture.backend.config.network_state_root)
        .expect("port authority should open");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("machine lease should inspect")
            .expect("machine lease should remain durable")
            .phase(),
        PortLeasePhase::Active
    );
    let pep_request = manifest
        .egress_proxy
        .as_ref()
        .expect("planned attachment should retain its PEP")
        .port_lease
        .clone();
    assert_eq!(
        authority
            .inspect(pep_request.lease_id())
            .expect("PEP should inspect")
            .expect("PEP should remain durable")
            .phase(),
        PortLeasePhase::Active
    );

    let contender = ContainerSandboxBackend::new(fixture.backend.config.clone())
        .with_egress_pin_provider(Arc::new(
            crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
        ));
    let before_contender = snapshot_files(fixture._root.path());
    let contender_error = contender
        .ensure_machine_port_proxies_running_with_publication(
            &fixture.id,
            &[assigned_ip],
            &manifest,
            crate::backends::oci::network::MachinePortPreparationReleaseAuthority::FreshPlannedLaunch {
                reservation_claim,
                plan_members: &plan_members,
            },
            || panic!("a contender must not reach publication while the exact owner is live"),
        )
        .expect_err("live planned listener ownership must fence a competing adapter");
    assert!(
        contender_error
            .to_string()
            .contains("live process lifetime")
            || contender_error.to_string().contains("live process"),
        "live-owner error must retain an explicit ownership diagnostic: {contender_error}"
    );
    assert_eq!(
        snapshot_files(fixture._root.path()),
        before_contender,
        "live-owner rejection must not mutate durable attachment or lease authority"
    );
    drop(contender);

    fixture.reopen_backend_after_process_death();
    assert!(matches!(
        fixture
            .backend
            .inspect_provision_network_attachment(&fixture.id, &fixture.execution_attempt_id)
            .expect("fresh owner should inspect the retained private attachment"),
        crate::SandboxProvisionPhaseObservation::Absent { .. }
    ));
    fixture
        .backend
        .attach_provision_network_with_test_host(&fixture.id, &fixture.execution_attempt_id)
        .expect("fresh owner should recover the exact planned PEP without reattaching");
    let forwarder = fixture.forwarder();
    fixture
        .backend
        .publish_provision_machine_ingress_with_test_provider(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect("fresh owner should recover exact planned local listeners and publish once");
    assert!(fixture.evidence_path().is_file());
    let reopened_authority =
        LocalPortLeaseAuthority::open(&fixture.backend.config.network_state_root)
            .expect("reopened port authority should inspect");
    for request in &manifest.port_leases {
        assert_eq!(
            reopened_authority
                .inspect(request.lease_id())
                .expect("planned lease should inspect")
                .expect("planned lease should remain durable")
                .phase(),
            PortLeasePhase::Active
        );
    }
    assert_eq!(
        reopened_authority
            .inspect(pep_request.lease_id())
            .expect("PEP should inspect after recovery")
            .expect("PEP should remain durable")
            .phase(),
        PortLeasePhase::Active,
        "machine publication must not mutate the independently recovered PEP sibling"
    );
}

#[test]
fn crossed_plan_listener_and_forwarder_generations_fail_before_publication() {
    let mut fixture = PlanOnlyMachineProvisionFixture::prepared("crossed-fences");
    fixture.attach();
    let forwarder = fixture.forwarder();
    let crossed_plan = sample_provision_network_plan(
        &fixture.spec,
        &SandboxId::new("crossed-sandbox-incarnation"),
        "crossed-plan",
    );
    let plan_error = fixture
        .backend
        .publish_provision_machine_ingress_with_test_provider(
            &fixture.id,
            &fixture.execution_attempt_id,
            &crossed_plan,
            forwarder.provider_instance(),
            forwarder.provider_generation(),
        )
        .expect_err("crossed plan and sandbox identities must fail before effect");
    assert!(plan_error.to_string().contains("crossed its exact plan"));

    let stale_generation =
        NetworkResourceGeneration::new(forwarder.provider_generation().as_u64() + 1);
    let provider_error = fixture
        .backend
        .publish_provision_machine_ingress_with_test_provider(
            &fixture.id,
            &fixture.execution_attempt_id,
            &fixture.network_plan,
            forwarder.provider_instance(),
            stale_generation,
        )
        .expect_err("stale forwarder generation must fail before effect");
    assert!(
        provider_error
            .to_string()
            .contains("crossed the configured forwarder")
    );

    assert_eq!(fixture.publication_registry_len(), 0);
    assert!(!fixture.evidence_path().exists());
    assert_published_lease_is_reserved(&fixture);
    assert!(
        fixture.published_reservation.is_some(),
        "crossed commands must not consume the listener tripwire"
    );
}
