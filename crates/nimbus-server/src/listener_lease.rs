//! Server-side adapter between durable port authority and TCP socket effects.
//!
//! `nimbus-network` owns portable identity and lifecycle state. This module
//! translates real Tokio listener observations into that vocabulary while
//! leaving every kernel bind in its existing effect owner.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkPlanId,
    NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration,
    PortBindAttempt, PortBindClaim, PortBindFailure, PortBindFailureKind, PortBindRealm,
    PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure,
    PortIpv6Overlap, PortLeaseAccounting, PortLeaseBinding, PortLeaseEffectScope, PortLeaseError,
    PortLeaseFence, PortLeaseLifetimeGuard, PortLeasePhase, PortLeaseRecoveryAttempt,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use ulid::Ulid;

use crate::network_composition::RetainedServerNetworkAuthority;

#[path = "listener_lease/restart_retain.rs"]
mod restart_retain;
pub(crate) use restart_retain::{
    RestartStoppingServerListener, stop_and_retain_server_listeners_for_restart,
};

const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
pub(crate) const SERVER_LISTENER_PROVIDER_KEY: &str = "nimbus-server.tcp-listener";
const EXTERNAL_MAIN_LISTENER_OWNER: &str = "nimbus-server";
const EXTERNAL_MAIN_LISTENER_NAME: &str = "main-http-external";

/// Stable provider context for one externally owned main-listener incarnation.
///
/// The caller that owns or inherits the socket must persist and replay this
/// context with the descriptor. A local address is diagnostic only and cannot
/// authenticate that a supplied descriptor is the same provider resource. A
/// provider must mint a new opaque incarnation or strictly newer generation
/// for every newly bound socket and must never reuse context from a closed
/// socket for a rebound descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalServerListenerContext {
    provider_handle: NetworkProviderHandle,
    resource_generation: NetworkResourceGeneration,
}

impl ExternalServerListenerContext {
    /// Construct context from a provider-stable opaque incarnation key.
    ///
    /// `provider_incarnation` identifies one exact socket incarnation, not an
    /// address or logical listener name. `resource_generation` must advance
    /// whenever that provider resource is replaced.
    pub fn new(
        provider_incarnation: impl Into<String>,
        resource_generation: NetworkResourceGeneration,
    ) -> io::Result<Self> {
        Ok(Self {
            provider_handle: NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY),
                provider_incarnation,
            )
            .map_err(network_error)?,
            resource_generation,
        })
    }
}

/// A kernel bind failure whose no-effect receipt is durable.
#[derive(Debug)]
pub struct RecordedListenerBindFailure {
    error: io::Error,
}

impl RecordedListenerBindFailure {
    /// Recover the original kernel bind error after its receipt is durable.
    pub fn into_error(self) -> io::Error {
        self.error
    }
}

/// One claimed server listener request prepared before a Nimbus-owned bind.
///
/// Dropping this value deliberately leaves its durable claim fenced. A caller
/// must report a proven no-effect bind failure or adopt the concrete listener;
/// ambiguous interruption is reconciled by the later cleanup owner.
pub struct PreparedServerListener {
    network_authority: RetainedServerNetworkAuthority,
    request: PortLeaseRequest,
    reservation_claim: Option<NetworkReservationClaim>,
    planned_authority: Option<PlannedWorkloadListenerAuthority>,
    claim: PortBindClaim,
    attempt: PortBindAttempt,
    provenance: PortBindingProvenance,
    owner_incarnation: Arc<str>,
    lifetime: PortLeaseLifetimeGuard,
}

enum PlannedWorkloadListenerAuthority {
    Initial {
        plan_members: Vec<PortLeaseRequest>,
        reservation_claim: NetworkReservationClaim,
    },
    Rebind {
        plan_members: Vec<PortLeaseRequest>,
        confirmed_stopped_binding: PortLeaseBinding,
    },
}

impl PreparedServerListener {
    /// Return the exact socket address authorized for this bind attempt.
    ///
    /// A first provider-assigned attempt carries port zero. Recovery after a
    /// confirmed-dead process owner carries the previously adopted concrete
    /// port so the effect owner cannot silently allocate a different slot.
    pub fn bind_addr(&self) -> io::Result<SocketAddr> {
        if self.attempt.protocol() != PortProtocol::Tcp
            || self.attempt.realm() != &PortBindRealm::Host
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "server listener authority issued a non-host TCP bind attempt",
            ));
        }
        let address = self.request.publication().host_address().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "server listener request omits host publication address",
            )
        })?;
        Ok(SocketAddr::new(address, self.attempt.port()))
    }

    /// Record a kernel bind failure that created no listener effect.
    ///
    /// `Ok` provides the original I/O error only after the durable failure
    /// receipt succeeds. `Err` retains both the bind and authority failures;
    /// callers must stop fallback because the lease remains fenced.
    pub fn record_bind_failure(
        self,
        error: io::Error,
    ) -> Result<RecordedListenerBindFailure, io::Error> {
        let failure = PortBindFailure::new(
            bind_failure_kind(error.kind()),
            self.attempt.clone(),
            self.claim.provider_attempt().clone(),
        );
        let authority = self.network_authority.port_leases();
        let recorded = match self.planned_authority.as_ref() {
            Some(PlannedWorkloadListenerAuthority::Initial {
                plan_members,
                reservation_claim,
            }) => authority.record_claimed_plan_member_bind_failure_with_lifetime_without_effect(
                plan_members,
                &self.request,
                reservation_claim,
                &self.claim,
                failure,
                &self.lifetime,
            ),
            Some(PlannedWorkloadListenerAuthority::Rebind {
                plan_members,
                confirmed_stopped_binding,
            }) => authority.abandon_rebind_plan_member_with_lifetime_without_effect(
                plan_members,
                &self.request,
                confirmed_stopped_binding,
                &self.claim,
                &self.lifetime,
            ),
            None => authority.record_claimed_bind_failure_with_lifetime_without_effect(
                &self.request,
                self.reservation_claim.as_ref(),
                &self.claim,
                failure,
                &self.lifetime,
            ),
        };
        match recorded {
            Ok(_) => Ok(RecordedListenerBindFailure { error }),
            Err(record_error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; failed to record durable no-effect bind failure for {}: \
                     {record_error}",
                    self.request.lease_id()
                ),
            )),
        }
    }

    /// Adopt and activate the concrete listener produced by the effect owner.
    pub fn adopt(self, listener: tokio::net::TcpListener) -> io::Result<LeasedServerListener> {
        let binding = match self.binding_for_listener(&listener) {
            Ok(binding) => binding,
            Err(error) => return Err(self.close_after_failed_adoption(listener, error)),
        };
        if let Err(error) = self.adopt_binding(binding.clone()) {
            return Err(self.close_after_failed_adoption(listener, network_error(error)));
        }
        Ok(LeasedServerListener {
            listener,
            lease: ActiveServerListenerLease {
                network_authority: self.network_authority,
                request: self.request,
                provenance: self.provenance,
                lifetime: self.lifetime,
                binding,
            },
            owner_incarnation: self.owner_incarnation,
        })
    }

    /// Adopt and activate a pre-bound standard-library listener.
    ///
    /// CLI composition uses this before a Tokio runtime owns the socket so a
    /// provider-assigned port can be advertised without dropping its kernel
    /// reservation. The returned listener remains bound and carries the same
    /// Active lease into [`ServeOptions`](crate::ServeOptions).
    pub fn adopt_std(self, listener: std::net::TcpListener) -> io::Result<PreboundServerListener> {
        let binding = match listener
            .local_addr()
            .and_then(|actual_addr| self.binding_for_addr(actual_addr))
        {
            Ok(binding) => binding,
            Err(error) => return Err(self.close_std_after_failed_adoption(listener, error)),
        };
        if let Err(error) = self.adopt_binding(binding.clone()) {
            return Err(self.close_std_after_failed_adoption(listener, network_error(error)));
        }
        Ok(PreboundServerListener {
            listener,
            lease: ActiveServerListenerLease {
                network_authority: self.network_authority,
                request: self.request,
                provenance: self.provenance,
                lifetime: self.lifetime,
                binding,
            },
            owner_incarnation: self.owner_incarnation,
        })
    }

    fn binding_for_listener(
        &self,
        listener: &tokio::net::TcpListener,
    ) -> io::Result<PortLeaseBinding> {
        let actual_addr = listener.local_addr()?;
        self.binding_for_addr(actual_addr)
    }

    fn adopt_binding(
        &self,
        binding: PortLeaseBinding,
    ) -> Result<nimbus_network::PortLeaseRecord, PortLeaseError> {
        let authority = self.network_authority.port_leases();
        match self.planned_authority.as_ref() {
            Some(PlannedWorkloadListenerAuthority::Initial {
                plan_members,
                reservation_claim,
            }) => authority.adopt_claimed_and_activate_plan_member_with_lifetime(
                plan_members,
                &self.request,
                reservation_claim,
                &self.claim,
                binding,
                &self.lifetime,
            ),
            Some(PlannedWorkloadListenerAuthority::Rebind {
                plan_members,
                confirmed_stopped_binding,
            }) => authority.adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
                plan_members,
                &self.request,
                confirmed_stopped_binding,
                &self.claim,
                binding,
                &self.lifetime,
            ),
            None => authority.adopt_claimed_and_activate_with_lifetime(
                &self.request,
                self.reservation_claim.as_ref(),
                &self.claim,
                binding,
                &self.lifetime,
            ),
        }
    }

    fn binding_for_addr(&self, actual_addr: SocketAddr) -> io::Result<PortLeaseBinding> {
        let actual_port = NonZeroU16::new(actual_addr.port()).ok_or_else(|| {
            io::Error::other("a bound server listener reported the non-concrete port zero")
        })?;
        let endpoint = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(actual_addr.ip())?,
            actual_port,
        )
        .map_err(network_error)?;
        Ok(PortLeaseBinding::new(
            endpoint,
            self.provenance,
            self.claim.provider_attempt().clone(),
        ))
    }

    fn close_after_failed_adoption(
        self,
        listener: tokio::net::TcpListener,
        primary: io::Error,
    ) -> io::Error {
        drop(listener);
        match self.abandon_after_confirmed_close() {
            Ok(()) => primary,
            Err(cleanup_error) => io::Error::new(
                primary.kind(),
                format!(
                    "{primary}; failed to settle the claimed listener after adoption failed: \
                     {cleanup_error}"
                ),
            ),
        }
    }

    fn close_std_after_failed_adoption(
        self,
        listener: std::net::TcpListener,
        primary: io::Error,
    ) -> io::Error {
        drop(listener);
        match self.abandon_after_confirmed_close() {
            Ok(()) => primary,
            Err(cleanup_error) => io::Error::new(
                primary.kind(),
                format!(
                    "{primary}; failed to settle the claimed pre-bound listener after adoption \
                     failed: {cleanup_error}"
                ),
            ),
        }
    }

    fn abandon_after_confirmed_close(self) -> io::Result<()> {
        let authority = self.network_authority.port_leases();
        match self.planned_authority.as_ref() {
            Some(PlannedWorkloadListenerAuthority::Initial {
                plan_members,
                reservation_claim,
            }) => authority
                .abandon_bind_plan_member_with_lifetime_without_effect(
                    plan_members,
                    &self.request,
                    reservation_claim,
                    &self.claim,
                    &self.lifetime,
                )
                .map(|_| ())
                .map_err(network_error),
            Some(PlannedWorkloadListenerAuthority::Rebind {
                plan_members,
                confirmed_stopped_binding,
            }) => authority
                .abandon_rebind_plan_member_with_lifetime_without_effect(
                    plan_members,
                    &self.request,
                    confirmed_stopped_binding,
                    &self.claim,
                    &self.lifetime,
                )
                .map(|_| ())
                .map_err(network_error),
            None => settle_claim_without_effect(
                authority,
                &self.request,
                self.reservation_claim.as_ref(),
                &self.claim,
                self.provenance,
                &self.lifetime,
            ),
        }
    }
}

/// A concrete TCP listener backed by an Active durable port lease.
pub struct LeasedServerListener {
    listener: tokio::net::TcpListener,
    lease: ActiveServerListenerLease,
    owner_incarnation: Arc<str>,
}

/// A standard-library TCP listener backed by an Active durable port lease.
///
/// This is the handoff form used when CLI composition must know an
/// OS-assigned port before the asynchronous server begins. It owns the real
/// descriptor continuously; converting it for Tokio never opens a second bind
/// window.
pub struct PreboundServerListener {
    listener: std::net::TcpListener,
    lease: ActiveServerListenerLease,
    owner_incarnation: Arc<str>,
}

impl fmt::Debug for PreboundServerListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreboundServerListener")
            .field("local_addr", &self.listener.local_addr())
            .field("owner_incarnation", &self.owner_incarnation)
            .finish_non_exhaustive()
    }
}

impl PreboundServerListener {
    /// Return the concrete address assigned to the continuously held socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Close a listener that never entered server ownership and settle its
    /// exact durable lease.
    pub fn close_and_settle(self) -> io::Result<()> {
        drop(self.listener);
        self.lease.settle_after_confirmed_local_close()
    }

    pub(crate) fn owner_incarnation(&self) -> &str {
        self.owner_incarnation.as_ref()
    }

    pub(crate) fn network_authority(&self) -> &RetainedServerNetworkAuthority {
        &self.lease.network_authority
    }

    pub(crate) fn into_leased(self) -> io::Result<LeasedServerListener> {
        let Self {
            listener,
            lease,
            owner_incarnation,
        } = self;
        if let Err(error) = listener.set_nonblocking(true) {
            drop(listener);
            return match lease.settle_after_confirmed_local_close() {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; failed to settle the pre-bound listener after nonblocking \
                         setup failed: {cleanup_error}"
                    ),
                )),
            };
        }
        match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => Ok(LeasedServerListener {
                listener,
                lease,
                owner_incarnation,
            }),
            Err(error) => {
                let cleanup_error = lease.settle_after_confirmed_local_close().err();
                match cleanup_error {
                    Some(cleanup_error) => Err(io::Error::new(
                        error.kind(),
                        format!(
                            "{error}; failed to settle the pre-bound listener after Tokio \
                             adoption failed: {cleanup_error}"
                        ),
                    )),
                    None => Err(error),
                }
            }
        }
    }

    pub(crate) fn into_std_parts(
        self,
    ) -> (std::net::TcpListener, ActiveServerListenerLease, Arc<str>) {
        (self.listener, self.lease, self.owner_incarnation)
    }
}

impl LeasedServerListener {
    /// Return the concrete address reported by the adopted TCP listener.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Close Nimbus's local descriptor and settle the durable lease.
    ///
    /// Nimbus-owned bindings are withdrawn and released after the confirmed
    /// close. An externally owned binding is only withdrawn because closing
    /// Nimbus's descriptor does not prove the external owner released its
    /// listener or host-port fence.
    pub fn close_and_settle(self) -> io::Result<()> {
        let (listener, lease, _) = self.into_parts();
        drop(listener);
        lease.settle_after_confirmed_local_close()
    }

    pub(crate) fn owner_incarnation(&self) -> &str {
        self.owner_incarnation.as_ref()
    }

    pub(crate) fn network_authority(&self) -> &RetainedServerNetworkAuthority {
        &self.lease.network_authority
    }

    pub(crate) fn into_parts(
        self,
    ) -> (tokio::net::TcpListener, ActiveServerListenerLease, Arc<str>) {
        (self.listener, self.lease, self.owner_incarnation)
    }
}

pub(crate) struct ActiveServerListenerLease {
    network_authority: RetainedServerNetworkAuthority,
    request: PortLeaseRequest,
    provenance: PortBindingProvenance,
    lifetime: PortLeaseLifetimeGuard,
    binding: PortLeaseBinding,
}

/// Effect-free evidence retained by one live server-owned listener.
///
/// The opaque provider handle and network authority deliberately remain in
/// the listener owner. This snapshot carries only the immutable request,
/// active lifetime, concrete endpoint, and provenance needed to authenticate
/// a post-Observed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveServerListenerEvidence {
    request: PortLeaseRequest,
    lifetime: nimbus_network::PortLeaseLifetime,
    bound_endpoint: PortBoundEndpoint,
    provenance: PortBindingProvenance,
}

impl ActiveServerListenerEvidence {
    pub(crate) fn request(&self) -> &PortLeaseRequest {
        &self.request
    }

    pub(crate) const fn lifetime(&self) -> nimbus_network::PortLeaseLifetime {
        self.lifetime
    }

    pub(crate) fn bound_endpoint(&self) -> &PortBoundEndpoint {
        &self.bound_endpoint
    }

    pub(crate) const fn provenance(&self) -> PortBindingProvenance {
        self.provenance
    }
}

impl ActiveServerListenerLease {
    /// Return a typed comparison snapshot without reading or mutating durable
    /// authority and without exposing provider effect authority.
    pub(crate) fn observation_evidence(&self) -> Option<ActiveServerListenerEvidence> {
        if self.lifetime.request() != &self.request || self.binding.provenance() != self.provenance
        {
            return None;
        }
        Some(ActiveServerListenerEvidence {
            request: self.request.clone(),
            lifetime: self.lifetime.lifetime(),
            bound_endpoint: self.binding.endpoint().clone(),
            provenance: self.provenance,
        })
    }

    pub(crate) fn settle_after_confirmed_local_close(self) -> io::Result<()> {
        debug_assert_eq!(self.lifetime.request(), &self.request);
        self.network_authority
            .port_leases()
            .withdraw(&self.request)
            .map_err(network_error)?;
        if self.provenance != PortBindingProvenance::ExternallyOwned {
            self.network_authority
                .port_leases()
                .release_with_lifetime(&self.request, &self.lifetime)
                .map_err(network_error)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ServerListenerLeaseAuthority {
    network_authority: RetainedServerNetworkAuthority,
    incarnation: Arc<str>,
    external_main_context: ExternalServerListenerContext,
    next_main_attempt: Arc<AtomicU64>,
}

/// Pre-bound sibling listeners plus the exact server authority incarnation
/// that claimed and activated them.
///
/// The bundle is intentionally not a numeric-port handoff: it retains every
/// concrete socket until [`ServeOptions`](crate::ServeOptions) consumes it.
pub struct PreboundServerListeners {
    authority: ServerListenerLeaseAuthority,
    listeners: BTreeMap<String, PreboundServerListener>,
}

impl Drop for PreboundServerListeners {
    fn drop(&mut self) {
        if self.listeners.is_empty() {
            return;
        }
        if let Err(error) = self.close_and_settle_remaining() {
            tracing::error!(
                %error,
                "failed to settle pre-bound server listeners during ownership drop"
            );
        }
    }
}

impl fmt::Debug for PreboundServerListeners {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addresses = self
            .listeners
            .iter()
            .map(|(name, listener)| (name.as_str(), listener.local_addr()))
            .collect::<Vec<_>>();
        formatter
            .debug_struct("PreboundServerListeners")
            .field("listeners", &addresses)
            .finish_non_exhaustive()
    }
}

impl PreboundServerListeners {
    /// Create one server-listener incarnation under the process manager's
    /// retained node authority.
    pub fn new(network_authority: LocalNetworkAuthority) -> Self {
        Self {
            authority: ServerListenerLeaseAuthority::new(network_authority),
            listeners: BTreeMap::new(),
        }
    }

    /// Explicitly reconstruct the primitive listener authority once.
    ///
    /// This direct embedder/test seam does not claim process-manager
    /// composition. Production composition should inject
    /// [`LocalNetworkAuthority`] through [`Self::new`].
    pub fn reconstruct_direct(state_root: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Ok(Self {
            authority: ServerListenerLeaseAuthority::reconstruct_direct(state_root)?,
            listeners: BTreeMap::new(),
        })
    }

    /// Reserve and claim a named sibling before the caller-owned socket bind.
    pub fn prepare(
        &self,
        listener_name: &str,
        requested_addr: SocketAddr,
    ) -> io::Result<PreparedServerListener> {
        self.authority.prepare(
            listener_name,
            requested_addr,
            nimbus_owned_provenance(requested_addr),
        )
    }

    /// Retain an adopted listener under the adapter name that will consume it.
    pub fn insert(
        &mut self,
        adapter_name: impl Into<String>,
        listener: PreboundServerListener,
    ) -> io::Result<()> {
        let adapter_name = adapter_name.into();
        if !self
            .authority
            .owns(listener.owner_incarnation(), listener.network_authority())
        {
            let primary = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "pre-bound {adapter_name} listener belongs to a different server authority"
                ),
            );
            return match listener.close_and_settle() {
                Ok(()) => Err(primary),
                Err(cleanup_error) => Err(io::Error::other(format!(
                    "{primary}; failed to settle the rejected listener: {cleanup_error}"
                ))),
            };
        }
        if self.listeners.contains_key(&adapter_name) {
            let primary = io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("pre-bound listener `{adapter_name}` is already registered"),
            );
            return match listener.close_and_settle() {
                Ok(()) => Err(primary),
                Err(cleanup_error) => Err(io::Error::new(
                    primary.kind(),
                    format!("{primary}; failed to settle the duplicate: {cleanup_error}"),
                )),
            };
        }
        self.listeners.insert(adapter_name, listener);
        Ok(())
    }

    /// Close and settle every listener that has not entered server ownership.
    pub fn close_and_settle(mut self) -> io::Result<()> {
        self.close_and_settle_remaining()
    }

    fn close_and_settle_remaining(&mut self) -> io::Result<()> {
        let mut failure: Option<io::Error> = None;
        for (adapter_name, listener) in std::mem::take(&mut self.listeners) {
            if let Err(error) = listener.close_and_settle() {
                failure = Some(match failure {
                    Some(previous) => io::Error::other(format!(
                        "{previous}; failed to settle pre-bound {adapter_name} listener: {error}"
                    )),
                    None => io::Error::other(format!(
                        "failed to settle pre-bound {adapter_name} listener: {error}"
                    )),
                });
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) fn authority(&self) -> ServerListenerLeaseAuthority {
        self.authority.clone()
    }

    pub(crate) fn remove(&mut self, adapter_name: &str) -> Option<PreboundServerListener> {
        self.listeners.remove(adapter_name)
    }

    pub(crate) fn first_name(&self) -> Option<&str> {
        self.listeners.keys().next().map(String::as_str)
    }
}

impl ServerListenerLeaseAuthority {
    pub(crate) fn new(network_authority: LocalNetworkAuthority) -> Self {
        Self::from_retained_authority(RetainedServerNetworkAuthority::manager_derived(
            network_authority,
        ))
    }

    pub(crate) fn reconstruct_direct(state_root: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Ok(Self::from_retained_authority(
            RetainedServerNetworkAuthority::reconstruct_direct(state_root)?,
        ))
    }

    fn from_retained_authority(network_authority: RetainedServerNetworkAuthority) -> Self {
        let incarnation: Arc<str> = Arc::from(format!("server:{}", Ulid::new()));
        let external_main_context = ExternalServerListenerContext::new(
            format!("process-owned:{incarnation}"),
            INITIAL_RESOURCE_GENERATION,
        )
        .expect("a generated server incarnation is valid provider context");
        Self {
            network_authority,
            incarnation,
            external_main_context,
            next_main_attempt: Arc::new(AtomicU64::new(1)),
        }
    }

    pub(crate) fn with_external_main_context(
        mut self,
        context: ExternalServerListenerContext,
    ) -> Self {
        self.external_main_context = context;
        self
    }

    pub(crate) fn authenticate_prebound_authority(&self, attempted: &Self) -> io::Result<Self> {
        self.network_authority
            .authenticate_same_authority(&attempted.network_authority)?;
        Ok(attempted.clone())
    }

    pub(crate) fn authenticate_prebound_bundle(&self, attempted: &Self) -> io::Result<()> {
        self.network_authority
            .authenticate_same_authority(&attempted.network_authority)?;
        if self.incarnation != attempted.incarnation {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pre-bound listeners belong to a different server authority incarnation",
            ));
        }
        Ok(())
    }

    pub(crate) fn owns(
        &self,
        incarnation: &str,
        network_authority: &RetainedServerNetworkAuthority,
    ) -> bool {
        self.incarnation.as_ref() == incarnation
            && self
                .network_authority
                .authenticate_same_authority(network_authority)
                .is_ok()
    }

    pub(crate) fn prepare_main(
        &self,
        requested_addr: SocketAddr,
    ) -> io::Result<PreparedServerListener> {
        let attempt = self.next_main_attempt.fetch_add(1, Ordering::Relaxed);
        self.prepare(
            &format!("main-http-attempt-{attempt}"),
            requested_addr,
            nimbus_owned_provenance(requested_addr),
        )
    }

    pub(crate) fn adopt_external_main(
        &self,
        listener: tokio::net::TcpListener,
    ) -> io::Result<LeasedServerListener> {
        let addr = listener.local_addr()?;
        if addr.port() == 0 {
            return Err(io::Error::other(
                "an externally supplied server listener must report a non-zero port",
            ));
        }
        let authority = self.network_authority.port_leases();
        authority
            .reconcile_dead_process_bound_leases()
            .map_err(network_error)?;
        let request = self.external_main_request(addr)?;
        let Some(existing) = authority
            .inspect(request.lease_id())
            .map_err(network_error)?
        else {
            return self
                .prepare_request(
                    self.network_authority.clone(),
                    request,
                    addr,
                    PortBindingProvenance::ExternallyOwned,
                )?
                .adopt(listener);
        };
        if existing.request() != &request {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "external main listener identity {} is fenced by a different durable request",
                    request.lease_id()
                ),
            ));
        }
        let expected_endpoint = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(addr.ip())?,
            NonZeroU16::new(addr.port())
                .expect("external main listener port was validated as non-zero"),
        )
        .map_err(network_error)?;
        let binding = match existing.binding().cloned() {
            Some(binding) => binding,
            None => {
                let claim = existing.bind_claim().ok_or_else(|| {
                    io::Error::other(
                        "external main listener recovery has neither adopted binding nor exact \
                         provider bind claim",
                    )
                })?;
                if claim.provider_attempt() != &self.external_main_context.provider_handle {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "supplied external main listener provider incarnation does not match its \
                         durable bind claim",
                    ));
                }
                PortLeaseBinding::new(
                    expected_endpoint.clone(),
                    PortBindingProvenance::ExternallyOwned,
                    claim.provider_attempt().clone(),
                )
            }
        };
        if binding.provenance() != PortBindingProvenance::ExternallyOwned
            || binding.endpoint() != &expected_endpoint
            || binding.provider_handle() != &self.external_main_context.provider_handle
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "supplied external main listener does not match its durable provider \
                 incarnation, generation, and binding",
            ));
        }
        let recovery = match authority
            .recover_dead_lifetime(&request)
            .map_err(network_error)?
        {
            PortLeaseRecoveryAttempt::LiveOwner(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "external main listener remains owned by a live server process",
                ));
            }
            PortLeaseRecoveryAttempt::Acquired(recovery) => recovery,
            PortLeaseRecoveryAttempt::Settled(_) => {
                return Err(io::Error::other(
                    "external main listener has terminal durable authority",
                ));
            }
        };
        let lifetime = authority
            .reclaim_provider_managed_binding_after_owner_death(&request, &binding, recovery)
            .map_err(network_error)?;
        Ok(LeasedServerListener {
            listener,
            lease: ActiveServerListenerLease {
                network_authority: self.network_authority.clone(),
                request,
                provenance: PortBindingProvenance::ExternallyOwned,
                lifetime,
                binding,
            },
            owner_incarnation: Arc::clone(&self.incarnation),
        })
    }

    pub(crate) fn prepare_sibling(
        &self,
        ordinal: usize,
        adapter_name: &str,
        requested_addr: SocketAddr,
    ) -> io::Result<PreparedServerListener> {
        self.prepare(
            &format!("wire-{ordinal}-{adapter_name}"),
            requested_addr,
            nimbus_owned_provenance(requested_addr),
        )
    }

    /// Load and authenticate one complete durable workload plan before any
    /// server-owned listener claim or socket effect begins.
    ///
    /// `ingress_requests` is only the subset this provider owns. The returned
    /// witness also retains unrelated members, such as an internal PEP lease,
    /// so later member-scoped transitions cannot redefine plan membership.
    pub(crate) fn authenticate_workload_ingress_plan(
        &self,
        plan_id: &NetworkPlanId,
        tenant_id: &nimbus_core::TenantId,
        generation: NetworkResourceGeneration,
        ingress_requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> io::Result<Vec<PortLeaseRequest>> {
        if ingress_requests.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload ingress plan has no provider-owned listener members",
            ));
        }
        let authority = self.network_authority.port_leases();
        let records = authority.list_plan(plan_id).map_err(network_error)?;
        if records.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("workload ingress plan {plan_id} has no durable lease members"),
            ));
        }
        let plan_members = records
            .iter()
            .map(|record| record.request().clone())
            .collect::<Vec<_>>();
        if plan_members.iter().any(|member| {
            member.plan_id() != Some(plan_id)
                || member.tenant_id() != Some(tenant_id)
                || member.generation() != generation
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "workload ingress plan {plan_id} has crossed tenant or generation membership"
                ),
            ));
        }
        let mut ingress_ids = BTreeSet::new();
        for request in ingress_requests {
            if !ingress_ids.insert(request.lease_id().clone())
                || request.plan_id() != Some(plan_id)
                || request.tenant_id() != Some(tenant_id)
                || request.generation() != generation
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "workload ingress request {} is crossed with plan {plan_id}",
                        request.lease_id()
                    ),
                ));
            }
            let record = authority
                .inspect_plan_member(&plan_members, request)
                .map_err(network_error)?;
            if record
                .reservation_claim()
                .is_some_and(|current| current != reservation_claim)
                || (record.phase() == PortLeasePhase::Reserved
                    && record.active_lifetime().is_none()
                    && record.reservation_claim().is_none()
                    && record.confirmed_stopped_binding().is_none())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "workload ingress request {} has crossed launch authority",
                        request.lease_id()
                    ),
                ));
            }
        }
        Ok(plan_members)
    }

    /// Claim one compiler-selected workload listener that was reserved by the
    /// sandbox launch coordinator before private attachment.
    ///
    /// A dead process-bound owner is converted to a retained rebind slot under
    /// its exact lifetime recovery guard. No numeric port scan or socket probe
    /// participates in identity or recovery.
    pub(crate) fn prepare_workload_ingress(
        &self,
        plan_members: Option<&[PortLeaseRequest]>,
        request: PortLeaseRequest,
        reservation_claim: &NetworkReservationClaim,
    ) -> io::Result<PreparedServerListener> {
        if request.accounting() != PortLeaseAccounting::TenantPublished
            || request.binding().protocol() != PortProtocol::Tcp
            || request.binding().realm() != &PortBindRealm::Host
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload ingress requires a tenant-published host TCP lease",
            ));
        }
        let host_address = request.publication().host_address().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload ingress lease omits exact host publication intent",
            )
        })?;
        if matches!(request.binding().port(), PortRequestMode::Range(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "compiled workload ingress cannot defer a ranged port request to bind time",
            ));
        }
        let authority = self.network_authority.port_leases();
        let planned_members = match (request.plan_id(), plan_members) {
            (Some(_), Some(members)) if !members.is_empty() => Some(members),
            (Some(plan_id), _) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "workload ingress lease {} requires complete plan {plan_id} membership",
                        request.lease_id()
                    ),
                ));
            }
            (None, None) => None,
            (None, Some([])) => None,
            (None, Some(_)) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unplanned server listener cannot carry a planned membership witness",
                ));
            }
        };
        let record = match planned_members {
            Some(members) => authority
                .inspect_plan_member(members, &request)
                .map_err(network_error)?,
            None => authority
                .inspect(request.lease_id())
                .map_err(network_error)?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "workload ingress lease {} was not reserved before publication",
                            request.lease_id()
                        ),
                    )
                })?,
        };
        let requested_port = match request.binding().port() {
            PortRequestMode::Exact(port) => port.get(),
            PortRequestMode::ProviderAssigned => {
                record.reserved_port().map_or(0, std::num::NonZeroU16::get)
            }
            PortRequestMode::Range(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ranged workload ingress passed prior validation",
                ));
            }
        };
        let requested_addr = SocketAddr::new(host_address, requested_port);
        if record.request() != &request {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "workload ingress lease {} is fenced by different durable authority",
                    request.lease_id()
                ),
            ));
        }

        let (active_claim, planned_authority) = match record.phase() {
            PortLeasePhase::Reserved if record.active_lifetime().is_none() => {
                match record.reservation_claim() {
                    Some(current) if current == reservation_claim => match planned_members {
                        Some(members) => (
                            None,
                            Some(PlannedWorkloadListenerAuthority::Initial {
                                plan_members: members.to_vec(),
                                reservation_claim: reservation_claim.clone(),
                            }),
                        ),
                        None => (Some(reservation_claim), None),
                    },
                    None if record.confirmed_stopped_binding().is_some() => match planned_members {
                        Some(members) => (
                            None,
                            Some(PlannedWorkloadListenerAuthority::Rebind {
                                plan_members: members.to_vec(),
                                confirmed_stopped_binding: record
                                    .confirmed_stopped_binding()
                                    .expect("the retained rebind binding was checked")
                                    .clone(),
                            }),
                        ),
                        None => (None, None),
                    },
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "workload ingress lease {} has crossed reservation authority",
                                request.lease_id()
                            ),
                        ));
                    }
                }
            }
            PortLeasePhase::Binding | PortLeasePhase::Active | PortLeasePhase::CleanupPending => {
                match planned_members {
                    Some(members) => {
                        let requested = vec![request.clone()];
                        let recoveries = match authority
                            .recover_dead_plan_members(members, &requested)
                        {
                            Ok(recoveries) => recoveries,
                            Err(PortLeaseError::LifetimeOwnerLive { .. }) => {
                                return Err(io::Error::new(
                                    io::ErrorKind::AddrInUse,
                                    format!(
                                        "workload ingress lease {} remains owned by a live listener",
                                        request.lease_id()
                                    ),
                                ));
                            }
                            Err(error) => return Err(network_error(error)),
                        };
                        if record.phase() != PortLeasePhase::CleanupPending {
                            authority
                                .mark_cleanup_pending_plan_members_after_owner_death(
                                    members,
                                    &requested,
                                    &recoveries,
                                )
                                .map_err(network_error)?;
                        }
                        let mut retained = authority
                            .prepare_rebind_process_bound_plan_members_after_owner_death(
                                members,
                                &requested,
                                &recoveries,
                            )
                            .map_err(network_error)?;
                        let retained = retained.pop().expect("one planned member was recovered");
                        let confirmed_stopped_binding = retained
                            .confirmed_stopped_binding()
                            .cloned()
                            .ok_or_else(|| {
                                io::Error::other(format!(
                                    "workload ingress lease {} lost retained binding evidence",
                                    request.lease_id()
                                ))
                            })?;
                        drop(recoveries);
                        (
                            None,
                            Some(PlannedWorkloadListenerAuthority::Rebind {
                                plan_members: members.to_vec(),
                                confirmed_stopped_binding,
                            }),
                        )
                    }
                    None => {
                        let recovery = match authority
                            .recover_dead_lifetime(&request)
                            .map_err(network_error)?
                        {
                            PortLeaseRecoveryAttempt::LiveOwner(_) => {
                                return Err(io::Error::new(
                                    io::ErrorKind::AddrInUse,
                                    format!(
                                        "workload ingress lease {} remains owned by a live listener",
                                        request.lease_id()
                                    ),
                                ));
                            }
                            PortLeaseRecoveryAttempt::Acquired(recovery) => recovery,
                            PortLeaseRecoveryAttempt::Settled(_) => {
                                return Err(io::Error::other(format!(
                                    "workload ingress lease {} became terminal during recovery",
                                    request.lease_id()
                                )));
                            }
                        };
                        if record.phase() != PortLeasePhase::CleanupPending {
                            authority
                                .mark_cleanup_pending_after_owner_death(&request, &recovery)
                                .map_err(network_error)?;
                        }
                        authority
                            .prepare_rebind_process_bound_after_owner_death(&request, &recovery)
                            .map_err(network_error)?;
                        (None, None)
                    }
                }
            }
            phase => {
                return Err(io::Error::other(format!(
                    "workload ingress lease {} cannot bind from {phase:?}",
                    request.lease_id()
                )));
            }
        };
        self.prepare_reserved_request(request, requested_addr, active_claim, planned_authority)
    }

    fn external_main_request(&self, requested_addr: SocketAddr) -> io::Result<PortLeaseRequest> {
        listener_request(
            EXTERNAL_MAIN_LISTENER_OWNER,
            EXTERNAL_MAIN_LISTENER_NAME,
            requested_addr,
            PortBindingProvenance::ExternallyOwned,
            self.external_main_context.resource_generation,
        )
    }

    fn prepare(
        &self,
        listener_name: &str,
        requested_addr: SocketAddr,
        provenance: PortBindingProvenance,
    ) -> io::Result<PreparedServerListener> {
        self.network_authority
            .port_leases()
            .reconcile_dead_process_bound_leases()
            .map_err(network_error)?;
        let request = listener_request(
            self.incarnation.as_ref(),
            listener_name,
            requested_addr,
            provenance,
            INITIAL_RESOURCE_GENERATION,
        )?;
        self.prepare_request(
            self.network_authority.clone(),
            request,
            requested_addr,
            provenance,
        )
    }

    fn prepare_request(
        &self,
        network_authority: RetainedServerNetworkAuthority,
        request: PortLeaseRequest,
        requested_addr: SocketAddr,
        provenance: PortBindingProvenance,
    ) -> io::Result<PreparedServerListener> {
        let provider_attempt = if provenance == PortBindingProvenance::ExternallyOwned {
            self.external_main_context.provider_handle.clone()
        } else {
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY),
                format!("bind-attempt:{}", Ulid::new()),
            )
            .map_err(network_error)?
        };
        let claim = PortBindClaim::new(provider_attempt);
        let attempt = PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(requested_addr.ip())?,
            requested_addr.port(),
        )
        .map_err(network_error)?;
        let reservation = network_authority
            .port_leases()
            .reserve_and_claim_bind_with_lifetime(
                request.clone(),
                claim.clone(),
                lifetime_scope(provenance),
            )
            .map_err(reservation_error)?;
        let (_, lifetime) = reservation.into_parts();
        Ok(PreparedServerListener {
            network_authority,
            request,
            reservation_claim: None,
            planned_authority: None,
            claim,
            attempt,
            provenance,
            owner_incarnation: Arc::clone(&self.incarnation),
            lifetime,
        })
    }

    fn prepare_reserved_request(
        &self,
        request: PortLeaseRequest,
        requested_addr: SocketAddr,
        reservation_claim: Option<&NetworkReservationClaim>,
        planned_authority: Option<PlannedWorkloadListenerAuthority>,
    ) -> io::Result<PreparedServerListener> {
        let provider_attempt = NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY),
            format!("workload-bind:{}:{}", request.lease_id(), Ulid::new()),
        )
        .map_err(network_error)?;
        let claim = PortBindClaim::new(provider_attempt);
        let attempt = PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(requested_addr.ip())?,
            requested_addr.port(),
        )
        .map_err(network_error)?;
        let authority = self.network_authority.port_leases();
        let lifetime = match planned_authority.as_ref() {
            Some(PlannedWorkloadListenerAuthority::Initial {
                plan_members,
                reservation_claim,
            }) => authority.claim_bind_plan_member_with_lifetime(
                plan_members,
                &request,
                reservation_claim,
                claim.clone(),
                PortLeaseEffectScope::ProcessBound,
            ),
            Some(PlannedWorkloadListenerAuthority::Rebind {
                plan_members,
                confirmed_stopped_binding,
            }) => authority.claim_rebind_plan_member_with_lifetime(
                plan_members,
                &request,
                confirmed_stopped_binding,
                claim.clone(),
                PortLeaseEffectScope::ProcessBound,
            ),
            None => authority.claim_bind_with_lifetime(
                &request,
                reservation_claim,
                claim.clone(),
                PortLeaseEffectScope::ProcessBound,
            ),
        }
        .map_err(reservation_error)?;
        let provenance = if matches!(request.binding().port(), PortRequestMode::ProviderAssigned) {
            PortBindingProvenance::ProviderAssigned
        } else {
            PortBindingProvenance::NimbusOwned
        };
        Ok(PreparedServerListener {
            network_authority: self.network_authority.clone(),
            request,
            reservation_claim: reservation_claim.cloned(),
            planned_authority,
            claim,
            attempt,
            provenance,
            owner_incarnation: Arc::clone(&self.incarnation),
            lifetime,
        })
    }
}

fn listener_request(
    identity_owner: &str,
    listener_name: &str,
    requested_addr: SocketAddr,
    provenance: PortBindingProvenance,
    resource_generation: NetworkResourceGeneration,
) -> io::Result<PortLeaseRequest> {
    let listener_id = ListenerId::for_workload_listener(identity_owner, listener_name);
    Ok(PortLeaseRequest::new(
        nimbus_network::PortLeaseId::for_listener(&listener_id),
        listener_id.into(),
        None,
        PortLeaseFence::new(resource_generation, INITIAL_LEASE_EPOCH),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(requested_addr.ip())?,
            exposure(requested_addr.ip()),
            request_mode(requested_addr, provenance)?,
        ),
    ))
}

fn settle_claim_without_effect(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    claim: &PortBindClaim,
    provenance: PortBindingProvenance,
    lifetime: &PortLeaseLifetimeGuard,
) -> io::Result<()> {
    authority
        .abandon_bind_with_lifetime_without_effect(request, reservation_claim, claim, lifetime)
        .map_err(network_error)?;
    authority.withdraw(request).map_err(network_error)?;
    if provenance != PortBindingProvenance::ExternallyOwned {
        authority.release(request).map_err(network_error)?;
    }
    Ok(())
}

fn request_mode(
    addr: SocketAddr,
    provenance: PortBindingProvenance,
) -> io::Result<PortRequestMode> {
    if provenance == PortBindingProvenance::ExternallyOwned {
        return NonZeroU16::new(addr.port())
            .map(PortRequestMode::Exact)
            .ok_or_else(|| io::Error::other("external listeners cannot adopt port zero"));
    }
    Ok(NonZeroU16::new(addr.port())
        .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact))
}

fn nimbus_owned_provenance(addr: SocketAddr) -> PortBindingProvenance {
    if addr.port() == 0 {
        PortBindingProvenance::ProviderAssigned
    } else {
        PortBindingProvenance::NimbusOwned
    }
}

fn lifetime_scope(provenance: PortBindingProvenance) -> PortLeaseEffectScope {
    if provenance == PortBindingProvenance::ExternallyOwned {
        PortLeaseEffectScope::ProviderManaged
    } else {
        PortLeaseEffectScope::ProcessBound
    }
}

fn bind_target(address: IpAddr) -> io::Result<PortBindTarget> {
    match canonical_ip(address) {
        IpAddr::V4(address) if address.is_unspecified() => Ok(PortBindTarget::ipv4_wildcard()),
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) if address.is_unspecified() => {
            Ok(PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown))
        }
        IpAddr::V6(address) => {
            PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown).map_err(network_error)
        }
    }
}

fn canonical_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    }
}

fn exposure(address: IpAddr) -> PortExposure {
    match canonical_ip(address) {
        IpAddr::V4(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V6(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if ipv4_is_private(address) => PortExposure::Private,
        IpAddr::V6(address) if ipv6_is_private(address) => PortExposure::Private,
        _ => PortExposure::Public,
    }
}

fn ipv4_is_private(address: Ipv4Addr) -> bool {
    address.is_private() || address.is_link_local()
}

fn ipv6_is_private(address: Ipv6Addr) -> bool {
    address.is_unique_local() || address.is_unicast_link_local()
}

fn bind_failure_kind(kind: io::ErrorKind) -> PortBindFailureKind {
    match kind {
        io::ErrorKind::AddrInUse => PortBindFailureKind::AddrInUse,
        io::ErrorKind::PermissionDenied => PortBindFailureKind::PermissionDenied,
        io::ErrorKind::AddrNotAvailable => PortBindFailureKind::AddressNotAvailable,
        io::ErrorKind::Unsupported => PortBindFailureKind::Unsupported,
        io::ErrorKind::OutOfMemory | io::ErrorKind::WouldBlock => {
            PortBindFailureKind::ResourceExhausted
        }
        _ => PortBindFailureKind::Other,
    }
}

fn network_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn reservation_error(error: PortLeaseError) -> io::Error {
    let kind = if matches!(error, PortLeaseError::PortConflict { .. }) {
        io::ErrorKind::AddrInUse
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, error.to_string())
}

pub(crate) fn abandon_prepared_after_guard_failure(
    prepared: PreparedServerListener,
    listener: tokio::net::TcpListener,
) -> io::Result<()> {
    drop(listener);
    prepared.abandon_after_confirmed_close()
}

#[cfg(test)]
#[path = "listener_lease/tests.rs"]
mod tests;
