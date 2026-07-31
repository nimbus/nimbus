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
use super::*;

pub(crate) fn machine_port_proxy_guest_listener_addr(binding: &SandboxPortBinding) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, binding.host_port))
}

impl OciPortLeaseCoordinator {
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
