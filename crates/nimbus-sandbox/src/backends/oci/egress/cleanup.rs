//! Retryable stop/restart orchestration for one workload egress PEP.

use nimbus_core::TenantId;
use nimbus_network::{PortLeaseBinding, PortLeasePhase, PortLeaseRecord, PortLeaseRequest};

use super::{
    EgressProxyAssignment, EgressProxyRegistry, ExpectedListenerAuthority, RegisteredArtifacts,
    SandboxError, SandboxId, egress_proxy_error, remove_trust_anchor_file,
};
use crate::backends::oci::port_lease::{
    inspect_exact, prepare_rebind_after_confirmed_stop,
    prepare_rebind_after_confirmed_stop_with_lifetime, release, release_after_confirmed_stop,
    release_with_lifetime, require_listener_authority, withdraw,
};
use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PepCleanupDisposition {
    Restart,
    Release,
}

#[cfg(test)]
type PostDurableTransitionFault = Box<dyn FnOnce() -> Result<()>>;

#[cfg(test)]
thread_local! {
    static PRE_WITHDRAW_OBSERVER: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
    static POST_DURABLE_TRANSITION_FAULT:
        std::cell::RefCell<Option<PostDurableTransitionFault>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_pre_withdraw_observer(observer: impl Fn() + 'static) {
    PRE_WITHDRAW_OBSERVER.with(|current| {
        *current.borrow_mut() = Some(Box::new(observer));
    });
}

#[cfg(test)]
fn observe_pre_withdraw() {
    PRE_WITHDRAW_OBSERVER.with(|current| {
        if let Some(observer) = current.borrow().as_ref() {
            observer();
        }
    });
}

#[cfg(test)]
pub(super) fn set_post_durable_transition_fault(fault: impl FnOnce() -> Result<()> + 'static) {
    POST_DURABLE_TRANSITION_FAULT.with(|current| {
        *current.borrow_mut() = Some(Box::new(fault));
    });
}

#[cfg(test)]
fn observe_post_durable_transition() -> Result<()> {
    POST_DURABLE_TRANSITION_FAULT
        .with(|current| current.borrow_mut().take().map_or(Ok(()), |fault| fault()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PepCleanupProgress {
    disposition: PepCleanupDisposition,
    expected_binding: Option<PortLeaseBinding>,
    durable_transition_complete: bool,
}

#[derive(Clone)]
struct RegisteredSnapshot {
    port_lease: Option<PortLeaseRequest>,
    cleanup: Option<PepCleanupProgress>,
}

impl From<&RegisteredArtifacts> for RegisteredSnapshot {
    fn from(artifacts: &RegisteredArtifacts) -> Self {
        // The attachment intentionally retains this RAII pin until the
        // stopping tombstone completes and drops the complete entry.
        let _tenant_lease_pin = &artifacts.tenant_lease;
        Self {
            port_lease: artifacts.port_lease.clone(),
            cleanup: artifacts.cleanup.clone(),
        }
    }
}

impl EgressProxyRegistry {
    /// Stop and retire the PEP described by a persisted sandbox assignment.
    ///
    /// The engine retains a non-ready Stopping tombstone through provider
    /// shutdown, trust-anchor removal, and durable release. Any failed step is
    /// retried against the same exact process-local evidence.
    pub(crate) fn stop_with_assignment(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
    ) -> Result<()> {
        let persisted_record = assignment
            .map(|assignment| {
                require_listener_authority(
                    &self.network_state_root,
                    ExpectedListenerAuthority::egress_pep(tenant_id, id, assignment.bind_addr()?)?,
                    &assignment.port_lease,
                )
            })
            .transpose()?;
        self.stop_with_lease_disposition(
            tenant_id,
            id,
            assignment.map(|assignment| &assignment.port_lease),
            persisted_record,
            PepCleanupDisposition::Release,
        )
    }

    /// Stop an exact PEP effect while retaining its selected port for rebind.
    ///
    /// Provider acknowledgement and trust-anchor removal precede the explicit
    /// Active → Reserved transition. The same request then re-enters the normal
    /// bind-claim lifecycle on restart.
    pub(crate) fn stop_for_restart(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
    ) -> Result<()> {
        let persisted_record = assignment
            .map(|assignment| {
                require_listener_authority(
                    &self.network_state_root,
                    ExpectedListenerAuthority::egress_pep(tenant_id, id, assignment.bind_addr()?)?,
                    &assignment.port_lease,
                )
            })
            .transpose()?;
        self.stop_with_lease_disposition(
            tenant_id,
            id,
            assignment.map(|assignment| &assignment.port_lease),
            persisted_record,
            PepCleanupDisposition::Restart,
        )
    }

    fn stop_with_lease_disposition(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        persisted_port_lease: Option<&PortLeaseRequest>,
        persisted_record: Option<PortLeaseRecord>,
        disposition: PepCleanupDisposition,
    ) -> Result<()> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        let registered = match persisted_port_lease {
            Some(expected) => self.engine.with_lifecycle_attachment_if(
                &workload_id,
                |artifacts| artifacts.port_lease.as_ref() == Some(expected),
                |artifacts| RegisteredSnapshot::from(artifacts),
            ),
            None => self
                .engine
                .with_lifecycle_attachment(&workload_id, |artifacts| {
                    RegisteredSnapshot::from(artifacts)
                }),
        }
        .map_err(egress_proxy_error)?;
        if persisted_port_lease.is_none()
            && let Some(registered_lease) = registered
                .as_ref()
                .and_then(|artifacts| artifacts.port_lease.as_ref())
        {
            // A process-local attachment proves that an effect exists; it does
            // not authorize destroying a durable lease that may belong to a
            // newer persisted workload generation. Only the exact assignment
            // carried by durable sandbox state may grant that authority.
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot stop egress proxy for sandbox {id}: process-local registry evidence \
                     for durable listener {} is not cleanup authority; an exact persisted egress \
                     proxy assignment is required",
                    registered_lease.lease_id()
                ),
            });
        }
        let persisted_phase = persisted_record.as_ref().map(PortLeaseRecord::phase);
        if disposition == PepCleanupDisposition::Restart
            && let Some(phase @ (PortLeasePhase::Failed | PortLeasePhase::Released)) =
                persisted_phase
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot stop egress proxy for sandbox {id} for restart: terminal durable \
                     listener phase {phase:?} is not rebindable"
                ),
            });
        }
        match (persisted_port_lease, registered.as_ref()) {
            (Some(request), None)
                if disposition == PepCleanupDisposition::Release
                    && persisted_record
                        .as_ref()
                        .is_some_and(restart_transition_is_durably_complete) =>
            {
                release_after_confirmed_stop(&self.network_state_root, request)?;
                return Ok(());
            }
            (Some(_), None)
                if disposition == PepCleanupDisposition::Restart
                    && persisted_record
                        .as_ref()
                        .is_some_and(restart_transition_is_durably_complete) =>
            {
                // The restart transition is the durable acknowledgement that
                // provider shutdown and trust-anchor withdrawal completed
                // before the process-local tombstone was removed. Accept only
                // the exact clean rebind state; every claimed or bound
                // Reserved record still requires live provider evidence.
                return Ok(());
            }
            (Some(_), None)
                if disposition == PepCleanupDisposition::Release
                    && matches!(
                        persisted_phase,
                        Some(PortLeasePhase::Failed | PortLeasePhase::Released)
                    ) =>
            {
                return Ok(());
            }
            (Some(_), None) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "cannot stop egress proxy for sandbox {id}: this registry has no \
                         process-local provider evidence for the persisted listener; the durable \
                         lease and trust anchor remain fenced for reconciliation"
                    ),
                });
            }
            (Some(persisted), Some(registered))
                if registered.port_lease.as_ref() != Some(persisted) =>
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "persisted egress proxy assignment for sandbox {id} does not match \
                         registered durable port lease {}",
                        registered.port_lease.as_ref().map_or_else(
                            || "<none>".to_owned(),
                            |request| request.lease_id().to_string()
                        )
                    ),
                });
            }
            (Some(_), Some(registered))
                if matches!(
                    persisted_phase,
                    Some(PortLeasePhase::Failed | PortLeasePhase::Released)
                ) && registered
                    .cleanup
                    .as_ref()
                    .is_none_or(|cleanup| cleanup.disposition != disposition) =>
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "cannot stop egress proxy for sandbox {id}: process-local provider \
                         evidence contradicts terminal durable listener phase \
                         {persisted_phase:?}; reconciliation must resolve the disagreement"
                    ),
                });
            }
            _ => {}
        }
        let Some(registered) = registered else {
            return Ok(());
        };
        let port_lease = persisted_port_lease
            .cloned()
            .or_else(|| registered.port_lease.clone());
        let durable_record = port_lease
            .as_ref()
            .map(|request| {
                persisted_record
                    .filter(|record| record.request() == request)
                    .map_or_else(|| inspect_exact(&self.network_state_root, request), Ok)
            })
            .transpose()?;
        let durable_record_proves_transition_complete = durable_record
            .as_ref()
            .is_some_and(|record| cleanup_transition_is_durably_complete(disposition, record));
        let expected_binding = registered
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.expected_binding.clone())
            .or_else(|| {
                durable_record
                    .as_ref()
                    .and_then(|record| record.binding().cloned())
            });
        if port_lease.is_some() && expected_binding.is_none() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot stop egress proxy for sandbox {id}: durable listener has no exact \
                     provider binding evidence"
                ),
            });
        }

        let stop = self
            .engine
            .begin_stop_if_attachment(&workload_id, |artifacts| {
                artifacts.port_lease.as_ref() == port_lease.as_ref()
            })
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "cannot confirm egress proxy stop for sandbox {id}: the exact process-local \
                     provider attachment changed; durable authority remains fenced for \
                     reconciliation"
                ),
            })?;
        stop.with_attachment_mut(|artifacts| match artifacts.cleanup.as_mut() {
            Some(cleanup)
                if cleanup.disposition != disposition
                    || cleanup.expected_binding != expected_binding =>
            {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "egress proxy cleanup for sandbox {id} is already executing a \
                             different exact disposition"
                    ),
                })
            }
            Some(cleanup) => {
                cleanup.durable_transition_complete |= durable_record_proves_transition_complete;
                Ok(())
            }
            None => {
                artifacts.cleanup = Some(PepCleanupProgress {
                    disposition,
                    expected_binding: expected_binding.clone(),
                    durable_transition_complete: port_lease.is_none()
                        || durable_record_proves_transition_complete,
                });
                Ok(())
            }
        })
        .map_err(egress_proxy_error)??;

        let durable_transition_complete = stop
            .with_attachment(|artifacts| {
                artifacts
                    .cleanup
                    .as_ref()
                    .is_some_and(|cleanup| cleanup.durable_transition_complete)
            })
            .map_err(egress_proxy_error)?;

        // Final teardown fences new use before the provider is stopped. Once
        // release has completed, a retry must not attempt Released ->
        // Withdrawing merely because tombstone removal was interrupted.
        if !durable_transition_complete
            && disposition == PepCleanupDisposition::Release
            && let Some(request) = port_lease.as_ref()
        {
            #[cfg(test)]
            observe_pre_withdraw();
            withdraw(&self.network_state_root, request)?;
        }

        // Explicit acknowledgement, not Drop or socket bindability, grants
        // provider-absence evidence. Timeout retains this exact handle.
        stop.shutdown_provider().map_err(egress_proxy_error)?;

        let anchor_path = stop
            .with_attachment(|artifacts| artifacts.trust_anchor_path.clone())
            .map_err(egress_proxy_error)?;
        if let Some(path) = anchor_path.as_deref() {
            remove_trust_anchor_file(path)?;
            stop.with_attachment_mut(|artifacts| {
                if artifacts.trust_anchor_path.as_deref() == Some(path) {
                    artifacts.trust_anchor_path = None;
                }
            })
            .map_err(egress_proxy_error)?;
        }

        let durable_transition_complete = stop
            .with_attachment(|artifacts| {
                artifacts
                    .cleanup
                    .as_ref()
                    .is_some_and(|cleanup| cleanup.durable_transition_complete)
            })
            .map_err(egress_proxy_error)?;
        if !durable_transition_complete {
            let request = port_lease
                .as_ref()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "egress proxy cleanup for sandbox {id} lost its durable lease request"
                    ),
                })?;
            match disposition {
                PepCleanupDisposition::Release => {
                    stop.with_attachment(|artifacts| match artifacts.lifetime.as_ref() {
                        Some(lifetime) => {
                            release_with_lifetime(&self.network_state_root, request, lifetime)
                        }
                        // Lifetime-free registrations are test-only fixtures
                        // whose durable record carries no live owner.
                        None => release(&self.network_state_root, request),
                    })
                    .map_err(egress_proxy_error)??;
                }
                PepCleanupDisposition::Restart => {
                    let expected_binding = expected_binding.as_ref().ok_or_else(|| {
                        SandboxError::OperationFailed {
                            message: format!(
                                "egress proxy restart for sandbox {id} lost exact binding evidence"
                            ),
                        }
                    })?;
                    stop.with_attachment(|artifacts| match artifacts.lifetime.as_ref() {
                        Some(lifetime) => prepare_rebind_after_confirmed_stop_with_lifetime(
                            &self.network_state_root,
                            request,
                            expected_binding,
                            lifetime,
                        ),
                        // Durable production registrations always retain a
                        // lifetime. The lifetime-free path exists solely for
                        // test fixtures that intentionally omit durable
                        // listener ownership.
                        None => prepare_rebind_after_confirmed_stop(
                            &self.network_state_root,
                            request,
                            expected_binding,
                        ),
                    })
                    .map_err(egress_proxy_error)??;
                }
            }
            #[cfg(test)]
            observe_post_durable_transition()?;
            stop.with_attachment_mut(|artifacts| {
                if let Some(cleanup) = artifacts.cleanup.as_mut() {
                    cleanup.durable_transition_complete = true;
                }
            })
            .map_err(egress_proxy_error)?;
        }

        self.engine.complete_stop(&stop).map_err(egress_proxy_error)
    }
}

fn cleanup_transition_is_durably_complete(
    disposition: PepCleanupDisposition,
    record: &PortLeaseRecord,
) -> bool {
    match disposition {
        PepCleanupDisposition::Release => record.phase() == PortLeasePhase::Released,
        PepCleanupDisposition::Restart => restart_transition_is_durably_complete(record),
    }
}

fn restart_transition_is_durably_complete(record: &PortLeaseRecord) -> bool {
    record.phase() == PortLeasePhase::Reserved
        && record.reservation_claim().is_none()
        && record.bind_claim().is_none()
        && record.binding().is_none()
        && record.confirmed_stopped_binding().is_some()
        && record.failure().is_none()
}
