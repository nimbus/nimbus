//! Provider-specific classification of durable published-listener batches.
//!
//! A process registry is evidence about live effects, not durable lifecycle
//! authority. These classifiers authenticate exact provider bindings and
//! confirmed-stop receipts before callers choose cleanup or reconciliation.

use super::*;

impl PortManager {
    /// Classify one Netavark publication batch for terminal cleanup.
    ///
    /// Initial launch compensation remains fenced by its exact reservation
    /// coordinator. A restarted workload has deliberately retired that
    /// coordinator, so its durable confirmed-stop receipts become the
    /// independent evidence for either a clean retained batch or the exact
    /// attempt-unique Netavark claims recorded by the next setup.
    pub(crate) fn classify_netavark_cleanup_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        reservation_claim: Option<&NetworkReservationClaim>,
    ) -> Result<LaunchPortBatchState> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.binding_lease_records(tenant_id, sandbox_id, bindings, leases)?;
        if let Some(reservation_claim) = reservation_claim {
            return self.classify_launch_port_batch(leases, reservation_claim);
        }
        if leases.is_empty() {
            return Ok(LaunchPortBatchState::TerminalNoEffect);
        }

        let records = self.port_lease_records_snapshot(leases, "Netavark cleanup")?;
        let expected_bindings =
            self.expected_netavark_bindings(tenant_id, sandbox_id, bindings, leases)?;
        let mut restart_retained = 0usize;
        let mut netavark_claims = Vec::new();
        let mut provider_owned = 0usize;
        let mut terminal_no_effect = 0usize;
        let mut terminal_coordinator = None;
        for ((request, record), expected_binding) in
            leases.iter().zip(records).zip(expected_bindings)
        {
            if record.phase() == PortLeasePhase::Reserved
                && record.reservation_claim().is_none()
                && record.binding().is_none()
                && record.failure().is_none()
                && record.confirmed_stopped_binding() == Some(&expected_binding)
            {
                match record.bind_claim() {
                    None => restart_retained += 1,
                    Some(claim)
                        if claim.provider_attempt().provider_id()
                            == &OciPortProvider::Netavark.provider_id() =>
                    {
                        netavark_claims.push(claim.clone());
                    }
                    Some(_) => {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "restart-retained port lease {} carries a non-Netavark provider \
                                 claim; retaining every fence for reconciliation",
                                request.lease_id()
                            ),
                        });
                    }
                }
                continue;
            }

            if record.phase() == PortLeasePhase::Released
                && record.bind_claim().is_none()
                && record
                    .binding()
                    .is_none_or(|binding| binding == &expected_binding)
                && record.confirmed_stopped_binding().is_none()
                && record.failure().is_none()
            {
                require_uniform_terminal_coordinator(
                    &mut terminal_coordinator,
                    record.reservation_claim(),
                    request,
                    "Netavark",
                )?;
                terminal_no_effect += 1;
                continue;
            }

            let terminal_failed = terminal_failed_has_no_effect(&record, OciPortProvider::Netavark);
            let live_provider_owned = record.reservation_claim().is_none()
                && record.bind_claim().is_none()
                && record.confirmed_stopped_binding().is_none()
                && matches!(
                    record.phase(),
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                )
                && record
                    .binding()
                    .is_some_and(|binding| binding == &expected_binding);
            if terminal_failed {
                require_uniform_terminal_coordinator(
                    &mut terminal_coordinator,
                    record.reservation_claim(),
                    request,
                    "Netavark",
                )?;
                terminal_no_effect += 1;
                continue;
            }
            if live_provider_owned {
                provider_owned += 1;
                continue;
            }

            return Err(SandboxError::OperationFailed {
                message: format!(
                    "port lease {} is neither launch-owned, restart-retained, nor an exact \
                     Netavark provider lifecycle record; retaining every fence for reconciliation",
                    request.lease_id()
                ),
            });
        }

        classify_uniform_batch(
            leases.len(),
            restart_retained,
            netavark_claims,
            provider_owned,
            terminal_no_effect,
            "Netavark",
        )
    }

    /// Classify one machine-proxy publication batch from durable authority.
    ///
    /// The exact wildcard guest listener is the provider effect. The external
    /// publication address remains part of the request identity, but is never
    /// substituted for the listener receipt during cleanup.
    pub(crate) fn classify_machine_cleanup_batch(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<LaunchPortBatchState> {
        self.require_published_listener_provider(PublishedListenerProvider::MachinePortProxy)?;
        self.binding_lease_records(tenant_id, sandbox_id, bindings, leases)?;
        if leases.is_empty() {
            return Ok(LaunchPortBatchState::TerminalNoEffect);
        }

        let records = self.port_lease_records_snapshot(leases, "MachinePortProxy cleanup")?;
        let expected_bindings = bindings
            .iter()
            .zip(leases)
            .map(|(binding, request)| {
                provider_binding(
                    request,
                    machine_port_proxy_guest_listener_addr(binding),
                    OciPortProvider::MachinePortProxy,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut restart_retained = 0usize;
        let mut provider_owned = 0usize;
        let mut terminal_no_effect = 0usize;
        let mut terminal_coordinator = None;
        for ((request, record), expected_binding) in
            leases.iter().zip(records).zip(expected_bindings)
        {
            if record.phase() == PortLeasePhase::Reserved
                && record.reservation_claim().is_none()
                && record.bind_claim().is_none()
                && record.binding().is_none()
                && record.failure().is_none()
                && record.confirmed_stopped_binding() == Some(&expected_binding)
            {
                restart_retained += 1;
                continue;
            }
            if record.phase() == PortLeasePhase::Released
                && record.bind_claim().is_none()
                && record
                    .binding()
                    .is_none_or(|binding| binding == &expected_binding)
                && record.confirmed_stopped_binding().is_none()
                && record.failure().is_none()
            {
                require_uniform_terminal_coordinator(
                    &mut terminal_coordinator,
                    record.reservation_claim(),
                    request,
                    "MachinePortProxy",
                )?;
                terminal_no_effect += 1;
                continue;
            }

            let terminal_failed =
                terminal_failed_has_no_effect(&record, OciPortProvider::MachinePortProxy);
            let live_provider_owned = record.reservation_claim().is_none()
                && record.bind_claim().is_none()
                && record.confirmed_stopped_binding().is_none()
                && matches!(
                    record.phase(),
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                )
                && record
                    .binding()
                    .is_some_and(|binding| binding == &expected_binding);
            if terminal_failed {
                require_uniform_terminal_coordinator(
                    &mut terminal_coordinator,
                    record.reservation_claim(),
                    request,
                    "MachinePortProxy",
                )?;
                terminal_no_effect += 1;
                continue;
            }
            if live_provider_owned {
                provider_owned += 1;
                continue;
            }

            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port lease {} is neither restart-retained, terminal, nor an exact \
                     MachinePortProxy lifecycle record; retaining every fence for reconciliation",
                    request.lease_id()
                ),
            });
        }

        classify_uniform_batch(
            leases.len(),
            restart_retained,
            Vec::new(),
            provider_owned,
            terminal_no_effect,
            "MachinePortProxy",
        )
    }

    /// Release one exact restart-retained Netavark batch after provider absence.
    pub(crate) fn release_restart_retained_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let requires_release = require_restart_retained(
            self.classify_netavark_cleanup_batch(tenant_id, sandbox_id, bindings, leases, None)?,
            "Netavark",
        )?;
        if requires_release {
            release_batch_after_confirmed_stop(&self.state_root, leases)?;
        }
        Ok(())
    }

    /// Release one exact machine-proxy batch from durable confirmed-stop receipts.
    pub(crate) fn release_restart_retained_machine_bindings(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let requires_release = require_restart_retained(
            self.classify_machine_cleanup_batch(tenant_id, sandbox_id, bindings, leases)?,
            "MachinePortProxy",
        )?;
        if requires_release {
            release_batch_after_confirmed_stop(&self.state_root, leases)?;
        }
        Ok(())
    }
}

fn require_uniform_terminal_coordinator(
    expected: &mut Option<Option<NetworkReservationClaim>>,
    actual: Option<&NetworkReservationClaim>,
    request: &PortLeaseRequest,
    provider_name: &str,
) -> Result<()> {
    let actual = actual.cloned();
    match expected {
        None => {
            *expected = Some(actual);
            Ok(())
        }
        Some(expected) if expected.as_ref() == actual.as_ref() => Ok(()),
        Some(_) => Err(SandboxError::OperationFailed {
            message: format!(
                "{provider_name} terminal port lease {} belongs to a different reservation \
                 coordinator; retaining every fence for reconciliation",
                request.lease_id()
            ),
        }),
    }
}

fn classify_uniform_batch(
    expected_len: usize,
    restart_retained: usize,
    provider_claims: Vec<PortBindClaim>,
    provider_owned: usize,
    terminal_no_effect: usize,
    provider_name: &str,
) -> Result<LaunchPortBatchState> {
    match (
        restart_retained,
        provider_claims.len(),
        provider_owned,
        terminal_no_effect,
    ) {
        (retained, 0, 0, 0) if retained == expected_len => {
            Ok(LaunchPortBatchState::RestartRetained)
        }
        (0, 0, 0, terminal) if terminal == expected_len => {
            Ok(LaunchPortBatchState::TerminalNoEffect)
        }
        (0, claimed, 0, 0) if claimed == expected_len => {
            Ok(LaunchPortBatchState::NetavarkClaimed(provider_claims))
        }
        (0, 0, owned, 0) if owned == expected_len => Ok(LaunchPortBatchState::ProviderOwned),
        _ => Err(SandboxError::OperationFailed {
            message: format!(
                "{provider_name} cleanup batch mixes restart-retained, claimed, provider-owned, \
                 or terminal lifecycle states; retaining every fence for reconciliation"
            ),
        }),
    }
}

fn require_restart_retained(state: LaunchPortBatchState, provider_name: &str) -> Result<bool> {
    match state {
        LaunchPortBatchState::RestartRetained => Ok(true),
        LaunchPortBatchState::TerminalNoEffect => Ok(false),
        state => Err(SandboxError::OperationFailed {
            message: format!(
                "cannot release restart-retained {provider_name} ports from {state:?}"
            ),
        }),
    }
}
