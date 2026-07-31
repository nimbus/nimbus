//! NNC5.3a machine-forwarded Container readiness proofs.

use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};

use crate::backends::oci::network::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, MachinePortProxyEntry,
    MachinePortProxyLeaseAuthority, MachinePortProxyLifetimeRegistry, OciAttachmentReadinessState,
    machine_port_proxy_routes,
};
use nimbus_network::{
    NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkResourcePhase,
};

const INSPECTION_TIMEOUT: Duration = Duration::from_secs(3);

struct CompleteMachineReadinessFixture {
    _temp_dir: TempDir,
    backend: ContainerSandboxBackend,
    manifest: ContainerSandboxManifest,
    forwarder_listener: Option<TcpListener>,
}

#[derive(Clone, Copy)]
enum InspectionReply {
    Exact,
    Unavailable,
}

impl CompleteMachineReadinessFixture {
    fn new(name: &str, with_binding: bool) -> Self {
        Self::with_binding_count(name, usize::from(with_binding))
    }

    fn with_binding_count(name: &str, binding_count: usize) -> Self {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let mut port_reservations = reserve_contiguous_loopback_ports(binding_count + 1);
        let endpoint_ports = port_reservations
            .iter()
            .take(binding_count)
            .map(|reservation| {
                reservation
                    .local_addr()
                    .expect("endpoint reservation should report its address")
                    .port()
            })
            .collect::<Vec<_>>();
        let pep_port = port_reservations
            .last()
            .expect("the PEP reservation should exist")
            .local_addr()
            .expect("PEP reservation should report its address")
            .port();
        let forwarder_listener =
            TcpListener::bind("127.0.0.1:0").expect("forwarder fixture should bind");
        let forwarder_port = forwarder_listener
            .local_addr()
            .expect("forwarder fixture should report its address")
            .port();
        let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
        config.node_network_supernet = "127.0.0.0/24".to_owned();
        config.published_port_range = port_reservations
            .first()
            .expect("the published range should have a start")
            .local_addr()
            .expect("range start should report its address")
            .port()..=pep_port;
        config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
        let pin = Arc::new(FixedOciEgressPinProvider::ready());
        let backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(pin.clone());
        let sandbox_id = SandboxId::new(format!("nnc53a-{name}"));
        let mut spec = sample_spec_for_tenant(&format!("nnc53a-{name}"), name);
        for (index, endpoint_port) in endpoint_ports.into_iter().enumerate() {
            spec = spec.with_port_binding(SandboxPortBinding::tcp(
                format!("published-api-{index}"),
                endpoint_port,
                8080 + u16::try_from(index).expect("fixture binding count should fit u16"),
            ));
        }
        let manifest = backend
            .plan_start_with_id(&spec, &sandbox_id, None, None)
            .expect("machine Execute planning should reserve attachment and listener authority")
            .manifest;
        let launch_claim = manifest
            .launch_reservation_claim
            .clone()
            .expect("Execute planning should retain its reservation claim");
        backend
            .segment_allocator
            .adopt_reserved_attachment(
                &manifest.spec.tenant_id,
                &default_network_attachment_id(&manifest.handle.id),
                &launch_claim,
            )
            .expect("fixture should adopt the exact attachment reservation");
        let network_config = manifest
            .require_network_config()
            .expect("Execute manifest should retain network config")
            .clone();
        let ports = backend
            .port_lease_coordinator_for_manifest(&manifest)
            .expect("manifest should select its exact port authority");
        let hostname = hostname_for(&manifest.spec);

        // The deterministic attachment provider and the machine/PEP adapters
        // now own these exact listeners.
        port_reservations.clear();
        backend
            .attachment_adapter(
                &manifest,
                &network_config,
                &hostname,
                manifest.runner_config.machine_port_forwarder.as_ref(),
            )
            .attach_with_test_host(
                &backend.attachment_lifecycle(&ports),
                AttachmentAttachAuthority::FreshLaunch(&launch_claim),
                |assigned_ips| {
                    pin.apply(
                        &manifest.network_layout,
                        manifest
                            .egress_proxy
                            .as_ref()
                            .expect("Execute manifest should retain its PEP assignment"),
                    )?;
                    backend.ensure_machine_port_proxies_running(
                        &manifest.handle.id,
                        assigned_ips,
                        &manifest,
                    )?;
                    backend.persist_exposed_machine_port_receipts(
                        &manifest,
                        exposed_receipts(&manifest),
                    )
                },
            )
            .expect("fixture should realize the complete machine attachment");
        backend
            .ensure_egress_proxy_running_with_release_authority(
                &manifest,
                PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
            )
            .expect("fixture should start the exact desired PEP");

        Self {
            _temp_dir: temp_dir,
            backend,
            manifest,
            forwarder_listener: Some(forwarder_listener),
        }
    }

    fn spawn_inspector(
        &mut self,
        replies: Vec<InspectionReply>,
    ) -> thread::JoinHandle<Vec<String>> {
        let listener = self
            .forwarder_listener
            .take()
            .expect("each fixture should start at most one inspector");
        let bindings = self.manifest.spec.port_bindings.clone();
        thread::spawn(move || {
            let mut requests = Vec::with_capacity(replies.len());
            for reply in replies {
                let (mut stream, _) = listener.accept().expect("inspection should connect");
                requests.push(read_complete_request(&mut stream));
                let response = match reply {
                    InspectionReply::Exact => exact_inspection_response(&bindings),
                    InspectionReply::Unavailable => {
                        http_response("503 Service Unavailable", b"provider unavailable")
                    }
                };
                stream
                    .set_write_timeout(Some(INSPECTION_TIMEOUT))
                    .expect("provider response timeout should configure");
                stream
                    .write_all(&response)
                    .expect("provider response should write");
                stream
                    .shutdown(Shutdown::Write)
                    .expect("provider response EOF should be explicit");
            }
            requests
        })
    }

    fn assert_no_provider_io(&mut self) {
        let listener = self
            .forwarder_listener
            .as_ref()
            .expect("unused provider listener should remain owned");
        listener
            .set_nonblocking(true)
            .expect("provider tripwire should become nonblocking");
        assert!(
            matches!(
                listener.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ),
            "readiness rejection must occur before provider I/O"
        );
    }
}

fn exposed_receipts(manifest: &ContainerSandboxManifest) -> Vec<MachinePortForwardReceipt> {
    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("machine fixture should retain its provider authority");
    manifest
        .spec
        .port_bindings
        .iter()
        .map(|binding| MachinePortForwardReceipt {
            outcome: MachinePortForwardOutcome::Exposed,
            tenant_id: manifest.spec.tenant_id.clone(),
            sandbox_id: manifest.handle.id.clone(),
            binding: binding.clone(),
            provider_instance: forwarder.provider_instance().clone(),
            provider_generation: forwarder.provider_generation(),
        })
        .collect()
}

fn reserve_contiguous_loopback_ports(count: usize) -> Vec<TcpListener> {
    assert!(count > 0, "at least the PEP listener must be reserved");
    for _ in 0..256 {
        let first = TcpListener::bind("127.0.0.1:0").expect("first port fixture should bind");
        let first_port = first
            .local_addr()
            .expect("first port fixture should report its address")
            .port();
        let Ok(last_offset) = u16::try_from(count - 1) else {
            panic!("fixture port count should fit u16");
        };
        let Some(last_port) = first_port.checked_add(last_offset) else {
            continue;
        };
        let mut reservations = vec![first];
        for port in first_port + 1..=last_port {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => reservations.push(listener),
                Err(_) => break,
            }
        }
        if reservations.len() == count {
            return reservations;
        }
    }
    panic!("{count} contiguous loopback ports should become available");
}

fn exact_inspection_response(bindings: &[SandboxPortBinding]) -> Vec<u8> {
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
    .expect("native forwarding list should encode");
    http_response("200 OK", &body)
}

fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.0 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn read_complete_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(INSPECTION_TIMEOUT))
        .expect("provider request timeout should configure");
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("request should be readable");
        assert_ne!(read, 0, "request must not close before its complete body");
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .expect("request headers should be UTF-8");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .map(|value| value.parse::<usize>().expect("content length should parse"))
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_length);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return String::from_utf8(request).expect("request should be UTF-8");
        }
    }
}

fn snapshot_regular_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .unwrap_or_else(|error| {
                panic!("snapshot directory {} should read: {error}", path.display())
            })
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap_or_else(|error| {
                panic!(
                    "snapshot directory {} should enumerate: {error}",
                    path.display()
                )
            });
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type().unwrap_or_else(|error| {
                panic!("snapshot entry {} should inspect: {error}", path.display())
            });
            if file_type.is_dir() {
                collect(root, &path, files);
            } else if file_type.is_file() {
                files.insert(
                    path.strip_prefix(root)
                        .expect("snapshot entry should remain below its root")
                        .to_path_buf(),
                    fs::read(&path).unwrap_or_else(|error| {
                        panic!("snapshot file {} should read: {error}", path.display())
                    }),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    if root.exists() {
        collect(root, root, &mut files);
    }
    files
}

#[derive(Debug, PartialEq, Eq)]
struct MachineRegistrySnapshot {
    entry_count: usize,
    bindings: Vec<SandboxPortBinding>,
    leases: Vec<nimbus_network::PortLeaseRequest>,
    routes: Vec<crate::backends::oci::network::MachinePortProxyRoute>,
    worker_liveness: Vec<bool>,
    live_claims: String,
    live_lifetimes: String,
}

fn snapshot_machine_registry(fixture: &CompleteMachineReadinessFixture) -> MachineRegistrySnapshot {
    let registry = fixture
        .backend
        .machine_port_proxies
        .lock()
        .expect("machine registry should lock for a read-only snapshot");
    let key = (
        fixture.manifest.spec.tenant_id.clone(),
        fixture.manifest.handle.id.clone(),
    );
    let MachinePortProxyEntry::Running(registration) = registry
        .get(&key)
        .expect("complete fixture should retain its exact machine registration")
    else {
        panic!("complete fixture registration should be running");
    };
    let Some(MachinePortProxyLeaseAuthority::Live(live)) = registration.lease_authority.as_ref()
    else {
        panic!("complete fixture should retain exact live listener authority");
    };
    MachineRegistrySnapshot {
        entry_count: registry.len(),
        bindings: registration.port_bindings.clone(),
        leases: registration.port_leases.clone(),
        routes: registration.routes.clone(),
        worker_liveness: registration
            .proxies
            .iter()
            .map(|proxy| proxy.provider_is_running())
            .collect(),
        live_claims: format!("{:?}", live.claims()),
        live_lifetimes: format!("{:?}", live.lifetimes()),
    }
}

/// Machine mode currently bypasses the complete attachment collector at the
/// final pre-spawn gate. No receipt, current provider observation, local route,
/// worker, or listener lifetime is present in this fixture.
#[test]
fn nnc5_3a_machine_pre_spawn_rejects_missing_complete_readiness() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("nnc53a-pre-spawn-missing-readiness"),
            None,
            None,
        )
        .expect("machine-mode plan should lower")
        .manifest;

    let readiness = backend.require_complete_attachment_readiness(&manifest);

    assert!(
        readiness.is_err(),
        "NNC5.3a: machine pre-spawn must reject missing common attachment, durable receipt, \
         current provider, route, worker, and listener-lifetime evidence"
    );
}

/// A live runtime plus an exact PEP is not complete machine network
/// readiness. The current status branch accepts that pair without inspecting
/// the attachment or forwarding providers.
#[test]
fn nnc5_3a_machine_live_status_rejects_pep_only_readiness() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP port fixture should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP port fixture should report its address")
        .port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("nnc53a-live-pep-only"),
            None,
            None,
        )
        .expect("machine-mode plan should lower")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("execute plan should retain reservation authority")
        .clone();
    drop(pep_reservation);
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("exact PEP should isolate the missing machine attachment evidence");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            manifest.handle.id
        ),
    ]);
    let _ = std::fs::remove_file(&manifest.conmon_layout.exit_status_file);
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);

    let status = backend
        .detect_runtime_status(&manifest)
        .expect("live runtime status should remain inspectable");

    assert_eq!(
        status,
        SandboxStatus::NotReady,
        "NNC5.3a: application plus PEP cannot report Ready without the common attachment, \
         durable receipt, current provider, route, worker, and listener-lifetime evidence"
    );
}

#[test]
fn complete_machine_forwarded_readiness_emits_one_portable_read_only_observation() {
    let mut fixture = CompleteMachineReadinessFixture::new("complete", true);
    let before = snapshot_regular_files(&fixture.backend.config.workload_state_root);
    let registry_before = snapshot_machine_registry(&fixture);
    let server = fixture.spawn_inspector(vec![InspectionReply::Exact]);

    let readiness = fixture
        .backend
        .complete_attachment_readiness(
            &fixture.manifest,
            fixture
                .backend
                .authenticated_egress_readiness(&fixture.manifest)
                .expect("PEP readiness should inspect"),
        )
        .expect("complete machine readiness should inspect");
    let requests = server.join().expect("inspection server should join");
    let after = snapshot_regular_files(&fixture.backend.config.workload_state_root);
    let registry_after = snapshot_machine_registry(&fixture);

    let OciAttachmentReadinessState::Ready(evidence) = readiness else {
        panic!("exact desired, durable, and observed evidence should be Ready");
    };
    let observation = evidence.observation();
    assert_eq!(observation.observed_phase(), NetworkResourcePhase::Active);
    assert!(observation.provider_id().is_some());
    assert_eq!(
        observation.conditions(),
        &[NetworkCondition::new(
            NetworkConditionKind::Ready,
            NetworkConditionState::True,
        )]
    );
    assert_eq!(evidence.assigned_ips(), &[Ipv4Addr::new(127, 0, 0, 2)]);
    assert_eq!(
        before, after,
        "readiness inspection must preserve desired and durable artifacts byte-for-byte"
    );
    assert_eq!(
        registry_before, registry_after,
        "readiness inspection must preserve process-local routes, workers, and listener lifetimes"
    );
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("GET /services/forwarder/all "));
    assert!(!requests[0].contains("/expose "));
}

#[test]
fn durable_exposed_receipt_without_current_provider_observation_is_not_ready() {
    let mut fixture = CompleteMachineReadinessFixture::new("historical-only", true);
    drop(
        fixture
            .forwarder_listener
            .take()
            .expect("provider listener should be removable"),
    );
    let before = snapshot_regular_files(&fixture.backend.config.workload_state_root);
    let registry_before = snapshot_machine_registry(&fixture);

    let error = fixture
        .backend
        .require_complete_attachment_readiness(&fixture.manifest)
        .expect_err("historical Exposed receipts cannot replace a current provider observation");
    let after = snapshot_regular_files(&fixture.backend.config.workload_state_root);
    let registry_after = snapshot_machine_registry(&fixture);

    assert!(
        error.to_string().contains("MachinePublicationRejected"),
        "the failure should name the missing machine publication evidence: {error}"
    );
    assert_eq!(
        before, after,
        "ambiguous current-provider inspection must preserve durable authority byte-for-byte"
    );
    assert_eq!(
        registry_before, registry_after,
        "ambiguous inspection must preserve process-local routes, workers, and listener lifetimes"
    );
    assert_eq!(
        fixture
            .backend
            .exposed_machine_port_receipts(&fixture.manifest.handle.id)
            .expect("historical evidence should remain readable"),
        exposed_receipts(&fixture.manifest)
    );
}

#[test]
fn missing_persisted_forwarder_authority_cannot_select_an_alternate_publication_mode() {
    let mut fixture = CompleteMachineReadinessFixture::new("missing-forwarder", true);
    let mut crossed_manifest = fixture.manifest.clone();
    crossed_manifest.runner_config.machine_port_forwarder = None;

    fixture
        .backend
        .require_complete_attachment_readiness(&crossed_manifest)
        .expect_err(
            "removing persisted machine authority must not reinterpret its attachment as host-managed",
        );
    fixture.assert_no_provider_io();
}

#[test]
fn empty_machine_publication_with_missing_authority_cannot_select_host_managed_mode() {
    let mut fixture = CompleteMachineReadinessFixture::new("empty-missing-forwarder", false);
    let mut crossed_manifest = fixture.manifest.clone();
    crossed_manifest.runner_config.machine_port_forwarder = None;

    fixture
        .backend
        .require_complete_attachment_readiness(&crossed_manifest)
        .expect_err(
            "an empty machine publication with missing authority must not become host-managed",
        );
    fixture.assert_no_provider_io();
}

#[test]
fn exact_empty_machine_publication_needs_no_provider_io_and_publishes_no_endpoint() {
    let mut fixture = CompleteMachineReadinessFixture::new("empty", false);

    fixture
        .backend
        .require_complete_attachment_readiness(&fixture.manifest)
        .expect("exact empty publication should satisfy its machine facet");
    fixture.assert_no_provider_io();
    assert!(
        fixture.manifest.handle.published_endpoints.is_empty(),
        "an empty forwarding set must not fabricate a published endpoint"
    );
}

#[test]
fn registry_route_lifetime_and_worker_substitutions_fail_before_provider_io() {
    for corruption in [
        RegistryCorruption::Missing,
        RegistryCorruption::Stopping,
        RegistryCorruption::PublicationAbsent,
        RegistryCorruption::WrongIdentity,
        RegistryCorruption::WrongBinding,
        RegistryCorruption::WrongLease,
        RegistryCorruption::PartialRoute,
        RegistryCorruption::DuplicateRoute,
        RegistryCorruption::ReorderedBindings,
        RegistryCorruption::StaleRoute,
        RegistryCorruption::WrongExternalAddress,
        RegistryCorruption::MissingLifetime,
        RegistryCorruption::MissingWorker,
        RegistryCorruption::ExtraWorker,
        RegistryCorruption::DeadWorker,
    ] {
        assert_registry_corruption_not_ready(corruption);
    }
}

#[derive(Clone, Copy, Debug)]
enum RegistryCorruption {
    Missing,
    Stopping,
    PublicationAbsent,
    WrongIdentity,
    WrongBinding,
    WrongLease,
    PartialRoute,
    DuplicateRoute,
    ReorderedBindings,
    StaleRoute,
    WrongExternalAddress,
    MissingLifetime,
    MissingWorker,
    ExtraWorker,
    DeadWorker,
}

fn assert_registry_corruption_not_ready(corruption: RegistryCorruption) {
    let binding_count =
        usize::from(matches!(corruption, RegistryCorruption::ReorderedBindings)) + 1;
    let mut fixture = CompleteMachineReadinessFixture::with_binding_count(
        &format!("registry-{corruption:?}"),
        binding_count,
    );
    let key = (
        fixture.manifest.spec.tenant_id.clone(),
        fixture.manifest.handle.id.clone(),
    );
    let mut removed = None;
    let mut original_routes = None;
    let mut original_lifetime = None;
    let mut held_worker = None;
    if matches!(corruption, RegistryCorruption::PublicationAbsent) {
        fixture
            .backend
            .converge_absent_machine_port_publication_for_test(&fixture.manifest)
            .expect("publication-absence substitution should persist durably");
    }
    let _stopping_cleanup = if matches!(corruption, RegistryCorruption::Stopping) {
        fixture
            .backend
            .begin_machine_port_proxy_release(
                &fixture.manifest.spec.tenant_id,
                &fixture.manifest.handle.id,
                &fixture.manifest.spec.port_bindings,
                &fixture.manifest.port_leases,
            )
            .expect("fixture cleanup should begin")
    } else {
        None
    };
    if !matches!(
        corruption,
        RegistryCorruption::Stopping | RegistryCorruption::PublicationAbsent
    ) {
        let mut registry = fixture
            .backend
            .machine_port_proxies
            .lock()
            .expect("machine registry should lock");
        match corruption {
            RegistryCorruption::Missing => {
                removed = registry.remove(&key);
            }
            RegistryCorruption::Stopping => unreachable!("handled before the registry mutation"),
            RegistryCorruption::PublicationAbsent => unreachable!("handled as durable evidence"),
            RegistryCorruption::WrongIdentity => {
                let entry = registry
                    .remove(&key)
                    .expect("fixture registration should exist");
                registry.insert(
                    (
                        nimbus_core::TenantId::new("nnc53a-crossed-tenant")
                            .expect("crossed tenant should validate"),
                        SandboxId::new("nnc53a-crossed-sandbox"),
                    ),
                    entry,
                );
            }
            RegistryCorruption::WrongBinding => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration.port_bindings[0].name = "crossed-binding".to_owned();
            }
            RegistryCorruption::WrongLease => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration.port_leases[0] = fixture
                    .manifest
                    .egress_proxy
                    .as_ref()
                    .expect("fixture should retain its PEP")
                    .port_lease
                    .clone();
            }
            RegistryCorruption::PartialRoute => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration.routes.pop();
            }
            RegistryCorruption::DuplicateRoute => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration.routes.push(registration.routes[0]);
            }
            RegistryCorruption::ReorderedBindings => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration.port_bindings.swap(0, 1);
            }
            RegistryCorruption::StaleRoute => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                original_routes = Some(std::mem::replace(
                    &mut registration.routes,
                    machine_port_proxy_routes(
                        &[Ipv4Addr::new(127, 0, 0, 3)],
                        &fixture.manifest.spec.port_bindings,
                    )
                    .expect("stale route fixture should normalize"),
                ));
            }
            RegistryCorruption::WrongExternalAddress => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                let mut crossed_bindings = fixture.manifest.spec.port_bindings.clone();
                crossed_bindings[0].host_address = Ipv4Addr::new(127, 0, 0, 3).into();
                original_routes = Some(std::mem::replace(
                    &mut registration.routes,
                    machine_port_proxy_routes(&[Ipv4Addr::new(127, 0, 0, 2)], &crossed_bindings)
                        .expect("crossed external address should normalize"),
                ));
            }
            RegistryCorruption::MissingLifetime => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                original_lifetime = registration.lease_authority.take();
            }
            RegistryCorruption::MissingWorker => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                held_worker = registration.proxies.pop();
            }
            RegistryCorruption::ExtraWorker => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                registration
                    .proxies
                    .push(panicking_machine_port_proxy_for_test(
                        fixture.manifest.spec.port_bindings[0].host_socket_addr(),
                    ));
            }
            RegistryCorruption::DeadWorker => {
                let MachinePortProxyEntry::Running(registration) = registry
                    .get_mut(&key)
                    .expect("fixture registration should exist")
                else {
                    panic!("fixture registration should be running");
                };
                let replacement = panicking_machine_port_proxy_for_test(
                    fixture.manifest.spec.port_bindings[0].host_socket_addr(),
                );
                let mut original = std::mem::replace(&mut registration.proxies[0], replacement);
                original
                    .shutdown()
                    .expect("live worker should stop before dead-worker substitution");
            }
        }
    }

    let error = fixture
        .backend
        .require_complete_attachment_readiness(&fixture.manifest)
        .expect_err("substituted registry evidence must fail closed");
    assert!(
        error.to_string().contains("MachinePublicationRejected"),
        "registry substitution should surface as machine publication NotReady: {error}"
    );
    fixture.assert_no_provider_io();

    let mut registry = fixture
        .backend
        .machine_port_proxies
        .lock()
        .expect("machine registry should relock");
    if let Some(entry) = removed {
        registry.insert(key.clone(), entry);
    }
    if let Some(routes) = original_routes {
        let MachinePortProxyEntry::Running(registration) =
            registry.get_mut(&key).expect("registration should remain")
        else {
            panic!("registration should remain running");
        };
        registration.routes = routes;
    }
    if let Some(lifetime) = original_lifetime {
        let MachinePortProxyEntry::Running(registration) =
            registry.get_mut(&key).expect("registration should remain")
        else {
            panic!("registration should remain running");
        };
        registration.lease_authority = Some(lifetime);
    }
    drop(held_worker);
}

#[test]
fn fresh_process_without_live_registry_rejects_durable_active_state_before_provider_io() {
    let mut fixture = CompleteMachineReadinessFixture::new("fresh-process", true);
    let mut reopened = fixture.backend.clone();
    reopened.machine_port_proxies = MachinePortProxyLifetimeRegistry::default();

    let error = reopened
        .require_complete_attachment_readiness(&fixture.manifest)
        .expect_err("a fresh process cannot promote durable Active state without live lifetimes");

    assert!(
        error
            .to_string()
            .contains("no live process-local registration"),
        "fresh-process rejection should identify the absent process lifetime: {error}"
    );
    fixture.assert_no_provider_io();
}

#[test]
fn live_status_withdraws_and_restores_machine_endpoints_with_current_evidence() {
    let mut fixture = CompleteMachineReadinessFixture::new("status-cycle", true);
    fixture.manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            fixture.manifest.handle.id
        ),
    ]);
    let _ = fs::remove_file(&fixture.manifest.conmon_layout.exit_status_file);
    synchronize_handle_status(&mut fixture.manifest, SandboxStatus::Ready);
    let server = fixture.spawn_inspector(vec![
        InspectionReply::Exact,
        InspectionReply::Unavailable,
        InspectionReply::Exact,
    ]);

    let ready = fixture
        .backend
        .detect_runtime_status(&fixture.manifest)
        .expect("exact machine readiness should inspect");
    assert_eq!(ready, SandboxStatus::Ready);
    synchronize_handle_status(&mut fixture.manifest, ready);
    assert_eq!(fixture.manifest.handle.published_endpoints.len(), 1);

    let withdrawn = fixture
        .backend
        .detect_runtime_status(&fixture.manifest)
        .expect("provider-unknown machine readiness should remain inspectable");
    assert_eq!(withdrawn, SandboxStatus::NotReady);
    synchronize_handle_status(&mut fixture.manifest, withdrawn);
    assert!(
        fixture.manifest.handle.published_endpoints.is_empty(),
        "lost current forwarding evidence must withdraw every endpoint"
    );

    let restored = fixture
        .backend
        .detect_runtime_status(&fixture.manifest)
        .expect("restored exact machine readiness should inspect");
    assert_eq!(restored, SandboxStatus::Ready);
    synchronize_handle_status(&mut fixture.manifest, restored);
    assert_eq!(fixture.manifest.handle.published_endpoints.len(), 1);

    let requests = server.join().expect("inspection server should join");
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.contains("/all") && !request.contains("/expose ")),
        "status retries must remain read-only: {requests:?}"
    );
}
