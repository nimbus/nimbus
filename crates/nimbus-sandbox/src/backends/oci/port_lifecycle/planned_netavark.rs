//! Complete-plan authentication for selected Netavark listener authority.

use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkReservationClaim, PortBindClaim, PortBindRealm, PortLeaseAccounting, PortLeaseBinding,
    PortLeaseRecord, PortLeaseRecoveryGuard, PortLeaseRequest, PortProtocol, PortRequestMode,
};

use super::{OciPortLeaseCoordinator, PublishedListenerProvider};
use crate::backends::oci::port_lease::{
    OciPortBindLifetimeBatch, OciPortProvider,
    prepare_provider_managed_plan_claims_after_confirmed_stop,
    prepare_provider_managed_plan_members_after_confirmed_stop,
    prepare_provider_managed_plan_members_after_confirmed_stop_with_lifetimes, provider_binding,
    recover_provider_managed_plan_members_after_owner_death,
    release_reserved_plan_members_without_effect,
};
use crate::error::{Result, SandboxError};
use crate::spec::SandboxPortBinding;

impl OciPortLeaseCoordinator {
    /// Release one never-bound compiler-issued provider subset.
    pub(crate) fn release_never_bound_plan_members(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        release_reserved_plan_members_without_effect(
            self.authority()?,
            plan_members,
            requests,
            reservation_claim,
        )?;
        Ok(())
    }

    pub(crate) fn planned_published_listener_records(
        &self,
        plan_members: &[PortLeaseRequest],
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecord>> {
        if bindings.len() != leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} compiled published bindings but {} durable port leases",
                    bindings.len(),
                    leases.len()
                ),
            });
        }
        let records = self
            .authority()?
            .inspect_plan_members(plan_members, leases)
            .map_err(super::port_lease_error)?;
        bindings
            .iter()
            .zip(leases)
            .zip(records)
            .map(|((binding, request), record)| {
                let expected_port =
                    NonZeroU16::new(binding.host_port).ok_or_else(|| SandboxError::InvalidSpec {
                        message: format!(
                            "sandbox port binding {:?} must use a non-zero host port",
                            binding.name
                        ),
                    })?;
                let (target, publication, exposure) = self.published_binding_scope(binding)?;
                let requested_port_matches = match request.binding().port() {
                    PortRequestMode::Exact(port) => *port == expected_port,
                    PortRequestMode::Range(range) => {
                        range.start() <= expected_port && expected_port <= range.end()
                    }
                    PortRequestMode::ProviderAssigned => true,
                };
                if request.tenant_id() != Some(tenant_id)
                    || request.accounting() != PortLeaseAccounting::TenantPublished
                    || request.publication() != &publication
                    || request.binding().protocol() != PortProtocol::Tcp
                    || request.binding().realm() != &PortBindRealm::Host
                    || request.binding().target() != &target
                    || request.binding().exposure() != exposure
                    || !requested_port_matches
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "compiled published listener {:?} crossed its tenant, accounting, publication, or binding intent",
                            binding.name
                        ),
                    });
                }
                if record.reserved_port() != Some(expected_port) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "compiled published listener {:?} does not own port {expected_port}",
                            binding.name
                        ),
                    });
                }
                Ok(record)
            })
            .collect()
    }

    pub(crate) fn port_lease_plan_member_records_snapshot(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        label: &str,
    ) -> Result<Vec<PortLeaseRecord>> {
        self.authority()?
            .inspect_plan_members(plan_members, requests)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to inspect {label} plan-member snapshot: {error}"),
            })
    }

    pub(crate) fn expected_planned_netavark_bindings(
        &self,
        plan_members: &[PortLeaseRequest],
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.planned_published_listener_records(plan_members, tenant_id, bindings, leases)?;
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

    pub(crate) fn prepare_planned_netavark_bindings_for_rebind_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        prepare_provider_managed_plan_members_after_confirmed_stop_with_lifetimes(
            self.authority()?,
            plan_members,
            leases,
            expected_bindings,
            batch.lifetimes(),
        )?;
        Ok(())
    }

    pub(crate) fn release_planned_restart_retained_bindings(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.authority()?
            .release_plan_members_after_confirmed_stop(plan_members, leases)
            .map_err(super::port_lease_error)?;
        Ok(())
    }

    pub(crate) fn recover_planned_netavark_bindings_after_owner_death(
        &self,
        plan_members: &[PortLeaseRequest],
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<(Vec<PortLeaseBinding>, Vec<PortLeaseRecoveryGuard>)> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        let records =
            self.planned_published_listener_records(plan_members, tenant_id, bindings, leases)?;
        let expected = bindings
            .iter()
            .zip(leases)
            .zip(records)
            .map(|((binding, request), record)| {
                let binding_evidence = provider_binding(
                    request,
                    binding.host_socket_addr(),
                    OciPortProvider::Netavark,
                )?;
                if record.binding() != Some(&binding_evidence) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "recoverable compiled Netavark listener {} lost exact provider binding evidence",
                            request.lease_id()
                        ),
                    });
                }
                Ok(binding_evidence)
            })
            .collect::<Result<Vec<_>>>()?;
        let recoveries = recover_provider_managed_plan_members_after_owner_death(
            self.authority()?,
            plan_members,
            leases,
        )?;
        self.authority()?
            .mark_cleanup_pending_plan_members_after_owner_death(plan_members, leases, &recoveries)
            .map_err(super::port_lease_error)?;
        Ok((expected, recoveries))
    }

    pub(crate) fn recover_planned_netavark_claims_after_owner_death(
        &self,
        plan_members: &[PortLeaseRequest],
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        claims: &[PortBindClaim],
    ) -> Result<Vec<PortLeaseRecoveryGuard>> {
        self.require_published_bind_claim_batch(
            PublishedListenerProvider::Netavark,
            OciPortProvider::Netavark,
            leases,
            claims,
        )?;
        self.planned_published_listener_records(plan_members, tenant_id, bindings, leases)?;
        let recoveries = recover_provider_managed_plan_members_after_owner_death(
            self.authority()?,
            plan_members,
            leases,
        )?;
        self.authority()?
            .mark_cleanup_pending_plan_members_after_owner_death(plan_members, leases, &recoveries)
            .map_err(super::port_lease_error)?;
        Ok(recoveries)
    }

    pub(crate) fn prepare_recovered_planned_netavark_bindings_for_rebind(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        expected_bindings: &[PortLeaseBinding],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        prepare_provider_managed_plan_members_after_confirmed_stop(
            self.authority()?,
            plan_members,
            leases,
            expected_bindings,
            recoveries,
        )?;
        Ok(())
    }

    pub(crate) fn prepare_recovered_planned_netavark_claims_for_rebind(
        &self,
        plan_members: &[PortLeaseRequest],
        leases: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<()> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        prepare_provider_managed_plan_claims_after_confirmed_stop(
            self.authority()?,
            plan_members,
            leases,
            recoveries,
        )?;
        Ok(())
    }
}
