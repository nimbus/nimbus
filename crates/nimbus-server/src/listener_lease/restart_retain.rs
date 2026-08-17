//! Restart-only stop and durable listener-retention ownership.
//!
//! A workload restart must make every old ingress effect unreachable before
//! it can retain the exact host-port leases for the next attempt. This module
//! owns that ordering. Terminal listener settlement remains in the parent
//! module and continues to withdraw and release Nimbus-owned leases.

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use nimbus_network::{PortLeaseId, PortLeaseRecord, PortLeaseRequest};

use super::ActiveServerListenerLease;

/// Stable failure class for one restart listener-retention batch.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartListenerRetainFailureKind {
    /// The caller supplied no listener effects to stop.
    InvalidBatch,
    /// One or more stop/join operations did not prove that their effects ended.
    StopOrJoinAmbiguous,
    /// Effects stopped, but exact durable batch retention did not commit.
    DurableRetentionAmbiguous,
}

/// Fail-closed result for a restart listener-retention batch.
#[derive(Debug)]
pub(crate) struct RestartListenerRetainError {
    #[cfg(test)]
    kind: RestartListenerRetainFailureKind,
    message: String,
}

impl RestartListenerRetainError {
    #[cfg(test)]
    pub(crate) const fn kind(&self) -> RestartListenerRetainFailureKind {
        self.kind
    }

    fn invalid_batch(message: impl Into<String>) -> Self {
        Self {
            #[cfg(test)]
            kind: RestartListenerRetainFailureKind::InvalidBatch,
            message: message.into(),
        }
    }

    fn stop_ambiguous(message: impl Into<String>) -> Self {
        Self {
            #[cfg(test)]
            kind: RestartListenerRetainFailureKind::StopOrJoinAmbiguous,
            message: message.into(),
        }
    }

    fn retention_ambiguous(message: impl Into<String>) -> Self {
        Self {
            #[cfg(test)]
            kind: RestartListenerRetainFailureKind::DurableRetentionAmbiguous,
            message: message.into(),
        }
    }
}

impl fmt::Display for RestartListenerRetainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RestartListenerRetainError {}

/// One live server listener plus its complete effect-owner stop operation.
///
/// The callback must signal shutdown and join the listener worker and every
/// connection worker that can still use the lease. A dropped value invokes
/// the callback as a fail-closed safeguard, but only the explicit batch
/// operation can report success or change durable lease authority.
pub(crate) struct RestartStoppingServerListener {
    lease: Option<ActiveServerListenerLease>,
    stop_and_join: Option<Box<dyn FnOnce() -> io::Result<()> + Send + 'static>>,
}

impl fmt::Debug for RestartStoppingServerListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestartStoppingServerListener")
            .field(
                "lease_id",
                &self.lease.as_ref().map(|lease| lease.request.lease_id()),
            )
            .field("stop_pending", &self.stop_and_join.is_some())
            .finish_non_exhaustive()
    }
}

impl RestartStoppingServerListener {
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
            Err(_) => Err("the server listener stop/join operation panicked".to_owned()),
        }
    }
}

impl Drop for RestartStoppingServerListener {
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
                "restart listener owner dropped without a confirmed stop/join"
            );
        }
    }
}

/// Durable proof that one complete stopped listener batch is retained.
#[derive(Debug)]
pub(crate) struct RestartRetainedServerListenerBatch {
    records: Vec<PortLeaseRecord>,
}

impl RestartRetainedServerListenerBatch {
    pub(crate) fn records(&self) -> &[PortLeaseRecord] {
        &self.records
    }
}

/// Stop every listener effect, join all of its workers, and atomically retain
/// the exact host-port lease batch for a higher-lifetime rebind.
///
/// Stop/join is attempted for every member even if a sibling fails. Durable
/// retention starts only after all effects report a confirmed stop. The
/// portable authority authenticates every request, binding, and non-cloneable
/// lifetime before one atomic state replacement. No failure path releases a
/// lease or reports success.
pub(crate) fn stop_and_retain_server_listeners_for_restart(
    plan_members: &[PortLeaseRequest],
    mut listeners: Vec<RestartStoppingServerListener>,
) -> Result<RestartRetainedServerListenerBatch, RestartListenerRetainError> {
    if listeners.is_empty() {
        return Err(RestartListenerRetainError::invalid_batch(
            "restart listener-retention batch cannot be empty",
        ));
    }

    let authority = listeners[0]
        .lease
        .as_ref()
        .expect("restart stopping listener must retain its lease")
        .network_authority
        .clone();
    let preflight_error = authenticate_batch(&authority, &listeners).err();

    let mut stop_failures = Vec::new();
    for listener in &mut listeners {
        if let Err(error) = listener.stop_and_join() {
            let lease_id = listener
                .lease
                .as_ref()
                .expect("restart stopping listener must retain its lease")
                .request
                .lease_id();
            stop_failures.push(format!("{lease_id}: {error}"));
        }
    }
    if !stop_failures.is_empty() {
        return Err(RestartListenerRetainError::stop_ambiguous(format!(
            "restart listener stop/join is ambiguous: {}",
            stop_failures.join("; ")
        )));
    }
    if let Some(error) = preflight_error {
        return Err(RestartListenerRetainError::retention_ambiguous(format!(
            "restart listeners stopped but their durable batch authority is crossed: {error}"
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
    let mut bindings = Vec::with_capacity(leases.len());
    let mut lifetimes = Vec::with_capacity(leases.len());
    for lease in leases {
        let ActiveServerListenerLease {
            request,
            lifetime,
            binding,
            ..
        } = lease;
        bindings.push((request, binding));
        lifetimes.push(lifetime);
    }
    let records = authority
        .port_leases()
        .prepare_rebind_plan_members_after_confirmed_stop_with_lifetimes(
            plan_members,
            &bindings,
            &lifetimes,
        )
        .map_err(|error| {
            RestartListenerRetainError::retention_ambiguous(format!(
                "restart listeners stopped but durable lease retention is ambiguous: {error}"
            ))
        })?;
    Ok(RestartRetainedServerListenerBatch { records })
}

fn authenticate_batch(
    authority: &crate::network_composition::RetainedServerNetworkAuthority,
    listeners: &[RestartStoppingServerListener],
) -> io::Result<()> {
    let mut lease_ids = BTreeSet::<PortLeaseId>::new();
    for listener in listeners {
        let lease = listener
            .lease
            .as_ref()
            .expect("restart stopping listener must retain its lease");
        authority.authenticate_same_authority(&lease.network_authority)?;
        let lease_id = lease.request.lease_id();
        if !lease_ids.insert(lease_id.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("restart listener batch repeats lease {lease_id}"),
            ));
        }
        authenticate_live_lease(lease)?;
    }
    Ok(())
}

fn authenticate_live_lease(lease: &ActiveServerListenerLease) -> io::Result<()> {
    if lease.observation_evidence().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "restart listener lease {} has crossed request, binding, or lifetime authority",
                lease.request.lease_id()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "restart_retain/tests.rs"]
mod tests;
