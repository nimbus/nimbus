//! Runner-owned TCP proxies from machine-published ports into container IPs.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkReservationClaim, PortBindClaim, PortLeaseLifetimeGuard, PortLeaseRequest,
};

use crate::backends::oci::port_lease::{OciPortBindLifetimeBatch, canonical_socket_ip};
use crate::backends::oci::port_lifecycle::{
    OciPortLeaseCoordinator, machine_port_proxy_guest_listener_addr,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::{MACHINE_PORT_PROXY_ACCEPT_SLEEP, MACHINE_PORT_PROXY_CONNECT_TIMEOUT};

const MACHINE_PORT_PROXY_IO_POLL: Duration = Duration::from_millis(100);
const MACHINE_PORT_PROXY_MAX_CONNECTIONS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MachinePortIoOperation {
    ClientRead,
    ClientWrite,
    TargetRead,
    TargetWrite,
}

impl fmt::Display for MachinePortIoOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ClientRead => "client read",
            Self::ClientWrite => "client write",
            Self::TargetRead => "target read",
            Self::TargetWrite => "target write",
        })
    }
}

#[derive(Debug)]
struct MachinePortIoSetupError {
    operation: MachinePortIoOperation,
    source: io::Error,
}

impl fmt::Display for MachinePortIoSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to configure {} polling timeout: {}",
            self.operation, self.source
        )
    }
}

impl std::error::Error for MachinePortIoSetupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

fn configure_machine_port_io_polling_with(
    client: &TcpStream,
    target: &TcpStream,
    mut configure: impl FnMut(&TcpStream, MachinePortIoOperation, Duration) -> io::Result<()>,
) -> std::result::Result<(), MachinePortIoSetupError> {
    for (stream, operation) in [
        (client, MachinePortIoOperation::ClientRead),
        (client, MachinePortIoOperation::ClientWrite),
        (target, MachinePortIoOperation::TargetRead),
        (target, MachinePortIoOperation::TargetWrite),
    ] {
        configure(stream, operation, MACHINE_PORT_PROXY_IO_POLL)
            .map_err(|source| MachinePortIoSetupError { operation, source })?;
    }
    Ok(())
}

fn configure_machine_port_timeout(
    stream: &TcpStream,
    operation: MachinePortIoOperation,
    timeout: Duration,
) -> io::Result<()> {
    match operation {
        MachinePortIoOperation::ClientRead | MachinePortIoOperation::TargetRead => {
            stream.set_read_timeout(Some(timeout))
        }
        MachinePortIoOperation::ClientWrite | MachinePortIoOperation::TargetWrite => {
            stream.set_write_timeout(Some(timeout))
        }
    }
}

/// Bound but inert host listener for one provider-managed machine endpoint.
pub(crate) struct PreparedMachinePortProxy {
    bind_addr: SocketAddr,
    target_addr: SocketAddr,
    listener: TcpListener,
}

/// Inert listener batch plus the exact live process generations that own it.
pub(crate) struct PreparedMachinePortProxyBatch {
    proxies: Vec<PreparedMachinePortProxy>,
    bind_authority: OciPortBindLifetimeBatch,
}

impl PreparedMachinePortProxyBatch {
    pub(crate) fn bind_authority(&self) -> &OciPortBindLifetimeBatch {
        &self.bind_authority
    }

    pub(crate) fn into_parts(self) -> (Vec<PreparedMachinePortProxy>, OciPortBindLifetimeBatch) {
        (self.proxies, self.bind_authority)
    }
}

/// Exact provider routing intent retained beside a running machine proxy.
///
/// A durable host-port lease proves authority over the listener, but it does
/// not identify the provider-managed target behind that listener. Registry
/// replay must therefore compare this normalized plan before reusing a live
/// provider effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MachinePortProxyRoute {
    guest_listener_addr: SocketAddr,
    target_addr: SocketAddr,
    external_publication_addr: SocketAddr,
}

impl MachinePortProxyRoute {
    fn new(binding: &SandboxPortBinding, container_ip: Ipv4Addr) -> Self {
        Self {
            guest_listener_addr: machine_port_proxy_guest_listener_addr(binding),
            target_addr: SocketAddr::new(IpAddr::V4(container_ip), binding.guest_port),
            external_publication_addr: SocketAddr::new(
                canonical_socket_ip(binding.host_address),
                binding.host_port,
            ),
        }
    }
}

pub(crate) struct MachinePortProxy {
    bind_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    listener_owned: Arc<AtomicBool>,
    stop_state: MachinePortProxyStopState,
}

enum MachinePortProxyStopState {
    Running(thread::JoinHandle<std::result::Result<(), String>>),
    ConfirmedStopped,
    Failed(String),
}

struct MachinePortProxyPreparation<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    binding: &'a SandboxPortBinding,
    port_lease: &'a PortLeaseRequest,
    bind_claim: PortBindClaim,
    lifetime: &'a PortLeaseLifetimeGuard,
    port_lease_coordinator: &'a OciPortLeaseCoordinator,
    route: MachinePortProxyRoute,
    release_authority: MachinePortPreparationReleaseAuthority<'a>,
}

pub(crate) struct MachinePortProxyStartFailure {
    error: SandboxError,
    running: Vec<MachinePortProxy>,
    bind_authority: OciPortBindLifetimeBatch,
}

impl MachinePortProxyStartFailure {
    pub(crate) fn into_parts(
        self,
    ) -> (
        SandboxError,
        Vec<MachinePortProxy>,
        OciPortBindLifetimeBatch,
    ) {
        (self.error, self.running, self.bind_authority)
    }
}

/// Running local workers plus every exact lifetime in their atomic lease batch.
pub(crate) struct RunningMachinePortProxyBatch {
    proxies: Vec<MachinePortProxy>,
    bind_authority: OciPortBindLifetimeBatch,
}

impl RunningMachinePortProxyBatch {
    pub(crate) fn into_parts(self) -> (Vec<MachinePortProxy>, OciPortBindLifetimeBatch) {
        (self.proxies, self.bind_authority)
    }
}

struct MachinePortConnectionSet {
    active: Vec<thread::JoinHandle<std::result::Result<(), String>>>,
    max_active: usize,
    provider_shutdown: Arc<AtomicBool>,
}

impl MachinePortConnectionSet {
    fn new(max_active: usize, provider_shutdown: Arc<AtomicBool>) -> Self {
        Self {
            active: Vec::new(),
            max_active,
            provider_shutdown,
        }
    }

    /// Reap completed workers before admitting another connection.
    ///
    /// Join handles are resources: retaining completed handles for the full
    /// listener lifetime would make memory grow with historical rather than
    /// concurrent traffic.
    fn reap_completed(&mut self) -> std::result::Result<(), String> {
        let mut first_failure = None;
        let mut index = 0;
        while index < self.active.len() {
            if self.active[index].is_finished() {
                let worker = self.active.swap_remove(index);
                match worker.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) if first_failure.is_none() => first_failure = Some(error),
                    Ok(Err(_)) => {}
                    Err(_) if first_failure.is_none() => {
                        first_failure = Some("machine port connection worker panicked".to_owned());
                    }
                    Err(_) => {}
                }
            } else {
                index += 1;
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    /// Spawn one connection worker when bounded concurrent capacity remains.
    ///
    /// Returning `Ok(false)` drops the supplied task (and its accepted socket)
    /// without growing an unbounded thread set.
    fn try_spawn(
        &mut self,
        task: impl FnOnce() -> std::result::Result<(), String> + Send + 'static,
    ) -> std::result::Result<bool, String> {
        self.reap_completed()?;
        if self.active.len() >= self.max_active {
            return Ok(false);
        }
        let worker = thread::Builder::new()
            .name("nimbus-machine-port-connection".to_owned())
            .spawn(task)
            .map_err(|error| {
                format!("failed to spawn a machine port connection worker: {error}")
            })?;
        self.active.push(worker);
        Ok(true)
    }

    fn join_all(&mut self) -> std::result::Result<(), String> {
        let mut first_failure = None;
        while let Some(worker) = self.active.pop() {
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) if first_failure.is_none() => first_failure = Some(error),
                Ok(Err(_)) => {}
                Err(_) if first_failure.is_none() => {
                    first_failure = Some("machine port connection worker panicked".to_owned());
                }
                Err(_) => {}
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    #[cfg(test)]
    fn active_len(&self) -> usize {
        self.active.len()
    }
}

impl Drop for MachinePortConnectionSet {
    fn drop(&mut self) {
        // An accept-worker unwind must not detach connection workers. Signal
        // the same provider-wide stop token they already poll, then join every
        // tracked handle. Drop is deliberately non-panicking; the accept
        // worker's own result remains the durable cleanup diagnostic.
        self.provider_shutdown.store(true, Ordering::SeqCst);
        let _ = self.join_all();
    }
}

impl PreparedMachinePortProxy {
    fn prepare(preparation: MachinePortProxyPreparation<'_>) -> Result<Self> {
        let MachinePortProxyPreparation {
            tenant_id,
            sandbox_id,
            binding,
            port_lease,
            bind_claim,
            lifetime,
            port_lease_coordinator,
            route,
            release_authority,
        } = preparation;
        port_lease_coordinator.require_binding_leases(
            tenant_id,
            sandbox_id,
            std::slice::from_ref(binding),
            std::slice::from_ref(port_lease),
        )?;
        let MachinePortProxyRoute {
            guest_listener_addr,
            target_addr,
            external_publication_addr: _,
        } = route;
        let listener = match TcpListener::bind(guest_listener_addr) {
            Ok(listener) => listener,
            Err(error) => {
                let bind_error = SandboxError::OperationFailed {
                    message: format!(
                        "failed to bind machine port proxy {} -> {} for {}:{}: {error}",
                        guest_listener_addr, target_addr, binding.host_address, binding.host_port
                    ),
                };
                if matches!(
                    release_authority,
                    MachinePortPreparationReleaseAuthority::FreshLaunch(_)
                ) && let Err(record_error) = port_lease_coordinator
                    .record_machine_proxy_bind_failure_with_lifetime(
                        port_lease,
                        &bind_claim,
                        guest_listener_addr,
                        error.kind(),
                        lifetime,
                    )
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{bind_error}; durable machine-port bind-failure recording also failed: \
                             {record_error}"
                        ),
                    });
                }
                return Err(bind_error);
            }
        };
        if let Err(error) = listener.set_nonblocking(true) {
            drop(listener);
            let bind_error = SandboxError::OperationFailed {
                message: format!(
                    "failed to configure machine port proxy listener {}: {error}",
                    guest_listener_addr
                ),
            };
            if matches!(
                release_authority,
                MachinePortPreparationReleaseAuthority::FreshLaunch(_)
            ) && let Err(record_error) = port_lease_coordinator
                .record_machine_proxy_bind_failure_with_lifetime(
                    port_lease,
                    &bind_claim,
                    guest_listener_addr,
                    error.kind(),
                    lifetime,
                )
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "{bind_error}; durable machine-port bind-failure recording also failed: \
                         {record_error}"
                    ),
                });
            }
            return Err(bind_error);
        }
        Ok(Self {
            bind_addr: guest_listener_addr,
            target_addr,
            listener,
        })
    }

    fn start(self) -> Result<MachinePortProxy> {
        let Self {
            bind_addr,
            target_addr,
            listener,
        } = self;
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let listener_owned = Arc::new(AtomicBool::new(true));
        let thread_listener_owned = Arc::clone(&listener_owned);
        let join = thread::Builder::new()
            .name(format!("nimbus-machine-port-{}", bind_addr.port()))
            .spawn(move || {
                accept_machine_port_proxy(
                    listener,
                    target_addr,
                    thread_shutdown,
                    thread_listener_owned,
                )
            })
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to spawn machine port proxy {} -> {}: {error}",
                    bind_addr, target_addr
                ),
            })?;

        Ok(MachinePortProxy {
            bind_addr,
            shutdown,
            listener_owned,
            stop_state: MachinePortProxyStopState::Running(join),
        })
    }
}

impl MachinePortProxy {
    /// Whether the process-local provider worker still owns a live listener.
    ///
    /// This is intentionally process-local evidence. Durable restart
    /// reconciliation belongs to NNC3.8; an exited retained worker may never be
    /// republished as though its `Active` lease proved current reachability.
    pub(crate) fn provider_is_running(&self) -> bool {
        matches!(&self.stop_state, MachinePortProxyStopState::Running(_))
            && self.listener_owned.load(Ordering::SeqCst)
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        self.stop()
    }

    fn stop(&mut self) -> Result<()> {
        let state = std::mem::replace(
            &mut self.stop_state,
            MachinePortProxyStopState::Failed(
                "machine port proxy shutdown outcome is unresolved".to_owned(),
            ),
        );
        let join = match state {
            MachinePortProxyStopState::ConfirmedStopped => {
                self.stop_state = MachinePortProxyStopState::ConfirmedStopped;
                return Ok(());
            }
            MachinePortProxyStopState::Failed(message) => {
                self.stop_state = MachinePortProxyStopState::Failed(message.clone());
                return Err(SandboxError::OperationFailed { message });
            }
            MachinePortProxyStopState::Running(join) => join,
        };

        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &machine_port_proxy_wake_addr(self.bind_addr),
            Duration::from_millis(100),
        );
        let outcome = match join.join() {
            Ok(Ok(())) => {
                self.stop_state = MachinePortProxyStopState::ConfirmedStopped;
                return Ok(());
            }
            Ok(Err(message)) => format!(
                "machine port proxy {} did not stop cleanly: {message}",
                self.bind_addr
            ),
            Err(_) => format!(
                "machine port proxy {} accept worker panicked during shutdown",
                self.bind_addr
            ),
        };
        // Joining the accept worker proves the listener is gone. Both the
        // ordinary error path and panic unwind drain the tracked connection
        // set before the join returns. Preserve the diagnostic for this
        // attempt, but let a retry consume the confirmed-absence state rather
        // than fencing a provider that no longer exists forever.
        self.stop_state = MachinePortProxyStopState::ConfirmedStopped;
        Err(SandboxError::OperationFailed { message: outcome })
    }
}

impl Drop for MachinePortProxy {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
pub(crate) fn panicking_machine_port_proxy_for_test(bind_addr: SocketAddr) -> MachinePortProxy {
    let shutdown = Arc::new(AtomicBool::new(false));
    let join = thread::spawn(|| -> std::result::Result<(), String> {
        panic!("injected machine accept-worker panic")
    });
    MachinePortProxy {
        bind_addr,
        shutdown,
        listener_owned: Arc::new(AtomicBool::new(false)),
        stop_state: MachinePortProxyStopState::Running(join),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MachinePortPreparationReleaseAuthority<'a> {
    Retain,
    FreshLaunch(&'a NetworkReservationClaim),
}

impl<'a> MachinePortPreparationReleaseAuthority<'a> {
    fn reservation_claim(self) -> Option<&'a NetworkReservationClaim> {
        match self {
            Self::Retain => None,
            Self::FreshLaunch(claim) => Some(claim),
        }
    }
}

#[cfg(test)]
pub(crate) fn prepare_machine_port_proxies(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
    port_leases: &[PortLeaseRequest],
    port_lease_coordinator: &OciPortLeaseCoordinator,
) -> Result<PreparedMachinePortProxyBatch> {
    let reservation_claim = port_lease_coordinator.reservation_claim_for_requests(port_leases)?;
    let release_authority = reservation_claim
        .as_ref()
        .map_or(MachinePortPreparationReleaseAuthority::Retain, |claim| {
            MachinePortPreparationReleaseAuthority::FreshLaunch(claim)
        });
    prepare_machine_port_proxies_with_release_authority(
        tenant_id,
        sandbox_id,
        assigned_ips,
        port_bindings,
        port_leases,
        port_lease_coordinator,
        release_authority,
    )
}

pub(crate) fn prepare_machine_port_proxies_with_release_authority(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
    port_leases: &[PortLeaseRequest],
    port_lease_coordinator: &OciPortLeaseCoordinator,
    release_authority: MachinePortPreparationReleaseAuthority<'_>,
) -> Result<PreparedMachinePortProxyBatch> {
    port_lease_coordinator.require_binding_leases(
        tenant_id,
        sandbox_id,
        port_bindings,
        port_leases,
    )?;
    let routes = match machine_port_proxy_routes(assigned_ips, port_bindings) {
        Ok(routes) => routes,
        Err(error) => {
            return Err(
                if let Some(reservation_claim) = release_authority.reservation_claim() {
                    port_lease_coordinator.compensate_failed_never_bound_requests(
                        port_leases,
                        reservation_claim,
                        error,
                        "machine port proxy preparation",
                    )
                } else {
                    error
                },
            );
        }
    };
    let bind_authority = port_lease_coordinator.claim_machine_bindings_with_lifetimes(
        tenant_id,
        sandbox_id,
        port_bindings,
        port_leases,
    )?;
    let mut proxies = Vec::with_capacity(port_bindings.len());
    for ((((binding, lease), route), bind_claim), lifetime) in port_bindings
        .iter()
        .zip(port_leases)
        .zip(routes)
        .zip(bind_authority.claims().iter().cloned())
        .zip(bind_authority.lifetimes())
    {
        match PreparedMachinePortProxy::prepare(MachinePortProxyPreparation {
            tenant_id,
            sandbox_id,
            binding,
            port_lease: lease,
            bind_claim,
            lifetime,
            port_lease_coordinator,
            route,
            release_authority,
        }) {
            Ok(proxy) => proxies.push(proxy),
            Err(error) => {
                // Drop every already-bound sibling before releasing the
                // durable request set. The failed member is terminal Failed
                // when bind lost to an external owner; the authority accepts
                // that no-effect evidence while retiring Reserved siblings.
                drop(proxies);
                let error = match port_lease_coordinator
                    .abandon_machine_bind_claims_with_lifetimes_without_effect(
                        port_leases,
                        &bind_authority,
                    ) {
                    Ok(()) => error,
                    Err(abandon_error) => SandboxError::OperationFailed {
                        message: format!(
                            "{error}; machine port bind-claim compensation also failed: \
                             {abandon_error}"
                        ),
                    },
                };
                return Err(
                    if let Some(reservation_claim) = release_authority.reservation_claim() {
                        port_lease_coordinator.compensate_failed_never_bound_requests(
                            port_leases,
                            reservation_claim,
                            error,
                            "machine port proxy preparation",
                        )
                    } else {
                        error
                    },
                );
            }
        }
    }
    Ok(PreparedMachinePortProxyBatch {
        proxies,
        bind_authority,
    })
}

/// Normalize the complete provider routing plan without binding or mutating
/// durable authority.
pub(crate) fn machine_port_proxy_routes(
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortProxyRoute>> {
    if port_bindings.is_empty() {
        return Ok(Vec::new());
    }
    let Some(container_ip) = assigned_ips.first().copied() else {
        return Err(SandboxError::OperationFailed {
            message: "cannot start machine port proxies without a container IPv4 address"
                .to_owned(),
        });
    };
    Ok(port_bindings
        .iter()
        .map(|binding| MachinePortProxyRoute::new(binding, container_ip))
        .collect())
}

/// Start a fully prepared batch only after exact durable activation.
#[cfg(test)]
pub(crate) fn start_machine_port_proxies(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
    port_leases: &[PortLeaseRequest],
    port_lease_coordinator: &OciPortLeaseCoordinator,
    prepared: PreparedMachinePortProxyBatch,
) -> Result<RunningMachinePortProxyBatch> {
    start_machine_port_proxies_with_recovery(
        tenant_id,
        sandbox_id,
        port_bindings,
        port_leases,
        port_lease_coordinator,
        prepared,
    )
    .map_err(|failure| {
        let (error, mut running, _bind_authority) = failure.into_parts();
        for proxy in &mut running {
            let _ = proxy.shutdown();
        }
        error
    })
}

pub(crate) fn start_machine_port_proxies_with_recovery(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    port_bindings: &[SandboxPortBinding],
    port_leases: &[PortLeaseRequest],
    port_lease_coordinator: &OciPortLeaseCoordinator,
    prepared: PreparedMachinePortProxyBatch,
) -> std::result::Result<RunningMachinePortProxyBatch, MachinePortProxyStartFailure> {
    if prepared.proxies.len() != port_bindings.len() {
        return Err(MachinePortProxyStartFailure {
            error: SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy batch has {} prepared listeners for {} bindings",
                    prepared.proxies.len(),
                    port_bindings.len()
                ),
            },
            running: Vec::new(),
            bind_authority: prepared.bind_authority,
        });
    }
    if let Err(error) = port_lease_coordinator.require_active_machine_bindings_with_lifetimes(
        tenant_id,
        sandbox_id,
        port_bindings,
        port_leases,
        &prepared.bind_authority,
    ) {
        let (proxies, bind_authority) = prepared.into_parts();
        drop(proxies);
        let error = match port_lease_coordinator
            .abandon_machine_bind_claims_with_lifetimes_without_effect(port_leases, &bind_authority)
        {
            Ok(()) => error,
            Err(abandon_error) => SandboxError::OperationFailed {
                message: format!(
                    "{error}; inactive machine proxy bind-claim compensation also failed: \
                     {abandon_error}"
                ),
            },
        };
        return Err(MachinePortProxyStartFailure {
            error,
            running: Vec::new(),
            bind_authority,
        });
    }

    let (prepared, bind_authority) = prepared.into_parts();
    let mut running = Vec::with_capacity(prepared.len());
    for proxy in prepared {
        match proxy.start() {
            Ok(proxy) => running.push(proxy),
            Err(error) => {
                return Err(MachinePortProxyStartFailure {
                    error,
                    running,
                    bind_authority,
                });
            }
        }
    }
    Ok(RunningMachinePortProxyBatch {
        proxies: running,
        bind_authority,
    })
}

fn accept_machine_port_proxy(
    listener: TcpListener,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    listener_owned: Arc<AtomicBool>,
) -> std::result::Result<(), String> {
    accept_machine_port_proxy_with(
        listener,
        target_addr,
        shutdown,
        listener_owned,
        proxy_machine_port_connection,
    )
}

fn accept_machine_port_proxy_with(
    listener: TcpListener,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    listener_owned: Arc<AtomicBool>,
    connection_handler: impl Fn(
        TcpStream,
        SocketAddr,
        Arc<AtomicBool>,
    ) -> std::result::Result<(), String>
    + Send
    + Sync
    + 'static,
) -> std::result::Result<(), String> {
    accept_machine_port_proxy_with_listener_observer(
        listener,
        target_addr,
        shutdown,
        listener_owned,
        connection_handler,
        || {},
    )
}

fn accept_machine_port_proxy_with_listener_observer(
    listener: TcpListener,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    listener_owned: Arc<AtomicBool>,
    connection_handler: impl Fn(
        TcpStream,
        SocketAddr,
        Arc<AtomicBool>,
    ) -> std::result::Result<(), String>
    + Send
    + Sync
    + 'static,
    on_listener_liveness_cleared: impl FnOnce(),
) -> std::result::Result<(), String> {
    let mut connections =
        MachinePortConnectionSet::new(MACHINE_PORT_PROXY_MAX_CONNECTIONS, Arc::clone(&shutdown));
    // Declared after the connection set so unwind drops the listener and
    // clears observed liveness before connection-worker Drop begins draining.
    let mut listener =
        MachinePortListenerOwner::new(listener, listener_owned, on_listener_liveness_cleared);
    let connection_handler = Arc::new(connection_handler);
    let mut failure = None;
    while !shutdown.load(Ordering::SeqCst) {
        if let Err(error) = connections.reap_completed() {
            failure = Some(error);
            shutdown.store(true, Ordering::SeqCst);
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let connection_shutdown = Arc::clone(&shutdown);
                let connection_handler = Arc::clone(&connection_handler);
                match connections.try_spawn(move || {
                    run_machine_port_connection_task(connection_shutdown, |connection_shutdown| {
                        connection_handler(stream, target_addr, connection_shutdown)
                    })
                }) {
                    Ok(true) | Ok(false) => {}
                    Err(error) => {
                        failure = Some(error);
                        shutdown.store(true, Ordering::SeqCst);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(MACHINE_PORT_PROXY_ACCEPT_SLEEP);
            }
            Err(error) => {
                failure = Some(format!(
                    "machine port proxy listener accept failed: {error}"
                ));
                shutdown.store(true, Ordering::SeqCst);
            }
        }
    }
    listener.close();
    if let Err(error) = connections.join_all()
        && failure.is_none()
    {
        failure = Some(error);
    }
    if let Some(failure) = failure {
        return Err(failure);
    }
    Ok(())
}

struct MachinePortListenerOwner<F: FnOnce()> {
    listener: Option<TcpListener>,
    listener_owned: Arc<AtomicBool>,
    on_liveness_cleared: Option<F>,
}

impl<F: FnOnce()> MachinePortListenerOwner<F> {
    fn new(listener: TcpListener, listener_owned: Arc<AtomicBool>, on_liveness_cleared: F) -> Self {
        Self {
            listener: Some(listener),
            listener_owned,
            on_liveness_cleared: Some(on_liveness_cleared),
        }
    }

    fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        self.listener
            .as_ref()
            .expect("machine listener owner must retain its socket until close")
            .accept()
    }

    fn close(&mut self) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        self.listener_owned.store(false, Ordering::SeqCst);
        if let Some(on_liveness_cleared) = self.on_liveness_cleared.take() {
            on_liveness_cleared();
        }
        drop(listener);
    }
}

impl<F: FnOnce()> Drop for MachinePortListenerOwner<F> {
    fn drop(&mut self) {
        self.close();
    }
}

fn run_machine_port_connection_task(
    provider_shutdown: Arc<AtomicBool>,
    task: impl FnOnce(Arc<AtomicBool>) -> std::result::Result<(), String>,
) -> std::result::Result<(), String> {
    // Accepted-connection setup and forwarding errors terminate only that
    // socket. Provider-wide lifecycle remains with the accept worker and the
    // explicit MachinePortProxy shutdown handle; otherwise one transient
    // descriptor/timeout error can leave durable Active evidence behind a dead
    // listener. A task panic still propagates through the JoinHandle and is
    // treated as a provider worker failure by MachinePortConnectionSet.
    if let Err(error) = task(provider_shutdown) {
        tracing::debug!(
            error,
            "machine port connection ended without affecting listener lifecycle"
        );
    }
    Ok(())
}

fn proxy_machine_port_connection(
    inbound: TcpStream,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
) -> std::result::Result<(), String> {
    proxy_machine_port_connection_with(
        inbound,
        target_addr,
        shutdown,
        configure_machine_port_timeout,
    )
}

fn proxy_machine_port_connection_with(
    inbound: TcpStream,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    configure_timeout: impl FnMut(&TcpStream, MachinePortIoOperation, Duration) -> io::Result<()>,
) -> std::result::Result<(), String> {
    proxy_machine_port_connection_with_spawner(
        inbound,
        target_addr,
        shutdown,
        configure_timeout,
        |direction, task| {
            thread::Builder::new()
                .name(format!("nimbus-machine-port-{direction}"))
                .spawn(task)
        },
    )
}

type MachinePortCopyTask = Box<dyn FnOnce() + Send + 'static>;

fn proxy_machine_port_connection_with_spawner(
    mut inbound: TcpStream,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    configure_timeout: impl FnMut(&TcpStream, MachinePortIoOperation, Duration) -> io::Result<()>,
    mut spawn: impl FnMut(&'static str, MachinePortCopyTask) -> io::Result<thread::JoinHandle<()>>,
) -> std::result::Result<(), String> {
    if shutdown.load(Ordering::SeqCst) {
        return Ok(());
    }
    let Ok(mut outbound) =
        TcpStream::connect_timeout(&target_addr, MACHINE_PORT_PROXY_CONNECT_TIMEOUT)
    else {
        return Ok(());
    };
    configure_machine_port_io_polling_with(&inbound, &outbound, configure_timeout)
        .map_err(|error| error.to_string())?;
    let mut inbound_reader = inbound.try_clone().map_err(|error| {
        format!("failed to clone machine port proxy client stream after polling setup: {error}")
    })?;
    let mut outbound_writer = outbound.try_clone().map_err(|error| {
        format!("failed to clone machine port proxy target stream after polling setup: {error}")
    })?;
    let inbound_control = inbound.try_clone().map_err(|error| {
        format!("failed to clone machine port proxy client shutdown handle: {error}")
    })?;
    let outbound_control = outbound.try_clone().map_err(|error| {
        format!("failed to clone machine port proxy target shutdown handle: {error}")
    })?;
    let client_shutdown = Arc::clone(&shutdown);
    let client_to_target = spawn(
        "client-to-target",
        Box::new(move || {
            copy_machine_port_stream(&mut inbound_reader, &mut outbound_writer, &client_shutdown);
            let _ = outbound_writer.shutdown(Shutdown::Write);
        }),
    )
    .map_err(|error| {
        format!("failed to spawn machine port client-to-target copy worker: {error}")
    })?;
    let target_to_client = match spawn(
        "target-to-client",
        Box::new(move || {
            copy_machine_port_stream(&mut outbound, &mut inbound, &shutdown);
            let _ = inbound.shutdown(Shutdown::Write);
        }),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = inbound_control.shutdown(Shutdown::Both);
            let _ = outbound_control.shutdown(Shutdown::Both);
            return match client_to_target.join() {
                Ok(()) => Err(format!(
                    "failed to spawn machine port target-to-client copy worker: {error}"
                )),
                Err(_) => Err(format!(
                    "failed to spawn machine port target-to-client copy worker: {error}; \
                     client-to-target copy worker panicked while draining"
                )),
            };
        }
    };
    let client_result = client_to_target.join();
    let target_result = target_to_client.join();
    if client_result.is_err() {
        return Err("machine port client-to-target copy worker panicked".to_owned());
    }
    if target_result.is_err() {
        return Err("machine port target-to-client copy worker panicked".to_owned());
    }
    Ok(())
}

fn copy_machine_port_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
    shutdown: &AtomicBool,
) {
    let mut buffer = [0_u8; 16 * 1024];
    while !shutdown.load(Ordering::SeqCst) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if !write_machine_port_chunk(writer, &buffer[..read], shutdown) {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
}

/// Write one complete chunk without losing the acknowledged prefix when a
/// polling timeout or nonblocking retry interrupts forward progress.
fn write_machine_port_chunk(
    writer: &mut impl Write,
    mut remaining: &[u8],
    shutdown: &AtomicBool,
) -> bool {
    while !remaining.is_empty() && !shutdown.load(Ordering::SeqCst) {
        match writer.write(remaining) {
            Ok(0) => return false,
            Ok(written) => remaining = &remaining[written..],
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    }
    remaining.is_empty()
}

fn machine_port_proxy_wake_addr(bind_addr: SocketAddr) -> SocketAddr {
    if bind_addr.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bind_addr.port())
    } else {
        bind_addr
    }
}

#[cfg(test)]
#[path = "proxy/tests.rs"]
mod tests;
