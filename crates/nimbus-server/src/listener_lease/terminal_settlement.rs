//! Terminal stop and durable release for server-owned TCP listeners.
//!
//! The portable authority owns lease state. This effect-owner adapter proves
//! the exact live bindings, fences the complete process-bound subset before
//! stop, joins every local worker, and only then releases that subset.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[cfg(test)]
use nimbus_network::LocalNetworkAuthority;
use nimbus_network::{
    PortBindingProvenance, PortBoundEndpoint, PortLeaseBinding, PortLeaseId, PortLeaseLifetime,
    PortLeaseLifetimeGuard, PortLeaseRecord, PortLeaseRequest,
};

use crate::network_composition::RetainedServerNetworkAuthority;

/// One active server listener's exact durable authority.
pub(crate) struct ActiveServerListenerLease {
    pub(super) network_authority: RetainedServerNetworkAuthority,
    pub(super) request: PortLeaseRequest,
    pub(super) provenance: PortBindingProvenance,
    pub(super) lifetime: PortLeaseLifetimeGuard,
    pub(super) binding: PortLeaseBinding,
}

/// Effect-free evidence retained by one live server-owned listener.
///
/// The opaque provider handle and network authority deliberately remain in
/// the listener owner. This snapshot carries only immutable evidence needed
/// to authenticate an observed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveServerListenerEvidence {
    request: PortLeaseRequest,
    lifetime: PortLeaseLifetime,
    bound_endpoint: PortBoundEndpoint,
    provenance: PortBindingProvenance,
}

impl ActiveServerListenerEvidence {
    pub(crate) fn request(&self) -> &PortLeaseRequest {
        &self.request
    }

    pub(crate) const fn lifetime(&self) -> PortLeaseLifetime {
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
    /// Return a typed snapshot without reading or mutating durable authority.
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

    /// Settle one independently owned listener after its descriptor closed.
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

/// One withdrawn server listener plus its complete local stop operation.
///
/// The callback must signal and join the listener worker and every transitive
/// connection worker. Dropping is only a best-effort safeguard; it never
/// changes durable lease state or supplies success evidence.
pub(crate) struct TerminalStoppingServerListener {
    lease: Option<ActiveServerListenerLease>,
    stop_and_join: Option<Box<dyn FnOnce() -> io::Result<()> + Send + 'static>>,
}

impl fmt::Debug for TerminalStoppingServerListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalStoppingServerListener")
            .field(
                "lease_id",
                &self.lease.as_ref().map(|lease| lease.request.lease_id()),
            )
            .field("stop_pending", &self.stop_and_join.is_some())
            .finish_non_exhaustive()
    }
}

impl TerminalStoppingServerListener {
    pub(crate) fn new(
        lease: ActiveServerListenerLease,
        stop_and_join: impl FnOnce() -> io::Result<()> + Send + 'static,
    ) -> Self {
        Self {
            lease: Some(lease),
            stop_and_join: Some(Box::new(stop_and_join)),
        }
    }

    fn stop_and_join(&mut self) -> Result<(), String> {
        let Some(stop_and_join) = self.stop_and_join.take() else {
            return Ok(());
        };
        match catch_unwind(AssertUnwindSafe(stop_and_join)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err("the server listener terminal stop/join operation panicked".to_owned()),
        }
    }
}

impl Drop for TerminalStoppingServerListener {
    fn drop(&mut self) {
        if let Err(error) = self.stop_and_join() {
            let lease_id = self
                .lease
                .as_ref()
                .map(|lease| lease.request.lease_id().to_string())
                .unwrap_or_else(|| "transferred-listener-lease".to_owned());
            tracing::error!(
                %lease_id,
                %error,
                "terminal listener owner dropped without a confirmed stop/join"
            );
        }
    }
}

/// Stable failure class for one final listener-settlement batch.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalListenerSettlementFailureKind {
    InvalidBatch,
    DurableWithdrawalAmbiguous,
    StopOrJoinAmbiguous,
    DurableReleaseAmbiguous,
}

/// Fail-closed result for terminal listener settlement.
#[derive(Debug)]
pub(crate) struct TerminalListenerSettlementError {
    #[cfg(test)]
    kind: TerminalListenerSettlementFailureKind,
    message: String,
}

impl TerminalListenerSettlementError {
    #[cfg(test)]
    pub(crate) const fn kind(&self) -> TerminalListenerSettlementFailureKind {
        self.kind
    }

    fn new(
        #[cfg(test)] kind: TerminalListenerSettlementFailureKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            #[cfg(test)]
            kind,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(
            #[cfg(test)]
            TerminalListenerSettlementFailureKind::InvalidBatch,
            message,
        )
    }

    fn withdrawal(message: impl Into<String>) -> Self {
        Self::new(
            #[cfg(test)]
            TerminalListenerSettlementFailureKind::DurableWithdrawalAmbiguous,
            message,
        )
    }

    fn stop(message: impl Into<String>) -> Self {
        Self::new(
            #[cfg(test)]
            TerminalListenerSettlementFailureKind::StopOrJoinAmbiguous,
            message,
        )
    }

    fn release(message: impl Into<String>) -> Self {
        Self::new(
            #[cfg(test)]
            TerminalListenerSettlementFailureKind::DurableReleaseAmbiguous,
            message,
        )
    }
}

impl fmt::Display for TerminalListenerSettlementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TerminalListenerSettlementError {}

/// Atomically fence an exact live process-bound subset before any local stop.
///
/// This operation only borrows the live owners. A failed durable transition
/// therefore leaves the caller's complete routable batch intact.
pub(crate) fn withdraw_server_listeners_for_final_withdrawal(
    plan_members: &[PortLeaseRequest],
    leases: &[&ActiveServerListenerLease],
) -> Result<(), TerminalListenerSettlementError> {
    let authority = authenticate_borrowed_batch(leases)?;
    let bindings = leases
        .iter()
        .map(|lease| (lease.request.clone(), lease.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = leases
        .iter()
        .map(|lease| &lease.lifetime)
        .collect::<Vec<_>>();
    authority
        .port_leases()
        .withdraw_process_bound_plan_members_with_lifetimes(plan_members, &bindings, &lifetimes)
        .map(|_| ())
        .map_err(|error| {
            TerminalListenerSettlementError::withdrawal(format!(
                "durable final listener withdrawal is ambiguous: {error}"
            ))
        })
}

/// Stop every withdrawn effect, join all workers, and atomically release it.
///
/// Every sibling stop is attempted. Durable release starts only after every
/// listener and transitive connection worker reports a confirmed stop.
pub(crate) fn settle_exact_listener_leases(
    plan_members: &[PortLeaseRequest],
    mut listeners: Vec<TerminalStoppingServerListener>,
) -> Result<Vec<PortLeaseRecord>, TerminalListenerSettlementError> {
    if listeners.is_empty() {
        return Err(TerminalListenerSettlementError::invalid(
            "terminal listener-settlement batch cannot be empty",
        ));
    }
    let authority = listeners[0]
        .lease
        .as_ref()
        .expect("terminal stopping listener must retain its lease")
        .network_authority
        .clone();
    let preflight_error = authenticate_owned_batch(&authority, &listeners).err();
    stop_server_listeners_for_final_withdrawal(&mut listeners)?;
    if let Some(error) = preflight_error {
        return Err(TerminalListenerSettlementError::release(format!(
            "terminal listeners stopped but their durable authority is crossed: {error}"
        )));
    }

    let leases = listeners
        .iter_mut()
        .map(|listener| {
            listener
                .lease
                .take()
                .expect("authenticated listener must retain its lease")
        })
        .collect::<Vec<_>>();
    let bindings = leases
        .iter()
        .map(|lease| (lease.request.clone(), lease.binding.clone()))
        .collect::<Vec<_>>();
    let lifetimes = leases
        .iter()
        .map(|lease| &lease.lifetime)
        .collect::<Vec<_>>();
    authority
        .port_leases()
        .release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
            plan_members,
            &bindings,
            &lifetimes,
        )
        .map_err(|error| {
            TerminalListenerSettlementError::release(format!(
                "listeners stopped but durable terminal release is ambiguous: {error}"
            ))
        })
}

/// Stop and join every supplied listener without changing durable lease state.
///
/// Final-withdrawal orchestration uses this when one sibling has already lost
/// its terminal effect owner. Every still-owned sibling is stopped, but no
/// selected lease is released because the complete batch cannot prove local
/// absence.
pub(crate) fn stop_server_listeners_for_final_withdrawal(
    listeners: &mut [TerminalStoppingServerListener],
) -> Result<(), TerminalListenerSettlementError> {
    let mut stop_failures = Vec::new();
    for listener in listeners {
        if let Err(error) = listener.stop_and_join() {
            let lease_id = listener
                .lease
                .as_ref()
                .expect("terminal stopping listener must retain its lease")
                .request
                .lease_id();
            stop_failures.push(format!("{lease_id}: {error}"));
        }
    }
    if stop_failures.is_empty() {
        Ok(())
    } else {
        Err(TerminalListenerSettlementError::stop(format!(
            "terminal listener stop/join is ambiguous: {}",
            stop_failures.join("; ")
        )))
    }
}

/// Reconcile an exact final subset only after process-owner death is proven.
#[cfg(test)]
pub(crate) fn recover_dead_process_bound_server_listeners_for_final_withdrawal(
    network_authority: &LocalNetworkAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
) -> io::Result<Vec<PortLeaseRecord>> {
    if requests.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "dead-owner final listener subset cannot be empty",
        ));
    }
    let authority = network_authority.port_leases();
    let recoveries = authority
        .recover_dead_plan_members(plan_members, requests)
        .map_err(network_error)?;
    authority
        .release_process_bound_plan_members_after_owner_death(plan_members, requests, &recoveries)
        .map_err(network_error)
}

fn authenticate_borrowed_batch(
    leases: &[&ActiveServerListenerLease],
) -> Result<RetainedServerNetworkAuthority, TerminalListenerSettlementError> {
    let Some(first) = leases.first() else {
        return Err(TerminalListenerSettlementError::invalid(
            "terminal listener-withdrawal batch cannot be empty",
        ));
    };
    let authority = first.network_authority.clone();
    let mut lease_ids = BTreeSet::<PortLeaseId>::new();
    for lease in leases {
        authority
            .authenticate_same_authority(&lease.network_authority)
            .map_err(|error| TerminalListenerSettlementError::invalid(error.to_string()))?;
        authenticate_live_lease(lease, &mut lease_ids)?;
    }
    Ok(authority)
}

fn authenticate_owned_batch(
    authority: &RetainedServerNetworkAuthority,
    listeners: &[TerminalStoppingServerListener],
) -> io::Result<()> {
    let mut lease_ids = BTreeSet::<PortLeaseId>::new();
    for listener in listeners {
        let lease = listener
            .lease
            .as_ref()
            .expect("terminal stopping listener must retain its lease");
        authority.authenticate_same_authority(&lease.network_authority)?;
        authenticate_live_lease(lease, &mut lease_ids)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    }
    Ok(())
}

fn authenticate_live_lease(
    lease: &ActiveServerListenerLease,
    lease_ids: &mut BTreeSet<PortLeaseId>,
) -> Result<(), TerminalListenerSettlementError> {
    let lease_id = lease.request.lease_id();
    if !lease_ids.insert(lease_id.clone()) {
        return Err(TerminalListenerSettlementError::invalid(format!(
            "terminal listener batch repeats lease {lease_id}"
        )));
    }
    if lease.provenance == PortBindingProvenance::ExternallyOwned {
        return Err(TerminalListenerSettlementError::invalid(format!(
            "terminal process-bound settlement cannot release externally owned lease {lease_id}"
        )));
    }
    if lease.observation_evidence().is_none() {
        return Err(TerminalListenerSettlementError::invalid(format!(
            "terminal listener lease {lease_id} has crossed local evidence"
        )));
    }
    Ok(())
}

fn network_error(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
#[path = "terminal_settlement/tests.rs"]
mod tests;
