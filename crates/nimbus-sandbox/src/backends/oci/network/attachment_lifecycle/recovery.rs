//! Inspect-before-retry decisions for the durable OCI attachment lifecycle.

use std::net::Ipv4Addr;

use nimbus_network::{
    DurableNetworkAttachmentState, NetworkResourcePhase, NetworkTransitionEvidence,
};

use super::super::dto::NetavarkProviderOperation;
use super::super::ipam::{
    ContainerIpamAuthorityState, inspect_container_ipam_authority,
    inspect_netavark_provider_operation, load_container_ips_for_segment_if_present,
};
use super::state::OciAttachmentDurableState;
use super::{
    AttachmentBackendKind, AttachmentDetachFailure, AttachmentDetachFailureStage,
    AttachmentTeardownMode, OciAttachmentContext, OciIpamAuthority,
};
use crate::backends::oci::port_lifecycle::LaunchPortBatchState;
use crate::error::{Result, SandboxError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttachmentProviderObservation {
    Absent,
    /// The exact IPAM setup attempt is durable, but no namespace or status
    /// effect has begun. A fresh owner may resume only this same attempt.
    PreparedSetup,
    Present {
        assigned_ips: Vec<Ipv4Addr>,
    },
    ExactCleanupRequired,
    Unknown {
        reason: String,
    },
}

pub(super) enum AttachmentAttachRecovery {
    Create {
        record: DurableNetworkAttachmentState,
    },
    ResumePublication {
        record: DurableNetworkAttachmentState,
        assigned_ips: Vec<Ipv4Addr>,
    },
    AlreadyActive {
        assigned_ips: Vec<Ipv4Addr>,
    },
}

pub(super) struct AttachmentDetachRecovery {
    pub(super) record: DurableNetworkAttachmentState,
    pub(super) provider_absent: bool,
    pub(super) already_terminal: bool,
}

pub(super) fn validate_restart_publication_state(
    context: &OciAttachmentContext<'_>,
    state: &LaunchPortBatchState,
) -> Result<()> {
    if context.backend == AttachmentBackendKind::Krun
        || matches!(
            state,
            LaunchPortBatchState::ProviderOwned
                | LaunchPortBatchState::RestartRetained
                | LaunchPortBatchState::TerminalNoEffect
        )
        || matches!(state, LaunchPortBatchState::NeverBound) && context.leases.is_empty()
    {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "{} restart cannot detach Netavark from published-listener authority {state:?}",
            context.provider_label
        ),
    })
}

pub(super) fn before_provider_detach_failure(error: SandboxError) -> AttachmentDetachFailure {
    AttachmentDetachFailure {
        stage: AttachmentDetachFailureStage::BeforeProviderDetach,
        error,
    }
}

pub(super) fn cleanup_pending_failure(error: SandboxError) -> AttachmentDetachFailure {
    AttachmentDetachFailure {
        stage: AttachmentDetachFailureStage::CleanupPending,
        error,
    }
}

pub(super) fn detach_error(
    context: &OciAttachmentContext<'_>,
    errors: Vec<String>,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "failed to release {} attachment {}: {}",
            context.provider_label,
            context.sandbox_id,
            errors.join("; ")
        ),
    }
}

pub(super) fn inspect_provider(
    ipam: &OciIpamAuthority,
    context: &OciAttachmentContext<'_>,
) -> AttachmentProviderObservation {
    let namespace_present = match std::fs::symlink_metadata(&context.layout.netns_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return AttachmentProviderObservation::Unknown {
                reason: format!(
                    "cannot inspect namespace {}: {error}",
                    context.layout.netns_path.display()
                ),
            };
        }
    };
    let status_present = match std::fs::symlink_metadata(&context.layout.status_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return AttachmentProviderObservation::Unknown {
                reason: format!(
                    "cannot inspect provider status {}: {error}",
                    context.layout.status_path.display()
                ),
            };
        }
    };
    let ipam_state = match inspect_container_ipam_authority(
        ipam,
        context.layout,
        context.config,
        context.sandbox_id,
    ) {
        Ok(state) => state,
        Err(error) => {
            return AttachmentProviderObservation::Unknown {
                reason: format!("cannot authenticate exact IPAM generation: {error}"),
            };
        }
    };
    match ipam_state {
        ContainerIpamAuthorityState::Absent | ContainerIpamAuthorityState::Released => {
            if namespace_present {
                AttachmentProviderObservation::Unknown {
                    reason: "namespace exists without live exact IPAM authority".to_owned(),
                }
            } else {
                AttachmentProviderObservation::Absent
            }
        }
        ContainerIpamAuthorityState::Live => {
            let operation = match inspect_netavark_provider_operation(
                ipam,
                context.layout,
                context.config,
                context.sandbox_id,
            ) {
                Ok(operation) => operation,
                Err(error) => {
                    return AttachmentProviderObservation::Unknown {
                        reason: format!("cannot inspect exact Netavark operation: {error}"),
                    };
                }
            };
            match operation {
                NetavarkProviderOperation::Ready { .. } if namespace_present => {
                    match load_container_ips_for_segment_if_present(
                        ipam,
                        context.layout,
                        context.config,
                        context.sandbox_id,
                    ) {
                        Ok(Some(assigned_ips)) => {
                            AttachmentProviderObservation::Present { assigned_ips }
                        }
                        Ok(None) => AttachmentProviderObservation::Unknown {
                            reason: "ready Netavark operation lost its exact IPAM allocation"
                                .to_owned(),
                        },
                        Err(error) => AttachmentProviderObservation::Unknown {
                            reason: format!(
                                "ready Netavark operation has invalid exact IPAM evidence: {error}"
                            ),
                        },
                    }
                }
                NetavarkProviderOperation::SetupPrepared { .. }
                    if !namespace_present && !status_present =>
                {
                    AttachmentProviderObservation::PreparedSetup
                }
                NetavarkProviderOperation::SetupPrepared { .. }
                | NetavarkProviderOperation::Provisioning { .. }
                | NetavarkProviderOperation::TeardownPrepared { .. }
                | NetavarkProviderOperation::NoEffectTeardownPrepared { .. }
                | NetavarkProviderOperation::Deleting { .. }
                | NetavarkProviderOperation::DetachedProjectionPending { .. } => {
                    AttachmentProviderObservation::ExactCleanupRequired
                }
                NetavarkProviderOperation::Reserved | NetavarkProviderOperation::Detached
                    if !namespace_present =>
                {
                    AttachmentProviderObservation::Absent
                }
                operation => AttachmentProviderObservation::Unknown {
                    reason: format!(
                        "namespace presence {namespace_present} and Netavark phase {} do not prove \
                         exact presence or absence",
                        operation.label()
                    ),
                },
            }
        }
    }
}

pub(super) fn prepare_attach(
    durable: &OciAttachmentDurableState<'_>,
    record: DurableNetworkAttachmentState,
    observation: AttachmentProviderObservation,
) -> Result<AttachmentAttachRecovery> {
    let phase = record.resource().phase();
    match observation {
        AttachmentProviderObservation::Absent => match phase {
            NetworkResourcePhase::Reserved => durable
                .transition(
                    &record,
                    NetworkResourcePhase::Provisioning,
                    NetworkTransitionEvidence::Progress,
                )
                .map(|record| AttachmentAttachRecovery::Create { record }),
            NetworkResourcePhase::Provisioning => Ok(AttachmentAttachRecovery::Create { record }),
            NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending => durable
                .transition(
                    &record,
                    NetworkResourcePhase::Provisioning,
                    NetworkTransitionEvidence::DeletionConfirmedForReprovision,
                )
                .map(|record| AttachmentAttachRecovery::Create { record }),
            _ => Err(recovery_error(
                phase,
                "confirmed provider absence does not authorize attach retry from this phase",
            )),
        },
        AttachmentProviderObservation::PreparedSetup => match phase {
            NetworkResourcePhase::Provisioning => Ok(AttachmentAttachRecovery::Create { record }),
            _ => Err(recovery_error(
                phase,
                "a prepared setup attempt can resume only its provisioning attachment",
            )),
        },
        AttachmentProviderObservation::Present { assigned_ips } => match phase {
            NetworkResourcePhase::Provisioning => {
                let record = durable.record_stable_handle(&record)?;
                let record = durable.transition(
                    &record,
                    NetworkResourcePhase::Ready,
                    NetworkTransitionEvidence::Progress,
                )?;
                Ok(AttachmentAttachRecovery::ResumePublication {
                    record,
                    assigned_ips,
                })
            }
            NetworkResourcePhase::Ready | NetworkResourcePhase::Publishing => {
                let record = durable.record_stable_handle(&record)?;
                Ok(AttachmentAttachRecovery::ResumePublication {
                    record,
                    assigned_ips,
                })
            }
            NetworkResourcePhase::Active => {
                durable.record_stable_handle(&record)?;
                Ok(AttachmentAttachRecovery::AlreadyActive { assigned_ips })
            }
            _ => Err(recovery_error(
                phase,
                "present provider effect cannot be adopted by this attachment phase",
            )),
        },
        AttachmentProviderObservation::ExactCleanupRequired => {
            let fenced = mark_cleanup_pending(durable, &record);
            match fenced {
                Ok(_) => Err(recovery_error(
                    phase,
                    "an exact provider attempt requires cleanup and cannot be recreated",
                )),
                Err(fence_error) => Err(recovery_error(
                    phase,
                    &format!(
                        "an exact provider attempt requires cleanup; cleanup fence also failed: \
                         {fence_error}"
                    ),
                )),
            }
        }
        AttachmentProviderObservation::Unknown { reason } => {
            let fenced = mark_cleanup_pending(durable, &record);
            match fenced {
                Ok(_) => Err(recovery_error(
                    phase,
                    &format!("provider inspection is ambiguous and remains fenced: {reason}"),
                )),
                Err(fence_error) => Err(recovery_error(
                    phase,
                    &format!(
                        "provider inspection is ambiguous ({reason}); cleanup fence also failed: \
                         {fence_error}"
                    ),
                )),
            }
        }
    }
}

pub(super) fn mark_provider_ready(
    durable: &OciAttachmentDurableState<'_>,
    record: &DurableNetworkAttachmentState,
) -> Result<DurableNetworkAttachmentState> {
    let record = durable.record_stable_handle(record)?;
    durable.transition(
        &record,
        NetworkResourcePhase::Ready,
        NetworkTransitionEvidence::Progress,
    )
}

pub(super) fn mark_publishing(
    durable: &OciAttachmentDurableState<'_>,
    record: &DurableNetworkAttachmentState,
) -> Result<DurableNetworkAttachmentState> {
    match record.resource().phase() {
        NetworkResourcePhase::Ready => durable.transition(
            record,
            NetworkResourcePhase::Publishing,
            NetworkTransitionEvidence::Progress,
        ),
        NetworkResourcePhase::Publishing => Ok(record.clone()),
        phase => Err(recovery_error(
            phase,
            "attachment cannot publish before provider readiness",
        )),
    }
}

pub(super) fn mark_active(
    durable: &OciAttachmentDurableState<'_>,
    record: &DurableNetworkAttachmentState,
) -> Result<DurableNetworkAttachmentState> {
    durable.transition(
        record,
        NetworkResourcePhase::Active,
        NetworkTransitionEvidence::Progress,
    )
}

pub(super) fn mark_cleanup_pending(
    durable: &OciAttachmentDurableState<'_>,
    record: &DurableNetworkAttachmentState,
) -> Result<DurableNetworkAttachmentState> {
    match record.resource().phase() {
        NetworkResourcePhase::CleanupPending => Ok(record.clone()),
        NetworkResourcePhase::Provisioning
        | NetworkResourcePhase::Ready
        | NetworkResourcePhase::Publishing
        | NetworkResourcePhase::Active
        | NetworkResourcePhase::Withdrawing
        | NetworkResourcePhase::Draining
        | NetworkResourcePhase::Deleting => durable.transition(
            record,
            NetworkResourcePhase::CleanupPending,
            NetworkTransitionEvidence::AmbiguousEffect,
        ),
        phase => Err(recovery_error(
            phase,
            "attachment phase cannot accept an ambiguous-effect cleanup fence",
        )),
    }
}

pub(super) fn prepare_detach(
    durable: &OciAttachmentDurableState<'_>,
    mut record: DurableNetworkAttachmentState,
    observation: AttachmentProviderObservation,
) -> Result<AttachmentDetachRecovery> {
    let phase = record.resource().phase();
    if phase.is_terminal() {
        return match observation {
            AttachmentProviderObservation::Absent => Ok(AttachmentDetachRecovery {
                record,
                provider_absent: true,
                already_terminal: true,
            }),
            AttachmentProviderObservation::Present { .. }
            | AttachmentProviderObservation::PreparedSetup
            | AttachmentProviderObservation::ExactCleanupRequired
            | AttachmentProviderObservation::Unknown { .. } => Err(recovery_error(
                phase,
                "terminal attachment authority contradicts provider evidence",
            )),
        };
    }
    let provider_absent = match observation {
        AttachmentProviderObservation::Absent => true,
        AttachmentProviderObservation::Present { .. }
        | AttachmentProviderObservation::PreparedSetup
        | AttachmentProviderObservation::ExactCleanupRequired => {
            if phase == NetworkResourcePhase::Reserved {
                record = durable.transition(
                    &record,
                    NetworkResourcePhase::Provisioning,
                    NetworkTransitionEvidence::Progress,
                )?;
            }
            record = durable.record_stable_handle(&record)?;
            false
        }
        AttachmentProviderObservation::Unknown { reason } => {
            let _ = mark_cleanup_pending(durable, &record)?;
            return Err(recovery_error(
                phase,
                &format!("provider inspection is ambiguous and remains fenced: {reason}"),
            ));
        }
    };

    record = match record.resource().phase() {
        NetworkResourcePhase::Reserved => record,
        NetworkResourcePhase::Provisioning
        | NetworkResourcePhase::Ready
        | NetworkResourcePhase::Publishing
        | NetworkResourcePhase::Active => durable.transition(
            &record,
            NetworkResourcePhase::Withdrawing,
            NetworkTransitionEvidence::Progress,
        )?,
        _ => record,
    };
    record = match record.resource().phase() {
        NetworkResourcePhase::Withdrawing
        | NetworkResourcePhase::Draining
        | NetworkResourcePhase::CleanupPending => durable.transition(
            &record,
            NetworkResourcePhase::Deleting,
            NetworkTransitionEvidence::Progress,
        )?,
        NetworkResourcePhase::Deleting | NetworkResourcePhase::Reserved => record,
        phase => {
            return Err(recovery_error(
                phase,
                "attachment cannot enter provider deletion from this phase",
            ));
        }
    };
    Ok(AttachmentDetachRecovery {
        record,
        provider_absent,
        already_terminal: false,
    })
}

pub(super) fn finish_detach(
    durable: &OciAttachmentDurableState<'_>,
    record: &DurableNetworkAttachmentState,
    mode: AttachmentTeardownMode,
) -> Result<DurableNetworkAttachmentState> {
    match (record.resource().phase(), mode) {
        (NetworkResourcePhase::Reserved, AttachmentTeardownMode::Final) => durable.transition(
            record,
            NetworkResourcePhase::Released,
            NetworkTransitionEvidence::ConfirmedNoEffect,
        ),
        (NetworkResourcePhase::Reserved, AttachmentTeardownMode::Restart) => durable.transition(
            record,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        ),
        (
            NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending,
            AttachmentTeardownMode::Restart,
        ) => durable.transition(
            record,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::DeletionConfirmedForReprovision,
        ),
        (
            NetworkResourcePhase::Deleting | NetworkResourcePhase::CleanupPending,
            AttachmentTeardownMode::Final,
        ) => durable.transition(
            record,
            NetworkResourcePhase::Released,
            NetworkTransitionEvidence::DeletionConfirmed,
        ),
        (NetworkResourcePhase::Released | NetworkResourcePhase::Failed, _) => Ok(record.clone()),
        (phase, _) => Err(recovery_error(
            phase,
            "confirmed provider absence cannot finish detach from this phase",
        )),
    }
}

fn recovery_error(phase: NetworkResourcePhase, detail: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("durable attachment recovery from {phase:?} failed closed: {detail}"),
    }
}
