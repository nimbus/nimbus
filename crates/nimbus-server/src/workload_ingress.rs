//! Server-owned deferred ingress publication for private sandbox attachments.
//!
//! The compute dispatcher supplies exact fenced commands. Sandbox providers
//! expose only authenticated private routes. This module owns the real host
//! TCP listeners and transparent byte forwarding; it does not terminate TLS,
//! resolve service names, decide tenant policy, or mutate attachment state.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::restart_provider_command::{
    ProviderRestartEffectObservation, ProviderRestartPhaseAdapter,
};
use nimbus_compute::workload_saga::restart_sandbox::{
    ValidatedSandboxRestartCommand, validate_sandbox_restart_command,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadRestartCommand,
    IngressPublicationCapability, IngressPublicationInspectionCapability,
    RestartPublicationCapability, RestartPublicationObservationCapability,
    RestartPublicationWithdrawalCapability, WorkloadProvisionCapabilityFuture,
    WorkloadRestartCapabilityFuture, validate_sandbox_provision_command,
};
use nimbus_compute::{
    WorkloadIngressBindingWitness, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest,
    WorkloadObservedIngressEndpoint, WorkloadProviderObservation,
};
use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalPortLeaseAuthority, NetworkCapabilityRole,
    NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration, NetworkResourceId, PortBindRealm,
    PortLeaseAccounting, PortLeaseError, PortLeaseId, PortLeasePhase, PortLeaseRequest,
    PortProtocol, PublishedEndpointId,
};
use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::backends::krun::KrunSandboxBackend;
use nimbus_sandbox::{
    ProviderCommandJournalError, SandboxBackendKind, SandboxError, SandboxProvisionIngressRoute,
    SandboxProvisionIngressTargetObservation, SandboxProvisionIngressTargets,
    SandboxProvisionNetworkPlan,
};
use nimbus_workloads::{WorkloadProvisionProviderTarget, WorkloadPublicationIntent};

#[cfg(test)]
use crate::listener_lease::{
    ActiveServerListenerLease, recover_dead_process_bound_server_listeners_for_final_withdrawal,
    withdraw_server_listeners_for_final_withdrawal,
};
use crate::listener_lease::{
    ServerListenerLeaseAuthority, stop_and_retain_server_listeners_for_restart,
};
use crate::network_capabilities::nimbus_owned_local_ingress_provider_id;

#[path = "workload_ingress/final_withdrawal.rs"]
mod final_withdrawal;
use final_withdrawal::{FinalIngressPhase, PublicationKey, PublishedIngressAuthority};
#[path = "workload_ingress/route_workers.rs"]
mod route_workers;
use route_workers::RunningIngressRoute;

const SERVER_INGRESS_JOURNAL_NAMESPACE: &str = "server-workload-ingress";
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(10);
const CONNECTION_IO_TIMEOUT: Duration = Duration::from_millis(100);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const DEFAULT_MAX_ACTIVE_CONNECTIONS: usize = 128;

/// Effect-free private-route source implemented by concrete sandbox owners.
pub trait LocalSandboxIngressTargetSource: Send + Sync {
    fn backend_kind(&self) -> SandboxBackendKind;

    fn inspect_targets(
        &self,
        sandbox_id: &nimbus_sandbox::SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<SandboxProvisionIngressTargetObservation, SandboxError>;
}

impl LocalSandboxIngressTargetSource for ContainerSandboxBackend {
    fn backend_kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect_targets(
        &self,
        sandbox_id: &nimbus_sandbox::SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<SandboxProvisionIngressTargetObservation, SandboxError> {
        self.inspect_provision_server_ingress_targets(
            sandbox_id,
            execution_attempt_id,
            network_plan,
        )
    }
}

impl LocalSandboxIngressTargetSource for KrunSandboxBackend {
    fn backend_kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect_targets(
        &self,
        sandbox_id: &nimbus_sandbox::SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<SandboxProvisionIngressTargetObservation, SandboxError> {
        self.inspect_provision_server_ingress_targets(
            sandbox_id,
            execution_attempt_id,
            network_plan,
        )
    }
}

/// Real server-owned publication capability for local Container or Krun work.
pub struct ServerIngressPublicationAdapter {
    source: Arc<dyn LocalSandboxIngressTargetSource>,
    phases: ProviderProvisionPhaseAdapter,
    restart_phases: ProviderRestartPhaseAdapter,
    listeners: ServerListenerLeaseAuthority,
    port_leases: LocalPortLeaseAuthority,
    running: Mutex<BTreeMap<PublicationKey, RunningIngressBatch>>,
}

impl ServerIngressPublicationAdapter {
    pub fn new<Source>(
        source: Arc<Source>,
        network_authority: LocalNetworkAuthority,
    ) -> Result<Self, ProviderCommandJournalError>
    where
        Source: LocalSandboxIngressTargetSource + 'static,
    {
        let journal = nimbus_sandbox::ProviderCommandAttemptJournal::open(
            network_authority.state_root(),
            SERVER_INGRESS_JOURNAL_NAMESPACE,
        )?;
        let port_leases = network_authority.port_leases();
        Ok(Self {
            source,
            phases: ProviderProvisionPhaseAdapter::new(journal.clone()),
            restart_phases: ProviderRestartPhaseAdapter::new(journal),
            listeners: ServerListenerLeaseAuthority::new(network_authority),
            port_leases,
            running: Mutex::new(BTreeMap::new()),
        })
    }

    fn validate(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> Result<ValidatedPublication, ProviderProvisionEffectObservation> {
        let provider_source_digest = match command.provider_target() {
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Ingress,
                provider_id,
                provider_source_digest,
            } if provider_id == &nimbus_owned_local_ingress_provider_id() => {
                *provider_source_digest
            }
            _ => {
                return Err(definite_failure(
                    "server_ingress_provider_mismatch",
                    "publication command does not target Nimbus-owned local ingress",
                ));
            }
        };
        let nimbus_workloads::WorkloadProvisionSubjects::Publication(reference) =
            command.subjects()
        else {
            return Err(definite_failure(
                "server_ingress_publication_subject_mismatch",
                "publication command does not carry its exact publication reference",
            ));
        };
        let validated = validate_sandbox_provision_command(command, self.source.backend_kind())?;
        Ok(ValidatedPublication {
            key: PublicationKey {
                saga_id: command.saga_id().as_str().to_owned(),
                attempt_id: command.attempt_id().as_str().to_owned(),
                execution_id: validated.sandbox_id().as_str().to_owned(),
                generation: command.generation().as_u64(),
                network_plan_digest: command.network_plan_digest().to_string(),
            },
            sandbox_id: validated.sandbox_id().clone(),
            execution_attempt_id: validated.execution_attempt_id().clone(),
            network_plan: validated.network_plan().clone(),
            publication: PublishedIngressAuthority::new(
                reference.clone(),
                provider_source_digest,
                command.source_digest(),
            ),
        })
    }

    fn publish(&self, validated: &ValidatedPublication) -> ProviderProvisionEffectObservation {
        let source = self.inspect_source(validated);
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => {
                return ambiguous("server ingress registry lock is poisoned");
            }
        };
        if let Some(existing) = running.get(&validated.key) {
            return classify_existing_publication(existing, &validated.key.execution_id, source);
        }
        let targets = match source {
            Ok(targets) => targets,
            Err(observation) => return observation,
        };
        let batch = match RunningIngressBatch::start(
            &self.listeners,
            &validated.key.execution_id,
            &targets,
            validated.publication.clone(),
        ) {
            Ok(batch) => batch,
            Err(error) => return bind_error(error),
        };
        let evidence = batch.evidence();
        running.insert(validated.key.clone(), batch);
        succeeded(evidence)
    }

    fn inspect(
        &self,
        validated: &ValidatedPublication,
        allow_absence: bool,
    ) -> ProviderProvisionEffectObservation {
        let source = self.inspect_source(validated);
        self.inspect_with_source(&validated.key, source, allow_absence)
    }

    fn inspect_with_source(
        &self,
        key: &PublicationKey,
        source: Result<SandboxProvisionIngressTargets, ProviderProvisionEffectObservation>,
        allow_absence: bool,
    ) -> ProviderProvisionEffectObservation {
        let running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return ambiguous("server ingress registry lock is poisoned"),
        };
        match (running.get(key), source) {
            (Some(batch), Ok(targets))
                if batch.matches(&key.execution_id, &targets) && batch.is_healthy() =>
            {
                succeeded(batch.evidence())
            }
            (
                Some(batch),
                Err(
                    ProviderProvisionEffectObservation::Absent { .. }
                    | ProviderProvisionEffectObservation::InProgress { .. },
                ),
            ) if batch.is_healthy() => succeeded(batch.evidence()),
            (Some(_), _) => ambiguous("server ingress worker state is unhealthy or crossed"),
            (None, Ok(targets)) if allow_absence => ProviderProvisionEffectObservation::Absent {
                evidence: source_evidence("server_ingress_absent", &targets),
            },
            (None, Ok(targets)) => ProviderProvisionEffectObservation::InProgress {
                evidence: source_evidence("server_ingress_not_observed", &targets),
            },
            (None, Err(observation)) => observation,
        }
    }

    fn inspect_source(
        &self,
        validated: &ValidatedPublication,
    ) -> Result<SandboxProvisionIngressTargets, ProviderProvisionEffectObservation> {
        match self.source.inspect_targets(
            &validated.sandbox_id,
            &validated.execution_attempt_id,
            &validated.network_plan,
        ) {
            Ok(SandboxProvisionIngressTargetObservation::Ready { targets, .. }) => Ok(targets),
            Ok(SandboxProvisionIngressTargetObservation::Absent { evidence }) => {
                Err(ProviderProvisionEffectObservation::Absent { evidence })
            }
            Ok(SandboxProvisionIngressTargetObservation::InProgress { evidence }) => {
                Err(ProviderProvisionEffectObservation::InProgress { evidence })
            }
            Err(SandboxError::InvalidSpec { message }) => {
                Err(definite_failure("server_ingress_target_rejected", message))
            }
            Err(error) => Err(ambiguous(error.to_string())),
        }
    }

    fn validate_restart(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> Result<ValidatedRestartPublication, ProviderRestartEffectObservation> {
        let validated = validate_sandbox_restart_command(command, self.source.backend_kind())?;
        ValidatedRestartPublication::new(command, validated).ok_or_else(|| {
            restart_definite_failure(
                "restart ingress command omits its exact publication or provider evidence",
            )
        })
    }

    fn withdraw_restart_publication(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> ProviderRestartEffectObservation {
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return restart_ambiguous("server ingress registry lock is poisoned"),
        };
        let Some(batch) = running.remove(&validated.source_key) else {
            return self.reconcile_restart_withdrawal_locked(&running, validated);
        };
        if !batch.matches_plan(&validated.source_key.execution_id, &validated.network_plan) {
            running.insert(validated.source_key.clone(), batch);
            return restart_definite_failure(
                "server ingress source publication is crossed with the restart command",
            );
        }
        match batch.stop_and_retain_for_restart() {
            Ok(evidence) => ProviderRestartEffectObservation::Succeeded { evidence },
            Err(error) => restart_ambiguous(error.to_string()),
        }
    }

    fn inspect_restart_withdrawal(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> ProviderRestartEffectObservation {
        let running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return restart_ambiguous("server ingress registry lock is poisoned"),
        };
        self.inspect_restart_withdrawal_locked(&running, validated)
    }

    fn inspect_restart_withdrawal_locked(
        &self,
        running: &BTreeMap<PublicationKey, RunningIngressBatch>,
        validated: &ValidatedRestartPublication,
    ) -> ProviderRestartEffectObservation {
        if running.contains_key(&validated.source_key) {
            return ProviderRestartEffectObservation::InProgress {
                evidence: b"source publication remains routable".to_vec(),
            };
        }
        if running
            .keys()
            .any(|key| key.saga_id == validated.source_key.saga_id && key != &validated.target_key)
        {
            return restart_ambiguous("a crossed publication attempt exists for the restart saga");
        }
        match self.inspect_retained_restart_listeners(validated) {
            Ok(true) => restart_withdrawal_succeeded(validated),
            Ok(false) => ProviderRestartEffectObservation::Absent {
                evidence: b"durable server listener retention is absent".to_vec(),
            },
            Err(observation) => observation,
        }
    }

    fn reconcile_restart_withdrawal_locked(
        &self,
        running: &BTreeMap<PublicationKey, RunningIngressBatch>,
        validated: &ValidatedRestartPublication,
    ) -> ProviderRestartEffectObservation {
        let inspected = self.inspect_restart_withdrawal_locked(running, validated);
        if !matches!(inspected, ProviderRestartEffectObservation::Absent { .. }) {
            return inspected;
        }
        let (plan_members, requests) = match self.restart_listener_plan(validated) {
            Ok(plan) => plan,
            Err(observation) => return observation,
        };
        if requests.is_empty() {
            return restart_withdrawal_succeeded(validated);
        }
        let recoveries = match self
            .port_leases
            .recover_dead_plan_members(&plan_members, &requests)
        {
            Ok(recoveries) => recoveries,
            Err(PortLeaseError::LifetimeOwnerLive { .. }) => {
                return ProviderRestartEffectObservation::InProgress {
                    evidence: b"server listener remains owned by a live process".to_vec(),
                };
            }
            Err(error) => return restart_ambiguous(error.to_string()),
        };
        let records = match self.port_leases.list_plan(validated.network_plan.plan_id()) {
            Ok(records) => records,
            Err(error) => return restart_ambiguous(error.to_string()),
        };
        if records.iter().any(|record| {
            requests.contains(record.request()) && record.phase() != PortLeasePhase::CleanupPending
        }) && let Err(error) = self
            .port_leases
            .mark_cleanup_pending_plan_members_after_owner_death(
                &plan_members,
                &requests,
                &recoveries,
            )
        {
            return restart_ambiguous(error.to_string());
        }
        if let Err(error) = self
            .port_leases
            .prepare_rebind_process_bound_plan_members_after_owner_death(
                &plan_members,
                &requests,
                &recoveries,
            )
        {
            return restart_ambiguous(error.to_string());
        }
        drop(recoveries);
        match self.inspect_retained_restart_listeners(validated) {
            Ok(true) => restart_withdrawal_succeeded(validated),
            Ok(false) => restart_ambiguous(
                "server listener recovery did not produce durable restart-retention evidence",
            ),
            Err(observation) => observation,
        }
    }

    fn inspect_retained_restart_listeners(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> Result<bool, ProviderRestartEffectObservation> {
        let (_, requests) = self.restart_listener_plan(validated)?;
        if requests.is_empty() {
            return Ok(true);
        }
        let records = self
            .port_leases
            .list_plan(validated.network_plan.plan_id())
            .map_err(|error| restart_ambiguous(error.to_string()))?;
        let plan_members = records
            .iter()
            .map(|record| record.request().clone())
            .collect::<Vec<_>>();
        let mut exact = Vec::with_capacity(requests.len());
        for request in &requests {
            exact.push(
                self.port_leases
                    .inspect_plan_member(&plan_members, request)
                    .map_err(|error| restart_definite_failure(error.to_string()))?,
            );
        }
        Ok(exact.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.bind_claim().is_none()
                && record.active_lifetime().is_none()
                && record.confirmed_stopped_binding().is_some()
        }))
    }

    fn restart_listener_plan(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> Result<(Vec<PortLeaseRequest>, Vec<PortLeaseRequest>), ProviderRestartEffectObservation>
    {
        let records = self
            .port_leases
            .list_plan(validated.network_plan.plan_id())
            .map_err(|error| restart_ambiguous(error.to_string()))?;
        let plan_members = records
            .iter()
            .map(|record| record.request().clone())
            .collect::<Vec<_>>();
        let requests = validated
            .network_plan
            .listeners()
            .iter()
            .map(|listener| listener.port_lease().clone())
            .collect::<Vec<_>>();
        for request in &requests {
            self.port_leases
                .inspect_plan_member(&plan_members, request)
                .map_err(|error| restart_definite_failure(error.to_string()))?;
        }
        Ok((plan_members, requests))
    }

    fn publish_restart_publication(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> ProviderRestartEffectObservation {
        let source = self.inspect_restart_source(validated);
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return restart_ambiguous("server ingress registry lock is poisoned"),
        };
        if running.contains_key(&validated.source_key) {
            return restart_definite_failure(
                "source publication remains routable before restart publication",
            );
        }
        if let Some(existing) = running.get(&validated.target_key) {
            return classify_existing_restart_publication(
                existing,
                &validated.target_key.execution_id,
                source,
            );
        }
        let targets = match source {
            Ok(targets) => targets,
            Err(observation) => return observation,
        };
        let batch = match RunningIngressBatch::start(
            &self.listeners,
            &validated.target_key.execution_id,
            &targets,
            validated.publication.clone(),
        ) {
            Ok(batch) => batch,
            Err(error) => return restart_bind_error(error),
        };
        let evidence = batch.evidence();
        running.insert(validated.target_key.clone(), batch);
        ProviderRestartEffectObservation::Succeeded { evidence }
    }

    fn inspect_restart_publication(
        &self,
        validated: &ValidatedRestartPublication,
        allow_absence: bool,
    ) -> ProviderRestartEffectObservation {
        let source = self.inspect_restart_source(validated);
        let running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return restart_ambiguous("server ingress registry lock is poisoned"),
        };
        if running.contains_key(&validated.source_key) {
            return restart_definite_failure(
                "source publication remains routable during target publication inspection",
            );
        }
        match (running.get(&validated.target_key), source) {
            (Some(batch), Ok(targets))
                if batch.matches(&validated.target_key.execution_id, &targets)
                    && batch.is_healthy() =>
            {
                ProviderRestartEffectObservation::Succeeded {
                    evidence: batch.evidence(),
                }
            }
            (Some(batch), Err(ProviderRestartEffectObservation::Absent { .. }))
                if batch.is_healthy() =>
            {
                ProviderRestartEffectObservation::Succeeded {
                    evidence: batch.evidence(),
                }
            }
            (Some(_), _) => {
                restart_ambiguous("restart ingress worker state is unhealthy or crossed")
            }
            (None, Ok(targets)) if allow_absence => ProviderRestartEffectObservation::Absent {
                evidence: source_evidence("server_restart_ingress_absent", &targets),
            },
            (None, Ok(targets)) => ProviderRestartEffectObservation::InProgress {
                evidence: source_evidence("server_restart_ingress_not_observed", &targets),
            },
            (None, Err(observation)) => observation,
        }
    }

    fn inspect_restart_source(
        &self,
        validated: &ValidatedRestartPublication,
    ) -> Result<SandboxProvisionIngressTargets, ProviderRestartEffectObservation> {
        match self.source.inspect_targets(
            &validated.sandbox_id,
            validated.attempt_fence.attempt_id(),
            &validated.network_plan,
        ) {
            Ok(SandboxProvisionIngressTargetObservation::Ready { targets, .. }) => Ok(targets),
            Ok(SandboxProvisionIngressTargetObservation::Absent { evidence }) => {
                Err(ProviderRestartEffectObservation::Absent { evidence })
            }
            Ok(SandboxProvisionIngressTargetObservation::InProgress { evidence }) => {
                Err(ProviderRestartEffectObservation::InProgress { evidence })
            }
            Err(SandboxError::InvalidSpec { message }) => Err(restart_definite_failure(message)),
            Err(error) => Err(restart_ambiguous(error.to_string())),
        }
    }
}

fn classify_existing_publication(
    existing: &RunningIngressBatch,
    execution_id: &str,
    source: Result<SandboxProvisionIngressTargets, ProviderProvisionEffectObservation>,
) -> ProviderProvisionEffectObservation {
    if !existing.is_healthy() {
        return ambiguous("server ingress worker state is unhealthy");
    }
    match source {
        Ok(targets) if existing.matches(execution_id, &targets) => succeeded(existing.evidence()),
        Ok(_) => definite_failure(
            "server_ingress_replay_mismatch",
            "the exact publication attempt is associated with different private-route authority",
        ),
        Err(
            ProviderProvisionEffectObservation::Absent { .. }
            | ProviderProvisionEffectObservation::InProgress { .. },
        ) => succeeded(existing.evidence()),
        Err(observation @ ProviderProvisionEffectObservation::Ambiguous { .. })
        | Err(observation @ ProviderProvisionEffectObservation::DefiniteFailure { .. }) => {
            observation
        }
        Err(ProviderProvisionEffectObservation::Succeeded { .. }) => {
            ambiguous("server ingress source returned an invalid nested success observation")
        }
    }
}

fn classify_existing_restart_publication(
    existing: &RunningIngressBatch,
    execution_id: &str,
    source: Result<SandboxProvisionIngressTargets, ProviderRestartEffectObservation>,
) -> ProviderRestartEffectObservation {
    if !existing.is_healthy() {
        return restart_ambiguous("restart ingress worker state is unhealthy");
    }
    match source {
        Ok(targets) if existing.matches(execution_id, &targets) => {
            ProviderRestartEffectObservation::Succeeded {
                evidence: existing.evidence(),
            }
        }
        Ok(_) => restart_definite_failure(
            "the exact restart publication is associated with different private-route authority",
        ),
        Err(
            ProviderRestartEffectObservation::Absent { .. }
            | ProviderRestartEffectObservation::InProgress { .. },
        ) => ProviderRestartEffectObservation::Succeeded {
            evidence: existing.evidence(),
        },
        Err(observation @ ProviderRestartEffectObservation::Ambiguous { .. })
        | Err(observation @ ProviderRestartEffectObservation::DefiniteFailure { .. }) => {
            observation
        }
        Err(ProviderRestartEffectObservation::Succeeded { .. }) => {
            restart_ambiguous("server ingress source returned an invalid nested restart success")
        }
    }
}

impl IngressPublicationCapability for ServerIngressPublicationAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = self.validate(command);
            self.phases.execute(command, || match validated {
                Ok(validated) => self.publish(&validated),
                Err(error) => error,
            })
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = self.validate(command);
            self.phases.inspect_live(command, || match validated {
                Ok(validated) => self.inspect(&validated, true),
                Err(error) => error,
            })
        })
    }
}

impl IngressPublicationInspectionCapability for ServerIngressPublicationAdapter {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = self.validate(command);
            self.phases.inspect_live(command, || match validated {
                Ok(validated) => self.inspect(&validated, false),
                Err(error) => error,
            })
        })
    }
}

impl RestartPublicationWithdrawalCapability for ServerIngressPublicationAdapter {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let validated = self.validate_restart(command);
        let observation = self.restart_phases.execute(command, || match validated {
            Ok(validated) => self.withdraw_restart_publication(&validated),
            Err(observation) => observation,
        });
        Box::pin(std::future::ready(observation))
    }

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let validated = self.validate_restart(command);
        let observation = self.restart_phases.inspect(command, || match validated {
            Ok(validated) => self.inspect_restart_withdrawal(&validated),
            Err(observation) => observation,
        });
        Box::pin(std::future::ready(observation))
    }
}

impl RestartPublicationCapability for ServerIngressPublicationAdapter {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let validated = self.validate_restart(command);
        let observation = self.restart_phases.execute(command, || match validated {
            Ok(validated) => self.publish_restart_publication(&validated),
            Err(observation) => observation,
        });
        Box::pin(std::future::ready(observation))
    }

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let validated = self.validate_restart(command);
        let observation = self
            .restart_phases
            .inspect_live(command, || match validated {
                Ok(validated) => self.inspect_restart_publication(&validated, true),
                Err(observation) => observation,
            });
        Box::pin(std::future::ready(observation))
    }
}

impl RestartPublicationObservationCapability for ServerIngressPublicationAdapter {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let validated = self.validate_restart(command);
        let observation = self
            .restart_phases
            .inspect_live(command, || match validated {
                Ok(validated) => self.inspect_restart_publication(&validated, false),
                Err(observation) => observation,
            });
        Box::pin(std::future::ready(observation))
    }
}

impl WorkloadIngressObservationCapability for ServerIngressPublicationAdapter {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async move {
            let Some(query) = LiveIngressObservationQuery::authenticate(request) else {
                return WorkloadProviderObservation::Ambiguous;
            };
            self.observe_live_publication(&query)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveIngressListenerExpectation {
    endpoint_id: PublishedEndpointId,
    listener_id: ListenerId,
    port_lease_id: PortLeaseId,
    desired_host_address: std::net::IpAddr,
}

/// Exact immutable comparison input authenticated from compute's request.
///
/// Tests construct this closed view directly because the public compute
/// request intentionally has no caller-visible constructor. Production always
/// enters through [`LiveIngressObservationQuery::authenticate`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveIngressObservationQuery {
    saga_id: String,
    execution_id: String,
    attempt_id: String,
    tenant_id: nimbus_core::TenantId,
    plan_id: NetworkPlanId,
    plan_digest: NetworkPlanDigest,
    generation: NetworkResourceGeneration,
    listeners: BTreeMap<ListenerId, LiveIngressListenerExpectation>,
}

impl LiveIngressObservationQuery {
    fn authenticate(request: &WorkloadIngressObservationRequest) -> Option<Self> {
        let plan = request.compiled_plan();
        let content = plan.content();
        let publication = request.publication();
        let network = publication.network();
        let identity = content.identity();
        if content.publication() != WorkloadPublicationIntent::PublishWhenReady
            || request.key().tenant_id() != identity.tenant_id()
            || request.execution().generation().as_u64() != identity.generation().as_u64()
            || network.plan_id() != plan.plan().plan_id()
            || network.digest() != plan.plan().digest()
            || network.generation() != identity.generation()
            || plan.plan().generation() != identity.generation()
            || content.capability_selection().is_none_or(|selection| {
                selection.ingress_provider_id() != &nimbus_owned_local_ingress_provider_id()
            })
        {
            return None;
        }

        let expected_endpoint_ids = publication
            .endpoints()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if expected_endpoint_ids.is_empty()
            || expected_endpoint_ids.len() != publication.endpoints().len()
            || expected_endpoint_ids.len() != content.listeners().len()
        {
            return None;
        }
        let mut listeners = BTreeMap::new();
        for listener in content.listeners() {
            if !expected_endpoint_ids.contains(listener.endpoint_id())
                || listeners
                    .insert(
                        listener.listener_id().clone(),
                        LiveIngressListenerExpectation {
                            endpoint_id: listener.endpoint_id().clone(),
                            listener_id: listener.listener_id().clone(),
                            port_lease_id: listener.port_lease_id().clone(),
                            desired_host_address: listener.desired_host_address(),
                        },
                    )
                    .is_some()
            {
                return None;
            }
        }
        Some(Self {
            saga_id: request.key().saga_id().as_str().to_owned(),
            execution_id: request.execution().execution_id().as_str().to_owned(),
            attempt_id: request.execution().attempt_id().as_str().to_owned(),
            tenant_id: request.key().tenant_id().clone(),
            plan_id: plan.plan().plan_id().clone(),
            plan_digest: plan.plan().digest(),
            generation: identity.generation(),
            listeners,
        })
    }
}

impl ServerIngressPublicationAdapter {
    /// Observe only the server-owned in-memory listener batch. This method has
    /// no path to source inspection, phase journals, durable lease mutation,
    /// repair, restart, bind, or target reconstruction.
    fn observe_live_publication(
        &self,
        query: &LiveIngressObservationQuery,
    ) -> WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>> {
        let running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return WorkloadProviderObservation::Ambiguous,
        };
        let same_saga = running
            .iter()
            .filter(|(key, _)| key.saga_id == query.saga_id)
            .collect::<Vec<_>>();
        if same_saga.is_empty() {
            return WorkloadProviderObservation::InProgress;
        }
        let mut matches = same_saga.iter().copied().filter(|(key, _)| {
            key.execution_id == query.execution_id
                && key.attempt_id == query.attempt_id
                && key.generation == query.generation.as_u64()
                && key.network_plan_digest == query.plan_digest.to_string()
        });
        let Some((_, batch)) = matches.next() else {
            return WorkloadProviderObservation::Ambiguous;
        };
        if same_saga.len() != 1 || matches.next().is_some() || !batch.is_healthy() {
            return WorkloadProviderObservation::Ambiguous;
        }
        match batch.observed_endpoints(query) {
            Some(endpoints) => WorkloadProviderObservation::Present(endpoints),
            None => WorkloadProviderObservation::Ambiguous,
        }
    }
}

struct ValidatedPublication {
    key: PublicationKey,
    sandbox_id: nimbus_sandbox::SandboxId,
    execution_attempt_id: nimbus_sandbox::SandboxExecutionAttemptId,
    network_plan: SandboxProvisionNetworkPlan,
    publication: PublishedIngressAuthority,
}

struct ValidatedRestartPublication {
    source_key: PublicationKey,
    target_key: PublicationKey,
    sandbox_id: nimbus_sandbox::SandboxId,
    attempt_fence: nimbus_sandbox::SandboxRestartAttemptFence,
    network_plan: SandboxProvisionNetworkPlan,
    publication: PublishedIngressAuthority,
}

impl ValidatedRestartPublication {
    fn new(
        command: &ConfirmedWorkloadRestartCommand,
        validated: ValidatedSandboxRestartCommand,
    ) -> Option<Self> {
        let key = |attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId| PublicationKey {
            saga_id: command.saga_id().as_str().to_owned(),
            attempt_id: attempt_id.as_str().to_owned(),
            execution_id: validated.sandbox_id().as_str().to_owned(),
            generation: command.generation().as_u64(),
            network_plan_digest: command.network_plan_digest().to_string(),
        };
        let selection = command
            .compiled_network_plan()
            .content()
            .capability_selection_evidence()?;
        let reference = command.publication_reference()?.clone();
        Some(Self {
            source_key: key(validated.attempt_fence().source_attempt_id()),
            target_key: key(validated.attempt_fence().attempt_id()),
            sandbox_id: validated.sandbox_id().clone(),
            attempt_fence: validated.attempt_fence().clone(),
            network_plan: validated.network_plan().clone(),
            publication: PublishedIngressAuthority::new(
                reference,
                selection.source_digest(),
                command.source_digest(),
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedRoute {
    listener_id: nimbus_network::ListenerId,
    request: nimbus_network::PortLeaseRequest,
    upstream: SocketAddr,
}

impl From<&SandboxProvisionIngressRoute> for ExpectedRoute {
    fn from(route: &SandboxProvisionIngressRoute) -> Self {
        Self {
            listener_id: route.listener_id().clone(),
            request: route.port_lease().clone(),
            upstream: route.upstream(),
        }
    }
}

struct RunningIngressBatch {
    execution_id: String,
    tenant_id: nimbus_core::TenantId,
    plan_id: nimbus_network::NetworkPlanId,
    generation: nimbus_network::NetworkResourceGeneration,
    attachment_id: nimbus_network::NetworkAttachmentId,
    plan_members: Vec<nimbus_network::PortLeaseRequest>,
    routes: Vec<RunningIngressRoute>,
    publication: PublishedIngressAuthority,
    final_phase: FinalIngressPhase,
}

impl RunningIngressBatch {
    fn start(
        authority: &ServerListenerLeaseAuthority,
        execution_id: &str,
        targets: &SandboxProvisionIngressTargets,
        publication: PublishedIngressAuthority,
    ) -> io::Result<Self> {
        let ingress_requests = targets
            .routes()
            .iter()
            .map(|target| target.port_lease().clone())
            .collect::<Vec<_>>();
        let plan_members = authority.authenticate_workload_ingress_plan(
            targets.plan_id(),
            targets.tenant_id(),
            targets.generation(),
            &ingress_requests,
            targets.reservation_claim(),
        )?;
        let mut routes = Vec::with_capacity(targets.routes().len());
        for target in targets.routes() {
            let expected = ExpectedRoute::from(target);
            let prepared = authority.prepare_workload_ingress(
                Some(&plan_members),
                expected.request.clone(),
                targets.reservation_claim(),
            )?;
            let bind_addr = prepared.bind_addr()?;
            let listener = match std::net::TcpListener::bind(bind_addr) {
                Ok(listener) => listener,
                Err(error) => return Err(prepared.record_bind_failure(error)?.into_error()),
            };
            let adopted = prepared.adopt_std(listener)?;
            routes.push(RunningIngressRoute::start(
                expected,
                adopted,
                DEFAULT_MAX_ACTIVE_CONNECTIONS,
            )?);
        }
        Ok(Self {
            execution_id: execution_id.to_owned(),
            tenant_id: targets.tenant_id().clone(),
            plan_id: targets.plan_id().clone(),
            generation: targets.generation(),
            attachment_id: targets.attachment_id().clone(),
            plan_members,
            routes,
            publication,
            final_phase: FinalIngressPhase::Published,
        })
    }

    fn matches_plan(&self, execution_id: &str, plan: &SandboxProvisionNetworkPlan) -> bool {
        self.execution_id == execution_id
            && self.tenant_id == *plan.tenant_id()
            && self.plan_id == *plan.plan_id()
            && self.generation == plan.generation()
            && self.attachment_id == *plan.attachment_id()
            && self.routes.len() == plan.listeners().len()
            && self.routes.iter().all(|route| {
                plan.listeners().iter().any(|listener| {
                    route.expected.listener_id == *listener.listener_id()
                        && route.expected.request == *listener.port_lease()
                })
            })
    }

    fn stop_and_retain_for_restart(mut self) -> io::Result<Vec<u8>> {
        if !self.is_healthy() {
            return Err(io::Error::other(
                "cannot retain an unhealthy workload ingress batch for restart",
            ));
        }
        let mut stopping = Vec::with_capacity(self.routes.len());
        for route in &mut self.routes {
            stopping.push(route.take_for_restart().ok_or_else(|| {
                io::Error::other(
                    "workload ingress route lost its listener ownership before restart",
                )
            })?);
        }
        let retained = stop_and_retain_server_listeners_for_restart(&self.plan_members, stopping)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let leases = retained
            .records()
            .iter()
            .map(|record| record.request().lease_id().to_string())
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            "tenant={};plan={};generation={};attachment={};restart_retained={leases}",
            self.tenant_id,
            self.plan_id,
            self.generation.as_u64(),
            self.attachment_id
        )
        .into_bytes())
    }

    fn matches(&self, execution_id: &str, targets: &SandboxProvisionIngressTargets) -> bool {
        self.execution_id == execution_id
            && self.tenant_id == *targets.tenant_id()
            && self.plan_id == *targets.plan_id()
            && self.generation == targets.generation()
            && self.attachment_id == *targets.attachment_id()
            && self.routes.len() == targets.routes().len()
            && self
                .routes
                .iter()
                .zip(targets.routes())
                .all(|(running, target)| running.expected == ExpectedRoute::from(target))
    }

    fn is_healthy(&self) -> bool {
        self.routes.iter().all(RunningIngressRoute::is_healthy)
    }

    fn evidence(&self) -> Vec<u8> {
        let routes = self
            .routes
            .iter()
            .map(|route| {
                format!(
                    "{}|{}|{}|{}",
                    route.expected.listener_id,
                    route.expected.request.lease_id(),
                    route.bound_addr,
                    route.expected.upstream
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        format!(
            "tenant={};plan={};generation={};attachment={};routes={routes}",
            self.tenant_id,
            self.plan_id,
            self.generation.as_u64(),
            self.attachment_id
        )
        .into_bytes()
    }

    fn observed_endpoints(
        &self,
        query: &LiveIngressObservationQuery,
    ) -> Option<Vec<WorkloadObservedIngressEndpoint>> {
        if self.tenant_id != query.tenant_id
            || self.execution_id != query.execution_id
            || self.plan_id != query.plan_id
            || self.generation != query.generation
            || self.routes.len() != query.listeners.len()
        {
            return None;
        }
        let mut seen = BTreeSet::new();
        let mut endpoints = Vec::with_capacity(self.routes.len());
        for route in &self.routes {
            let expected = query.listeners.get(&route.expected.listener_id)?;
            if !seen.insert(route.expected.listener_id.clone())
                || expected.listener_id != route.expected.listener_id
            {
                return None;
            }
            let evidence = route.lease.as_ref()?.observation_evidence()?;
            let request = evidence.request();
            if request != &route.expected.request
                || request.lease_id() != &expected.port_lease_id
                || request.owner_id() != &NetworkResourceId::from(expected.listener_id.clone())
                || request.plan_id() != Some(&query.plan_id)
                || request.tenant_id() != Some(&query.tenant_id)
                || request.generation() != query.generation
                || request.accounting() != PortLeaseAccounting::TenantPublished
                || request.publication().host_address() != Some(expected.desired_host_address)
                || request.binding().protocol() != PortProtocol::Tcp
                || request.binding().realm() != &PortBindRealm::Host
                || route.bound_addr.port() == 0
                || route.bound_addr.ip().is_unspecified()
                || evidence.bound_endpoint().protocol() != PortProtocol::Tcp
                || evidence.bound_endpoint().realm() != &PortBindRealm::Host
                || evidence.bound_endpoint().port().get() != route.bound_addr.port()
                || evidence.bound_endpoint().target().specific_address()
                    != Some(route.bound_addr.ip())
            {
                return None;
            }
            endpoints.push(WorkloadObservedIngressEndpoint::new(
                expected.endpoint_id.clone(),
                route.bound_addr,
                WorkloadIngressBindingWitness::new(
                    query.plan_id.clone(),
                    query.plan_digest,
                    query.generation,
                    expected.listener_id.clone(),
                    expected.port_lease_id.clone(),
                    evidence.lifetime(),
                    evidence.lifetime(),
                    evidence.bound_endpoint().clone(),
                    evidence.provenance(),
                ),
            ));
        }
        if seen.len() != query.listeners.len() {
            return None;
        }
        endpoints.sort_by(|left, right| left.endpoint_id().cmp(right.endpoint_id()));
        Some(endpoints)
    }
}

fn source_evidence(label: &str, targets: &SandboxProvisionIngressTargets) -> Vec<u8> {
    format!(
        "{label}:{}:{}:{}:{}",
        targets.tenant_id(),
        targets.plan_id(),
        targets.generation().as_u64(),
        targets.attachment_id()
    )
    .into_bytes()
}

fn succeeded(evidence: Vec<u8>) -> ProviderProvisionEffectObservation {
    ProviderProvisionEffectObservation::Succeeded { evidence }
}

fn ambiguous(evidence: impl Into<Vec<u8>>) -> ProviderProvisionEffectObservation {
    ProviderProvisionEffectObservation::Ambiguous {
        evidence: evidence.into(),
    }
}

fn definite_failure(
    code: &str,
    evidence: impl Into<Vec<u8>>,
) -> ProviderProvisionEffectObservation {
    ProviderProvisionEffectObservation::DefiniteFailure {
        code: code.to_owned(),
        evidence: evidence.into(),
    }
}

fn bind_error(error: io::Error) -> ProviderProvisionEffectObservation {
    match error.kind() {
        io::ErrorKind::AddrInUse
        | io::ErrorKind::PermissionDenied
        | io::ErrorKind::AddrNotAvailable
        | io::ErrorKind::InvalidInput
        | io::ErrorKind::NotFound => {
            definite_failure("server_ingress_bind_rejected", error.to_string())
        }
        _ => ambiguous(error.to_string()),
    }
}

fn restart_ambiguous(evidence: impl Into<Vec<u8>>) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::Ambiguous {
        evidence: evidence.into(),
    }
}

fn restart_withdrawal_succeeded(
    validated: &ValidatedRestartPublication,
) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::Succeeded {
        evidence: format!(
            "withdrawn:{}:{}:{}",
            validated.source_key.saga_id,
            validated.source_key.attempt_id,
            validated.source_key.network_plan_digest
        )
        .into_bytes(),
    }
}

fn restart_definite_failure(evidence: impl Into<Vec<u8>>) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::DefiniteFailure {
        evidence: evidence.into(),
    }
}

fn restart_bind_error(error: io::Error) -> ProviderRestartEffectObservation {
    match error.kind() {
        io::ErrorKind::AddrInUse
        | io::ErrorKind::PermissionDenied
        | io::ErrorKind::AddrNotAvailable
        | io::ErrorKind::InvalidInput
        | io::ErrorKind::NotFound => restart_definite_failure(error.to_string()),
        _ => restart_ambiguous(error.to_string()),
    }
}

#[cfg(test)]
#[path = "workload_ingress/tests.rs"]
mod tests;
