//! Shared per-sandbox egress PEP (Policy Enforcement Point) lifecycle.
//!
//! Every sandbox backend places its workload inside a deny-by-default network
//! namespace whose only outbound path is a host-side `nimbus_proxy::WorkloadPep`
//! bound on the bridge gateway. The "compile policy -> start the PEP -> register
//! / reload / stop" glue is security-critical and identical across backends
//! (the container backend today, the krun microVM backend next), so it lives
//! here once instead of being forked per backend.

use std::fs;
use std::net::{IpAddr, SocketAddr};
#[cfg(test)]
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_egress::{
    CompiledEgressPolicy, EGRESS_CA_BUNDLE_ENV, EGRESS_NODE_EXTRA_CA_CERTS_ENV,
    EGRESS_PROXY_URL_ENV, EGRESS_RESERVED_ENV_KEYS, EgressPolicy,
};
use nimbus_network::{
    ListenerId, NetworkReservationClaim, PortBindClaim, PortExposure, PortLeaseRecord,
    PortLeaseRequest,
};
#[cfg(test)]
use nimbus_proxy::WorkloadPep;
use nimbus_proxy::{
    AppendOnlyDecisionLogSink, DecisionLogSinkContext, EgressEngine, EgressProxyError,
    PreparedWorkloadPep, RegisteredLifecyclePhase, RegistrationDecision,
    RetainedFailedRegistration, WorkloadPepConfig, WorkloadPepReadiness, WorkloadPepTlsAuthority,
    fan_out_decision_loggers, tenant_decision_counter_sink,
};
use serde::{Deserialize, Serialize};

use crate::backends::oci::network::{OciNetworkConfig, bridge_gateway_addr};
use crate::backends::oci::port_lease::{
    ExpectedListenerAuthority, OciPortProvider, abandon_bind_attempts_without_effect,
    adopt_claimed_and_activate, claim_bind_attempts, prepare_rebind_after_confirmed_stop,
    record_bind_failure, release_reserved_batch_without_effect, require_active_provider_binding,
    require_current_listener_authority, target_for_ip,
};
#[cfg(test)]
use crate::backends::oci::port_lease::{
    OciPortLeaseIntent, port_lease_request, reserve_provider_assigned,
};
#[cfg(test)]
use crate::backends::oci::port_manager::PortManager;
use crate::backends::oci::port_manager::{InternalListenerReservation, ReservedInternalListener};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

mod cleanup;
use cleanup::PepCleanupProgress;

/// Registry of running per-sandbox egress proxies, shared by every sandbox
/// backend. Cloning shares the underlying registry (it is `Arc`-backed), so a
/// backend can hold one and hand clones to its async tasks.
///
/// The transport-clean lifecycle map lives in the node-scoped
/// [`nimbus_proxy::EgressEngine`], keyed by the opaque `nimbus-core`
/// [`WorkloadId`] (never `SandboxId`, so `nimbus-proxy` never depends on
/// `nimbus-sandbox`). This registry remains the sandbox-facing surface and
/// keeps the sandbox-layer publishing machinery — the trust-anchor writer, the
/// decision-log / trust-anchor roots, and path derivation — injecting its
/// published trust-anchor path as the engine entry's opaque attachment, so the
/// published CA file still lives in the same registry entry (under one lock)
/// as the PEP it belongs to.
#[derive(Clone)]
pub(crate) struct EgressProxyRegistry {
    engine: Arc<EgressEngine<RegisteredArtifacts>>,
    decision_log_root: Arc<PathBuf>,
    trust_anchor_root: Arc<PathBuf>,
    network_state_root: Arc<PathBuf>,
    #[cfg(test)]
    _test_state_root: Option<Arc<tempfile::TempDir>>,
    #[cfg(test)]
    pre_adoption_cleanup_observer: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    post_bind_claim_observer: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    post_activation_observer: Option<Arc<dyn Fn() -> Result<()> + Send + Sync>>,
}

/// Sandbox-owned per-registration artifacts riding the engine entry as its
/// opaque attachment: the published trust-anchor path (unwound at stop) and
/// the tenant fairness handle (pin released at stop so the node-wide fairness
/// map cannot grow monotonically).
struct RegisteredArtifacts {
    trust_anchor_path: Option<PathBuf>,
    /// RAII pin on the tenant's fairness entry; dropping it (at stop, or on
    /// any registration failure unwind) releases the pin — the registry
    /// evicts at zero leases, and capture+pin are atomic so no fork is
    /// possible.
    tenant_lease: nimbus_proxy::TenantLease,
    /// Present on every production registration. Test-only injection may omit
    /// it because it never exercises provider lease lifecycle.
    port_lease: Option<PortLeaseRequest>,
    /// Exact retry progress retained by the engine-owned Stopping tombstone.
    cleanup: Option<PepCleanupProgress>,
}

enum PepPreAdoptionAttempt<'a> {
    Claimed(&'a PortBindClaim),
    Bound {
        listener: Box<PreparedWorkloadPep>,
        claim: &'a PortBindClaim,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PepPreAdoptionReleaseAuthority<'a> {
    Retain,
    FreshLaunch(&'a NetworkReservationClaim),
}

impl<'a> PepPreAdoptionReleaseAuthority<'a> {
    fn reservation_claim(self) -> Option<&'a NetworkReservationClaim> {
        match self {
            Self::Retain => None,
            Self::FreshLaunch(claim) => Some(claim),
        }
    }
}

struct PepPreAdoptionCompensation<'a> {
    attempt: PepPreAdoptionAttempt<'a>,
    trust_anchor_path: &'a Path,
    port_lease: &'a PortLeaseRequest,
    release_authority: PepPreAdoptionReleaseAuthority<'a>,
}

struct FailedPepPostAdoption<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    release_authority: PepPreAdoptionReleaseAuthority<'a>,
    failure_context: &'static str,
    primary_error: EgressProxyError,
    actual_addr: SocketAddr,
    retained: RetainedFailedRegistration<RegisteredArtifacts>,
}

impl<'a> PepPreAdoptionCompensation<'a> {
    fn claimed(
        trust_anchor_path: &'a Path,
        port_lease: &'a PortLeaseRequest,
        claim: &'a PortBindClaim,
        release_authority: PepPreAdoptionReleaseAuthority<'a>,
    ) -> Self {
        Self {
            attempt: PepPreAdoptionAttempt::Claimed(claim),
            trust_anchor_path,
            port_lease,
            release_authority,
        }
    }

    fn bound(
        listener: PreparedWorkloadPep,
        trust_anchor_path: &'a Path,
        port_lease: &'a PortLeaseRequest,
        claim: &'a PortBindClaim,
        release_authority: PepPreAdoptionReleaseAuthority<'a>,
    ) -> Self {
        Self {
            attempt: PepPreAdoptionAttempt::Bound {
                listener: Box::new(listener),
                claim,
            },
            trust_anchor_path,
            port_lease,
            release_authority,
        }
    }
}

impl EgressProxyRegistry {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        let state_root =
            Arc::new(tempfile::TempDir::new().expect("egress test state root should exist"));
        let mut registry = Self::with_roots_and_network_state(
            state_root.path().join("decision-logs"),
            state_root.path().join("trust-anchors"),
            state_root.path(),
        );
        registry._test_state_root = Some(state_root);
        registry
    }

    #[cfg(test)]
    pub(crate) fn with_decision_log_root(decision_log_root: impl Into<PathBuf>) -> Self {
        let decision_log_root = decision_log_root.into();
        let state_root = decision_log_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("nimbus-egress-test-state"));
        let trust_anchor_root = state_root.join("egress-trust-anchors");
        Self::with_roots_and_network_state(decision_log_root, trust_anchor_root, state_root)
    }

    #[cfg(test)]
    pub(crate) fn with_roots(
        decision_log_root: impl Into<PathBuf>,
        trust_anchor_root: impl Into<PathBuf>,
    ) -> Self {
        let decision_log_root = decision_log_root.into();
        let state_root = decision_log_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("nimbus-egress-test-state"));
        Self::with_roots_and_network_state(decision_log_root, trust_anchor_root, state_root)
    }

    pub(crate) fn with_roots_and_network_state(
        decision_log_root: impl Into<PathBuf>,
        trust_anchor_root: impl Into<PathBuf>,
        network_state_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            engine: Arc::new(EgressEngine::new()),
            decision_log_root: Arc::new(decision_log_root.into()),
            trust_anchor_root: Arc::new(trust_anchor_root.into()),
            network_state_root: Arc::new(network_state_root.into()),
            #[cfg(test)]
            _test_state_root: None,
            #[cfg(test)]
            pre_adoption_cleanup_observer: None,
            #[cfg(test)]
            post_bind_claim_observer: None,
            #[cfg(test)]
            post_activation_observer: None,
        }
    }

    /// Derive the engine's opaque workload id from a sandbox id.
    ///
    /// Fail-closed: `WorkloadId` rejects only the empty string, and an empty
    /// sandbox id is a spec bug — surfaced as `InvalidSpec`, never registered.
    fn workload_id(tenant_id: &TenantId, id: &SandboxId) -> Result<WorkloadId> {
        let listener_id =
            ListenerId::for_tenant_workload_listener(tenant_id, id.as_str(), "egress-pep");
        WorkloadId::new(listener_id.as_str()).map_err(|_| SandboxError::InvalidSpec {
            message: "tenant-scoped sandbox identity cannot be empty (required for egress PEP registration)"
                .to_owned(),
        })
    }

    fn require_pep_lease(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        bind_addr: SocketAddr,
        port_lease: &PortLeaseRequest,
    ) -> Result<PortLeaseRecord> {
        require_current_listener_authority(
            &self.network_state_root,
            ExpectedListenerAuthority::egress_pep(tenant_id, id, bind_addr)?,
            port_lease,
        )
    }

    fn observe_post_activation(&self) -> Result<()> {
        #[cfg(test)]
        if let Some(observer) = self.post_activation_observer.as_ref() {
            observer()?;
        }
        Ok(())
    }

    /// Retire a PEP request only while the caller can prove no provider effect
    /// was adopted. Cleanup preserves the original preparation failure and
    /// reports trust-anchor or durable-release failures as secondary evidence.
    fn compensate_pep_pre_adoption_failure(
        &self,
        compensation: PepPreAdoptionCompensation<'_>,
        preparation_error: SandboxError,
    ) -> SandboxError {
        let PepPreAdoptionCompensation {
            attempt,
            trust_anchor_path,
            port_lease,
            release_authority,
        } = compensation;
        let (bound_listener, bind_claim, remove_owned_trust_anchor) = match attempt {
            PepPreAdoptionAttempt::Claimed(claim) => (None, Some(claim), false),
            PepPreAdoptionAttempt::Bound { listener, claim } => {
                (Some(*listener), Some(claim), true)
            }
        };
        let mut cleanup_errors = Vec::new();
        let mut trust_anchor_cleanup_confirmed = true;
        let bound_addr = bound_listener
            .as_ref()
            .map(|listener| listener.local_addr());
        if remove_owned_trust_anchor {
            #[cfg(test)]
            if let Some(observer) = self.pre_adoption_cleanup_observer.as_ref() {
                observer();
            }
            if let Err(error) = remove_trust_anchor_file(trust_anchor_path) {
                trust_anchor_cleanup_confirmed = false;
                cleanup_errors.push(error);
            }
        }
        // The provider effect must disappear before its durable authority can
        // be released. A bound attempt keeps the socket through trust-anchor
        // cleanup so an overlapping process cannot replace the anchor, then
        // drops it before making the port available to another lease owner.
        drop(bound_listener);
        let bind_claim_abandoned = if let Some(bind_claim) = bind_claim {
            match abandon_bind_attempts_without_effect(
                &self.network_state_root,
                std::slice::from_ref(port_lease),
                std::slice::from_ref(bind_claim),
                release_authority.reservation_claim(),
            ) {
                Ok(_) => true,
                Err(abandon_error) => {
                    let Some(bound_addr) = bound_addr else {
                        cleanup_errors.push(abandon_error);
                        return Self::finish_pep_pre_adoption_compensation(
                            preparation_error,
                            cleanup_errors,
                        );
                    };
                    match require_active_provider_binding(
                        &self.network_state_root,
                        port_lease,
                        bound_addr,
                        OciPortProvider::EgressPep,
                    ) {
                        Ok(record) => {
                            let binding = record
                                .binding()
                                .expect("exact Active provider evidence carries a binding");
                            if !trust_anchor_cleanup_confirmed {
                                cleanup_errors.push(SandboxError::OperationFailed {
                                    message: format!(
                                        "bind-claim abandonment failed: {abandon_error}; exact \
                                         Active listener remains fenced until trust-anchor removal \
                                         is confirmed"
                                    ),
                                });
                                false
                            } else {
                                match prepare_rebind_after_confirmed_stop(
                                    &self.network_state_root,
                                    port_lease,
                                    binding,
                                ) {
                                    Ok(_) => true,
                                    Err(rebind_error) => {
                                        cleanup_errors.push(SandboxError::OperationFailed {
                                            message: format!(
                                                "bind-claim abandonment failed: {abandon_error}; \
                                                 exact Active compensation also failed: \
                                                 {rebind_error}"
                                            ),
                                        });
                                        false
                                    }
                                }
                            }
                        }
                        Err(inspect_error) => {
                            cleanup_errors.push(SandboxError::OperationFailed {
                                message: format!(
                                    "bind-claim abandonment failed: {abandon_error}; exact Active \
                                     inspection also failed: {inspect_error}"
                                ),
                            });
                            false
                        }
                    }
                }
            }
        } else {
            false
        };
        if let Some(reservation_claim) = release_authority
            .reservation_claim()
            .filter(|_| trust_anchor_cleanup_confirmed && bind_claim_abandoned)
        {
            match release_reserved_batch_without_effect(
                &self.network_state_root,
                std::slice::from_ref(port_lease),
                reservation_claim,
            ) {
                Ok(_) => {}
                Err(error) => cleanup_errors.push(error),
            }
        }
        Self::finish_pep_pre_adoption_compensation(preparation_error, cleanup_errors)
    }

    fn finish_pep_pre_adoption_compensation(
        preparation_error: SandboxError,
        cleanup_errors: Vec<SandboxError>,
    ) -> SandboxError {
        if cleanup_errors.is_empty() {
            return preparation_error;
        }
        SandboxError::OperationFailed {
            message: format!(
                "egress PEP preparation failed: {preparation_error}; \
                 pre-adoption compensation also failed: {}",
                cleanup_errors
                    .into_iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }

    fn compensate_pep_post_adoption_failure(
        &self,
        failure: FailedPepPostAdoption<'_>,
    ) -> SandboxError {
        let FailedPepPostAdoption {
            tenant_id,
            sandbox_id,
            release_authority,
            failure_context,
            primary_error,
            actual_addr,
            retained,
        } = failure;
        let primary = egress_proxy_error(primary_error);
        let (stop, retention_conflict) = retained.into_parts();
        let artifact_evidence = stop.with_attachment(|artifacts| {
            (
                artifacts.port_lease.clone(),
                artifacts.trust_anchor_path.is_some(),
            )
        });
        let (port_lease, has_trust_anchor) = match artifact_evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                return SandboxError::OperationFailed {
                    message: format!(
                        "{failure_context}: {primary}; cleanup tombstone \
                         inspection failed and remains retained: {error}"
                    ),
                };
            }
        };
        let Some(port_lease) = port_lease else {
            return SandboxError::OperationFailed {
                message: format!(
                    "{failure_context}: {primary}; cleanup tombstone did not \
                     carry durable listener authority and remains retained"
                ),
            };
        };
        if !has_trust_anchor {
            return SandboxError::OperationFailed {
                message: format!(
                    "{failure_context}: {primary}; cleanup tombstone did not \
                     carry the published trust-anchor path and remains retained"
                ),
            };
        }
        let assignment = EgressProxyAssignment {
            host: actual_addr.ip().to_string(),
            port: actual_addr.port(),
            port_lease,
        };
        // Retention creates the first exclusive cleanup executor. The normal
        // stop path must reacquire that exact tombstone so all compensation
        // steps use one lifecycle path; releasing this handle transfers
        // execution authority without dropping provider evidence.
        drop(stop);
        let cleanup = match release_authority {
            PepPreAdoptionReleaseAuthority::Retain => {
                self.stop_for_restart(tenant_id, sandbox_id, Some(&assignment))
            }
            PepPreAdoptionReleaseAuthority::FreshLaunch(_) => {
                self.stop_with_assignment(tenant_id, sandbox_id, Some(&assignment))
            }
        };
        match cleanup {
            Ok(_) => match retention_conflict {
                Some(conflict) => SandboxError::OperationFailed {
                    message: format!(
                        "{failure_context}: {primary}; exact provider cleanup \
                         completed after quarantined retention conflict: {conflict}"
                    ),
                },
                None => primary,
            },
            Err(cleanup_error) => SandboxError::OperationFailed {
                message: format!(
                    "{failure_context}: {primary}; {}exact retryable \
                     compensation remains in the stopping tombstone: {cleanup_error}",
                    retention_conflict.map_or_else(String::new, |conflict| {
                        format!("quarantined retention conflict: {conflict}; ")
                    })
                ),
            },
        }
    }

    /// Ensure a PEP is running for `id`, bound on `bind_addr`.
    ///
    /// Idempotent: a no-op if a proxy is already registered for `id`.
    /// Fail-closed: a policy compile error or proxy start error returns `Err`
    /// and registers nothing — callers must treat that as deny.
    #[cfg(test)]
    pub(crate) fn ensure_running_with_lease(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        policy: &EgressPolicy,
        bind_addr: SocketAddr,
        port_lease: &PortLeaseRequest,
    ) -> Result<()> {
        self.ensure_running_with_lease_and_release_authority(
            tenant_id,
            id,
            policy,
            bind_addr,
            port_lease,
            PepPreAdoptionReleaseAuthority::Retain,
        )
    }

    fn ensure_running_with_lease_and_release_authority(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        policy: &EgressPolicy,
        bind_addr: SocketAddr,
        port_lease: &PortLeaseRequest,
        release_authority: PepPreAdoptionReleaseAuthority<'_>,
    ) -> Result<()> {
        let decision_log_path = self.decision_log_path(tenant_id, id);
        let trust_anchor_path = self.trust_anchor_path(tenant_id, id);
        let workload_id = Self::workload_id(tenant_id, id)?;
        let registration = self
            .engine
            .reserve_or_inspect(workload_id.clone(), |artifacts| {
                artifacts.port_lease.as_ref() == Some(port_lease)
            })
            .map_err(egress_proxy_error)?;
        let slot = match registration {
            RegistrationDecision::Reserved(slot) => slot,
            RegistrationDecision::Occupied {
                phase: RegisteredLifecyclePhase::Running,
                evidence: true,
            } => {
                self.require_pep_lease(tenant_id, id, bind_addr, port_lease)?;
                return Ok(());
            }
            RegistrationDecision::Occupied {
                phase: RegisteredLifecyclePhase::Running,
                evidence: false,
            } => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "running egress proxy for sandbox {id} does not match durable port lease {}",
                        port_lease.lease_id()
                    ),
                });
            }
            RegistrationDecision::Occupied {
                phase: RegisteredLifecyclePhase::Stopping,
                evidence,
            } => {
                let detail = if evidence {
                    "the exact provider cleanup is still in progress"
                } else {
                    "a different provider attachment owns the stopping fence"
                };
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "cannot start egress proxy for sandbox {id}: {detail}; durable authority \
                         remains fenced for reconciliation"
                    ),
                });
            }
        };
        self.require_pep_lease(tenant_id, id, bind_addr, port_lease)?;
        // Durable attempt exclusivity precedes every fallible preparation.
        // This prevents a concurrent same-request preparation failure from
        // compensating another invocation's restart or fresh-launch authority.
        let bind_claim = claim_bind_attempts(
            &self.network_state_root,
            std::slice::from_ref(port_lease),
            OciPortProvider::EgressPep,
            release_authority.reservation_claim(),
        )?
        .pop()
        .expect("one PEP request must return one bind claim");
        #[cfg(test)]
        if let Some(observer) = self.post_bind_claim_observer.as_ref() {
            observer();
        }
        // Expensive preparation (policy compile, decision-log open, CA keypair
        // generation) runs outside the registry lock so one slow start cannot
        // stall reload/readiness/stop for every other sandbox.
        let compiled = policy
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })
            .map_err(|error| {
                self.compensate_pep_pre_adoption_failure(
                    PepPreAdoptionCompensation::claimed(
                        &trust_anchor_path,
                        port_lease,
                        &bind_claim,
                        release_authority,
                    ),
                    error,
                )
            })?;
        let durable_decision_sink = AppendOnlyDecisionLogSink::open(
            &decision_log_path,
            DecisionLogSinkContext::new(tenant_id.as_str(), id.as_str()),
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to prepare append-only egress decision log for sandbox {id} at {}: {error}",
                decision_log_path.display()
            ),
        })
        .map_err(|error| {
            self.compensate_pep_pre_adoption_failure(
                PepPreAdoptionCompensation::claimed(
                    &trust_anchor_path,
                    port_lease,
                    &bind_claim,
                    release_authority,
                ),
                error,
            )
        })?
        .durable_sink();
        let tls_authority = WorkloadPepTlsAuthority::generate_ephemeral()
            .map_err(egress_proxy_error)
            .map_err(|error| {
                self.compensate_pep_pre_adoption_failure(
                    PepPreAdoptionCompensation::claimed(
                        &trust_anchor_path,
                        port_lease,
                        &bind_claim,
                        release_authority,
                    ),
                    error,
                )
            })?;
        let trust_anchor_pem = tls_authority.trust_anchor_pem();
        // EE3/EE4: check out the tenant's fairness lease once, at
        // registration — capture+pin atomic; any failure below auto-releases
        // the pin via Drop (no zero-pin zombie entries).
        let tenant_lease = self.engine.fairness().checkout(tenant_id);
        let tenant_fairness = Arc::clone(tenant_lease.handle());
        // EE4/GR3: fan out only best-effort telemetry sinks. The append-only
        // file sink is installed separately as the durable-before-response
        // audit sink.
        let decision_logger = fan_out_decision_loggers(vec![tenant_decision_counter_sink(
            Arc::clone(&tenant_fairness),
        )]);
        let prepared = match PreparedWorkloadPep::prepare(
            WorkloadPepConfig::new(compiled)
                .with_bind_addr(bind_addr)
                .with_tls_authority(tls_authority)
                .with_durable_decision_sink(durable_decision_sink)
                .with_decision_logger(decision_logger)
                // EE3: capture the tenant's fairness handle at registration —
                // the request path never looks tenants up.
                .with_tenant_fairness(Arc::clone(&tenant_fairness)),
        ) {
            Ok(prepared) => prepared,
            Err(EgressProxyError::BindFailed { address, kind }) => {
                let bind_error = egress_proxy_error(EgressProxyError::BindFailed { address, kind });
                let bind_error = match release_authority {
                    PepPreAdoptionReleaseAuthority::Retain => bind_error,
                    PepPreAdoptionReleaseAuthority::FreshLaunch(_) => {
                        match record_bind_failure(
                            &self.network_state_root,
                            port_lease,
                            &bind_claim,
                            address,
                            OciPortProvider::EgressPep,
                            kind,
                            release_authority.reservation_claim(),
                        ) {
                            Ok(_) => bind_error,
                            Err(record_error) => SandboxError::OperationFailed {
                                message: format!(
                                    "{bind_error}; durable PEP bind-failure recording also failed: \
                                     {record_error}"
                                ),
                            },
                        }
                    }
                };
                return Err(self.compensate_pep_pre_adoption_failure(
                    PepPreAdoptionCompensation::claimed(
                        &trust_anchor_path,
                        port_lease,
                        &bind_claim,
                        release_authority,
                    ),
                    bind_error,
                ));
            }
            Err(error) => {
                return Err(self.compensate_pep_pre_adoption_failure(
                    PepPreAdoptionCompensation::claimed(
                        &trust_anchor_path,
                        port_lease,
                        &bind_claim,
                        release_authority,
                    ),
                    egress_proxy_error(error),
                ));
            }
        };
        // The real socket establishes provider ownership before this
        // invocation mutates the workload's shared trust-anchor path. A
        // concurrent process that still owns the listener therefore loses at
        // bind and can never have its live anchor replaced or removed.
        if let Err(error) = write_trust_anchor_file(
            &self.trust_anchor_root,
            &trust_anchor_path,
            &trust_anchor_pem,
        ) {
            return Err(self.compensate_pep_pre_adoption_failure(
                PepPreAdoptionCompensation::bound(
                    prepared,
                    &trust_anchor_path,
                    port_lease,
                    &bind_claim,
                    release_authority,
                ),
                error,
            ));
        }
        if let Err(error) = adopt_claimed_and_activate(
            &self.network_state_root,
            port_lease,
            release_authority.reservation_claim(),
            &bind_claim,
            prepared.local_addr(),
            OciPortProvider::EgressPep,
        ) {
            return Err(self.compensate_pep_pre_adoption_failure(
                PepPreAdoptionCompensation::bound(
                    prepared,
                    &trust_anchor_path,
                    port_lease,
                    &bind_claim,
                    release_authority,
                ),
                error,
            ));
        }
        if let Err(error) = self.observe_post_activation() {
            let proxy = prepared.start();
            let actual_addr = proxy.local_addr();
            let artifacts = RegisteredArtifacts {
                trust_anchor_path: Some(trust_anchor_path),
                tenant_lease,
                port_lease: Some(port_lease.clone()),
                cleanup: None,
            };
            let (primary_error, retained) = slot.retain_failed(
                EgressProxyError::OperationFailed {
                    message: error.to_string(),
                },
                proxy,
                artifacts,
            );
            return Err(
                self.compensate_pep_post_adoption_failure(FailedPepPostAdoption {
                    tenant_id,
                    sandbox_id: id,
                    release_authority,
                    failure_context: "egress PEP activation acknowledgement failed",
                    primary_error,
                    actual_addr,
                    retained,
                }),
            );
        }
        let proxy = prepared.start();
        let artifacts = RegisteredArtifacts {
            trust_anchor_path: Some(trust_anchor_path),
            tenant_lease,
            port_lease: Some(port_lease.clone()),
            cleanup: None,
        };
        match slot.commit(proxy, artifacts) {
            Ok(()) => Ok(()),
            Err(failure) => {
                let actual_addr = failure.provider_local_addr();
                let (commit_error, retained) = failure.retain();
                Err(
                    self.compensate_pep_post_adoption_failure(FailedPepPostAdoption {
                        tenant_id,
                        sandbox_id: id,
                        release_authority,
                        failure_context: "egress PEP registration commit failed",
                        primary_error: commit_error,
                        actual_addr,
                        retained,
                    }),
                )
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn ensure_running(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        policy: &EgressPolicy,
        bind_addr: SocketAddr,
    ) -> Result<()> {
        let request = port_lease_request(
            tenant_id,
            id,
            "egress-pep",
            OciPortLeaseIntent::host_internal(
                target_for_ip(bind_addr.ip())?,
                PortExposure::Private,
            ),
            nimbus_network::PortRequestMode::ProviderAssigned,
        );
        let request = reserve_provider_assigned(&self.network_state_root, request)?;
        self.ensure_running_with_lease(tenant_id, id, policy, bind_addr, &request)
    }

    /// Hot-reload the policy on the running PEP for `id`.
    ///
    /// Errors if no proxy is registered for `id` (the caller ensures it is
    /// running first). Fail-closed: a reload error is surfaced, not swallowed.
    pub(crate) fn reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        compiled: CompiledEgressPolicy,
    ) -> Result<()> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.reload_policy(compiled))
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("egress proxy for sandbox {id} is not running"),
            })?
            .map_err(egress_proxy_error)?;
        Ok(())
    }

    /// Report the readiness of the PEP registered for `id`.
    ///
    /// Returns `Ok(None)` when no proxy is registered (so the caller treats an
    /// absent PEP as deny), and `Ok(Some(readiness))` carrying the proxy's
    /// active-policy state otherwise. A readiness gate must require both that a
    /// proxy is registered AND that its `WorkloadPepReadiness` reports an active
    /// policy generation before permitting a workload to launch.
    pub(crate) fn readiness(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> Result<Option<WorkloadPepReadiness>> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.readiness())
            .map_err(egress_proxy_error)?
            .map(|readiness| readiness.map_err(egress_proxy_error))
            .transpose()
    }

    /// True if a PEP is currently registered for `id`.
    #[cfg(test)]
    pub(crate) fn contains(&self, tenant_id: &TenantId, id: &SandboxId) -> Result<bool> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .contains(&workload_id)
            .map_err(egress_proxy_error)
    }

    #[cfg(test)]
    pub(crate) fn local_addr(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> Result<Option<SocketAddr>> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.local_addr())
            .map_err(egress_proxy_error)
    }

    #[cfg(test)]
    pub(crate) fn decision_log_path_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> PathBuf {
        self.decision_log_path(tenant_id, id)
    }

    #[cfg(test)]
    pub(crate) fn trust_anchor_path_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> PathBuf {
        self.trust_anchor_path(tenant_id, id)
    }

    #[cfg(test)]
    fn with_pre_adoption_cleanup_observer(
        mut self,
        observer: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.pre_adoption_cleanup_observer = Some(Arc::new(observer));
        self
    }

    #[cfg(test)]
    fn with_post_bind_claim_observer(
        mut self,
        observer: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.post_bind_claim_observer = Some(Arc::new(observer));
        self
    }

    #[cfg(test)]
    fn with_post_activation_observer(
        mut self,
        observer: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.post_activation_observer = Some(Arc::new(observer));
        self
    }

    /// Register an already-started proxy for `id` under test, so a readiness
    /// gate can be exercised against a not-ready (policy-less) PEP without a
    /// live VMM. Production code only ever registers a PEP through
    /// [`EgressProxyRegistry::ensure_running`], which always loads a policy.
    #[cfg(test)]
    pub(crate) fn insert_running_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        proxy: WorkloadPep,
    ) -> Result<()> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        let slot = self
            .engine
            .try_reserve(workload_id)
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("test egress proxy for sandbox {id} is already registered"),
            })?;
        slot.commit(
            proxy,
            RegisteredArtifacts {
                trust_anchor_path: None,
                tenant_lease: self.engine.fairness().checkout(tenant_id),
                port_lease: None,
                cleanup: None,
            },
        )
        .map_err(|failure| {
            let (error, retained) = failure.retain();
            let (stop, conflict) = retained.into_parts();
            let cleanup = stop
                .shutdown_provider()
                .and_then(|()| self.engine.complete_stop(&stop));
            match (conflict, cleanup) {
                (None, Ok(())) => egress_proxy_error(error),
                (conflict, cleanup) => SandboxError::OperationFailed {
                    message: format!(
                        "test PEP registration failed: {error}; retained provider cleanup \
                         conflict={conflict:?}, result={cleanup:?}"
                    ),
                },
            }
        })?;
        Ok(())
    }

    fn decision_log_path(&self, tenant_id: &TenantId, id: &SandboxId) -> PathBuf {
        self.decision_log_root
            .join(tenant_id.as_str())
            .join(format!("{}.jsonl", id.as_str()))
    }

    fn trust_anchor_path(&self, tenant_id: &TenantId, id: &SandboxId) -> PathBuf {
        egress_trust_anchor_path(&self.trust_anchor_root, tenant_id, id)
    }
}

pub(crate) fn egress_decision_log_root(state_root: &Path) -> PathBuf {
    state_root.join("egress-decision-logs")
}

pub(crate) fn egress_trust_anchor_root(state_root: &Path) -> PathBuf {
    state_root.join("egress-trust-anchors")
}

pub(crate) const EGRESS_TRUST_ANCHOR_GUEST_PATH: &str = "/run/nimbus/egress/ca.pem";

const EGRESS_TRUST_ANCHOR_PLACEHOLDER: &str =
    "# Nimbus egress trust anchor placeholder; overwritten before launch\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EgressTrustAnchorMount {
    pub(crate) host_path: PathBuf,
    pub(crate) guest_path: String,
}

pub(crate) fn egress_trust_anchor_mount(
    state_root: &Path,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> Result<EgressTrustAnchorMount> {
    let trust_anchor_root = egress_trust_anchor_root(state_root);
    let host_path = egress_trust_anchor_path(&trust_anchor_root, tenant_id, id);
    prepare_egress_trust_anchor_file(&trust_anchor_root, &host_path)?;
    Ok(EgressTrustAnchorMount {
        host_path,
        guest_path: EGRESS_TRUST_ANCHOR_GUEST_PATH.to_owned(),
    })
}

pub(crate) fn egress_trust_anchor_path(
    trust_anchor_root: &Path,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> PathBuf {
    trust_anchor_root
        .join(tenant_id.as_str())
        .join(format!("{}.pem", id.as_str()))
}

pub(crate) fn prepare_egress_trust_anchor_file(
    trust_anchor_root: &Path,
    path: &Path,
) -> Result<()> {
    write_trust_anchor_file(trust_anchor_root, path, EGRESS_TRUST_ANCHOR_PLACEHOLDER)
}

/// Canonical trust-anchor writer: every trust-anchor byte that reaches disk
/// goes through here. The write is rooted (the target must sit inside
/// `trust_anchor_root` with no traversal components), permissioned explicitly
/// (0644 — the guest bind-mounts the file read-only and must be able to read
/// it), durable (file fsync before rename, directory fsync after), and atomic
/// (temp file in the target directory, then rename), so a concurrent reader —
/// including a guest that already bind-mounted the path — can never observe a
/// torn or half-written PEM.
///
/// Trust boundary: the root is host-owned Nimbus state that only this writer
/// populates, and the tenant/sandbox ids are validated, so the path guards are
/// defense-in-depth against a path-construction bug or a tampered/stale state
/// tree — not a sandbox against an attacker who already holds host write access
/// under our state dir.
fn write_trust_anchor_file(trust_anchor_root: &Path, path: &Path, contents: &str) -> Result<()> {
    validate_trust_anchor_path(trust_anchor_root, path)?;
    let parent = path.parent().ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "egress trust-anchor path {} has no parent directory",
            path.display()
        ),
    })?;
    fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create egress trust-anchor directory {}: {error}",
            parent.display()
        ),
    })?;
    // Tamper tripwire the lexical check can't provide: fail closed if any
    // directory component under the root is a symlink, so a tampered state tree
    // can never redirect where the anchor lands. The final-component write is
    // already symlink-safe on its own — `create_new` refuses a symlinked temp
    // path and `rename` replaces the destination name without following it — so
    // together no anchor bytes are ever written through a symlink.
    reject_symlinked_components(trust_anchor_root, parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "egress trust-anchor path {} has no file name",
                path.display()
            ),
        })?;
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // 0o644: guest-readable trust anchor. Unix-only; Windows ACLs
        // inherit from the parent directory (the OCI backend is not
        // functional on Windows — this keeps the crate compiling there).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o644);
        }
        let mut temp_file = options.open(&temp_path)?;
        temp_file.write_all(contents.as_bytes())?;
        temp_file.sync_all()?;
        fs::rename(&temp_path, path)?;
        // Make the rename itself durable: fsync the containing directory so a
        // crash cannot resurrect the old entry after the guest saw the new one.
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    write_result.map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        SandboxError::OperationFailed {
            message: format!(
                "failed to write egress trust anchor {}: {error}",
                path.display()
            ),
        }
    })
}

/// Fail closed if any path component from `trust_anchor_root` down to and
/// including `parent` is a symlink. `lstat`-based, so it never follows a link;
/// combined with the symlink-safe final-component write it guarantees no anchor
/// bytes are written through a symlinked directory. Callers pass a `parent`
/// already validated to sit under the root by [`validate_trust_anchor_path`].
fn reject_symlinked_components(trust_anchor_root: &Path, parent: &Path) -> Result<()> {
    let Ok(relative) = parent.strip_prefix(trust_anchor_root) else {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "egress trust-anchor directory {} is not under root {}",
                parent.display(),
                trust_anchor_root.display()
            ),
        });
    };
    let mut current = trust_anchor_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata =
            fs::symlink_metadata(&current).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to stat egress trust-anchor component {}: {error}",
                    current.display()
                ),
            })?;
        if metadata.file_type().is_symlink() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "egress trust-anchor component {} is a symlink; refusing to write through it",
                    current.display()
                ),
            });
        }
    }
    Ok(())
}

/// Reject any trust-anchor target that does not sit strictly inside
/// `trust_anchor_root` via normal path components. The tenant/sandbox
/// identifiers that build these paths are already validated, so this is a
/// defense-in-depth boundary: no caller can turn the canonical writer into an
/// arbitrary-file write primitive.
fn validate_trust_anchor_path(trust_anchor_root: &Path, path: &Path) -> Result<()> {
    let invalid = || SandboxError::OperationFailed {
        message: format!(
            "egress trust-anchor path {} escapes trust-anchor root {}",
            path.display(),
            trust_anchor_root.display()
        ),
    };
    let relative = path
        .strip_prefix(trust_anchor_root)
        .map_err(|_| invalid())?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(())
}

fn remove_trust_anchor_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to remove egress trust anchor {}: {error}",
                path.display()
            ),
        }),
    }
}

/// Remove the trust anchor materialized for a workload that never activated.
///
/// Plan-only runner preparation writes the same canonical path as live PEP
/// preparation, but has no process-local registry entry to drive normal PEP
/// shutdown. The identifiers are validated and the derived path is rechecked
/// against the canonical root before deletion.
pub(crate) fn remove_unactivated_egress_trust_anchor(
    state_root: &Path,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> Result<()> {
    let trust_anchor_root = egress_trust_anchor_root(state_root);
    let path = egress_trust_anchor_path(&trust_anchor_root, tenant_id, id);
    validate_trust_anchor_path(&trust_anchor_root, &path)?;
    remove_trust_anchor_file(&path)
}

/// Map a `nimbus_proxy` error into a sandbox operation failure.
pub(crate) fn egress_proxy_error(error: EgressProxyError) -> SandboxError {
    SandboxError::OperationFailed {
        message: error.to_string(),
    }
}

/// Build the container-shape HTTP-proxy environment entries that point a sandbox
/// workload at its host-side egress PEP.
///
/// The shape (`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` plus the lowercase variants,
/// the Nimbus `EGRESS_PROXY_URL_ENV` handle, and an empty `NO_PROXY` so nothing
/// is exempt) is backend-agnostic, so both the container backend and the krun
/// microVM backend call this one helper instead of forking the env shape. The
/// caller is responsible for first scrubbing `EGRESS_RESERVED_ENV_KEYS` so a
/// tenant-supplied proxy override can never survive into the launched workload.
pub(crate) fn egress_proxy_env_entries(egress_proxy_url: &str) -> Vec<String> {
    [
        (EGRESS_PROXY_URL_ENV, egress_proxy_url),
        ("HTTP_PROXY", egress_proxy_url),
        ("http_proxy", egress_proxy_url),
        ("HTTPS_PROXY", egress_proxy_url),
        ("https_proxy", egress_proxy_url),
        ("ALL_PROXY", egress_proxy_url),
        ("all_proxy", egress_proxy_url),
        ("NO_PROXY", ""),
        ("no_proxy", ""),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

/// Build trust-anchor env entries for workloads that are routed through a
/// host-side PEP capable of selective HTTPS interception.
pub(crate) fn egress_trust_anchor_env_entries(guest_path: &str) -> Vec<String> {
    [
        (EGRESS_CA_BUNDLE_ENV, guest_path),
        (EGRESS_NODE_EXTRA_CA_CERTS_ENV, guest_path),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

/// Tier-neutral host-side egress PEP assignment for an execute-mode sandbox.
///
/// The proxy binds on the bridge gateway address so it is the only reachable
/// outbound path from inside the sandbox's deny-by-default network namespace.
/// Every sandbox backend (container today, krun microVM next) embeds an
/// `Option<EgressProxyAssignment>` in its persisted manifest and renders the
/// guest-facing proxy URL through [`EgressProxyAssignment::proxy_url`], so the
/// assignment shape and its IPv6-safe URL rendering live here once instead of
/// being forked per backend. (egress audit M9.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EgressProxyAssignment {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) port_lease: PortLeaseRequest,
}

impl EgressProxyAssignment {
    #[cfg(test)]
    pub(crate) fn for_test(host: &str, port: u16) -> Self {
        let tenant_id = TenantId::new("egress-assignment-test").expect("static tenant id");
        let sandbox_id = SandboxId::new(format!("egress-assignment-{port}"));
        let ip = host
            .parse::<IpAddr>()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        let mode = NonZeroU16::new(port)
            .map(nimbus_network::PortRequestMode::Exact)
            .unwrap_or(nimbus_network::PortRequestMode::ProviderAssigned);
        Self {
            host: host.to_owned(),
            port,
            port_lease: port_lease_request(
                &tenant_id,
                &sandbox_id,
                "egress-pep",
                OciPortLeaseIntent::host_internal(
                    target_for_ip(ip)
                        .expect("parsed test IP should produce a portable bind target"),
                    PortExposure::Private,
                ),
                mode,
            ),
        }
    }

    /// Bind address the PEP listens on. The host must be an IP literal (the
    /// bridge gateway), so a non-IP value fails closed as an invalid spec.
    pub(crate) fn bind_addr(&self) -> Result<SocketAddr> {
        let host = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| SandboxError::InvalidSpec {
                message: format!("egress proxy host {:?} must be an IP address", self.host),
            })?;
        Ok(SocketAddr::new(host, self.port))
    }

    /// Container-shape proxy URL the guest env is pointed at. Rendered through
    /// [`SocketAddr`] so an IPv6 gateway is bracketed correctly
    /// (`http://[::1]:15000`, never the malformed `http://::1:15000`).
    pub(crate) fn proxy_url(&self) -> Result<String> {
        Ok(format!("http://{}", self.bind_addr()?))
    }
}

/// Assign a host-side egress PEP for an execute-mode launch: the proxy binds on
/// the bridge gateway address so it is the only outbound path reachable from
/// inside the sandbox's deny-by-default network namespace. Shared by every
/// sandbox backend so the gateway+port allocation is defined once.
#[cfg(test)]
pub(crate) fn allocate_egress_proxy(
    network_config: &OciNetworkConfig,
    port_manager: &PortManager,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> Result<EgressProxyAssignment> {
    let gateway = bridge_gateway_addr(network_config)?;
    let (port, port_lease) = port_manager.reserve_internal_listener(
        tenant_id,
        id,
        "egress-pep",
        target_for_ip(IpAddr::V4(gateway))?,
        PortExposure::Private,
    )?;
    Ok(EgressProxyAssignment {
        host: gateway.to_string(),
        port,
        port_lease,
    })
}

/// Portable egress-listener intent to include in one sandbox launch batch.
pub(crate) fn egress_listener_reservation(
    network_config: &OciNetworkConfig,
) -> Result<InternalListenerReservation> {
    let gateway = bridge_gateway_addr(network_config)?;
    Ok(InternalListenerReservation::new(
        "egress-pep",
        target_for_ip(IpAddr::V4(gateway))?,
        PortExposure::Private,
    ))
}

/// Convert one atomically reserved internal listener into persisted PEP state.
pub(crate) fn egress_proxy_assignment(
    network_config: &OciNetworkConfig,
    reservation: ReservedInternalListener,
) -> Result<EgressProxyAssignment> {
    Ok(EgressProxyAssignment {
        host: bridge_gateway_addr(network_config)?.to_string(),
        port: reservation.port,
        port_lease: reservation.lease,
    })
}

/// Start the host-side egress PEP for a sandbox on its assigned bridge-gateway
/// bind address. Fail-closed: a missing assignment or a proxy start error
/// returns `Err`, which every backend's launch path treats as deny. Shared so
/// the "no assignment means deny" invariant cannot drift between backends.
pub(crate) fn ensure_egress_proxy_running(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    assignment: Option<&EgressProxyAssignment>,
    policy: &EgressPolicy,
) -> Result<()> {
    ensure_egress_proxy_running_with_release_authority(
        registry,
        tenant_id,
        id,
        assignment,
        policy,
        PepPreAdoptionReleaseAuthority::Retain,
    )
}

pub(crate) fn ensure_egress_proxy_running_with_release_authority(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    assignment: Option<&EgressProxyAssignment>,
    policy: &EgressPolicy,
    release_authority: PepPreAdoptionReleaseAuthority<'_>,
) -> Result<()> {
    let Some(assignment) = assignment else {
        return Err(SandboxError::OperationFailed {
            message: format!("sandbox {id} has no egress proxy assignment"),
        });
    };
    let bind_addr = assignment.bind_addr()?;
    #[cfg(test)]
    let test_port_lease = if assignment.port == 0 {
        let request = port_lease_request(
            tenant_id,
            id,
            "egress-pep",
            OciPortLeaseIntent::host_internal(
                target_for_ip(bind_addr.ip())?,
                PortExposure::Private,
            ),
            nimbus_network::PortRequestMode::ProviderAssigned,
        );
        Some(reserve_provider_assigned(
            &registry.network_state_root,
            request,
        )?)
    } else {
        None
    };
    #[cfg(test)]
    let port_lease = test_port_lease.as_ref().unwrap_or(&assignment.port_lease);
    #[cfg(not(test))]
    let port_lease = &assignment.port_lease;
    registry.ensure_running_with_lease_and_release_authority(
        tenant_id,
        id,
        policy,
        bind_addr,
        port_lease,
        release_authority,
    )
}

/// Extract the key (`KEY` of `KEY=VALUE`) of an OCI process env entry, or `None`
/// for a malformed or empty-key entry.
fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

/// Scrub every reserved egress env key ([`EGRESS_RESERVED_ENV_KEYS`]) from a
/// process env vector so a tenant-supplied proxy override can never survive into
/// the launched workload.
///
/// This is the security companion of [`egress_proxy_env_entries`]: a backend
/// MUST scrub before injecting the PEP env so the two halves can never be
/// half-applied. Defined here once so both backends call the same scrub.
/// (egress audit L11.)
pub(crate) fn scrub_reserved_egress_env(env: &mut Vec<String>) {
    env.retain(|entry| env_key(entry).is_none_or(|key| !EGRESS_RESERVED_ENV_KEYS.contains(&key)));
}

#[cfg(test)]
#[path = "egress/tests.rs"]
mod tests;
