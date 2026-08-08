//! Shared Linux live-provider fixtures for the phased sandbox provision seam.
//!
//! These helpers are test-only stand-ins for compute's orchestration and the
//! external ingress owner. They deliberately call every provider phase and do
//! not restore a sandbox-owned coarse lifecycle entry point.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use nimbus_network::{
    ListenerId, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkResourceGeneration, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence, PortLeaseId,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::backends::krun::KrunSandboxBackend;
use nimbus_sandbox::{
    SandboxExecutionAttemptId, SandboxHandle, SandboxId, SandboxProvisionDependencyListener,
    SandboxProvisionListener, SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation,
    SandboxSpec, sandbox_network_plan_requirements,
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct ProvisionedSandbox {
    pub(crate) handle: SandboxHandle,
    pub(crate) ingress: TestIngressSet,
}

#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn provision_container(
    backend: &ContainerSandboxBackend,
    workload_state_root: &Path,
    spec: SandboxSpec,
    install_ingress: bool,
) -> nimbus_sandbox::Result<ProvisionedSandbox> {
    let id = fixture_id("container", spec.display_name());
    let plan = compiled_network_plan(&spec, &id);
    let attempt = fixture_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    backend.prepare_provision_workload(&id, &attempt)?;
    require_succeeded(
        "container attachment",
        backend.attach_provision_network(&id, &attempt)?,
    )?;
    require_succeeded(
        "container activation prerequisite",
        backend.inspect_provision_activation_prerequisites(&id, &attempt)?,
    )?;
    require_succeeded(
        "container activation",
        backend.activate_provision_workload(&id, &attempt)?,
    )?;
    require_readiness_observation(
        "container readiness",
        backend.inspect_provision_workload_readiness(&id, &attempt)?,
    )?;
    let manifest = read_manifest(workload_state_root, &id)?;
    let (handle, ingress) = finish_fixture(manifest, install_ingress)?;
    Ok(ProvisionedSandbox { handle, ingress })
}

#[allow(dead_code)] // Included separately by Container-only and krun-only integration targets.
pub(crate) fn provision_krun(
    backend: &KrunSandboxBackend,
    workload_state_root: &Path,
    spec: SandboxSpec,
    install_ingress: bool,
) -> nimbus_sandbox::Result<ProvisionedSandbox> {
    let id = fixture_id("krun", spec.display_name());
    let plan = compiled_network_plan(&spec, &id);
    let attempt = fixture_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    backend.prepare_provision_workload(&id, &attempt)?;
    require_succeeded(
        "krun attachment",
        backend.attach_provision_network(&id, &attempt)?,
    )?;
    require_succeeded(
        "krun activation prerequisite",
        backend.inspect_provision_activation_prerequisites(&id, &attempt)?,
    )?;
    require_succeeded(
        "krun activation",
        backend.activate_provision_workload(&id, &attempt)?,
    )?;
    require_readiness_observation(
        "krun readiness",
        backend.inspect_provision_workload_readiness(&id, &attempt)?,
    )?;
    let manifest = read_manifest(workload_state_root, &id)?;
    let (handle, ingress) = finish_fixture(manifest, install_ingress)?;
    Ok(ProvisionedSandbox { handle, ingress })
}

fn fixture_id(provider: &str, display_name: &str) -> SandboxId {
    let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
    let label = display_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    SandboxId::new(format!("phase-{provider}-{label}-{sequence}"))
}

fn fixture_attempt_id(sandbox_id: &SandboxId) -> SandboxExecutionAttemptId {
    SandboxExecutionAttemptId::new(format!("linux-smoke:{sandbox_id}"))
        .expect("Linux smoke execution attempt should validate")
}

fn compiled_network_plan(spec: &SandboxSpec, id: &SandboxId) -> SandboxProvisionNetworkPlan {
    let incarnation = format!("linux-smoke:{}", id.as_str());
    let generation = NetworkResourceGeneration::new(1);
    let requirements = sandbox_network_plan_requirements(spec.backend);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&spec.tenant_id, &incarnation),
        generation,
        NetworkPlanContentDigest::sha256(format!("linux-smoke:{incarnation}")),
        requirements.capability_requirements().clone(),
    );
    let plan_id = plan.plan_id().clone();
    let listeners = spec.port_bindings.iter().map(|binding| {
        let listener_id =
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, &binding.name);
        let request = PortLeaseRequest::new(
            PortLeaseId::for_listener(&listener_id),
            listener_id.clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(binding.host_address),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                bind_target(binding.host_address),
                exposure(binding.host_address),
                NonZeroU16::new(binding.host_port)
                    .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact),
            ),
        )
        .with_plan_id(plan_id.clone());
        SandboxProvisionListener::new(listener_id, binding.clone(), request)
    });
    SandboxProvisionNetworkPlan::new(
        plan,
        spec.tenant_id.clone(),
        generation,
        NetworkAttachmentId::for_workload_attachment(&incarnation, "primary"),
        listeners,
        [SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, "egress-pep"),
            "egress-pep",
            requirements.pep_provider_id().clone(),
        )],
    )
    .expect("Linux smoke compiled network plan should validate")
}

fn bind_target(address: IpAddr) -> PortBindTarget {
    match address {
        IpAddr::V4(address) if address == Ipv4Addr::UNSPECIFIED => PortBindTarget::ipv4_wildcard(),
        IpAddr::V4(address) => PortBindTarget::ipv4_specific(address),
        IpAddr::V6(address) if address == Ipv6Addr::UNSPECIFIED => {
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown)
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .expect("Linux smoke fixture never uses IPv4-mapped IPv6"),
    }
}

fn exposure(address: IpAddr) -> PortExposure {
    match address {
        address if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        _ => PortExposure::Public,
    }
}

fn require_succeeded(
    phase: &str,
    observation: SandboxProvisionPhaseObservation,
) -> nimbus_sandbox::Result<()> {
    if matches!(
        observation,
        SandboxProvisionPhaseObservation::Succeeded { .. }
    ) {
        return Ok(());
    }
    Err(nimbus_sandbox::SandboxError::OperationFailed {
        message: format!("{phase} did not publish exact success: {observation:?}"),
    })
}

fn require_readiness_observation(
    phase: &str,
    observation: SandboxProvisionPhaseObservation,
) -> nimbus_sandbox::Result<()> {
    if matches!(
        observation,
        SandboxProvisionPhaseObservation::Succeeded { .. }
            | SandboxProvisionPhaseObservation::InProgress { .. }
    ) {
        return Ok(());
    }
    Err(nimbus_sandbox::SandboxError::OperationFailed {
        message: format!("{phase} returned non-progress evidence: {observation:?}"),
    })
}

#[derive(serde::Deserialize)]
struct ProviderManifestProjection {
    handle: SandboxHandle,
    spec: SandboxSpec,
    network_layout: ProviderNetworkLayoutProjection,
}

#[derive(serde::Deserialize)]
struct ProviderNetworkLayoutProjection {
    status_path: std::path::PathBuf,
}

#[derive(serde::Deserialize)]
struct ProviderStatusProjection {
    assigned_ips: Vec<Ipv4Addr>,
}

fn read_manifest(
    workload_state_root: &Path,
    id: &SandboxId,
) -> nimbus_sandbox::Result<ProviderManifestProjection> {
    let mut matches = std::fs::read_dir(workload_state_root.join("tenants"))
        .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate Linux smoke tenants under {}: {error}",
                workload_state_root.display()
            ),
        })?
        .filter_map(Result::ok)
        .map(|tenant| {
            tenant
                .path()
                .join("sandboxes")
                .join(id.as_str())
                .join("state")
                .join("containers")
                .join(id.as_str())
                .join("manifest.json")
        })
        .filter(|path| path.is_file());
    let path = matches
        .next()
        .ok_or_else(|| nimbus_sandbox::SandboxError::NotFound {
            sandbox_id: id.as_str().to_owned(),
        })?;
    if matches.next().is_some() {
        return Err(nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "Linux smoke sandbox {} is not tenant-qualified uniquely under {}",
                id,
                workload_state_root.display()
            ),
        });
    }
    serde_json::from_slice(&std::fs::read(&path).map_err(|error| {
        nimbus_sandbox::SandboxError::OperationFailed {
            message: format!(
                "failed to read Linux smoke manifest {}: {error}",
                path.display()
            ),
        }
    })?)
    .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
        message: format!(
            "failed to parse Linux smoke manifest {}: {error}",
            path.display()
        ),
    })
}

fn finish_fixture(
    manifest: ProviderManifestProjection,
    install_ingress: bool,
) -> nimbus_sandbox::Result<(SandboxHandle, TestIngressSet)> {
    let ingress = if install_ingress {
        let status: ProviderStatusProjection = serde_json::from_slice(
            &std::fs::read(&manifest.network_layout.status_path).map_err(|error| {
                nimbus_sandbox::SandboxError::OperationFailed {
                    message: format!(
                        "failed to read Linux smoke provider status {}: {error}",
                        manifest.network_layout.status_path.display()
                    ),
                }
            })?,
        )
        .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
            message: format!("failed to parse Linux smoke provider status: {error}"),
        })?;
        let assigned_ip = status.assigned_ips.first().copied().ok_or_else(|| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: "Linux smoke provider status has no assigned private address".to_owned(),
            }
        })?;
        TestIngressSet::bind(&manifest.spec, assigned_ip)?
    } else {
        TestIngressSet::default()
    };
    Ok((manifest.handle, ingress))
}

#[derive(Default)]
pub(crate) struct TestIngressSet {
    listeners: Vec<TestIngress>,
}

impl TestIngressSet {
    fn bind(spec: &SandboxSpec, assigned_ip: Ipv4Addr) -> nimbus_sandbox::Result<Self> {
        let mut listeners = Vec::with_capacity(spec.port_bindings.len());
        for binding in &spec.port_bindings {
            listeners.push(TestIngress::bind(
                SocketAddr::new(binding.host_address, binding.host_port),
                SocketAddr::new(assigned_ip.into(), binding.guest_port),
            )?);
        }
        Ok(Self { listeners })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }
}

struct TestIngress {
    stop: Arc<AtomicBool>,
    wake_address: SocketAddr,
    worker: Option<thread::JoinHandle<()>>,
}

impl TestIngress {
    fn bind(listen_address: SocketAddr, target: SocketAddr) -> nimbus_sandbox::Result<Self> {
        let listener = TcpListener::bind(listen_address).map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to bind {listen_address}: {error}"),
            }
        })?;
        let wake_address = listener.local_addr().map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to inspect {listen_address}: {error}"),
            }
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to make {wake_address} nonblocking: {error}"),
            }
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name(format!("linux-smoke-ingress-{}", wake_address.port()))
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            thread::spawn(move || forward_connection(client, target));
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| nimbus_sandbox::SandboxError::OperationFailed {
                message: format!("test ingress failed to spawn for {wake_address}: {error}"),
            })?;
        Ok(Self {
            stop,
            wake_address,
            worker: Some(worker),
        })
    }
}

impl Drop for TestIngress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.wake_address, Duration::from_millis(100));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn forward_connection(client: TcpStream, target: SocketAddr) {
    let Ok(upstream) = TcpStream::connect_timeout(&target, Duration::from_secs(5)) else {
        return;
    };
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = upstream.set_read_timeout(Some(Duration::from_secs(30)));
    let (Ok(mut client_read), Ok(mut upstream_write)) = (client.try_clone(), upstream.try_clone())
    else {
        return;
    };
    let one_direction = thread::spawn(move || io::copy(&mut client_read, &mut upstream_write));
    let mut upstream_read = upstream;
    let mut client_write = client;
    let _ = io::copy(&mut upstream_read, &mut client_write);
    let _ = one_direction.join();
}
