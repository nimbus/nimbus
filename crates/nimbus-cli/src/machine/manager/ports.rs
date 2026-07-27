//! Machine SSH listener adapter for the shared host-global port authority.
//!
//! gvproxy remains the effect owner. This module gives each managed SSH
//! listener an address-independent identity, durably claims its selected port
//! before gvproxy starts, and translates exact provider observations into the
//! portable `nimbus-network` lifecycle.

#[cfg(test)]
use std::io;
use std::net::Ipv4Addr;
use std::num::NonZeroU16;

use nimbus::Error;
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, PortBindClaim, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseAccounting,
    PortLeaseBinding, PortLeaseFence, PortLeaseId, PortLeasePhase, PortLeaseRequest, PortProtocol,
    PortPublicationIntent, PortRequestMode,
};
#[cfg(test)]
use nimbus_network::{PortBindAttempt, PortBindFailure, PortBindFailureKind};
use ulid::Ulid;

use super::{MACHINE_PORT_MAX, MACHINE_PORT_MIN, MachineRuntimeState};
use crate::machine::{MachineRootLayout, MachineStateRecord};

const MACHINE_SSH_PROVIDER_KEY: &str = "nimbus-cli.machine-gvproxy-ssh";
const MACHINE_SSH_LISTENER_NAME: &str = "ssh-forward";
const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);

/// One exact gvproxy bind claim prepared before the provider process starts.
///
/// Dropping this value retains the claim deliberately. Only a proven
/// synchronous no-effect failure may terminally fail it; an interrupted or
/// ambiguous provider effect stays fenced for NNC3.8 reconciliation.
#[derive(Debug)]
pub(super) struct PreparedMachineSshPortLease {
    authority: LocalPortLeaseAuthority,
    request: PortLeaseRequest,
    claim: PortBindClaim,
    #[cfg(test)]
    attempt: PortBindAttempt,
    listener_id: ListenerId,
    selected_port: NonZeroU16,
}

impl PreparedMachineSshPortLease {
    pub(super) fn prepare(
        roots: &MachineRootLayout,
        machine_name: &str,
        state: &MachineStateRecord,
    ) -> Result<Self, Error> {
        let authority =
            LocalPortLeaseAuthority::open(&roots.network_state_root).map_err(|error| {
                network_error("failed to open the machine SSH port authority", error)
            })?;
        let listener_id = reusable_listener_id(&authority, state)?
            .unwrap_or_else(|| fresh_listener_id(machine_name));
        let request = machine_ssh_request(&listener_id)?;
        let provider_attempt = NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(MACHINE_SSH_PROVIDER_KEY),
            format!("bind-attempt:{}", Ulid::new()),
        )
        .map_err(|error| network_error("failed to create the gvproxy SSH bind claim", error))?;
        let claim = PortBindClaim::new(provider_attempt);
        let reserved = authority.reserve(request.clone()).map_err(|error| {
            network_error(
                &format!("failed to reserve a managed SSH port for machine '{machine_name}'"),
                error,
            )
        })?;
        let selected_port = match reserved.reserved_port() {
            Some(port) => port,
            None => {
                let primary = Error::Internal(format!(
                    "machine SSH range lease {} reserved no concrete port",
                    request.lease_id()
                ));
                return Err(with_never_started_cleanup(
                    &authority, &request, &claim, primary,
                ));
            }
        };
        #[cfg(test)]
        let target = PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST);
        #[cfg(test)]
        let attempt = match PortBindAttempt::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            target,
            selected_port.get(),
        ) {
            Ok(attempt) => attempt,
            Err(error) => {
                let primary =
                    network_error("failed to describe the gvproxy SSH bind attempt", error);
                return Err(with_never_started_cleanup(
                    &authority, &request, &claim, primary,
                ));
            }
        };
        if let Err(error) = authority.claim_bind(&request, None, claim.clone()) {
            let primary = network_error(
                &format!(
                    "failed to claim managed SSH port {} for machine '{machine_name}'",
                    selected_port
                ),
                error,
            );
            return Err(with_never_started_cleanup(
                &authority, &request, &claim, primary,
            ));
        }

        Ok(Self {
            authority,
            request,
            claim,
            #[cfg(test)]
            attempt,
            listener_id,
            selected_port,
        })
    }

    pub(super) const fn selected_port(&self) -> u16 {
        self.selected_port.get()
    }

    pub(super) fn listener_id(&self) -> &ListenerId {
        &self.listener_id
    }

    /// Settle a claim when every caller-owned step before gvproxy spawn proves
    /// that no provider process or listener could have been created.
    pub(super) fn abandon_before_provider_start(&self) -> Result<(), Error> {
        settle_never_started_claim(&self.authority, &self.request, &self.claim)
    }

    /// Record a faithful provider bind failure after proving no gvproxy effect
    /// was created.
    #[cfg(test)]
    pub(super) fn record_bind_failure(self, error: io::Error) -> Result<io::Error, Error> {
        let failure = PortBindFailure::new(
            bind_failure_kind(error.kind()),
            self.attempt,
            self.claim.provider_attempt().clone(),
        );
        self.authority
            .record_claimed_bind_failure_without_effect(
                &self.request,
                None,
                &self.claim,
                failure,
            )
            .map_err(|record_error| {
                Error::Internal(format!(
                    "{error}; failed to record the durable no-effect gvproxy SSH bind failure for {}: {record_error}",
                    self.request.lease_id()
                ))
            })?;
        Ok(error)
    }

    /// Adopt and activate exact loopback evidence after the SSH readiness gate
    /// observes gvproxy serving the selected port.
    pub(super) fn activate_exact_loopback(&self) -> Result<(), Error> {
        let endpoint = PortBoundEndpoint::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            self.selected_port,
        )
        .map_err(|error| {
            network_error("failed to describe the observed gvproxy SSH binding", error)
        })?;
        let binding = PortLeaseBinding::new(
            endpoint,
            PortBindingProvenance::NimbusOwned,
            self.claim.provider_attempt().clone(),
        );
        self.authority
            .adopt_claimed_and_activate_batch(
                &[(self.request.clone(), self.claim.clone(), binding)],
                None,
            )
            .map_err(|error| {
                network_error(
                    &format!(
                        "failed to activate observed gvproxy SSH listener {}",
                        self.request.lease_id()
                    ),
                    error,
                )
            })?;
        Ok(())
    }
}

/// Fence an active machine SSH listener before any provider stop effect.
pub(super) fn withdraw_machine_ssh_port(
    roots: &MachineRootLayout,
    runtime: &MachineRuntimeState,
) -> Result<(), Error> {
    let authority = open_authority(roots)?;
    let request = machine_ssh_request(&runtime.ssh_listener_id)?;
    let record = exact_record(&authority, &request)?;
    match record.phase() {
        PortLeasePhase::Active | PortLeasePhase::Withdrawing => {
            authority.withdraw(&request).map_err(|error| {
                network_error(
                    &format!(
                        "failed to fence machine SSH listener {} before provider stop",
                        request.lease_id()
                    ),
                    error,
                )
            })?;
            Ok(())
        }
        PortLeasePhase::Reserved if record.confirmed_stopped_binding().is_some() => Ok(()),
        PortLeasePhase::Released | PortLeasePhase::Failed => Ok(()),
        phase => Err(unresolved_lifecycle_error(
            &request,
            phase,
            "withdraw before provider stop",
        )),
    }
}

/// Retain the exact selected port only after gvproxy absence is confirmed.
pub(super) fn retain_machine_ssh_port_after_confirmed_stop(
    roots: &MachineRootLayout,
    runtime: &MachineRuntimeState,
) -> Result<(), Error> {
    let authority = open_authority(roots)?;
    let request = machine_ssh_request(&runtime.ssh_listener_id)?;
    let record = exact_record(&authority, &request)?;
    if record.phase() == PortLeasePhase::Reserved && record.confirmed_stopped_binding().is_some() {
        return Ok(());
    }
    let binding = record.binding().cloned().ok_or_else(|| {
        unresolved_lifecycle_error(
            &request,
            record.phase(),
            "retain after confirmed provider stop without exact binding evidence",
        )
    })?;
    authority
        .prepare_rebind_after_confirmed_stop(&request, &binding)
        .map_err(|error| {
            network_error(
                &format!(
                    "failed to retain machine SSH listener {} after confirmed provider stop",
                    request.lease_id()
                ),
                error,
            )
        })?;
    Ok(())
}

/// Release a stopped machine's retained SSH port before deleting its records.
pub(super) fn release_machine_ssh_port(
    roots: &MachineRootLayout,
    state: &MachineStateRecord,
) -> Result<(), Error> {
    let Some(runtime) = state.runtime.as_ref() else {
        return Ok(());
    };
    let authority = open_authority(roots)?;
    let request = machine_ssh_request(&runtime.ssh_listener_id)?;
    let Some(record) = authority
        .inspect(request.lease_id())
        .map_err(|error| network_error("failed to inspect the machine SSH lease", error))?
    else {
        return Ok(());
    };
    if record.request() != &request {
        return Err(Error::conflict(format!(
            "machine SSH listener {} does not match its durable lease request",
            request.lease_id()
        )));
    }
    match record.phase() {
        PortLeasePhase::Reserved if record.confirmed_stopped_binding().is_some() => {
            authority
                .release_after_confirmed_stop(&request)
                .map_err(|error| {
                    network_error(
                        &format!(
                            "failed to release stopped machine SSH listener {}",
                            request.lease_id()
                        ),
                        error,
                    )
                })?;
            Ok(())
        }
        PortLeasePhase::Released | PortLeasePhase::Failed => Ok(()),
        phase => Err(unresolved_lifecycle_error(
            &request,
            phase,
            "release for machine removal",
        )),
    }
}

#[cfg(test)]
pub(super) fn managed_machine_port_range_contains(port: u16) -> bool {
    (MACHINE_PORT_MIN..=MACHINE_PORT_MAX).contains(&port)
}

fn reusable_listener_id(
    authority: &LocalPortLeaseAuthority,
    state: &MachineStateRecord,
) -> Result<Option<ListenerId>, Error> {
    let Some(runtime) = state.runtime.as_ref() else {
        return Ok(None);
    };
    let lease_id = PortLeaseId::for_listener(&runtime.ssh_listener_id);
    let record = authority
        .inspect(&lease_id)
        .map_err(|error| network_error("failed to inspect the prior machine SSH lease", error))?;
    Ok(match record {
        Some(record) if !record.phase().is_terminal() => Some(runtime.ssh_listener_id.clone()),
        Some(_) => None,
        None => Some(runtime.ssh_listener_id.clone()),
    })
}

fn fresh_listener_id(machine_name: &str) -> ListenerId {
    ListenerId::for_workload_listener(
        &format!("managed-machine:{machine_name}:{}", Ulid::new()),
        MACHINE_SSH_LISTENER_NAME,
    )
}

fn machine_ssh_request(listener_id: &ListenerId) -> Result<PortLeaseRequest, Error> {
    let start = NonZeroU16::new(MACHINE_PORT_MIN)
        .ok_or_else(|| Error::Internal("machine SSH port range starts at zero".to_owned()))?;
    let end = NonZeroU16::new(MACHINE_PORT_MAX)
        .ok_or_else(|| Error::Internal("machine SSH port range ends at zero".to_owned()))?;
    let mode = PortRequestMode::range(start, end)
        .map_err(|error| network_error("invalid managed SSH port range", error))?;
    Ok(PortLeaseRequest::new(
        PortLeaseId::for_listener(listener_id),
        listener_id.clone().into(),
        None,
        PortLeaseFence::new(INITIAL_RESOURCE_GENERATION, INITIAL_LEASE_EPOCH),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            mode,
        ),
    ))
}

fn open_authority(roots: &MachineRootLayout) -> Result<LocalPortLeaseAuthority, Error> {
    LocalPortLeaseAuthority::open(&roots.network_state_root)
        .map_err(|error| network_error("failed to open the machine SSH port authority", error))
}

fn exact_record(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<nimbus_network::PortLeaseRecord, Error> {
    let record = authority
        .inspect(request.lease_id())
        .map_err(|error| network_error("failed to inspect the machine SSH lease", error))?
        .ok_or_else(|| {
            Error::conflict(format!(
                "machine SSH listener {} has no durable lease",
                request.lease_id()
            ))
        })?;
    if record.request() != request {
        return Err(Error::conflict(format!(
            "machine SSH listener {} does not match its durable lease request",
            request.lease_id()
        )));
    }
    Ok(record)
}

fn unresolved_lifecycle_error(
    request: &PortLeaseRequest,
    phase: PortLeasePhase,
    operation: &str,
) -> Error {
    Error::conflict(format!(
        "machine SSH listener {} is {phase:?} and cannot {operation}; its port remains fenced for reconciliation",
        request.lease_id()
    ))
}

fn settle_never_started_claim(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
) -> Result<(), Error> {
    authority
        .abandon_bind_claims_without_effect(&[(request.clone(), claim.clone())], None)
        .map_err(|error| {
            network_error(
                "failed to abandon the never-started gvproxy SSH bind claim",
                error,
            )
        })?;
    authority.withdraw(request).map_err(|error| {
        network_error(
            "failed to withdraw the never-started gvproxy SSH reservation",
            error,
        )
    })?;
    authority.release(request).map_err(|error| {
        network_error(
            "failed to release the never-started gvproxy SSH reservation",
            error,
        )
    })?;
    Ok(())
}

fn with_never_started_cleanup(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    primary: Error,
) -> Error {
    match settle_never_started_claim(authority, request, claim) {
        Ok(()) => primary,
        Err(cleanup) => Error::Internal(format!(
            "{primary}; failed to settle the never-started gvproxy SSH reservation: {cleanup}"
        )),
    }
}

#[cfg(test)]
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

fn network_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::Internal(format!("{context}: {error}"))
}
