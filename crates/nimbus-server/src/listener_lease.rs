//! Server-side adapter between durable port authority and TCP socket effects.
//!
//! `nimbus-network` owns portable identity and lifecycle state. This module
//! translates real Tokio listener observations into that vocabulary while
//! leaving every kernel bind in its existing effect owner.

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
    PortLeaseError, PortLeaseFence, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};
use ulid::Ulid;

const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
const SERVER_LISTENER_PROVIDER_KEY: &str = "nimbus-server.tcp-listener";

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
        match self.authority.record_claimed_bind_failure_without_effect(
            &self.request,
            None,
            &self.claim,
            failure,
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
        if let Err(error) = self.authority.adopt_claimed_and_activate_batch(
            &[(self.request.clone(), self.claim.clone(), binding.clone())],
            None,
        ) {
            return Err(self.close_after_failed_adoption(listener, network_error(error)));
        }
        Ok(LeasedServerListener {
            listener,
            lease: ActiveServerListenerLease {
                authority: self.authority,
                request: self.request,
                provenance: self.provenance,
            },
            owner_incarnation: self.owner_incarnation,
        })
    }

    fn binding_for_listener(
        &self,
        listener: &tokio::net::TcpListener,
    ) -> io::Result<PortLeaseBinding> {
        let actual_addr = listener.local_addr()?;
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

    fn abandon_after_confirmed_close(self) -> io::Result<()> {
        settle_claim_without_effect(&self.authority, &self.request, self.claim, self.provenance)
    }
}

/// A concrete TCP listener backed by an Active durable port lease.
pub struct LeasedServerListener {
    listener: tokio::net::TcpListener,
    lease: ActiveServerListenerLease,
    owner_incarnation: Arc<str>,
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
}

impl ActiveServerListenerLease {
    pub(crate) fn settle_after_confirmed_local_close(self) -> io::Result<()> {
        self.authority
            .withdraw(&self.request)
            .map_err(network_error)?;
        if self.provenance != PortBindingProvenance::ExternallyOwned {
            self.authority
                .release(&self.request)
                .map_err(network_error)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ServerListenerLeaseAuthority {
    state_root: PathBuf,
    incarnation: Arc<str>,
    next_main_attempt: Arc<AtomicU64>,
}

impl ServerListenerLeaseAuthority {
    pub(crate) fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            incarnation: Arc::from(format!("server:{}", Ulid::new())),
            next_main_attempt: Arc::new(AtomicU64::new(1)),
        }
    }

    pub(crate) fn with_state_root(mut self, state_root: impl Into<PathBuf>) -> Self {
        self.state_root = state_root.into();
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
        let attempt = self.next_main_attempt.fetch_add(1, Ordering::Relaxed);
        self.prepare(
            &format!("main-http-external-{attempt}"),
            addr,
            PortBindingProvenance::ExternallyOwned,
        )?
        .adopt(listener)
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

    fn prepare(
        &self,
        listener_name: &str,
        requested_addr: SocketAddr,
        provenance: PortBindingProvenance,
    ) -> io::Result<PreparedServerListener> {
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(network_error)?;
        let listener_id =
            ListenerId::for_workload_listener(self.incarnation.as_ref(), listener_name);
        let request = PortLeaseRequest::new(
            nimbus_network::PortLeaseId::for_listener(&listener_id),
            listener_id.into(),
            None,
            PortLeaseFence::new(INITIAL_RESOURCE_GENERATION, INITIAL_LEASE_EPOCH),
            PortLeaseAccounting::HostInternal,
            PortPublicationIntent::Unpublished,
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                bind_target(requested_addr.ip())?,
                exposure(requested_addr.ip()),
                request_mode(requested_addr, provenance)?,
            ),
        );
        let provider_attempt = NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(SERVER_LISTENER_PROVIDER_KEY),
            format!("bind-attempt:{}", Ulid::new()),
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
        authority
            .reserve(request.clone())
            .map_err(reservation_error)?;
        if let Err(claim_error) = authority.claim_bind(&request, None, claim.clone()) {
            return match settle_claim_without_effect(&authority, &request, claim, provenance) {
                Ok(()) => Err(network_error(claim_error)),
                Err(cleanup_error) => Err(io::Error::other(format!(
                    "{claim_error}; failed to settle the never-bound reservation after its bind \
                     claim receipt failed: {cleanup_error}"
                ))),
            };
        }
        Ok(PreparedServerListener {
            authority,
            request,
            claim,
            attempt,
            provenance,
            owner_incarnation: Arc::clone(&self.incarnation),
        })
    }
}

fn settle_claim_without_effect(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: PortBindClaim,
    provenance: PortBindingProvenance,
) -> io::Result<()> {
    authority
        .abandon_bind_claims_without_effect(&[(request.clone(), claim)], None)
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
    use nimbus_network::{PortBindingProvenance, PortLeasePhase};

    use super::*;

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
}
