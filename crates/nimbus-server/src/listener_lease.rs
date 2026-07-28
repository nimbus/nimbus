//! Server-side adapter between durable port authority and TCP socket effects.
//!
//! `nimbus-network` owns portable identity and lifecycle state. This module
//! translates real Tokio listener observations into that vocabulary while
//! leaving every kernel bind in its existing effect owner.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, PortBindAttempt, PortBindClaim, PortBindFailure,
    PortBindFailureKind, PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec,
    PortBoundEndpoint, PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseBinding,
    PortLeaseEffectScope, PortLeaseError, PortLeaseFence, PortLeaseLifetimeGuard,
    PortLeaseRecoveryAttempt, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};
use ulid::Ulid;

const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
const SERVER_LISTENER_PROVIDER_KEY: &str = "nimbus-server.tcp-listener";
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
    authority: LocalPortLeaseAuthority,
    request: PortLeaseRequest,
    claim: PortBindClaim,
    attempt: PortBindAttempt,
    provenance: PortBindingProvenance,
    owner_incarnation: Arc<str>,
    lifetime: PortLeaseLifetimeGuard,
}

impl PreparedServerListener {
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
        match self
            .authority
            .record_claimed_bind_failure_with_lifetime_without_effect(
                &self.request,
                None,
                &self.claim,
                failure,
                &self.lifetime,
            ) {
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
        if let Err(error) = self.authority.adopt_claimed_and_activate_with_lifetime(
            &self.request,
            None,
            &self.claim,
            binding.clone(),
            &self.lifetime,
        ) {
            return Err(self.close_after_failed_adoption(listener, network_error(error)));
        }
        Ok(LeasedServerListener {
            listener,
            lease: ActiveServerListenerLease {
                authority: self.authority,
                request: self.request,
                provenance: self.provenance,
                lifetime: self.lifetime,
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
        if let Err(error) = self.authority.adopt_claimed_and_activate_with_lifetime(
            &self.request,
            None,
            &self.claim,
            binding,
            &self.lifetime,
        ) {
            return Err(self.close_std_after_failed_adoption(listener, network_error(error)));
        }
        Ok(PreboundServerListener {
            listener,
            lease: ActiveServerListenerLease {
                authority: self.authority,
                request: self.request,
                provenance: self.provenance,
                lifetime: self.lifetime,
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
        settle_claim_without_effect(
            &self.authority,
            &self.request,
            &self.claim,
            self.provenance,
            &self.lifetime,
        )
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

    pub(crate) fn into_parts(
        self,
    ) -> (tokio::net::TcpListener, ActiveServerListenerLease, Arc<str>) {
        (self.listener, self.lease, self.owner_incarnation)
    }
}

pub(crate) struct ActiveServerListenerLease {
    authority: LocalPortLeaseAuthority,
    request: PortLeaseRequest,
    provenance: PortBindingProvenance,
    lifetime: PortLeaseLifetimeGuard,
}

impl ActiveServerListenerLease {
    pub(crate) fn settle_after_confirmed_local_close(self) -> io::Result<()> {
        debug_assert_eq!(self.lifetime.request(), &self.request);
        self.authority
            .withdraw(&self.request)
            .map_err(network_error)?;
        if self.provenance != PortBindingProvenance::ExternallyOwned {
            self.authority
                .release_with_lifetime(&self.request, &self.lifetime)
                .map_err(network_error)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ServerListenerLeaseAuthority {
    state_root: PathBuf,
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
    /// Create one server listener authority for an upcoming serve
    /// incarnation.
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            authority: ServerListenerLeaseAuthority::new(state_root),
            listeners: BTreeMap::new(),
        }
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
        if !self.authority.owns(listener.owner_incarnation()) {
            let primary = io::Error::other(format!(
                "pre-bound {adapter_name} listener belongs to a different server authority"
            ));
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
    pub(crate) fn new(state_root: impl Into<PathBuf>) -> Self {
        let incarnation: Arc<str> = Arc::from(format!("server:{}", Ulid::new()));
        let external_main_context = ExternalServerListenerContext::new(
            format!("process-owned:{incarnation}"),
            INITIAL_RESOURCE_GENERATION,
        )
        .expect("a generated server incarnation is valid provider context");
        Self {
            state_root: state_root.into(),
            incarnation,
            external_main_context,
            next_main_attempt: Arc::new(AtomicU64::new(1)),
        }
    }

    pub(crate) fn with_state_root(mut self, state_root: impl Into<PathBuf>) -> Self {
        self.state_root = state_root.into();
        self
    }

    pub(crate) fn with_external_main_context(
        mut self,
        context: ExternalServerListenerContext,
    ) -> Self {
        self.external_main_context = context;
        self
    }

    pub(crate) fn owns(&self, incarnation: &str) -> bool {
        self.incarnation.as_ref() == incarnation
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
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(network_error)?;
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
                    authority,
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
                authority,
                request,
                provenance: PortBindingProvenance::ExternallyOwned,
                lifetime,
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
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(network_error)?;
        authority
            .reconcile_dead_process_bound_leases()
            .map_err(network_error)?;
        let request = listener_request(
            self.incarnation.as_ref(),
            listener_name,
            requested_addr,
            provenance,
            INITIAL_RESOURCE_GENERATION,
        )?;
        self.prepare_request(authority, request, requested_addr, provenance)
    }

    fn prepare_request(
        &self,
        authority: LocalPortLeaseAuthority,
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
        let reservation = authority
            .reserve_and_claim_bind_with_lifetime(
                request.clone(),
                claim.clone(),
                lifetime_scope(provenance),
            )
            .map_err(reservation_error)?;
        let (_, lifetime) = reservation.into_parts();
        Ok(PreparedServerListener {
            authority,
            request,
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
    claim: &PortBindClaim,
    provenance: PortBindingProvenance,
    lifetime: &PortLeaseLifetimeGuard,
) -> io::Result<()> {
    authority
        .abandon_bind_with_lifetime_without_effect(request, None, claim, lifetime)
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
mod tests {
    use nimbus_network::{PortBindingProvenance, PortLeaseEffectScope, PortLeasePhase};

    use super::*;

    fn external_context(incarnation: &str, generation: u64) -> ExternalServerListenerContext {
        ExternalServerListenerContext::new(
            format!("test-external:{incarnation}"),
            NetworkResourceGeneration::new(generation),
        )
        .expect("fixture external-listener context should validate")
    }

    #[tokio::test]
    async fn provider_assigned_bind_is_claimed_before_effect_and_released_after_close() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let authority = ServerListenerLeaseAuthority::new(state_root.path());
        let requested_addr = "127.0.0.1:0".parse().expect("fixture address should parse");

        let prepared = authority
            .prepare_main(requested_addr)
            .expect("provider-assigned listener should prepare");
        let durable = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should open")
            .list()
            .expect("port records should list");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].phase(), PortLeasePhase::Reserved);
        assert!(
            durable[0].bind_claim().is_some(),
            "the durable bind claim must precede the kernel effect"
        );
        assert_eq!(durable[0].reserved_port(), None);

        let raw = tokio::net::TcpListener::bind(requested_addr)
            .await
            .expect("kernel should assign a listener");
        let actual_addr = raw.local_addr().expect("bound address should resolve");
        let leased = prepared
            .adopt(raw)
            .expect("concrete listener should adopt and activate");
        let active = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(active[0].phase(), PortLeasePhase::Active);
        assert_eq!(
            active[0]
                .binding()
                .expect("Active lease should retain binding evidence")
                .actual_port()
                .get(),
            actual_addr.port()
        );
        assert_eq!(
            active[0]
                .binding()
                .expect("Active lease should retain binding evidence")
                .provenance(),
            PortBindingProvenance::ProviderAssigned
        );

        leased
            .close_and_settle()
            .expect("confirmed close should release authority");
        let released = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(released[0].phase(), PortLeasePhase::Released);
    }

    #[tokio::test]
    async fn exact_bind_collision_records_terminal_no_effect_failure() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let external = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let occupied_addr = external
            .local_addr()
            .expect("external address should resolve");
        let prepared = ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(occupied_addr)
            .expect("exact durable request should prepare before the kernel collision");

        let error = tokio::net::TcpListener::bind(occupied_addr)
            .await
            .expect_err("the external owner must win the real kernel bind");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        let returned = prepared
            .record_bind_failure(error)
            .expect("durable failure receipt should commit")
            .into_error();
        assert_eq!(returned.kind(), io::ErrorKind::AddrInUse);

        let durable = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].phase(), PortLeasePhase::Failed);
        assert_eq!(
            durable[0]
                .failure()
                .expect("failed lease should retain exact evidence")
                .kind(),
            PortBindFailureKind::AddrInUse
        );
        assert_eq!(
            durable[0]
                .failure()
                .expect("failed lease should retain exact evidence")
                .attempt()
                .port(),
            occupied_addr.port()
        );
    }

    #[tokio::test]
    async fn external_listener_adoption_records_external_provenance() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let actual_addr = raw.local_addr().expect("bound address should resolve");
        let leased = ServerListenerLeaseAuthority::new(state_root.path())
            .adopt_external_main(raw)
            .expect("external listener should adopt");

        let active = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].phase(), PortLeasePhase::Active);
        let binding = active[0]
            .binding()
            .expect("Active external lease should retain binding evidence");
        assert_eq!(binding.actual_port().get(), actual_addr.port());
        assert_eq!(binding.provenance(), PortBindingProvenance::ExternallyOwned);

        leased
            .close_and_settle()
            .expect("local close should withdraw external adoption");
        let withdrawn = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(withdrawn[0].phase(), PortLeasePhase::Withdrawing);
        assert_eq!(
            withdrawn[0]
                .binding()
                .expect("withdrawn external fence should retain binding evidence")
                .provenance(),
            PortBindingProvenance::ExternallyOwned
        );
    }

    #[tokio::test]
    async fn dead_process_bound_listener_drop_reconciles_before_next_prepare() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let first_authority = ServerListenerLeaseAuthority::new(state_root.path());
        let requested_addr = "127.0.0.1:0".parse().expect("fixture address should parse");
        let first_prepared = first_authority
            .prepare_main(requested_addr)
            .expect("first listener should prepare");
        let first_raw = tokio::net::TcpListener::bind(requested_addr)
            .await
            .expect("first listener should bind");
        let actual_addr = first_raw
            .local_addr()
            .expect("bound address should resolve");
        let first = first_prepared
            .adopt(first_raw)
            .expect("first listener should activate");
        drop(first);

        let retained = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].phase(), PortLeasePhase::Active);
        assert_eq!(
            retained[0]
                .active_lifetime()
                .expect("dropped listener must retain lifetime evidence")
                .effect_scope(),
            PortLeaseEffectScope::ProcessBound
        );

        let second_prepared = ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(actual_addr)
            .expect("fresh preparation should reconcile the dead process-bound owner");
        let second_raw = tokio::net::TcpListener::bind(actual_addr)
            .await
            .expect("replacement listener should bind the released port");
        let second = second_prepared
            .adopt(second_raw)
            .expect("replacement listener should activate");
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(
            records
                .iter()
                .filter(|record| record.phase() == PortLeasePhase::Released)
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.phase() == PortLeasePhase::Active)
                .count(),
            1
        );
        second
            .close_and_settle()
            .expect("replacement should close cleanly");
    }

    #[tokio::test]
    async fn external_listener_drop_remains_provider_managed_and_fenced() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let actual_addr = raw.local_addr().expect("bound address should resolve");
        let leased = ServerListenerLeaseAuthority::new(state_root.path())
            .adopt_external_main(raw)
            .expect("external listener should adopt");
        drop(leased);

        let error =
            match ServerListenerLeaseAuthority::new(state_root.path()).prepare_main(actual_addr) {
                Ok(_) => panic!("process death cannot release an external adoption"),
                Err(error) => error,
            };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        let retained = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].phase(), PortLeasePhase::Active);
        assert_eq!(
            retained[0]
                .active_lifetime()
                .expect("external adoption must retain lifetime evidence")
                .effect_scope(),
            PortLeaseEffectScope::ProviderManaged
        );
    }

    #[tokio::test]
    async fn fresh_authority_reclaims_the_same_surviving_external_listener() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let context = external_context("inherited-main", 1);
        let external_owner =
            std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
        external_owner
            .set_nonblocking(true)
            .expect("external listener should become nonblocking");
        let inherited = external_owner
            .try_clone()
            .expect("fresh process fixture should inherit the same listener");
        let addr = external_owner
            .local_addr()
            .expect("external address should resolve");
        let first = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context.clone())
            .adopt_external_main(
                tokio::net::TcpListener::from_std(external_owner)
                    .expect("first process should adopt its descriptor"),
            )
            .expect("first external owner should activate");
        let first_lifetime = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list")[0]
            .active_lifetime()
            .expect("first external owner should carry a lifetime");
        drop(first);

        let second = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context)
            .adopt_external_main(
                tokio::net::TcpListener::from_std(inherited)
                    .expect("fresh process should adopt the inherited descriptor"),
            )
            .expect("fresh authority should reclaim the exact surviving listener");
        assert_eq!(second.local_addr().expect("listener should inspect"), addr);
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1, "recovery must not fork listener identity");
        assert_eq!(records[0].phase(), PortLeasePhase::Active);
        assert!(
            records[0]
                .active_lifetime()
                .expect("replacement owner should carry a lifetime")
                .generation()
                > first_lifetime.generation(),
            "fresh ownership must fence the dead server generation"
        );

        second
            .close_and_settle()
            .expect("fresh external owner should withdraw cleanly");
    }

    #[tokio::test]
    async fn rebound_same_address_external_listener_cannot_reclaim_prior_provider_incarnation() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let original_context = external_context("original-main", 1);
        let original = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("original external owner should bind");
        original
            .set_nonblocking(true)
            .expect("original listener should become nonblocking");
        let addr = original
            .local_addr()
            .expect("original address should resolve");
        let first = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(original_context)
            .adopt_external_main(
                tokio::net::TcpListener::from_std(original)
                    .expect("first process should adopt the original descriptor"),
            )
            .expect("original external owner should activate");
        drop(first);

        let rebound = std::net::TcpListener::bind(addr)
            .expect("a newly created provider socket should rebind the released address");
        rebound
            .set_nonblocking(true)
            .expect("rebound listener should become nonblocking");
        let before = LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .list()
            .expect("port records should list");
        let error = match ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(external_context("replacement-main", 1))
            .adopt_external_main(
                tokio::net::TcpListener::from_std(rebound)
                    .expect("replacement listener should enter Tokio"),
            ) {
            Ok(_) => panic!("a new provider incarnation must not inherit old listener authority"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(
            LocalPortLeaseAuthority::open(state_root.path())
                .expect("portable authority should reopen")
                .list()
                .expect("port records should list"),
            before,
            "provider-incarnation substitution must not mutate durable authority"
        );
    }

    #[tokio::test]
    async fn external_listener_recovery_rejects_stale_provider_generation() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let original =
            std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
        original
            .set_nonblocking(true)
            .expect("external listener should become nonblocking");
        let inherited = original
            .try_clone()
            .expect("the fixture should inherit the exact same descriptor");
        let first = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(external_context("stable-main", 2))
            .adopt_external_main(
                tokio::net::TcpListener::from_std(original)
                    .expect("first process should adopt the descriptor"),
            )
            .expect("current provider generation should activate");
        drop(first);

        let before = LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .list()
            .expect("port records should list");
        let error = match ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(external_context("stable-main", 1))
            .adopt_external_main(
                tokio::net::TcpListener::from_std(inherited)
                    .expect("stale contender should adopt its cloned descriptor"),
            ) {
            Ok(_) => panic!("a stale provider generation must not reclaim the descriptor"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(
            LocalPortLeaseAuthority::open(state_root.path())
                .expect("portable authority should reopen")
                .list()
                .expect("port records should list"),
            before,
            "stale-generation rejection must not mutate durable authority"
        );
    }

    #[tokio::test]
    async fn external_main_pre_adoption_crash_reclaims_supplied_descriptor() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let context = external_context("pre-adoption-main", 1);
        let external =
            std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
        external
            .set_nonblocking(true)
            .expect("external listener should become nonblocking");
        let addr = external
            .local_addr()
            .expect("external address should resolve");
        let first_authority = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context.clone());
        let request = first_authority
            .external_main_request(addr)
            .expect("external request should build");
        let prepared = first_authority
            .prepare_request(
                LocalPortLeaseAuthority::open(state_root.path())
                    .expect("portable authority should open"),
                request.clone(),
                addr,
                PortBindingProvenance::ExternallyOwned,
            )
            .expect("first process should durably claim before adoption");
        let first_claim = prepared.claim.clone();
        let first_lifetime = prepared.lifetime.lifetime();
        drop(prepared);

        let reclaimed = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context)
            .adopt_external_main(
                tokio::net::TcpListener::from_std(external)
                    .expect("replacement should adopt the inherited descriptor"),
            )
            .expect("dead pre-adoption owner should be reclaimed");
        assert_eq!(
            reclaimed.local_addr().expect("listener should inspect"),
            addr
        );
        let record = LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .inspect(request.lease_id())
            .expect("external request should inspect")
            .expect("external request should remain");
        assert_eq!(record.phase(), PortLeasePhase::Active);
        assert_eq!(record.adoption_claim(), Some(&first_claim));
        assert!(record.bind_claim().is_none());
        assert_eq!(
            record
                .binding()
                .expect("reclaimed request should carry binding")
                .provider_handle(),
            first_claim.provider_attempt()
        );
        assert!(
            record
                .active_lifetime()
                .expect("replacement should own a lifetime")
                .generation()
                > first_lifetime.generation()
        );

        reclaimed
            .close_and_settle()
            .expect("external replacement should withdraw cleanly");
    }

    #[tokio::test]
    async fn external_main_pre_adoption_live_owner_rejects_reclaim() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let context = external_context("live-pre-adoption-main", 1);
        let external =
            std::net::TcpListener::bind("127.0.0.1:0").expect("external owner should bind");
        external
            .set_nonblocking(true)
            .expect("external listener should become nonblocking");
        let inherited = external
            .try_clone()
            .expect("contender should receive the same descriptor");
        let addr = external
            .local_addr()
            .expect("external address should resolve");
        let first_authority = ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context.clone());
        let request = first_authority
            .external_main_request(addr)
            .expect("external request should build");
        let prepared = first_authority
            .prepare_request(
                LocalPortLeaseAuthority::open(state_root.path())
                    .expect("portable authority should open"),
                request.clone(),
                addr,
                PortBindingProvenance::ExternallyOwned,
            )
            .expect("first process should retain its live pre-adoption claim");
        let before = LocalPortLeaseAuthority::open(state_root.path())
            .expect("portable authority should reopen")
            .inspect(request.lease_id())
            .expect("external request should inspect")
            .expect("external request should remain");

        let error = match ServerListenerLeaseAuthority::new(state_root.path())
            .with_external_main_context(context)
            .adopt_external_main(
                tokio::net::TcpListener::from_std(inherited)
                    .expect("contender should adopt its cloned descriptor"),
            ) {
            Ok(_) => panic!("a live pre-adoption owner must reject reclaim"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(
            LocalPortLeaseAuthority::open(state_root.path())
                .expect("portable authority should reopen")
                .inspect(request.lease_id())
                .expect("rejected request should inspect"),
            Some(before),
            "live-owner rejection must not mutate durable authority"
        );
        drop(prepared);
        drop(external);
    }

    #[tokio::test]
    async fn adoption_failure_closes_socket_and_releases_never_bound_owned_claim() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let requested_owner = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("requested-port selector should bind");
        let requested_addr = requested_owner
            .local_addr()
            .expect("requested address should resolve");
        let prepared = ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(requested_addr)
            .expect("exact request should prepare independently of the kernel owner");
        let wrong_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mismatched listener should bind");
        let wrong_addr = wrong_listener
            .local_addr()
            .expect("mismatched listener address should resolve");

        let error = match prepared.adopt(wrong_listener) {
            Ok(_) => panic!("an exact request must reject a listener on another port"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        let durable = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].phase(), PortLeasePhase::Released);
        tokio::net::TcpListener::bind(wrong_addr)
            .await
            .expect("failed adoption must close the concrete listener");
    }

    #[tokio::test]
    async fn durable_reservation_conflict_is_reported_as_addr_in_use() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let selector = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("port selector should bind");
        let requested_addr = selector
            .local_addr()
            .expect("selected address should resolve");
        let _winner = ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(requested_addr)
            .expect("first authority should prepare");

        let error = match ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(requested_addr)
        {
            Ok(_) => panic!("a second durable owner must lose the exact port"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    }

    #[tokio::test]
    async fn bind_failure_receipt_error_is_distinguishable_from_recorded_collision() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let external = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let occupied_addr = external
            .local_addr()
            .expect("external address should resolve");
        let prepared = ServerListenerLeaseAuthority::new(state_root.path())
            .prepare_main(occupied_addr)
            .expect("exact durable request should prepare");
        let error = tokio::net::TcpListener::bind(occupied_addr)
            .await
            .expect_err("the external owner must win the real kernel bind");
        std::fs::write(
            state_root
                .path()
                .join("networks")
                .join("control-plane")
                .join("state.json"),
            b"corrupt after prepare",
        )
        .expect("authority fixture should corrupt");

        let receipt_error = prepared
            .record_bind_failure(error)
            .expect_err("a corrupt authority must not masquerade as a recorded bind failure");
        assert_eq!(receipt_error.kind(), io::ErrorKind::AddrInUse);
        assert!(
            receipt_error
                .to_string()
                .contains("failed to record durable no-effect bind failure")
        );
    }

    #[test]
    fn dropping_prebound_bundle_closes_socket_and_settles_active_lease() {
        let state_root = tempfile::tempdir().expect("state root should be created");
        let mut listeners = PreboundServerListeners::new(state_root.path());
        let requested_addr = "127.0.0.1:0"
            .parse()
            .expect("provider-assigned address should parse");
        let prepared = listeners
            .prepare("dev-mongodb-provider-assigned", requested_addr)
            .expect("pre-bound listener should reserve");
        let raw = std::net::TcpListener::bind(requested_addr)
            .expect("provider should bind its requested socket");
        let listener = prepared
            .adopt_std(raw)
            .expect("pre-bound listener should activate");
        let actual_addr = listener
            .local_addr()
            .expect("pre-bound address should resolve");
        listeners
            .insert("mongodb", listener)
            .expect("listener should enter the handoff bundle");

        drop(listeners);

        std::net::TcpListener::bind(actual_addr)
            .expect("dropping pre-serve ownership must close the retained socket");
        let records = LocalPortLeaseAuthority::open(state_root.path())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Released);
    }
}
