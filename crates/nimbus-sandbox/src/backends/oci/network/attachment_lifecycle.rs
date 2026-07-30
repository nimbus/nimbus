//! Shared OCI-family network attachment ordering and compensation.
//!
//! Container and krun keep workload/runtime state, creator handoff, readiness,
//! and policy decisions. This deep sandbox-owned module owns the common
//! host-managed netns/Netavark/IPAM/port-lifetime choreography so those callers
//! cannot drift on effect ordering or reverse compensation.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkAttachmentSegmentAssociation, NetworkReservationClaim,
    PortLeaseRequest,
};

use super::provider_locator::OciAttachmentProviderKind;
use super::{
    OciIpamAuthority, OciMachinePortForwarderConfig, OciNetavarkOperation, OciNetworkConfig,
    OciNetworkDirectEgress, OciNetworkLayout, OciPlacementProvider, OciSegmentAllocator,
    OciSegmentRealization, ReservedNetworkLaunchAuthority,
    authenticate_container_network_generation,
    authenticate_container_network_generation_for_cleanup,
    compensate_reserved_network_launch_after_ports,
    deallocate_container_ips_after_confirmed_detach, default_network_attachment_id,
    place_sandbox_on_block, quarantine_network_segment_hold, release_network_segment_hold,
    release_reserved_network_launch_after_ports,
};
use crate::backends::oci::port_lease::{OciPortBindLifetimeBatch, OciPortProvider};
use crate::backends::oci::port_lifecycle::{
    LaunchPortBatchState, NetavarkPortLifetimeRegistry, OciPortLeaseCoordinator,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

/// Real OCI backend adapter consuming the shared attachment contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentBackendKind {
    Container,
    Krun,
}

/// Provider publication shape selected by the already-admitted backend.
#[derive(Clone, Copy)]
enum AttachmentPublicationMode<'a> {
    /// Netavark owns the host listener effects and exact live port lifetimes.
    HostManaged,
    /// A container-only machine adapter owns publication outside Netavark.
    MachineForwarded(&'a OciMachinePortForwarderConfig),
}

impl<'a> AttachmentPublicationMode<'a> {
    fn machine_forwarder(self) -> Option<&'a OciMachinePortForwarderConfig> {
        match self {
            Self::HostManaged => None,
            Self::MachineForwarded(forwarder) => Some(forwarder),
        }
    }

    fn owns_netavark_bindings(self) -> bool {
        matches!(self, Self::HostManaged)
    }
}

impl AttachmentBackendKind {
    fn provider_label(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Krun => "krun",
        }
    }

    pub(crate) fn provider_kind(self) -> OciAttachmentProviderKind {
        match self {
            Self::Container => OciAttachmentProviderKind::Container,
            Self::Krun => OciAttachmentProviderKind::Krun,
        }
    }
}

mod authority;
mod host;
mod machine_forwarded;
mod recovery;
mod state;

#[cfg(test)]
mod test_api;
#[cfg(test)]
mod tests;

use host::{AttachmentHostEffects, RealAttachmentHostEffects};

/// Explicit authority disposition for one confirmed provider detach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentTeardownMode {
    /// Retain the exact generation, IPAM, segment, and publication authority.
    Restart,
    /// Release authority only after provider and persistent-netns absence.
    Final,
}

/// Exact pre-effect authority for one attachment attempt.
///
/// Callers select this from their already-authenticated launch branch rather
/// than inferring it from an optional manifest field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentAttachAuthority<'a> {
    FreshLaunch(&'a NetworkReservationClaim),
    RestartRetained,
}

/// Authenticated process-local disposition of the attachment's auxiliary
/// listener provider (currently the egress PEP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentAuxiliaryDisposition {
    ProviderOwned,
    NoEffect,
    Unknown,
}

/// The furthest trustworthy boundary reached by a failed detach.
///
/// Container launch-artifact cleanup deliberately depends on this distinction:
/// artifacts remain retry evidence when provider detach never started, while
/// cleanup that reached the provider/authority phase may remove independent
/// launch artifacts without claiming network convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentDetachFailureStage {
    BeforeProviderDetach,
    CleanupPending,
}

/// A failed detach plus the exact progress boundary needed by thin adapters.
#[derive(Debug)]
pub(crate) struct AttachmentDetachFailure {
    stage: AttachmentDetachFailureStage,
    error: SandboxError,
}

impl AttachmentDetachFailure {
    pub(crate) fn stage(&self) -> AttachmentDetachFailureStage {
        self.stage
    }

    pub(crate) fn into_error(self) -> SandboxError {
        self.error
    }
}

impl From<AttachmentDetachFailure> for SandboxError {
    fn from(failure: AttachmentDetachFailure) -> Self {
        failure.into_error()
    }
}

pub(crate) type AttachmentDetachResult = std::result::Result<(), AttachmentDetachFailure>;

impl AttachmentTeardownMode {
    pub(crate) fn releases_authority(self) -> bool {
        matches!(self, Self::Final)
    }
}

/// Immutable exact input consumed by one attachment lifecycle operation.
struct OciAttachmentContext<'a> {
    workload_state_root: &'a Path,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    display_name: &'a str,
    hostname: &'a str,
    bindings: &'a [SandboxPortBinding],
    leases: &'a [PortLeaseRequest],
    /// One launch-owned internal listener (currently the egress PEP) whose
    /// provider is stopped by the backend prerequisite hook.
    auxiliary_listener: Option<OciAttachmentAuxiliaryListener<'a>>,
    layout: &'a OciNetworkLayout,
    config: &'a OciNetworkConfig,
    launch_claim: Option<&'a NetworkReservationClaim>,
    publication: AttachmentPublicationMode<'a>,
    backend: AttachmentBackendKind,
    provider_label: &'static str,
}

/// Backend-neutral values from one authenticated workload manifest.
///
/// The concrete adapter constructors below add the backend kind, provider
/// label, and supported publication mode. Production callers cannot assemble
/// those discriminants independently.
pub(crate) struct OciAttachmentInput<'a> {
    pub(crate) workload_state_root: &'a Path,
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) sandbox_id: &'a SandboxId,
    pub(crate) display_name: &'a str,
    pub(crate) hostname: &'a str,
    pub(crate) bindings: &'a [SandboxPortBinding],
    pub(crate) leases: &'a [PortLeaseRequest],
    pub(crate) auxiliary_listener: Option<OciAttachmentAuxiliaryListener<'a>>,
    pub(crate) layout: &'a OciNetworkLayout,
    pub(crate) config: &'a OciNetworkConfig,
    pub(crate) launch_claim: Option<&'a NetworkReservationClaim>,
}

/// Concrete container/krun adapter into the one OCI attachment lifecycle.
///
/// This is deliberately not a provider trait: it seals construction and
/// routing for the two real backends while provider effects remain in their
/// current owners.
pub(crate) struct OciAttachmentAdapter<'a> {
    context: OciAttachmentContext<'a>,
}

impl<'a> OciAttachmentAdapter<'a> {
    fn new(
        backend: AttachmentBackendKind,
        input: OciAttachmentInput<'a>,
        publication: AttachmentPublicationMode<'a>,
    ) -> Self {
        Self {
            context: OciAttachmentContext {
                workload_state_root: input.workload_state_root,
                tenant_id: input.tenant_id,
                sandbox_id: input.sandbox_id,
                display_name: input.display_name,
                hostname: input.hostname,
                bindings: input.bindings,
                leases: input.leases,
                auxiliary_listener: input.auxiliary_listener,
                layout: input.layout,
                config: input.config,
                launch_claim: input.launch_claim,
                publication,
                backend,
                provider_label: backend.provider_label(),
            },
        }
    }

    pub(crate) fn attach(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        authority: AttachmentAttachAuthority<'_>,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        lifecycle.attach(&self.context, authority, after_provider_setup)
    }

    pub(crate) fn detach_host_managed(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        mode: AttachmentTeardownMode,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        lifecycle.detach_host_managed(&self.context, mode, before_provider_detach)
    }

    pub(crate) fn detach_machine_forwarded<T>(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        mode: AttachmentTeardownMode,
        before_provider_detach: impl FnOnce() -> Result<T>,
        after_provider_detach: impl FnOnce(T) -> Result<()>,
    ) -> AttachmentDetachResult {
        lifecycle.detach_machine_forwarded(
            &self.context,
            mode,
            before_provider_detach,
            after_provider_detach,
        )
    }
}

/// Exact logical identity and assignment of one attachment-adjacent listener.
///
/// The durable request alone is insufficient authority: callers also supply
/// the assignment address persisted by the workload manifest so the lifecycle
/// can authenticate tenant, sandbox, listener, target, and selected port before
/// any filesystem or provider effect.
#[derive(Clone, Copy)]
pub(crate) struct OciAttachmentAuxiliaryListener<'a> {
    request: &'a PortLeaseRequest,
    host: &'a str,
    port: u16,
}

impl<'a> OciAttachmentAuxiliaryListener<'a> {
    pub(crate) fn egress_pep(request: &'a PortLeaseRequest, host: &'a str, port: u16) -> Self {
        Self {
            request,
            host,
            port,
        }
    }

    fn request(self) -> &'a PortLeaseRequest {
        self.request
    }

    fn bind_addr(self) -> Result<SocketAddr> {
        let host = self.host.parse().map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "attachment auxiliary listener host {:?} must be an IP address",
                self.host
            ),
        })?;
        Ok(SocketAddr::new(host, self.port))
    }
}

/// Canonical host-managed route implemented by the real OCI backend types.
///
/// The adapter constructor stays private to this owner. Production callers and
/// the shared contract therefore exercise the same type-bound route rather
/// than manufacturing a test profile.
pub(crate) trait OciHostManagedAttachmentBackend {
    const ATTACHMENT_BACKEND_KIND: AttachmentBackendKind;

    fn reserve_attachment_config(
        lifecycle: &OciAttachmentLifecycle<'_>,
        tenant_id: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
        netavark_path: PathBuf,
        aardvark_dns_path: PathBuf,
    ) -> Result<OciNetworkConfig> {
        lifecycle.reserve_config(
            tenant_id,
            layout,
            sandbox_id,
            reservation_claim,
            OciAttachmentProviderConfig {
                backend: Self::ATTACHMENT_BACKEND_KIND,
                netavark_path,
                aardvark_dns_path,
            },
        )
    }

    fn host_managed_attachment_adapter<'a>(
        input: OciAttachmentInput<'a>,
    ) -> OciAttachmentAdapter<'a> {
        OciAttachmentAdapter::new(
            Self::ATTACHMENT_BACKEND_KIND,
            input,
            AttachmentPublicationMode::HostManaged,
        )
    }
}

struct OciAttachmentProviderConfig {
    backend: AttachmentBackendKind,
    netavark_path: PathBuf,
    aardvark_dns_path: PathBuf,
}

/// Machine-forwarded publication is a container-only backend capability.
pub(crate) trait OciMachineForwardedAttachmentBackend:
    OciHostManagedAttachmentBackend
{
    fn machine_forwarded_attachment_adapter<'a>(
        input: OciAttachmentInput<'a>,
        forwarder: &'a OciMachinePortForwarderConfig,
    ) -> OciAttachmentAdapter<'a> {
        OciAttachmentAdapter::new(
            Self::ATTACHMENT_BACKEND_KIND,
            input,
            AttachmentPublicationMode::MachineForwarded(forwarder),
        )
    }
}

impl OciAttachmentContext<'_> {
    fn operation(&self) -> OciNetavarkOperation<'_> {
        OciNetavarkOperation::new(
            self.layout,
            self.config,
            self.sandbox_id,
            self.display_name,
            self.hostname,
            self.bindings,
            self.publication.machine_forwarder(),
        )
    }

    fn validate_backend_publication(&self) -> Result<()> {
        let expected_layout = OciNetworkLayout::with_roots(
            self.workload_state_root,
            &self.layout.network_state_root,
            self.tenant_id,
            self.sandbox_id,
        );
        if self.layout != &expected_layout {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "{} attachment {} carries tenant, sandbox, or workload-root provenance that \
                     does not match its network layout",
                    self.provider_label, self.sandbox_id
                ),
            });
        }
        if self.backend == AttachmentBackendKind::Krun
            && matches!(
                self.publication,
                AttachmentPublicationMode::MachineForwarded(_)
            )
        {
            return Err(SandboxError::BackendUnavailable {
                message: format!(
                    "krun attachment {} does not support machine-forwarded publication",
                    self.sandbox_id
                ),
            });
        }
        Ok(())
    }
}

/// Ordered milestones emitted by the executable attach algorithm.
///
/// Production uses a no-op observer. The concept-owned contract installs a
/// deterministic observer so failure at a represented boundary exercises the
/// same compensation branches as the real host effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentAttachPhase {
    GenerationAuthenticated,
    LeasesAuthenticated,
    AuthorityAuthenticated,
    ProviderAttemptAuthenticated,
    NamespaceCreated,
    ListenerClaimsHeld,
    ProviderSetupComplete,
    ListenerBindingsActive,
    BackendPublicationComplete,
    LifetimeRegistered,
    AttachmentConfirmed,
}

trait AttachmentPhaseObserver {
    fn checkpoint(&mut self, phase: AttachmentAttachPhase) -> Result<()>;
}

struct NoopAttachmentPhaseObserver;

impl AttachmentPhaseObserver for NoopAttachmentPhaseObserver {
    fn checkpoint(&mut self, _phase: AttachmentAttachPhase) -> Result<()> {
        Ok(())
    }
}

/// Deep OCI attachment composition over the already-earned local authorities.
pub(crate) struct OciAttachmentLifecycle<'a> {
    allocator: &'a OciSegmentAllocator,
    attachments: Option<&'a LocalNetworkAttachmentAuthority>,
    ipam: &'a OciIpamAuthority,
    ports: &'a OciPortLeaseCoordinator,
    lifetimes: &'a NetavarkPortLifetimeRegistry,
}

impl<'a> OciAttachmentLifecycle<'a> {
    pub(crate) fn new(
        allocator: &'a OciSegmentAllocator,
        attachments: Option<&'a LocalNetworkAttachmentAuthority>,
        ipam: &'a OciIpamAuthority,
        ports: &'a OciPortLeaseCoordinator,
        lifetimes: &'a NetavarkPortLifetimeRegistry,
    ) -> Self {
        Self {
            allocator,
            attachments,
            ipam,
            ports,
            lifetimes,
        }
    }

    /// Build provider-local realization without leaking it into the portable
    /// allocation contract.
    pub(crate) fn config_from_segment(
        backend: AttachmentBackendKind,
        netavark_path: PathBuf,
        aardvark_dns_path: PathBuf,
        segment: &OciSegmentRealization,
        reservation_claim: &NetworkReservationClaim,
    ) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path,
            aardvark_dns_path,
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            segment_id: segment.segment_id().as_str().to_owned(),
            reservation_claim: reservation_claim.clone(),
            provider_kind: backend.provider_kind(),
            direct_egress: OciNetworkDirectEgress::Deny,
            // Both host-managed OCI backends resolve names through the host PEP.
            // A bridge-local DNS stub would be unreachable and create a
            // cross-tenant exfiltration path.
            enable_dns: false,
            network_id: segment.network_id().as_str().to_owned(),
        }
    }

    /// Reserve the attachment before IPAM and bind it to one exact segment.
    fn reserve_config(
        &self,
        tenant_id: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
        provider: OciAttachmentProviderConfig,
    ) -> Result<OciNetworkConfig> {
        place_sandbox_on_block(
            self.allocator,
            self.ipam,
            tenant_id,
            layout,
            sandbox_id,
            reservation_claim,
            OciPlacementProvider::new(provider.backend.provider_kind(), move |segment, claim| {
                Self::config_from_segment(
                    provider.backend,
                    provider.netavark_path.clone(),
                    provider.aardvark_dns_path.clone(),
                    segment,
                    claim,
                )
            }),
        )
    }

    /// Preserve a planning failure while compensating ports, IPAM, and the
    /// exact reserved attachment in reverse order.
    pub(crate) fn compensate_reserved(
        &self,
        backend: AttachmentBackendKind,
        layout: &OciNetworkLayout,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
        primary: SandboxError,
    ) -> SandboxError {
        let port_compensation = self
            .ports
            .release_never_bound_launch_claim(reservation_claim);
        compensate_reserved_network_launch_after_ports(
            ReservedNetworkLaunchAuthority::new(
                self.allocator,
                self.ipam,
                layout,
                tenant_id,
                sandbox_id,
                reservation_claim,
                backend.provider_kind(),
            ),
            primary,
            port_compensation,
        )
    }

    /// Release an exact launch reservation before any provider effect.
    pub(crate) fn release_reserved(
        &self,
        backend: AttachmentBackendKind,
        layout: &OciNetworkLayout,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
        port_compensation: Result<()>,
    ) -> Result<()> {
        release_reserved_network_launch_after_ports(
            ReservedNetworkLaunchAuthority::new(
                self.allocator,
                self.ipam,
                layout,
                tenant_id,
                sandbox_id,
                reservation_claim,
                backend.provider_kind(),
            ),
            port_compensation,
        )
    }

    /// Realize one already-adopted attachment.
    ///
    /// `after_provider_setup` owns only backend-specific post-Netavark work,
    /// such as egress pinning and container machine publication. Any failure
    /// re-enters the same exact compensation path as setup or bind activation.
    fn attach(
        &self,
        context: &OciAttachmentContext<'_>,
        authority: AttachmentAttachAuthority<'_>,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        self.attach_with(
            context,
            authority,
            &RealAttachmentHostEffects,
            &mut NoopAttachmentPhaseObserver,
            after_provider_setup,
        )
    }

    fn attach_with(
        &self,
        context: &OciAttachmentContext<'_>,
        authority: AttachmentAttachAuthority<'_>,
        host: &impl AttachmentHostEffects,
        observer: &mut impl AttachmentPhaseObserver,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        context.validate_backend_publication()?;
        authenticate_container_network_generation(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        observer.checkpoint(AttachmentAttachPhase::GenerationAuthenticated)?;
        self.ports.require_binding_leases(
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
        )?;
        observer.checkpoint(AttachmentAttachPhase::LeasesAuthenticated)?;
        let association = self.authenticate_attach_authority(context, authority)?;
        observer.checkpoint(AttachmentAttachPhase::AuthorityAuthenticated)?;
        let durable =
            state::OciAttachmentDurableState::compile(self.attachments, context, association)?;
        let durable_record = durable.reserve()?;
        let recovery = recovery::prepare_attach(
            &durable,
            durable_record,
            host.inspect_provider(self.ipam, context),
        )?;
        let (mut durable_record, create_provider, recovered_ips) = match recovery {
            recovery::AttachmentAttachRecovery::Create { record } => (record, true, None),
            recovery::AttachmentAttachRecovery::ResumePublication {
                record,
                assigned_ips,
            } => (record, false, Some(assigned_ips)),
            recovery::AttachmentAttachRecovery::AlreadyActive { assigned_ips } => {
                return Ok(assigned_ips);
            }
        };
        let mut prepared_setup = if create_provider {
            Some(host.prepare_provider_setup(self.ipam, context)?)
        } else {
            None
        };
        observer.checkpoint(AttachmentAttachPhase::ProviderAttemptAuthenticated)?;

        if create_provider {
            host.create_namespace(context)?;
            if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::NamespaceCreated) {
                return Err(self.compensate_namespace_failure(context, host, primary));
            }
        }
        let mut netavark_lifetimes = if context.publication.owns_netavark_bindings() {
            match self.ports.claim_netavark_bindings_with_lifetimes(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            ) {
                Ok(batch) => Some(batch),
                Err(error) => {
                    let _ = host.remove_namespace(context);
                    return Err(error);
                }
            }
        } else {
            None
        };
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::ListenerClaimsHeld) {
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }

        let assigned_ips = match recovered_ips {
            Some(assigned_ips) => assigned_ips,
            None => {
                let prepared = prepared_setup
                    .take()
                    .expect("provider creation requires its durable prepared attempt");
                let assigned_ips = match host.setup_provider(self.ipam, context, prepared) {
                    Ok(assigned_ips) => assigned_ips,
                    Err(primary) => {
                        let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                        return Err(self.compensate_setup_failure_with(
                            context,
                            host,
                            netavark_lifetimes.take(),
                            primary,
                        ));
                    }
                };
                durable_record = match recovery::mark_provider_ready(&durable, &durable_record) {
                    Ok(record) => record,
                    Err(primary) => {
                        let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                        return Err(self.compensate_setup_failure_with(
                            context,
                            host,
                            netavark_lifetimes.take(),
                            primary,
                        ));
                    }
                };
                assigned_ips
            }
        };
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::ProviderSetupComplete) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }
        durable_record = match recovery::mark_publishing(&durable, &durable_record) {
            Ok(record) => record,
            Err(primary) => {
                let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                return Err(self.compensate_setup_failure_with(
                    context,
                    host,
                    netavark_lifetimes.take(),
                    primary,
                ));
            }
        };

        if let Some(batch) = netavark_lifetimes.as_ref()
            && let Err(primary) = self.ports.activate_netavark_bindings_with_lifetimes(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
                batch,
            )
        {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::ListenerBindingsActive) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }

        if let Err(primary) = after_provider_setup(&assigned_ips) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::BackendPublicationComplete)
        {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(
                context,
                host,
                netavark_lifetimes.take(),
                primary,
            ));
        }

        if let Some(batch) = netavark_lifetimes.take()
            && let Err((primary, batch)) =
                self.lifetimes
                    .insert(context.tenant_id, context.sandbox_id, batch)
        {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_setup_failure_with(context, host, Some(batch), primary));
        }
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::LifetimeRegistered) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_registered_failure(
                context,
                host,
                "lifetime checkpoint",
                primary,
            ));
        }

        // Adoption already moved the exact reservation to Held. This call is an
        // idempotent confirmation that the same attachment remains current
        // after provider setup; it does not create a second hold.
        if let Err(primary) = self.allocator.acquire(
            context.tenant_id,
            &default_network_attachment_id(context.sandbox_id),
        ) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_registered_failure(
                context,
                host,
                "hold confirmation",
                primary,
            ));
        }
        if let Err(primary) = observer.checkpoint(AttachmentAttachPhase::AttachmentConfirmed) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_registered_failure(
                context,
                host,
                "attachment confirmation",
                primary,
            ));
        }
        if let Err(primary) = recovery::mark_active(&durable, &durable_record) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(self.compensate_registered_failure(
                context,
                host,
                "durable active checkpoint",
                primary,
            ));
        }

        Ok(assigned_ips)
    }

    fn authenticate_attach_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        attach_authority: AttachmentAttachAuthority<'_>,
    ) -> Result<NetworkAttachmentSegmentAssociation> {
        let association = authority::authenticate_attach_association(self.allocator, context)?;
        let port_authority: Result<()> = match attach_authority {
            AttachmentAttachAuthority::FreshLaunch(claim) => {
                let Some(context_claim) = context.launch_claim else {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} is missing its fresh launch reservation claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                };
                if context_claim != claim || &context.config.reservation_claim != claim {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} launch reservation claim does not match its exact \
                             attachment generation",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                self.ports
                    .require_never_bound_launch_batch(context.leases, claim)?;
                if let Some(auxiliary) = context.auxiliary_listener {
                    self.ports.require_internal_listener_authority(
                        context.tenant_id,
                        context.sandbox_id,
                        auxiliary.bind_addr()?,
                        auxiliary.request(),
                    )?;
                    self.ports.require_never_bound_launch_batch(
                        std::slice::from_ref(auxiliary.request()),
                        claim,
                    )?;
                }
                Ok(())
            }
            AttachmentAttachAuthority::RestartRetained => {
                if context.launch_claim.is_some() {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} restart still carries a launch reservation claim",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                let publication_state = if context.publication.owns_netavark_bindings() {
                    self.ports.classify_netavark_cleanup_batch(
                        context.tenant_id,
                        context.sandbox_id,
                        context.bindings,
                        context.leases,
                        None,
                    )?
                } else {
                    self.ports.classify_machine_cleanup_batch(
                        context.tenant_id,
                        context.sandbox_id,
                        context.bindings,
                        context.leases,
                    )?
                };
                if publication_state != LaunchPortBatchState::RestartRetained
                    && !(context.leases.is_empty()
                        && publication_state == LaunchPortBatchState::TerminalNoEffect)
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} restart requires confirmed-stop publication \
                             authority before effects, got {publication_state:?}",
                            context.provider_label, context.sandbox_id
                        ),
                    });
                }
                if let Some(auxiliary) = context.auxiliary_listener {
                    self.ports.require_restart_retained_internal_listener(
                        context.tenant_id,
                        context.sandbox_id,
                        auxiliary.bind_addr()?,
                        auxiliary.request(),
                        OciPortProvider::EgressPep,
                    )?;
                }
                Ok(())
            }
        };
        port_authority?;
        Ok(association)
    }

    fn take_registered_lifetime(
        &self,
        context: &OciAttachmentContext<'_>,
        checkpoint: &str,
    ) -> Result<Option<OciPortBindLifetimeBatch>> {
        if !context.publication.owns_netavark_bindings() {
            return Ok(None);
        }
        self.lifetimes
            .take(context.tenant_id, context.sandbox_id)?
            .map(Some)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {checkpoint} for {} lost its exact live Netavark lifetime \
                     batch",
                    context.provider_label, context.sandbox_id
                ),
            })
    }

    fn compensate_registered_failure(
        &self,
        context: &OciAttachmentContext<'_>,
        host: &impl AttachmentHostEffects,
        checkpoint: &str,
        primary: SandboxError,
    ) -> SandboxError {
        match self.take_registered_lifetime(context, checkpoint) {
            Ok(batch) => self.compensate_setup_failure_with(context, host, batch, primary),
            Err(recovery) => SandboxError::OperationFailed {
                message: format!(
                    "{} attachment {checkpoint} failed: {primary}; exact lifetime recovery also \
                     failed, so the provider remains fenced together with namespace, port, IPAM, \
                     and segment authority: {recovery}",
                    context.provider_label
                ),
            },
        }
    }

    /// Detach a host-managed attachment after the backend proves that its
    /// runtime/creator and PEP prerequisites permit provider teardown.
    fn detach_host_managed(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        self.detach_host_managed_with(
            context,
            mode,
            &RealAttachmentHostEffects,
            before_provider_detach,
        )
    }

    fn detach_host_managed_with(
        &self,
        context: &OciAttachmentContext<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        context
            .validate_backend_publication()
            .map_err(recovery::before_provider_detach_failure)?;
        if !context.publication.owns_netavark_bindings() {
            return Err(recovery::before_provider_detach_failure(
                SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} cannot use host-managed detach for machine-forwarded \
                     publication",
                        context.provider_label, context.sandbox_id
                    ),
                },
            ));
        }
        authenticate_container_network_generation_for_cleanup(
            self.ipam,
            context.layout,
            context.config,
            context.sandbox_id,
        )
        .map_err(recovery::before_provider_detach_failure)?;
        let association = authority::authenticate_detach_association(
            self.attachments,
            self.allocator,
            context,
            mode,
        )
        .map_err(recovery::before_provider_detach_failure)?;
        let durable =
            state::OciAttachmentDurableState::compile(self.attachments, context, association)
                .map_err(recovery::before_provider_detach_failure)?;
        let durable_record = durable
            .inspect()
            .map_err(recovery::before_provider_detach_failure)?;
        let provider_observation = host.inspect_provider(self.ipam, context);
        if durable_record
            .as_ref()
            .is_some_and(|record| record.resource().phase().is_terminal())
        {
            let terminal = recovery::prepare_detach(
                &durable,
                durable_record.expect("terminal attachment record was just authenticated"),
                provider_observation,
            )
            .map_err(recovery::before_provider_detach_failure)?;
            debug_assert!(terminal.already_terminal);
            return Ok(());
        }
        if matches!(
            &provider_observation,
            recovery::AttachmentProviderObservation::Unknown { .. }
        ) {
            let Some(record) = durable_record else {
                return Err(recovery::before_provider_detach_failure(
                    SandboxError::OperationFailed {
                        message: format!(
                            "{} attachment {} has ambiguous provider evidence without durable \
                             attachment authority; refusing teardown",
                            context.provider_label, context.sandbox_id
                        ),
                    },
                ));
            };
            let rejection = match recovery::prepare_detach(&durable, record, provider_observation) {
                Ok(_) => SandboxError::OperationFailed {
                    message: format!(
                        "{} attachment {} unexpectedly authorized teardown from ambiguous \
                             provider evidence",
                        context.provider_label, context.sandbox_id
                    ),
                },
                Err(error) => error,
            };
            return Err(recovery::before_provider_detach_failure(rejection));
        }

        let mut errors = Vec::new();
        let mut detach_permitted = true;
        let published_batch_state = self.ports.classify_netavark_cleanup_batch(
            context.tenant_id,
            context.sandbox_id,
            context.bindings,
            context.leases,
            context.launch_claim,
        );
        let auxiliary_requests = context
            .auxiliary_listener
            .map(OciAttachmentAuxiliaryListener::request)
            .map(std::slice::from_ref)
            .unwrap_or_default();
        let auxiliary_batch_state = if mode.releases_authority()
            && let Some(claim) = context.launch_claim
        {
            self.ports
                .classify_launch_port_batch(auxiliary_requests, claim)
        } else {
            Ok(LaunchPortBatchState::ProviderOwned)
        };
        if let Err(error) = &published_batch_state {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        if let Err(error) = &auxiliary_batch_state {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        if mode == AttachmentTeardownMode::Restart
            && let Ok(state) = &published_batch_state
            && let Err(error) = recovery::validate_restart_publication_state(context, state)
        {
            detach_permitted = false;
            errors.push(error.to_string());
        }
        let auxiliary_disposition = match &auxiliary_batch_state {
            Ok(LaunchPortBatchState::ProviderOwned) => {
                AttachmentAuxiliaryDisposition::ProviderOwned
            }
            Ok(
                LaunchPortBatchState::NeverBound
                | LaunchPortBatchState::RestartRetained
                | LaunchPortBatchState::TerminalNoEffect,
            ) => AttachmentAuxiliaryDisposition::NoEffect,
            Ok(LaunchPortBatchState::NetavarkClaimed(_)) | Err(_) => {
                AttachmentAuxiliaryDisposition::Unknown
            }
        };

        if !detach_permitted {
            return Err(recovery::before_provider_detach_failure(
                recovery::detach_error(context, errors),
            ));
        }

        let durable_record = match durable_record {
            Some(record) => record,
            None => durable
                .reserve()
                .map_err(recovery::before_provider_detach_failure)?,
        };
        let durable_detach =
            recovery::prepare_detach(&durable, durable_record, provider_observation)
                .map_err(recovery::before_provider_detach_failure)?;
        debug_assert!(!durable_detach.already_terminal);
        let durable_record = durable_detach.record;
        let provider_was_absent = durable_detach.provider_absent;
        let prepared_teardown = if provider_was_absent {
            None
        } else {
            match host.prepare_provider_teardown(self.ipam, context) {
                Ok(prepared) => Some(prepared),
                Err(error) => {
                    let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
                    return Err(recovery::before_provider_detach_failure(error));
                }
            }
        };

        if let Err(error) = before_provider_detach(auxiliary_disposition) {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            return Err(recovery::before_provider_detach_failure(error));
        }

        if mode.releases_authority()
            && let Err(error) = quarantine_network_segment_hold(
                self.allocator,
                context.tenant_id,
                context.sandbox_id,
                &context.config.reservation_claim,
            )
        {
            detach_permitted = false;
            errors.push(error.to_string());
        }

        let mut cleanup = None;
        let mut recoveries = None;
        if matches!(
            &published_batch_state,
            Ok(LaunchPortBatchState::ProviderOwned)
        ) {
            match self.ports.begin_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
            ) {
                Ok(authority) => cleanup = authority,
                Err(error) => {
                    detach_permitted = false;
                    errors.push(error.to_string());
                }
            }
        } else if let Ok(LaunchPortBatchState::NetavarkClaimed(claims)) = &published_batch_state {
            match self.ports.recover_netavark_claims_after_owner_death(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
                claims,
            ) {
                Ok(authority) => recoveries = Some(authority),
                Err(error) => {
                    detach_permitted = false;
                    errors.push(error.to_string());
                }
            }
        }

        if !detach_permitted {
            if let Err(error) = self.ports.retain_ambiguous_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                cleanup.take(),
            ) {
                errors.push(error.to_string());
            }
            return Err(recovery::before_provider_detach_failure(
                recovery::detach_error(context, errors),
            ));
        }

        let mut provider_detached = provider_was_absent;
        if let Some(prepared_teardown) = prepared_teardown {
            provider_detached = match host.teardown_provider(self.ipam, context, prepared_teardown)
            {
                Ok(()) => true,
                Err(error) => {
                    errors.push(error.to_string());
                    false
                }
            };
            if provider_detached && let Err(error) = host.remove_namespace(context) {
                provider_detached = false;
                errors.push(error.to_string());
            }
        }

        if provider_detached
            && matches!(
                &published_batch_state,
                Ok(LaunchPortBatchState::ProviderOwned)
            )
        {
            if let Err(error) = self.ports.complete_netavark_cleanup(
                context.leases,
                cleanup.as_ref(),
                mode.releases_authority(),
            ) {
                provider_detached = false;
                errors.push(error.to_string());
            } else {
                cleanup = None;
            }
        }

        if provider_detached
            && let Some(recoveries) = recoveries.take()
            && let Err(error) = if mode.releases_authority() {
                self.ports
                    .release_recovered_netavark_bindings(context.leases, &recoveries)
            } else {
                self.ports
                    .prepare_recovered_netavark_claims_for_rebind(context.leases, &recoveries)
            }
        {
            provider_detached = false;
            errors.push(error.to_string());
        }

        if !provider_detached {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            if let Err(error) = self.ports.retain_ambiguous_netavark_cleanup(
                self.lifetimes,
                context.tenant_id,
                context.sandbox_id,
                cleanup.take(),
            ) {
                errors.push(error.to_string());
            }
            return Err(recovery::cleanup_pending_failure(recovery::detach_error(
                context, errors,
            )));
        }

        if mode.releases_authority() {
            self.release_terminal_authority(
                context,
                published_batch_state,
                auxiliary_batch_state,
                auxiliary_requests,
                &mut errors,
            );
        }

        if errors.is_empty() {
            recovery::finish_detach(&durable, &durable_record, mode)
                .map(|_| ())
                .map_err(recovery::cleanup_pending_failure)
        } else {
            let _ = recovery::mark_cleanup_pending(&durable, &durable_record);
            Err(recovery::cleanup_pending_failure(recovery::detach_error(
                context, errors,
            )))
        }
    }

    fn compensate_namespace_failure(
        &self,
        context: &OciAttachmentContext<'_>,
        host: &impl AttachmentHostEffects,
        primary: SandboxError,
    ) -> SandboxError {
        match host.remove_namespace(context) {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "{} attachment namespace creation checkpoint failed: {primary}; namespace \
                     compensation also failed: {cleanup}",
                    context.provider_label
                ),
            },
        }
    }

    fn compensate_setup_failure_with(
        &self,
        context: &OciAttachmentContext<'_>,
        host: &impl AttachmentHostEffects,
        batch: Option<OciPortBindLifetimeBatch>,
        primary: SandboxError,
    ) -> SandboxError {
        let cleanup = host
            .prepare_provider_teardown(self.ipam, context)
            .and_then(|prepared| host.teardown_provider(self.ipam, context, prepared))
            .and_then(|()| host.remove_namespace(context));
        if let Err(cleanup) = cleanup {
            return SandboxError::OperationFailed {
                message: format!(
                    "{} attachment configuration failed: {primary}; exact-generation detach \
                     compensation also failed while the namespace remains fenced: {cleanup}",
                    context.provider_label
                ),
            };
        }

        let Some(batch) = batch else {
            return primary;
        };
        let compensation = self
            .ports
            .abandon_netavark_bind_claims_with_lifetimes_without_effect(
                context.tenant_id,
                context.sandbox_id,
                context.bindings,
                context.leases,
                &batch,
                context.launch_claim,
            )
            .or_else(|abandon_error| {
                let expected = self.ports.expected_netavark_bindings(
                    context.tenant_id,
                    context.sandbox_id,
                    context.bindings,
                    context.leases,
                )?;
                self.ports
                    .prepare_netavark_bindings_for_rebind_with_lifetimes(
                        context.leases,
                        &expected,
                        &batch,
                    )
                    .map_err(|rebind_error| SandboxError::OperationFailed {
                        message: format!(
                            "Netavark claim abandonment failed: {abandon_error}; exact Active \
                             lifetime compensation also failed: {rebind_error}"
                        ),
                    })
            });
        match compensation {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "{} attachment configuration failed: {primary}; detached Netavark \
                     port-lifetime compensation also failed: {cleanup}",
                    context.provider_label
                ),
            },
        }
    }

    fn release_terminal_authority(
        &self,
        context: &OciAttachmentContext<'_>,
        published_batch_state: Result<LaunchPortBatchState>,
        auxiliary_batch_state: Result<LaunchPortBatchState>,
        auxiliary_requests: &[PortLeaseRequest],
        errors: &mut Vec<String>,
    ) {
        match published_batch_state {
            Ok(LaunchPortBatchState::NeverBound) => {
                if let Some(claim) = context.launch_claim
                    && let Err(error) = self
                        .ports
                        .release_never_bound_requests(context.leases, claim)
                {
                    errors.push(error.to_string());
                    return;
                }
            }
            Ok(LaunchPortBatchState::RestartRetained) => {
                if let Err(error) = self.ports.release_restart_retained_bindings(
                    context.tenant_id,
                    context.sandbox_id,
                    context.bindings,
                    context.leases,
                ) {
                    errors.push(error.to_string());
                    return;
                }
            }
            Ok(
                LaunchPortBatchState::NetavarkClaimed(_)
                | LaunchPortBatchState::ProviderOwned
                | LaunchPortBatchState::TerminalNoEffect,
            ) => {}
            Err(_) => return,
        }

        if matches!(auxiliary_batch_state, Ok(LaunchPortBatchState::NeverBound))
            && let Some(claim) = context.launch_claim
            && let Err(error) = self
                .ports
                .release_never_bound_requests(auxiliary_requests, claim)
        {
            errors.push(error.to_string());
            return;
        }

        if let Err(error) = deallocate_container_ips_after_confirmed_detach(
            self.ipam,
            context.layout,
            context.sandbox_id,
            &context.config.reservation_claim,
            context.config.provider_kind(),
        ) {
            errors.push(error.to_string());
            return;
        }
        errors.extend(
            release_network_segment_hold(
                self.allocator,
                context.tenant_id,
                context.sandbox_id,
                &context.config.reservation_claim,
            )
            .into_iter()
            .map(|error| error.to_string()),
        );
    }
}
