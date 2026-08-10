//! MachinePortProxy-specific published-listener lifecycle capability.
//!
//! The portable lease authority remains owned by `OciPortLeaseCoordinator`.
//! This module only translates machine-listener intent and observations into
//! that authority's existing transitions.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

#[cfg(test)]
use super::super::port_lease::{
    abandon_bind_attempts_without_effect, adopt_claimed_and_activate_batch, claim_bind_attempts,
    prepare_rebind_batch_after_confirmed_stop, record_bind_failure,
};
use super::super::port_lease::{
    prepare_provider_managed_plan_members_after_confirmed_stop,
    recover_provider_managed_plan_members_after_owner_death,
};
use super::*;

pub(crate) fn machine_port_proxy_guest_listener_addr(binding: &SandboxPortBinding) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, binding.host_port))
}

/// Compiler-selected endpoint represented by the forwarded-machine lease.
///
/// The guest wildcard socket is a provider-local transport hop. The portable
/// planned lease describes the externally selected bind target that the
/// machine-forwarding owner publishes, while publication intent independently
/// preserves the exact desired host address.
fn planned_machine_forwarded_binding_addr(
    request: &PortLeaseRequest,
    binding: &SandboxPortBinding,
) -> SocketAddr {
    let address = request
        .binding()
        .target()
        .specific_address()
        .unwrap_or_else(|| match request.binding().target().family() {
            Some(nimbus_network::PortAddressFamily::Ipv4) => Ipv4Addr::UNSPECIFIED.into(),
            Some(nimbus_network::PortAddressFamily::Ipv6) => std::net::Ipv6Addr::UNSPECIFIED.into(),
            None => canonical_socket_ip(binding.host_address),
        });
    SocketAddr::new(address, binding.host_port)
}

impl OciPortLeaseCoordinator {
    /// Authenticate the exact compiler-planned publication subset without
    /// deriving listener identity or resource fences from the sandbox ID.
    pub(crate) fn require_planned_machine_binding_leases(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        if bindings.len() != leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} planned machine bindings but {} durable port leases",
                    bindings.len(),
                    leases.len()
                ),
            });
        }
        for (binding, request) in bindings.iter().zip(leases) {
            let record = self
                .authority()?
                .inspect_plan_member(plan_members, request)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "planned machine listener {} failed complete-plan authentication: {error}",
                        request.lease_id()
                    ),
                })?;
            let expected_port = std::num::NonZeroU16::new(binding.host_port).ok_or_else(|| {
                SandboxError::InvalidSpec {
                    message: format!(
                        "sandbox port binding {:?} must use a non-zero host port",
                        binding.name
                    ),
                }
            })?;
            let (_, exposure) = published_scope(binding.host_address)?;
            let exact_scope = request.tenant_id() == Some(tenant_id)
                && request.accounting() == PortLeaseAccounting::TenantPublished
                && request.publication().host_address()
                    == Some(canonical_socket_ip(binding.host_address))
                && request.binding().protocol() == PortProtocol::Tcp
                && request.binding().realm() == &PortBindRealm::Host
                && request.binding().exposure() == exposure
                && record.reserved_port() == Some(expected_port);
            if !exact_scope {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "planned machine listener {} diverges from the compiler-selected tenant, publication, protocol, realm, exposure, or port",
                        request.lease_id()
                    ),
                });
            }
        }
        Ok(())
    }

    #[cfg(test)]
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
            require_current_bind_authority(self.authority()?, request)?;
        }
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        let actual_addrs = bindings
            .iter()
            .map(machine_port_proxy_guest_listener_addr)
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_batch(
            self.authority()?,
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

    pub(crate) fn activate_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        for request in leases {
            require_current_bind_authority(self.authority()?, request)?;
        }
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        let actual_addrs = bindings
            .iter()
            .map(machine_port_proxy_guest_listener_addr)
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_batch_with_lifetimes(
            self.authority()?,
            leases,
            batch,
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

    pub(crate) fn activate_planned_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        for request in leases {
            require_current_bind_authority(self.authority()?, request)?;
        }
        let actual_addrs = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| planned_machine_forwarded_binding_addr(request, binding))
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_plan_members_with_lifetimes(
            self.authority()?,
            plan_members,
            leases,
            batch,
            &actual_addrs,
            OciPortProvider::MachinePortProxy,
            reservation_claim,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "planned machine publication activation rejected compiler targets {:?} and provider bindings {:?}: {error}",
                leases
                    .iter()
                    .map(|request| request.binding().target())
                    .collect::<Vec<_>>(),
                actual_addrs
            ),
        })?
        .into_iter()
        .map(|record| {
            record
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "activated planned machine port lease {} lost exact provider binding evidence",
                        record.request().lease_id()
                    ),
                })
        })
        .collect()
    }

    pub(crate) fn activate_rebind_planned_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        let actual_addrs = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| planned_machine_forwarded_binding_addr(request, binding))
            .collect::<Vec<_>>();
        adopt_claimed_and_activate_rebind_plan_members_with_lifetimes(
            self.authority()?,
            plan_members,
            leases,
            &actual_addrs,
            OciPortProvider::MachinePortProxy,
            batch,
        )?
        .into_iter()
        .map(|record| {
            record
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "rebound planned machine port lease {} lost exact provider binding evidence",
                        record.request().lease_id()
                    ),
                })
        })
        .collect()
    }

    #[cfg(test)]
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
            self.authority()?,
            leases,
            OciPortProvider::MachinePortProxy,
            reservation_claim.as_ref(),
        )
    }

    pub(crate) fn claim_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<OciPortBindLifetimeBatch> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_leases(tenant_id, sandbox_id, bindings, leases)?;
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        claim_bind_attempts_with_lifetimes(
            self.authority()?,
            leases,
            OciPortProvider::MachinePortProxy,
            reservation_claim.as_ref(),
            PortLeaseEffectScope::ProviderManaged,
        )
    }

    pub(crate) fn claim_planned_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciPortBindLifetimeBatch> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        claim_bind_plan_members_attempts_with_lifetimes(
            self.authority()?,
            plan_members,
            leases,
            OciPortProvider::MachinePortProxy,
            reservation_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
    }

    pub(crate) fn claim_rebind_planned_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<OciPortBindLifetimeBatch> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        let actual_addrs = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| planned_machine_forwarded_binding_addr(request, binding))
            .collect::<Vec<_>>();
        claim_rebind_plan_members_attempts_with_lifetimes(
            self.authority()?,
            plan_members,
            leases,
            &actual_addrs,
            OciPortProvider::MachinePortProxy,
            PortLeaseEffectScope::ProviderManaged,
        )
    }

    #[cfg(test)]
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
            self.authority()?,
            leases,
            claims,
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    pub(crate) fn abandon_machine_bind_claims_with_lifetimes_without_effect(
        &self,
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<()> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::MachinePortProxy,
            OciPortProvider::MachinePortProxy,
            leases,
            batch.claims(),
        )?;
        let reservation_claim = self.reservation_claim_for_requests(leases)?;
        abandon_bind_attempts_with_lifetimes_without_effect(
            self.authority()?,
            leases,
            batch,
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    pub(crate) fn abandon_planned_machine_bind_claims_with_lifetimes_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::MachinePortProxy,
            OciPortProvider::MachinePortProxy,
            leases,
            batch.claims(),
        )?;
        abandon_bind_plan_members_attempts_with_lifetimes_without_effect(
            self.authority()?,
            plan_members,
            leases,
            batch,
            reservation_claim,
        )?;
        Ok(())
    }

    pub(crate) fn abandon_rebind_planned_machine_bind_claims_with_lifetimes_without_effect(
        &self,
        bindings: &[SandboxPortBinding],
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<()> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::MachinePortProxy,
            OciPortProvider::MachinePortProxy,
            leases,
            batch.claims(),
        )?;
        let actual_addrs = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| planned_machine_forwarded_binding_addr(request, binding))
            .collect::<Vec<_>>();
        abandon_rebind_plan_members_attempts_with_lifetimes_without_effect(
            self.authority()?,
            plan_members,
            leases,
            &actual_addrs,
            OciPortProvider::MachinePortProxy,
            batch,
        )?;
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
                    self.authority()?,
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

    pub(crate) fn require_active_planned_machine_bindings(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        leases
            .iter()
            .zip(bindings)
            .map(|(request, binding)| {
                require_active_provider_binding(
                    self.authority()?,
                    request,
                    planned_machine_forwarded_binding_addr(request, binding),
                    OciPortProvider::MachinePortProxy,
                )?
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "active planned machine port lease {} lost exact provider binding evidence",
                        request.lease_id()
                    ),
                })
            })
            .collect()
    }

    pub(crate) fn require_active_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        if leases.len() != batch.lifetimes().len() || leases.len() != batch.claims().len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} machine listener leases but {} live process lifetimes and {} \
                     bind claims",
                    leases.len(),
                    batch.lifetimes().len(),
                    batch.claims().len()
                ),
            });
        }
        let active =
            self.require_active_machine_bindings(tenant_id, sandbox_id, bindings, leases)?;
        for ((request, binding), lifetime) in leases.iter().zip(bindings).zip(batch.lifetimes()) {
            let record = require_active_provider_binding(
                self.authority()?,
                request,
                machine_port_proxy_guest_listener_addr(binding),
                OciPortProvider::MachinePortProxy,
            )?;
            if lifetime.request() != request
                || record.active_lifetime() != Some(lifetime.lifetime())
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox machine port lease {} is not owned by the retained live process \
                         lifetime",
                        request.lease_id()
                    ),
                });
            }
        }
        Ok(active)
    }

    pub(crate) fn require_active_planned_machine_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        if leases.len() != batch.lifetimes().len() || leases.len() != batch.claims().len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} planned machine listener leases but {} live process lifetimes and {} bind claims",
                    leases.len(),
                    batch.lifetimes().len(),
                    batch.claims().len()
                ),
            });
        }
        leases
            .iter()
            .zip(bindings)
            .zip(batch.lifetimes())
            .map(|((request, binding), lifetime)| {
                let record = require_active_provider_binding(
                    self.authority()?,
                    request,
                    planned_machine_forwarded_binding_addr(request, binding),
                    OciPortProvider::MachinePortProxy,
                )?;
                if lifetime.request() != request
                    || record.active_lifetime() != Some(lifetime.lifetime())
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "planned machine port lease {} is not owned by the retained live process lifetime",
                            request.lease_id()
                        ),
                    });
                }
                record
                    .binding()
                    .cloned()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "active planned machine port lease {} lost exact provider binding evidence",
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
                    self.authority()?,
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

    /// Authenticate the exact durable listener generation before an external
    /// machine-forwarding withdrawal or retry.
    ///
    /// A fresh process first acquires the dead owner's lifetime guards and
    /// durably moves the batch to `CleanupPending`. That phase fences reuse but
    /// does not mean the external forwarder effect is absent, so publication
    /// reconciliation must still be able to inspect and withdraw it.
    pub(crate) fn require_machine_publication_withdrawal_fence(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        let records = self.port_lease_records_snapshot(leases, "machine publication withdrawal")?;
        for ((binding, request), record) in bindings.iter().zip(leases).zip(records) {
            let expected = provider_binding(
                request,
                machine_port_proxy_guest_listener_addr(binding),
                OciPortProvider::MachinePortProxy,
            )?;
            let live_or_ambiguous_effect = matches!(
                record.phase(),
                PortLeasePhase::Active
                    | PortLeasePhase::Withdrawing
                    | PortLeasePhase::CleanupPending
            ) && record.binding() == Some(&expected)
                && record.confirmed_stopped_binding().is_none();
            let restart_retained = record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.active_lifetime().is_none()
                && record.failure().is_none()
                && record.confirmed_stopped_binding() == Some(&expected);
            if !(live_or_ambiguous_effect || restart_retained)
                || record.reservation_claim().is_some()
                || record.bind_claim().is_some()
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "machine publication withdrawal for tenant {tenant_id} sandbox \
                         {sandbox_id} lacks exact fenced MachinePortProxy authority for lease {} \
                         in phase {:?}",
                        request.lease_id(),
                        record.phase()
                    ),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn require_planned_machine_publication_withdrawal_fence(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        let records =
            self.port_lease_records_snapshot(leases, "planned machine publication withdrawal")?;
        for ((binding, request), record) in bindings.iter().zip(leases).zip(records) {
            let expected = provider_binding(
                request,
                planned_machine_forwarded_binding_addr(request, binding),
                OciPortProvider::MachinePortProxy,
            )?;
            let live_or_ambiguous_effect = matches!(
                record.phase(),
                PortLeasePhase::Active
                    | PortLeasePhase::Withdrawing
                    | PortLeasePhase::CleanupPending
            ) && record.binding() == Some(&expected)
                && record.confirmed_stopped_binding().is_none();
            let restart_retained = record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.active_lifetime().is_none()
                && record.failure().is_none()
                && record.confirmed_stopped_binding() == Some(&expected);
            if !(live_or_ambiguous_effect || restart_retained) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "planned machine publication withdrawal for tenant {tenant_id} lacks exact compiler-owned MachinePortProxy authority for lease {} in phase {:?}",
                        request.lease_id(),
                        record.phase()
                    ),
                });
            }
        }
        Ok(())
    }

    /// Authenticate the immutable listener identities carried by terminal
    /// machine-publication evidence without reinterpreting the current lease
    /// phase as provider truth.
    pub(crate) fn require_machine_publication_identity(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)
    }

    pub(crate) fn require_planned_machine_publication_identity(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)
    }

    #[cfg(test)]
    pub(crate) fn prepare_machine_bindings_for_rebind(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[nimbus_network::PortLeaseBinding],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        prepare_rebind_batch_after_confirmed_stop(self.authority()?, leases, expected_bindings)?;
        Ok(())
    }

    pub(crate) fn prepare_machine_bindings_for_rebind_with_lifetimes(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        prepare_rebind_batch_after_confirmed_stop_with_lifetimes(
            self.authority()?,
            leases,
            expected_bindings,
            batch.lifetimes(),
        )?;
        Ok(())
    }

    pub(crate) fn release_machine_bindings_after_confirmed_stop_with_lifetimes(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
            self.authority()?,
            leases,
            expected_bindings,
            batch.lifetimes(),
        )?;
        Ok(())
    }

    pub(crate) fn recover_machine_bindings_after_owner_death(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<(Vec<PortLeaseBinding>, Vec<PortLeaseRecoveryGuard>)> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        let expected = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                require_provider_recovery_binding(
                    self.authority()?,
                    request,
                    machine_port_proxy_guest_listener_addr(binding),
                    OciPortProvider::MachinePortProxy,
                )?
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "recoverable machine port lease {} lost exact provider binding evidence",
                        request.lease_id()
                    ),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let recoveries =
            recover_provider_managed_batch_after_owner_death(self.authority()?, leases)?;
        Ok((expected, recoveries))
    }

    pub(crate) fn prepare_recovered_planned_machine_bindings_for_rebind(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        prepare_provider_managed_plan_members_after_confirmed_stop(
            self.authority()?,
            plan_members,
            leases,
            expected_bindings,
            recoveries,
        )?;
        Ok(())
    }

    /// Acquire exact dead-owner authority for one compiler-planned publication
    /// subset without falling back to sandbox-derived listener identity or the
    /// guest wildcard transport address.
    pub(crate) fn recover_planned_machine_bindings_after_owner_death(
        &self,
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        plan_members: &[PortLeaseRequest],
    ) -> Result<(Vec<PortLeaseBinding>, Vec<PortLeaseRecoveryGuard>)> {
        self.require_planned_machine_binding_leases(tenant_id, bindings, leases, plan_members)?;
        let expected = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                require_provider_recovery_binding(
                    self.authority()?,
                    request,
                    planned_machine_forwarded_binding_addr(request, binding),
                    OciPortProvider::MachinePortProxy,
                )?
                .binding()
                .cloned()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "recoverable planned machine port lease {} lost exact compiler binding evidence",
                        request.lease_id()
                    ),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let recoveries = recover_provider_managed_plan_members_after_owner_death(
            self.authority()?,
            plan_members,
            leases,
        )?;
        Ok((expected, recoveries))
    }

    pub(crate) fn prepare_recovered_machine_bindings_for_rebind(
        &self,
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        prepare_provider_managed_batch_after_confirmed_stop(
            self.authority()?,
            leases,
            expected_bindings,
            recoveries,
        )?;
        Ok(())
    }

    pub(crate) fn release_recovered_machine_bindings(
        &self,
        leases: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        release_provider_managed_batch_after_confirmed_stop(self.authority()?, leases, recoveries)?;
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

    #[cfg(test)]
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
            self.authority()?,
            request,
            claim,
            OciConfirmedBindFailure::new(
                attempted_addr,
                OciPortProvider::MachinePortProxy,
                error_kind,
            ),
            reservation_claim.as_ref(),
        )?;
        Ok(())
    }

    pub(crate) fn record_machine_proxy_bind_failure_with_lifetime(
        &self,
        request: &PortLeaseRequest,
        claim: &PortBindClaim,
        attempted_addr: SocketAddr,
        error_kind: io::ErrorKind,
        lifetime: &nimbus_network::PortLeaseLifetimeGuard,
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        let reservation_claim =
            self.reservation_claim_for_requests(std::slice::from_ref(request))?;
        record_bind_failure_with_lifetime(
            self.authority()?,
            request,
            claim,
            OciConfirmedBindFailure::new(
                attempted_addr,
                OciPortProvider::MachinePortProxy,
                error_kind,
            ),
            reservation_claim.as_ref(),
            lifetime,
        )?;
        Ok(())
    }
}
