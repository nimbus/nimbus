use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU16;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkReservationClaim, PortBindClaim, PortBindRealm, PortBindTarget,
    PortBindingSpec, PortExposure, PortLeaseAccounting, PortLeasePhase, PortLeaseRecord,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::buildah::{OciExposedPort, OciExposedPortProtocol};
use super::port_lease::{
    ExpectedListenerAuthority, OciPortLeaseIntent, OciPortProvider,
    abandon_bind_attempts_without_effect, adopt_claimed_and_activate_batch, claim_bind_attempts,
    port_lease_request, prepare_rebind_batch_after_confirmed_stop, provider_binding,
    published_scope, record_bind_failure, release, release_batch_after_confirmed_stop,
    release_reserved_batch_without_effect, require_active_provider_binding,
    require_current_bind_authority, require_current_listener_authority, require_listener_authority,
    reserve_batch, reserve_batch_with_tenant_limit, verify_reserved_batch_for_coordinator,
    withdraw,
};
#[cfg(test)]
use super::port_lease::{new_launch_reservation_claim, reserve};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

mod batch_state;

pub(crate) const DEFAULT_MAX_PORTS_PER_TENANT: usize = 128;

pub(crate) fn machine_port_proxy_guest_listener_addr(binding: &SandboxPortBinding) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, binding.host_port))
}

#[derive(Debug, Clone)]
pub(crate) struct PortManager {
    range: RangeInclusive<u16>,
    state_root: PathBuf,
    max_ports_per_tenant: Option<usize>,
    published_listener_provider: PublishedListenerProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishedListenerProvider {
    Netavark,
    MachinePortProxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchPortBatchState {
    NeverBound,
    NetavarkClaimed(Vec<PortBindClaim>),
    RestartRetained,
    /// Every exact member is terminal proof that no provider effect remains.
    TerminalNoEffect,
    ProviderOwned,
}

fn terminal_failed_has_no_effect(
    record: &PortLeaseRecord,
    expected_provider: OciPortProvider,
) -> bool {
    record.phase() == PortLeasePhase::Failed
        && record.reservation_claim().is_some()
        && record.bind_claim().is_none()
        && record.binding().is_none()
        && record.confirmed_stopped_binding().is_none()
        && record.failure().is_some_and(|failure| {
            failure.provider_attempt().provider_id() == &expected_provider.provider_id()
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InternalListenerReservation {
    listener_name: String,
    target: PortBindTarget,
    exposure: PortExposure,
}

impl InternalListenerReservation {
    pub(crate) fn new(
        listener_name: impl Into<String>,
        target: PortBindTarget,
        exposure: PortExposure,
    ) -> Self {
        Self {
            listener_name: listener_name.into(),
            target,
            exposure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReservedInternalListener {
    pub(crate) port: u16,
    pub(crate) lease: PortLeaseRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReservedLaunchPorts {
    pub(crate) published_bindings: Vec<SandboxPortBinding>,
    pub(crate) published_leases: Vec<PortLeaseRequest>,
    pub(crate) internal_listener: Option<ReservedInternalListener>,
    pub(crate) reservation_claim: NetworkReservationClaim,
}

pub(crate) struct SandboxLaunchPortPlan<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    existing_bindings: &'a [SandboxPortBinding],
    exposed_ports: &'a [OciExposedPort],
    reallocatable_listener_names: Option<&'a BTreeSet<String>>,
    internal_listener: Option<InternalListenerReservation>,
}

impl<'a> SandboxLaunchPortPlan<'a> {
    pub(crate) fn new(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        existing_bindings: &'a [SandboxPortBinding],
        exposed_ports: &'a [OciExposedPort],
    ) -> Self {
        Self {
            tenant_id,
            sandbox_id,
            existing_bindings,
            exposed_ports,
            reallocatable_listener_names: None,
            internal_listener: None,
        }
    }

    pub(crate) fn with_reallocatable_listener_names(
        mut self,
        listener_names: &'a BTreeSet<String>,
    ) -> Self {
        self.reallocatable_listener_names = Some(listener_names);
        self
    }

    pub(crate) fn with_internal_listener(
        mut self,
        internal_listener: InternalListenerReservation,
    ) -> Self {
        self.internal_listener = Some(internal_listener);
        self
    }
}

impl PortManager {
    pub(crate) fn new(state_root: impl Into<PathBuf>, range: RangeInclusive<u16>) -> Self {
        Self {
            range,
            state_root: state_root.into(),
            max_ports_per_tenant: None,
            published_listener_provider: PublishedListenerProvider::Netavark,
        }
    }

    /// Model published listeners by the socket this process actually binds.
    ///
    /// The machine forwarder retains `SandboxPortBinding::host_address` as its
    /// external desired exposure, while the guest-side proxy binds an IPv4
    /// wildcard listener. The durable conflict target must describe that real
    /// wildcard effect so a specific-address lease cannot overlap it.
    pub(crate) fn with_machine_port_proxy_bindings(mut self) -> Self {
        self.published_listener_provider = PublishedListenerProvider::MachinePortProxy;
        self
    }

    pub(crate) fn with_max_ports_per_tenant(mut self, max_ports_per_tenant: Option<usize>) -> Self {
        self.max_ports_per_tenant = max_ports_per_tenant;
        self
    }

    /// Atomically reserve every published endpoint and optional internal
    /// listener needed by one sandbox launch.
    ///
    /// `reallocatable_listener_names` identifies authority-free plan previews:
    /// their rendered numbers are replaced by range-selected durable ports.
    /// All other existing bindings remain exact operator requests.
    pub(crate) fn reserve_launch_ports_for_sandbox(
        &self,
        plan: SandboxLaunchPortPlan<'_>,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<ReservedLaunchPorts> {
        let SandboxLaunchPortPlan {
            tenant_id,
            sandbox_id,
            existing_bindings,
            exposed_ports,
            reallocatable_listener_names,
            internal_listener,
        } = plan;
        let unmapped_tcp_guest_ports = unmapped_tcp_guest_ports(existing_bindings, exposed_ports);

        let request_count = existing_bindings
            .len()
            .saturating_add(unmapped_tcp_guest_ports.len())
            .saturating_add(usize::from(internal_listener.is_some()));
        let mut requests = Vec::with_capacity(request_count);
        let mut published_bindings =
            Vec::with_capacity(existing_bindings.len() + unmapped_tcp_guest_ports.len());
        for binding in existing_bindings {
            let host_port =
                NonZeroU16::new(binding.host_port).ok_or_else(|| SandboxError::InvalidSpec {
                    message: format!(
                        "sandbox port binding {:?} must use a non-zero host port",
                        binding.name
                    ),
                })?;
            let stable_listener_name = published_listener_name(binding);
            let port = if reallocatable_listener_names
                .is_some_and(|names| names.contains(&stable_listener_name))
            {
                self.range_request_mode()?
            } else {
                PortRequestMode::Exact(host_port)
            };
            let (target, _, exposure) = self.published_binding_scope(binding)?;
            requests.push(port_lease_request(
                tenant_id,
                sandbox_id,
                &stable_listener_name,
                OciPortLeaseIntent::tenant_published(target, binding.host_address, exposure),
                port,
            ));
            published_bindings.push(binding.clone());
        }
        for guest_port in &unmapped_tcp_guest_ports {
            let name = auto_binding_name(*guest_port);
            let binding = SandboxPortBinding::tcp(name.clone(), *self.range.start(), *guest_port);
            let (target, _, exposure) = self.published_binding_scope(&binding)?;
            requests.push(port_lease_request(
                tenant_id,
                sandbox_id,
                &listener_name(name.as_str(), *guest_port),
                OciPortLeaseIntent::tenant_published(target, binding.host_address, exposure),
                self.range_request_mode()?,
            ));
            published_bindings.push(binding);
        }
        if let Some(internal) = &internal_listener {
            requests.push(port_lease_request(
                tenant_id,
                sandbox_id,
                &internal.listener_name,
                OciPortLeaseIntent::host_internal(internal.target.clone(), internal.exposure),
                self.range_request_mode()?,
            ));
        }

        let reserved_batch = match self.max_ports_per_tenant {
            Some(maximum) => reserve_batch_with_tenant_limit(
                &self.state_root,
                requests,
                tenant_id,
                maximum,
                reservation_claim,
            )?,
            None => reserve_batch(&self.state_root, requests, reservation_claim)?,
        };
        let (reserved, reservation_claim) = reserved_batch.into_parts();
        let reserved_requests = reserved
            .iter()
            .map(|(request, _)| request.clone())
            .collect::<Vec<_>>();
        if reserved.len() != request_count {
            let projection_error = SandboxError::OperationFailed {
                message: format!(
                    "sandbox port authority returned {} reservations for {request_count} requests",
                    reserved.len()
                ),
            };
            return Err(self.compensate_failed_never_bound_requests(
                &reserved_requests,
                &reservation_claim,
                projection_error,
                "sandbox reservation projection",
            ));
        }
        let published_count = published_bindings.len();
        let mut published_leases = Vec::with_capacity(published_count);
        let mut internal_reserved = None;
        for (index, (request, selected)) in reserved.into_iter().enumerate() {
            if let Some(binding) = published_bindings.get_mut(index) {
                let stable_listener_name = published_listener_name(binding);
                let is_reallocatable = index >= existing_bindings.len()
                    || reallocatable_listener_names
                        .is_some_and(|names| names.contains(&stable_listener_name));
                if !is_reallocatable && selected.get() != binding.host_port {
                    let projection_error = SandboxError::OperationFailed {
                        message: format!(
                            "sandbox port lease {} selected {} for explicit binding {}",
                            request.lease_id(),
                            selected,
                            binding.host_port
                        ),
                    };
                    return Err(self.compensate_failed_never_bound_requests(
                        &reserved_requests,
                        &reservation_claim,
                        projection_error,
                        "sandbox reservation projection",
                    ));
                }
                binding.host_port = selected.get();
                published_leases.push(request);
            } else {
                if index != published_count {
                    let projection_error = SandboxError::OperationFailed {
                        message: format!(
                            "sandbox port authority projected internal listener at index {index}, \
                             expected {published_count}"
                        ),
                    };
                    return Err(self.compensate_failed_never_bound_requests(
                        &reserved_requests,
                        &reservation_claim,
                        projection_error,
                        "sandbox reservation projection",
                    ));
                }
                internal_reserved = Some(ReservedInternalListener {
                    port: selected.get(),
                    lease: request,
                });
            }
        }
        Ok(ReservedLaunchPorts {
            published_bindings,
            published_leases,
            internal_listener: internal_reserved,
            reservation_claim,
        })
    }

    /// Compensate an exact request set retained in a launch manifest.
    pub(crate) fn release_never_bound_requests(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        release_reserved_batch_without_effect(&self.state_root, requests, reservation_claim)?;
        Ok(())
    }

    /// Release every still-reserved port owned by one launch claim.
    ///
    /// This recovery form is used when request projection itself failed before
    /// a complete batch could be returned to the caller. The list is only a
    /// selector: the subsequent batch transition atomically revalidates exact
    /// claim ownership and `Reserved` phase before releasing anything.
    pub(crate) fn release_never_bound_launch_claim(
        &self,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to open port reservation authority: {error}"),
            }
        })?;
        let requests = authority
            .list()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to list launch port reservations: {error}"),
            })?
            .into_iter()
            .filter(|record| record.reservation_claim() == Some(reservation_claim))
            .map(|record| record.request().clone())
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return Ok(());
        }
        self.release_never_bound_requests(&requests, reservation_claim)
    }

    /// Prove that an initial-launch coordinator still owns every reservation.
    pub(crate) fn require_never_bound_launch_batch(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        verify_reserved_batch_for_coordinator(&self.state_root, requests, reservation_claim)?;
        Ok(())
    }

    /// Classify one launch-owned request group without changing authority.
    ///
    /// A group is `NeverBound` when every member remains exact no-effect
    /// authority owned by the supplied launch claim. That includes a clean
    /// `Reserved` member, a terminal bind failure from the same coordinator,
    /// and an already-released identical compensation replay. A uniformly
    /// adopted group is provider-owned cleanup input. Claimless failures and
    /// mixed ownership fail closed.
    pub(crate) fn classify_launch_port_batch(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<LaunchPortBatchState> {
        let records = self.port_lease_records_snapshot(requests, "launch")?;
        let mut coordinator_no_effect = 0usize;
        let mut netavark_claims = Vec::new();
        let mut provider_owned = 0usize;
        for (request, record) in requests.iter().zip(records) {
            if record.reservation_claim() == Some(reservation_claim) {
                let clean_coordinator_record = record.bind_claim().is_none()
                    && record.binding().is_none()
                    && record.confirmed_stopped_binding().is_none()
                    && match record.phase() {
                        PortLeasePhase::Reserved => record.failure().is_none(),
                        PortLeasePhase::Failed => record.failure().is_some(),
                        PortLeasePhase::Released => record.failure().is_none(),
                        _ => false,
                    };
                if clean_coordinator_record {
                    coordinator_no_effect += 1;
                    continue;
                }
                if record.phase() == PortLeasePhase::Reserved
                    && record.binding().is_none()
                    && record.failure().is_none()
                    && record.confirmed_stopped_binding().is_none()
                {
                    match record.bind_claim() {
                        Some(claim)
                            if claim.provider_attempt().provider_id()
                                == &OciPortProvider::Netavark.provider_id() =>
                        {
                            netavark_claims.push(claim.clone());
                            continue;
                        }
                        Some(_) => {
                            return Err(SandboxError::OperationFailed {
                                message: format!(
                                    "launch port lease {} retains a non-Netavark provider claim \
                                     under the launch coordinator",
                                    request.lease_id()
                                ),
                            });
                        }
                        None => unreachable!(
                            "a clean coordinator-owned reservation was classified above"
                        ),
                    }
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "launch port lease {} carries ambiguous coordinator-owned lifecycle \
                         evidence",
                        request.lease_id()
                    ),
                });
            }
            if record.reservation_claim().is_some() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "launch port lease {} belongs to a different or ambiguous reservation coordinator",
                        request.lease_id()
                    ),
                });
            }
            if record.phase() == PortLeasePhase::Failed {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "launch port lease {} has claimless failed provider evidence; retaining \
                         every fence for reconciliation",
                        request.lease_id()
                    ),
                });
            }
            provider_owned += 1;
        }
        match (coordinator_no_effect, netavark_claims.len(), provider_owned) {
            (owned, 0, 0) if owned == requests.len() => Ok(LaunchPortBatchState::NeverBound),
            (0, claimed, 0) if claimed == requests.len() => {
                Ok(LaunchPortBatchState::NetavarkClaimed(netavark_claims))
            }
            (0, 0, owned) if owned == requests.len() => Ok(LaunchPortBatchState::ProviderOwned),
            _ => Err(SandboxError::OperationFailed {
                message:
                    "launch port batch mixes coordinator-owned no-effect, Netavark-claimed, or \
                     provider-owned lifecycle states; retaining every fence for reconciliation"
                        .to_owned(),
            }),
        }
    }

    /// Read one exact request group from one durable authority snapshot.
    ///
    /// Classifiers must not synthesize a lifecycle decision from records read
    /// across multiple independently locked store generations.
    fn port_lease_records_snapshot(
        &self,
        requests: &[PortLeaseRequest],
        provider_name: &str,
    ) -> Result<Vec<PortLeaseRecord>> {
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to open {provider_name} port authority: {error}"),
            }
        })?;
        let records = authority
            .list()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to inspect {provider_name} port authority: {error}"),
            })?;
        let records = records
            .into_iter()
            .map(|record| (record.request().lease_id().clone(), record))
            .collect::<BTreeMap<_, _>>();
        requests
            .iter()
            .map(|request| {
                let record = records.get(request.lease_id()).ok_or_else(|| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "{provider_name} port lease {} does not exist",
                            request.lease_id()
                        ),
                    }
                })?;
                if record.request() != request {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "{provider_name} port lease {} does not match its durable identity and \
                             fence",
                            request.lease_id()
                        ),
                    });
                }
                Ok(record.clone())
            })
            .collect()
    }

    /// Preserve a provider-preparation failure together with failed durable
    /// compensation for an authenticated request set that never reached
    /// adoption.
    pub(crate) fn compensate_failed_never_bound_requests(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
        provider_error: SandboxError,
        operation: &str,
    ) -> SandboxError {
        match self.release_never_bound_requests(requests, reservation_claim) {
            Ok(()) => provider_error,
            Err(compensation_error) => SandboxError::OperationFailed {
                message: format!(
                    "{operation} failed: {provider_error}; \
                     never-bound port reservation compensation also failed: {compensation_error}"
                ),
            },
        }
    }

    /// Produce inert plan-only bindings without claiming production authority.
    ///
    /// This exists only for plan rendering. Execute-mode callers must use
    /// [`Self::reserve_launch_ports_for_sandbox`].
    pub(crate) fn preview_bindings_for_sandbox(
        &self,
        tenant_id: &TenantId,
        existing_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<Vec<SandboxPortBinding>> {
        let unmapped_count = unmapped_tcp_guest_ports(existing_bindings, exposed_ports).len();
        self.ensure_preview_tenant_port_quota(
            tenant_id,
            existing_bindings.len().saturating_add(unmapped_count),
        )?;
        self.preview_missing_bindings(existing_bindings, exposed_ports)
    }

    /// Produce the deterministic, authority-free rendering for missing ports.
    ///
    /// Callers that represent a sandbox launch must use
    /// [`Self::preview_bindings_for_sandbox`] so the rendered launch is checked
    /// against current durable tenant usage. A preview never creates usage:
    /// execute-mode reservation is the sole atomic quota/allocation authority.
    pub(crate) fn preview_missing_bindings(
        &self,
        existing_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<Vec<SandboxPortBinding>> {
        // Plan rendering and execute reservation must validate the same range.
        // In particular, never emit port zero into a preview that the durable
        // authority will deterministically reject during handoff.
        self.range_request_mode()?;
        let mut occupied_bindings = existing_bindings.to_vec();
        unmapped_tcp_guest_ports(existing_bindings, exposed_ports)
            .into_iter()
            .map(|guest_port| {
                let binding = self.next_preview_binding(&occupied_bindings, guest_port)?;
                occupied_bindings.push(binding.clone());
                Ok(binding)
            })
            .collect()
    }

    /// Authenticate the plan-rendered binding list against its canonical
    /// operator and image inputs before the runner converts previews into
    /// durable range requests.
    pub(crate) fn validate_plan_binding_provenance(
        &self,
        requested_bindings: &[SandboxPortBinding],
        rendered_bindings: &[SandboxPortBinding],
        exposed_ports: &[OciExposedPort],
    ) -> Result<BTreeSet<String>> {
        let automatic_guest_ports = unmapped_tcp_guest_ports(requested_bindings, exposed_ports);
        let expected_len = requested_bindings
            .len()
            .saturating_add(automatic_guest_ports.len());
        if rendered_bindings.len() != expected_len
            || !rendered_bindings.starts_with(requested_bindings)
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner port binding provenance mismatch: {} canonical operator \
                     bindings and {} image-derived bindings do not match {} rendered bindings",
                    requested_bindings.len(),
                    automatic_guest_ports.len(),
                    rendered_bindings.len()
                ),
            });
        }

        let automatic_bindings = &rendered_bindings[requested_bindings.len()..];
        for (binding, guest_port) in automatic_bindings.iter().zip(&automatic_guest_ports) {
            let expected = SandboxPortBinding::tcp(
                auto_binding_name(*guest_port),
                binding.host_port,
                *guest_port,
            );
            if binding.host_port == 0 || binding != &expected {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner port binding provenance mismatch: image-derived TCP \
                         guest port {guest_port} has non-canonical rendered binding {binding:?}"
                    ),
                });
            }
        }

        Ok(automatic_bindings
            .iter()
            .map(published_listener_name)
            .collect())
    }

    /// Atomically reserve one internal host-side listener such as an egress PEP.
    #[cfg(test)]
    pub(crate) fn reserve_internal_listener(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        listener_name: &str,
        target: PortBindTarget,
        exposure: PortExposure,
    ) -> Result<(u16, PortLeaseRequest)> {
        let request = port_lease_request(
            tenant_id,
            sandbox_id,
            listener_name,
            OciPortLeaseIntent::host_internal(target, exposure),
            self.range_request_mode()?,
        );
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to open port reservation authority: {error}"),
            }
        })?;
        let record =
            authority
                .reserve(request.clone())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to reserve internal listener authority: {error}"),
                })?;
        let port = record
            .reserved_port()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "internal listener reservation did not select a port".to_owned(),
            })?
            .get();
        Ok((port, request))
    }

    #[cfg(test)]
    pub(crate) fn reserve_internal_listener_for_coordinator(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        listener_name: &str,
        target: PortBindTarget,
        exposure: PortExposure,
    ) -> Result<(u16, PortLeaseRequest, NetworkReservationClaim)> {
        let request = port_lease_request(
            tenant_id,
            sandbox_id,
            listener_name,
            OciPortLeaseIntent::host_internal(target, exposure),
            self.range_request_mode()?,
        );
        let (request, selected, reservation_claim) = reserve(&self.state_root, request)?;
        Ok((selected.get(), request, reservation_claim))
    }

    pub(crate) fn reservation_claim_for_requests(
        &self,
        requests: &[PortLeaseRequest],
    ) -> Result<Option<NetworkReservationClaim>> {
        self.reservation_claim_for_requests_with_observer(requests, |_| {})
    }

    fn reservation_claim_for_requests_with_observer(
        &self,
        requests: &[PortLeaseRequest],
        mut after_record: impl FnMut(usize),
    ) -> Result<Option<NetworkReservationClaim>> {
        let records = self.port_lease_records_snapshot(requests, "reservation claim")?;
        let mut expected: Option<Option<NetworkReservationClaim>> = None;
        for (index, (_request, record)) in requests.iter().zip(records).enumerate() {
            let current = record.reservation_claim().cloned();
            after_record(index);
            match expected.as_ref() {
                None => expected = Some(current),
                Some(expected) if expected == &current => {}
                Some(_) => {
                    return Err(SandboxError::OperationFailed {
                        message: "port request batch does not share one reservation coordinator"
                            .to_owned(),
                    });
                }
            }
        }
        Ok(expected.flatten())
    }

    pub(crate) fn require_binding_leases(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        if bindings.len() != leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} published bindings but {} durable port leases",
                    bindings.len(),
                    leases.len()
                ),
            });
        }
        for (binding, request) in bindings.iter().zip(leases) {
            let expected_port =
                NonZeroU16::new(binding.host_port).ok_or_else(|| SandboxError::InvalidSpec {
                    message: format!(
                        "sandbox port binding {:?} must use a non-zero host port",
                        binding.name
                    ),
                })?;
            let (target, publication, exposure) = self.published_binding_scope(binding)?;
            require_current_listener_authority(
                &self.state_root,
                ExpectedListenerAuthority::published(
                    tenant_id,
                    sandbox_id,
                    listener_name(binding.name.as_str(), binding.guest_port),
                    target,
                    publication,
                    exposure,
                    expected_port,
                ),
                request,
            )?;
        }
        Ok(())
    }

    pub(crate) fn activate_netavark_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        if leases.len() != claims.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} Netavark listener leases but {} durable bind claims",
                    leases.len(),
                    claims.len()
                ),
            });
        }
        for request in leases {
            require_current_bind_authority(&self.state_root, request)?;
        }
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        let actual_addrs = bindings
            .iter()
            .map(SandboxPortBinding::host_socket_addr)
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_batch(
            &self.state_root,
            leases,
            claims,
            &actual_addrs,
            OciPortProvider::Netavark,
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    pub(crate) fn claim_netavark_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortBindClaim>> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        claim_bind_attempts(
            &self.state_root,
            leases,
            OciPortProvider::Netavark,
            reservation_claim.as_ref(),
        )
    }

    /// Abandon one exact Netavark claim batch after provider detach is confirmed.
    pub(crate) fn abandon_netavark_bind_claims_without_effect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<()> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::Netavark,
            OciPortProvider::Netavark,
            leases,
            claims,
        )?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        abandon_bind_attempts_without_effect(&self.state_root, leases, claims, reservation_claim)?;
        Ok(())
    }

    pub(crate) fn expected_netavark_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<nimbus_network::PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                provider_binding(
                    request,
                    binding.host_socket_addr(),
                    OciPortProvider::Netavark,
                )
            })
            .collect()
    }

    pub(crate) fn prepare_netavark_bindings_for_rebind(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[nimbus_network::PortLeaseBinding],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        prepare_rebind_batch_after_confirmed_stop(&self.state_root, leases, expected_bindings)?;
        Ok(())
    }

    fn require_binding_lease_identities(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        self.binding_lease_records(tenant_id, sandbox_id, bindings, leases)?;
        Ok(())
    }

    pub(crate) fn activate_machine_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
    ) -> Result<Vec<nimbus_network::PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        if leases.len() != claims.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} machine listener leases but {} durable bind claims",
                    leases.len(),
                    claims.len()
                ),
            });
        }
        for request in leases {
            require_current_bind_authority(&self.state_root, request)?;
        }
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        let actual_addrs = bindings
            .iter()
            .map(machine_port_proxy_guest_listener_addr)
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_batch(
            &self.state_root,
            leases,
            claims,
            &actual_addrs,
            OciPortProvider::MachinePortProxy,
            reservation_claim.as_ref(),
        )?
        .into_iter()
        .map(|record| {
            record
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "activated machine port lease {} lost exact provider binding evidence",
                        record.request().lease_id()
                    ),
                })
        })
        .collect()
    }

    pub(crate) fn claim_machine_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortBindClaim>> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        claim_bind_attempts(
            &self.state_root,
            leases,
            OciPortProvider::MachinePortProxy,
            reservation_claim.as_ref(),
        )
    }

    pub(crate) fn abandon_machine_bind_claims_without_effect(
        &self,
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
    ) -> Result<()> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::MachinePortProxy,
            OciPortProvider::MachinePortProxy,
            leases,
            claims,
        )?;
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        abandon_bind_attempts_without_effect(
            &self.state_root,
            leases,
            claims,
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    fn require_published_bind_claim_batch(
        &self,
        expected_manager: PublishedListenerProvider,
        expected_provider: OciPortProvider,
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
    ) -> Result<()> {
        self.require_published_listener_provider(expected_manager)?;
        if leases.len() != claims.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} {expected_manager:?} listener leases but {} durable bind claims",
                    leases.len(),
                    claims.len()
                ),
            });
        }
        let expected_provider_id = expected_provider.provider_id();
        if let Some(foreign) = claims
            .iter()
            .find(|claim| claim.provider_attempt().provider_id() != &expected_provider_id)
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot abandon {expected_manager:?} claim from provider {}",
                    foreign.provider_attempt().provider_id()
                ),
            });
        }
        Ok(())
    }

    pub(crate) fn require_active_machine_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<nimbus_network::PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                require_active_provider_binding(
                    &self.state_root,
                    request,
                    machine_port_proxy_guest_listener_addr(binding),
                    OciPortProvider::MachinePortProxy,
                )?
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "active machine port lease {} lost exact provider binding evidence",
                        request.lease_id()
                    ),
                })
            })
            .collect()
    }

    pub(crate) fn require_releasable_machine_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<nimbus_network::PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                crate::backends::oci::port_lease::require_releasable_provider_binding(
                    &self.state_root,
                    request,
                    machine_port_proxy_guest_listener_addr(binding),
                    OciPortProvider::MachinePortProxy,
                )?
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "releasable machine port lease {} lost exact provider binding evidence",
                        request.lease_id()
                    ),
                })
            })
            .collect()
    }

    pub(crate) fn prepare_machine_bindings_for_rebind(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[nimbus_network::PortLeaseBinding],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        prepare_rebind_batch_after_confirmed_stop(&self.state_root, leases, expected_bindings)?;
        Ok(())
    }

    /// Return whether every exact machine listener is durably terminal with no
    /// live effect that this process must stop.
    ///
    /// `Failed` requires the exact adapter provider and a retained reservation
    /// coordinator because bind-failure recording is no-effect evidence.
    /// `Released` requires exact historical provider evidence when present.
    /// A mixed terminal batch must share one coordinator. Every other shape
    /// retains ambiguity or live authority.
    pub(crate) fn machine_bindings_are_terminal_without_effect(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<bool> {
        Ok(
            self.classify_machine_cleanup_batch(tenant_id, sandbox_id, bindings, leases)?
                == LaunchPortBatchState::TerminalNoEffect,
        )
    }

    pub(crate) fn record_machine_proxy_bind_failure(
        &self,
        request: &PortLeaseRequest,
        claim: &PortBindClaim,
        attempted_addr: std::net::SocketAddr,
        error_kind: io::ErrorKind,
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        let reservation_claim =
            self.reservation_claim_for_requests(std::slice::from_ref(request))?;
        record_bind_failure(
            &self.state_root,
            request,
            claim,
            attempted_addr,
            OciPortProvider::MachinePortProxy,
            error_kind,
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    fn binding_lease_records(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecord>> {
        if bindings.len() != leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} published bindings but {} durable port leases",
                    bindings.len(),
                    leases.len()
                ),
            });
        }
        bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                let expected_port = NonZeroU16::new(binding.host_port).ok_or_else(|| {
                    SandboxError::InvalidSpec {
                        message: format!(
                            "sandbox port binding {:?} must use a non-zero host port",
                            binding.name
                        ),
                    }
                })?;
                let (target, publication, exposure) = self.published_binding_scope(binding)?;
                require_listener_authority(
                    &self.state_root,
                    ExpectedListenerAuthority::published(
                        tenant_id,
                        sandbox_id,
                        listener_name(binding.name.as_str(), binding.guest_port),
                        target,
                        publication,
                        exposure,
                        expected_port,
                    ),
                    request,
                )
            })
            .collect()
    }

    fn published_binding_scope(
        &self,
        binding: &SandboxPortBinding,
    ) -> Result<(PortBindTarget, PortPublicationIntent, PortExposure)> {
        let (external_target, exposure) = published_scope(binding.host_address)?;
        let target = match self.published_listener_provider {
            PublishedListenerProvider::Netavark => external_target,
            PublishedListenerProvider::MachinePortProxy => PortBindTarget::ipv4_wildcard(),
        };
        let publication = PortPublicationIntent::host(binding.host_address);
        Ok((target, publication, exposure))
    }

    fn require_published_listener_provider(
        &self,
        expected: PublishedListenerProvider,
    ) -> Result<()> {
        if self.published_listener_provider == expected {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "published listener authority is configured for {:?}, not {:?}",
                self.published_listener_provider, expected
            ),
        })
    }

    pub(crate) fn withdraw_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let authenticated = self.binding_lease_records(tenant_id, sandbox_id, bindings, leases)?;
        let mut errors = Vec::new();
        for ((binding, request), record) in bindings.iter().zip(leases).zip(authenticated) {
            if matches!(
                record.phase(),
                PortLeasePhase::Withdrawing | PortLeasePhase::Released | PortLeasePhase::Failed
            ) {
                continue;
            }
            if let Err(error) = withdraw(&self.state_root, request) {
                errors.push(format!(
                    "listener {:?} lease {} at {}:{} withdrawal was blocked: {error}",
                    binding.name,
                    request.lease_id(),
                    binding.host_address,
                    binding.host_port
                ));
            }
        }
        if !errors.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "port withdrawal for tenant {tenant_id} sandbox {sandbox_id} was incomplete: \
                     {}",
                    errors.join("; ")
                ),
            });
        }
        Ok(())
    }

    pub(crate) fn release_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let authenticated = self.binding_lease_records(tenant_id, sandbox_id, bindings, leases)?;
        let mut errors = Vec::new();
        for ((binding, request), record) in bindings.iter().zip(leases).zip(authenticated) {
            if matches!(
                record.phase(),
                PortLeasePhase::Released | PortLeasePhase::Failed
            ) {
                continue;
            }
            if let Err(error) = release(&self.state_root, request) {
                errors.push(format!(
                    "listener {:?} lease {} at {}:{} release was blocked: {error}",
                    binding.name,
                    request.lease_id(),
                    binding.host_address,
                    binding.host_port
                ));
            }
        }
        if !errors.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "port release for tenant {tenant_id} sandbox {sandbox_id} was incomplete: {}",
                    errors.join("; ")
                ),
            });
        }
        Ok(())
    }

    fn next_preview_binding(
        &self,
        occupied_bindings: &[SandboxPortBinding],
        guest_port: u16,
    ) -> Result<SandboxPortBinding> {
        for host_port in self.range.clone() {
            let candidate =
                SandboxPortBinding::tcp(auto_binding_name(guest_port), host_port, guest_port);
            let candidate_spec = self.preview_binding_spec(&candidate)?;
            let overlaps = occupied_bindings
                .iter()
                .filter(|existing| existing.host_port == host_port)
                .map(|existing| self.preview_binding_spec(existing))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|existing| candidate_spec.overlaps(existing));
            if !overlaps {
                return Ok(candidate);
            }
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "published port range {}-{} is exhausted",
                self.range.start(),
                self.range.end()
            ),
        })
    }

    fn preview_binding_spec(&self, binding: &SandboxPortBinding) -> Result<PortBindingSpec> {
        let port = NonZeroU16::new(binding.host_port).ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "sandbox port binding {:?} must use a non-zero host port",
                binding.name
            ),
        })?;
        let (target, _, exposure) = self.published_binding_scope(binding)?;
        Ok(PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            target,
            exposure,
            PortRequestMode::Exact(port),
        ))
    }

    fn range_request_mode(&self) -> Result<PortRequestMode> {
        let start =
            NonZeroU16::new(*self.range.start()).ok_or_else(|| SandboxError::InvalidSpec {
                message: "published port range must start above zero".to_owned(),
            })?;
        let end = NonZeroU16::new(*self.range.end()).ok_or_else(|| SandboxError::InvalidSpec {
            message: "published port range must end above zero".to_owned(),
        })?;
        PortRequestMode::range(start, end).map_err(|error| SandboxError::InvalidSpec {
            message: format!("invalid published port range: {error}"),
        })
    }

    fn ensure_preview_tenant_port_quota(
        &self,
        tenant_id: &TenantId,
        launch_ports: usize,
    ) -> Result<()> {
        let Some(max_ports_per_tenant) = self.max_ports_per_tenant else {
            return Ok(());
        };
        let active_ports = self.read_published_lease_count_for_tenant(tenant_id)?;
        let requested_ports = active_ports.saturating_add(launch_ports);
        if requested_ports <= max_ports_per_tenant {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "published port quota exceeded for tenant {tenant_id}: {requested_ports} requested/reserved ports exceeds limit {max_ports_per_tenant}"
            ),
        })
    }

    fn read_published_lease_count_for_tenant(&self, tenant_id: &TenantId) -> Result<usize> {
        let authority = LocalPortLeaseAuthority::open(&self.state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to inspect durable tenant port usage at {} for tenant {tenant_id}: \
                     {error}",
                    self.state_root.display()
                ),
            }
        })?;
        Ok(authority
            .list()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to list durable tenant port usage at {} for tenant {tenant_id}: \
                     {error}",
                    self.state_root.display()
                ),
            })?
            .into_iter()
            .filter(|record| {
                !record.phase().is_terminal()
                    && record.request().accounting() == PortLeaseAccounting::TenantPublished
                    && record.request().tenant_id() == Some(tenant_id)
            })
            .count())
    }
}

fn unmapped_tcp_guest_ports(
    existing_bindings: &[SandboxPortBinding],
    exposed_ports: &[OciExposedPort],
) -> Vec<u16> {
    let mut mapped_guest_ports: BTreeSet<u16> = existing_bindings
        .iter()
        .map(|binding| binding.guest_port)
        .collect();
    exposed_ports
        .iter()
        .filter(|exposed_port| exposed_port.protocol == OciExposedPortProtocol::Tcp)
        .filter_map(|exposed_port| {
            mapped_guest_ports
                .insert(exposed_port.port)
                .then_some(exposed_port.port)
        })
        .collect()
}

fn auto_binding_name(guest_port: u16) -> String {
    format!("tcp-{guest_port}")
}

fn listener_name(binding_name: &str, guest_port: u16) -> String {
    format!("published:{binding_name}:{guest_port}")
}

pub(crate) fn published_listener_name(binding: &SandboxPortBinding) -> String {
    listener_name(binding.name.as_str(), binding.guest_port)
}

#[cfg(test)]
#[path = "port_manager/tests.rs"]
mod tests;
