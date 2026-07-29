//! Standalone RESP listener identity and socket wrapper.
//!
//! `nimbus-kv` retains every TCP effect. This module is the local
//! ports-and-adapters boundary through which those effects consume portable
//! `nimbus-network` identity and durable authority.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::path::Path;

use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalPortLeaseAuthority, NetworkLeaseEpoch,
    NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration, PortBindAttempt,
    PortBindClaim, PortBindFailure, PortBindFailureKind, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure, PortIpv6Overlap,
    PortLeaseAccounting, PortLeaseBinding, PortLeaseEffectScope, PortLeaseError, PortLeaseFence,
    PortLeaseId, PortLeaseLifetimeGuard, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};
use tokio::net::{TcpListener, TcpStream};
use ulid::Ulid;

use crate::KvError;

const KV_LISTENER_PROVIDER_KEY: &str = "nimbus-kv.tcp-listener";

/// Retained authority for one standalone KV listener lifecycle.
#[derive(Debug, Clone)]
enum NimbusKvListenerAuthority {
    ManagerDerived(LocalNetworkAuthority),
    Direct(LocalPortLeaseAuthority),
}

impl NimbusKvListenerAuthority {
    fn port_leases(&self) -> LocalPortLeaseAuthority {
        match self {
            Self::ManagerDerived(authority) => authority.port_leases(),
            Self::Direct(authority) => authority.clone(),
        }
    }
}

/// Stable identity and retained authority for one standalone KV listener
/// incarnation.
#[derive(Debug, Clone)]
pub struct NimbusKvListenerConfig {
    authority: NimbusKvListenerAuthority,
    listener_id: ListenerId,
    fence: PortLeaseFence,
}

impl NimbusKvListenerConfig {
    /// Create a fresh listener identity under the process manager's authority.
    #[must_use]
    pub fn from_network_authority(authority: LocalNetworkAuthority) -> Self {
        Self::from_network_authority_for_incarnation(
            authority,
            format!("standalone-kv:{}", Ulid::new()),
        )
    }

    /// Create a deterministic identity under the process manager's authority.
    ///
    /// Orchestrators and deterministic process tests may supply an
    /// address-independent incarnation. A fresh launch must use a fresh value
    /// unless durable generation handoff deliberately replays an existing
    /// incarnation.
    #[must_use]
    pub fn from_network_authority_for_incarnation(
        authority: LocalNetworkAuthority,
        incarnation: impl AsRef<str>,
    ) -> Self {
        Self::with_authority(
            NimbusKvListenerAuthority::ManagerDerived(authority),
            incarnation,
        )
    }

    /// Explicitly reconstruct primitive listener authority from a durable root.
    ///
    /// Production composition must inject [`LocalNetworkAuthority`] through
    /// [`Self::from_network_authority`]. This fallible seam exists for tests,
    /// embedders, and deliberate restart recovery that do not own a process
    /// manager.
    pub fn reconstruct_direct(state_root: impl AsRef<Path>) -> Result<Self, PortLeaseError> {
        Self::reconstruct_direct_for_incarnation(
            state_root,
            format!("standalone-kv:{}", Ulid::new()),
        )
    }

    /// Explicitly reconstruct primitive authority with a deterministic
    /// listener incarnation.
    pub fn reconstruct_direct_for_incarnation(
        state_root: impl AsRef<Path>,
        incarnation: impl AsRef<str>,
    ) -> Result<Self, PortLeaseError> {
        let authority = LocalPortLeaseAuthority::open(state_root)?;
        Ok(Self::with_authority(
            NimbusKvListenerAuthority::Direct(authority),
            incarnation,
        ))
    }

    fn with_authority(authority: NimbusKvListenerAuthority, incarnation: impl AsRef<str>) -> Self {
        Self {
            authority,
            listener_id: ListenerId::for_workload_listener(incarnation.as_ref(), "resp"),
            fence: PortLeaseFence::new(
                NetworkResourceGeneration::new(1),
                NetworkLeaseEpoch::new(1),
            ),
        }
    }

    fn authority(&self) -> NimbusKvListenerAuthority {
        self.authority.clone()
    }

    /// Address-independent owner identity for this listener incarnation.
    pub fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    /// Desired generation and lease epoch for this incarnation.
    pub const fn fence(&self) -> PortLeaseFence {
        self.fence
    }
}

/// One durable claim prepared before a standalone KV bind or adoption.
///
/// Dropping a prepared claim intentionally retains its fence. Every
/// synchronous no-effect path instead records failure or settles the exact
/// claim; ambiguous interruption is reconciled by NNC3.8.
struct PreparedKvListener {
    authority: NimbusKvListenerAuthority,
    request: PortLeaseRequest,
    claim: PortBindClaim,
    attempt: PortBindAttempt,
    provenance: PortBindingProvenance,
    lifetime: PortLeaseLifetimeGuard,
}

impl PreparedKvListener {
    fn prepare(
        config: &NimbusKvListenerConfig,
        requested_addr: SocketAddr,
        provenance: PortBindingProvenance,
    ) -> Result<Self, KvError> {
        let target = bind_target(requested_addr.ip())?;
        let request = PortLeaseRequest::new(
            PortLeaseId::for_listener(config.listener_id()),
            config.listener_id().clone().into(),
            None,
            config.fence(),
            PortLeaseAccounting::HostInternal,
            PortPublicationIntent::Unpublished,
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                target.clone(),
                PortExposure::Loopback,
                request_mode(requested_addr, provenance)?,
            ),
        );
        let provider_attempt = NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(KV_LISTENER_PROVIDER_KEY),
            format!("bind-attempt:{}", Ulid::new()),
        )
        .map_err(other_io)?;
        let claim = PortBindClaim::new(provider_attempt);
        let attempt = PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            target,
            requested_addr.port(),
        )
        .map_err(other_io)?;
        let authority = config.authority();
        let port_leases = authority.port_leases();
        port_leases.reconcile_dead_process_bound_leases()?;
        let reservation = port_leases.reserve_and_claim_bind_with_lifetime(
            request.clone(),
            claim.clone(),
            lifetime_scope(provenance),
        )?;
        let (_, lifetime) = reservation.into_parts();
        Ok(Self {
            authority,
            request,
            claim,
            attempt,
            provenance,
            lifetime,
        })
    }

    fn record_bind_failure(self, error: io::Error) -> KvError {
        let failure = PortBindFailure::new(
            bind_failure_kind(error.kind()),
            self.attempt.clone(),
            self.claim.provider_attempt().clone(),
        );
        match self
            .authority
            .port_leases()
            .record_claimed_bind_failure_with_lifetime_without_effect(
                &self.request,
                None,
                &self.claim,
                failure,
                &self.lifetime,
            ) {
            Ok(_) => error.into(),
            Err(cleanup) => KvError::ListenerLifecycle {
                primary: Box::new(error.into()),
                context: "failed to record the durable no-effect bind receipt",
                cleanup: Box::new(cleanup),
            },
        }
    }

    fn adopt(self, listener: TcpListener) -> Result<NimbusKvListener, KvError> {
        let binding = match self.binding_for_listener(&listener) {
            Ok(binding) => binding,
            Err(primary) => return Err(self.close_after_failed_adoption(listener, primary)),
        };
        if let Err(primary) = self
            .authority
            .port_leases()
            .adopt_claimed_and_activate_with_lifetime(
                &self.request,
                None,
                &self.claim,
                binding,
                &self.lifetime,
            )
        {
            return Err(self.close_after_failed_adoption(listener, primary.into()));
        }
        Ok(NimbusKvListener {
            listener,
            authority: self.authority,
            request: self.request,
            provenance: self.provenance,
            lifetime: self.lifetime,
        })
    }

    fn binding_for_listener(&self, listener: &TcpListener) -> Result<PortLeaseBinding, KvError> {
        let actual_addr = listener.local_addr()?;
        let actual_port = NonZeroU16::new(actual_addr.port())
            .ok_or_else(|| io::Error::other("a bound standalone KV listener reported port zero"))?;
        let endpoint = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            bind_target(actual_addr.ip())?,
            actual_port,
        )
        .map_err(other_io)?;
        Ok(PortLeaseBinding::new(
            endpoint,
            self.provenance,
            self.claim.provider_attempt().clone(),
        ))
    }

    fn close_after_failed_adoption(self, listener: TcpListener, primary: KvError) -> KvError {
        drop(listener);
        match settle_claim_without_effect(
            &self.authority,
            &self.request,
            &self.claim,
            self.provenance,
            &self.lifetime,
        ) {
            Ok(()) => primary,
            Err(cleanup) => KvError::ListenerLifecycle {
                primary: Box::new(primary),
                context: "failed to settle the claimed listener after adoption failed",
                cleanup: Box::new(cleanup),
            },
        }
    }
}

/// Concrete RESP socket owned by `nimbus-kv`.
///
/// Dropping this wrapper closes Nimbus's local descriptor but deliberately
/// retains the Active durable fence: task cancellation does not prove whether
/// a duplicated or inherited provider handle still exists. Confirmed
/// synchronous shutdown paths use [`Self::close_and_settle`].
pub struct NimbusKvListener {
    listener: TcpListener,
    authority: NimbusKvListenerAuthority,
    request: PortLeaseRequest,
    provenance: PortBindingProvenance,
    lifetime: PortLeaseLifetimeGuard,
}

impl NimbusKvListener {
    /// Concrete address reported by the kernel.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Close Nimbus's descriptor and settle the exact Active lease.
    ///
    /// Nimbus-owned and provider-assigned bindings reach `Released` after the
    /// confirmed close. Externally supplied listeners stop at `Withdrawing`
    /// because closing Nimbus's descriptor cannot prove that the external
    /// owner released every duplicate handle.
    pub fn close_and_settle(self) -> Result<(), PortLeaseError> {
        let Self {
            listener,
            authority,
            request,
            provenance,
            lifetime,
        } = self;
        drop(listener);
        debug_assert_eq!(lifetime.request(), &request);
        let port_leases = authority.port_leases();
        port_leases.withdraw(&request)?;
        if provenance != PortBindingProvenance::ExternallyOwned {
            port_leases.release_with_lifetime(&request, &lifetime)?;
        }
        Ok(())
    }

    pub(crate) async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        self.listener.accept().await
    }

    pub(crate) fn close_after_confirmed_local_error(self, error: KvError) -> KvError {
        match self.close_and_settle() {
            Ok(()) => error,
            Err(cleanup) => KvError::ListenerLifecycle {
                primary: Box::new(error),
                context: "failed to settle the listener after confirmed local close",
                cleanup: Box::new(cleanup),
            },
        }
    }
}

pub(crate) async fn bind(
    config: &NimbusKvListenerConfig,
    requested_addr: SocketAddr,
) -> Result<NimbusKvListener, KvError> {
    let provenance = if requested_addr.port() == 0 {
        PortBindingProvenance::ProviderAssigned
    } else {
        PortBindingProvenance::NimbusOwned
    };
    let prepared = PreparedKvListener::prepare(config, requested_addr, provenance)?;
    match TcpListener::bind(requested_addr).await {
        Ok(listener) => prepared.adopt(listener),
        Err(error) => Err(prepared.record_bind_failure(error)),
    }
}

pub(crate) fn adopt(
    config: &NimbusKvListenerConfig,
    requested_addr: SocketAddr,
    listener: TcpListener,
) -> Result<NimbusKvListener, KvError> {
    let actual_addr = listener.local_addr()?;
    if requested_addr != actual_addr {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "pre-bound standalone KV listener address {actual_addr} does not match configured address {requested_addr}"
            ),
        )
        .into());
    }
    PreparedKvListener::prepare(config, actual_addr, PortBindingProvenance::ExternallyOwned)?
        .adopt(listener)
}

fn settle_claim_without_effect(
    authority: &NimbusKvListenerAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    provenance: PortBindingProvenance,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<(), PortLeaseError> {
    let port_leases = authority.port_leases();
    port_leases.abandon_bind_with_lifetime_without_effect(request, None, claim, lifetime)?;
    port_leases.withdraw(request)?;
    if provenance != PortBindingProvenance::ExternallyOwned {
        port_leases.release(request)?;
    }
    Ok(())
}

fn request_mode(
    addr: SocketAddr,
    provenance: PortBindingProvenance,
) -> Result<PortRequestMode, KvError> {
    if provenance == PortBindingProvenance::ExternallyOwned {
        return NonZeroU16::new(addr.port())
            .map(PortRequestMode::Exact)
            .ok_or_else(|| {
                io::Error::other("external KV listeners cannot adopt port zero").into()
            });
    }
    Ok(NonZeroU16::new(addr.port())
        .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact))
}

fn lifetime_scope(provenance: PortBindingProvenance) -> PortLeaseEffectScope {
    if provenance == PortBindingProvenance::ExternallyOwned {
        PortLeaseEffectScope::ProviderManaged
    } else {
        PortLeaseEffectScope::ProcessBound
    }
}

fn bind_target(address: IpAddr) -> Result<PortBindTarget, KvError> {
    match address {
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(other_io)
            .map_err(Into::into),
    }
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

fn other_io(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
