//! Server-owned deferred ingress publication for private sandbox attachments.
//!
//! The compute dispatcher supplies exact fenced commands. Sandbox providers
//! expose only authenticated private routes. This module owns the real host
//! TCP listeners and transparent byte forwarding; it does not terminate TLS,
//! resolve service names, decide tenant policy, or mutate attachment state.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
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
    ListenerId, LocalNetworkAuthority, NetworkCapabilityRole, NetworkPlanDigest, NetworkPlanId,
    NetworkResourceGeneration, NetworkResourceId, PortBindRealm, PortLeaseAccounting, PortLeaseId,
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

use crate::listener_lease::{
    ActiveServerListenerLease, RestartStoppingServerListener, ServerListenerLeaseAuthority,
    stop_and_retain_server_listeners_for_restart,
};
use crate::network_capabilities::nimbus_owned_local_ingress_provider_id;

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
        Ok(Self {
            source,
            phases: ProviderProvisionPhaseAdapter::new(journal.clone()),
            restart_phases: ProviderRestartPhaseAdapter::new(journal),
            listeners: ServerListenerLeaseAuthority::new(network_authority),
            running: Mutex::new(BTreeMap::new()),
        })
    }

    fn validate(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> Result<ValidatedPublication, ProviderProvisionEffectObservation> {
        match command.provider_target() {
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Ingress,
                provider_id,
                ..
            } if provider_id == &nimbus_owned_local_ingress_provider_id() => {}
            _ => {
                return Err(definite_failure(
                    "server_ingress_provider_mismatch",
                    "publication command does not target Nimbus-owned local ingress",
                ));
            }
        }
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
        Ok(ValidatedRestartPublication::new(command, validated))
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
            return self.inspect_restart_withdrawal_locked(&running, validated);
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
}

struct ValidatedRestartPublication {
    source_key: PublicationKey,
    target_key: PublicationKey,
    sandbox_id: nimbus_sandbox::SandboxId,
    attempt_fence: nimbus_sandbox::SandboxRestartAttemptFence,
    network_plan: SandboxProvisionNetworkPlan,
}

impl ValidatedRestartPublication {
    fn new(
        command: &ConfirmedWorkloadRestartCommand,
        validated: ValidatedSandboxRestartCommand,
    ) -> Self {
        let key = |attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId| PublicationKey {
            saga_id: command.saga_id().as_str().to_owned(),
            attempt_id: attempt_id.as_str().to_owned(),
            execution_id: validated.sandbox_id().as_str().to_owned(),
            generation: command.generation().as_u64(),
            network_plan_digest: command.network_plan_digest().to_string(),
        };
        Self {
            source_key: key(validated.attempt_fence().source_attempt_id()),
            target_key: key(validated.attempt_fence().attempt_id()),
            sandbox_id: validated.sandbox_id().clone(),
            attempt_fence: validated.attempt_fence().clone(),
            network_plan: validated.network_plan().clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PublicationKey {
    saga_id: String,
    attempt_id: String,
    execution_id: String,
    generation: u64,
    network_plan_digest: String,
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
}

impl RunningIngressBatch {
    fn start(
        authority: &ServerListenerLeaseAuthority,
        execution_id: &str,
        targets: &SandboxProvisionIngressTargets,
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

struct RunningIngressRoute {
    expected: ExpectedRoute,
    bound_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    #[cfg(test)]
    active_connections: Arc<AtomicUsize>,
    #[cfg(test)]
    peak_connections: Arc<AtomicUsize>,
    #[cfg(test)]
    rejected_connections: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
    lease: Option<ActiveServerListenerLease>,
}

impl RunningIngressRoute {
    fn start(
        expected: ExpectedRoute,
        listener: crate::PreboundServerListener,
        max_active_connections: usize,
    ) -> io::Result<Self> {
        if max_active_connections == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload ingress connection limit must be greater than zero",
            ));
        }
        let bound_addr = listener.local_addr()?;
        let (listener, lease, _) = listener.into_std_parts();
        if let Err(error) = listener.set_nonblocking(true) {
            drop(listener);
            return match lease.settle_after_confirmed_local_close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(io::Error::new(
                    error.kind(),
                    format!("{error}; failed to settle listener: {cleanup}"),
                )),
            };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let active_connections = Arc::new(AtomicUsize::new(0));
        let peak_connections = Arc::new(AtomicUsize::new(0));
        let rejected_connections = Arc::new(AtomicUsize::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_failed = Arc::clone(&failed);
        let worker_active = Arc::clone(&active_connections);
        let worker_peak = Arc::clone(&peak_connections);
        let worker_rejected = Arc::clone(&rejected_connections);
        let upstream = expected.upstream;
        let name = format!("nimbus-ingress-{}", expected.listener_id);
        let worker = thread::Builder::new().name(name).spawn(move || {
            let mut connections = Vec::new();
            while !worker_stop.load(Ordering::Acquire) {
                if reap_finished_connections(&mut connections) {
                    worker_failed.store(true, Ordering::Release);
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let Some(permit) = ConnectionPermit::try_acquire(
                            Arc::clone(&worker_active),
                            Arc::clone(&worker_peak),
                            max_active_connections,
                        ) else {
                            worker_rejected.fetch_add(1, Ordering::AcqRel);
                            drop(stream);
                            continue;
                        };
                        let connection_stop = Arc::clone(&worker_stop);
                        match thread::Builder::new()
                            .name("nimbus-ingress-connection".to_owned())
                            .spawn(move || {
                                let _permit = permit;
                                proxy_connection(stream, upstream, &connection_stop);
                            }) {
                            Ok(connection) => connections.push(connection),
                            Err(_) => {
                                worker_failed.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_RETRY_DELAY);
                    }
                    Err(_) => {
                        worker_failed.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            for connection in connections {
                if connection.join().is_err() {
                    worker_failed.store(true, Ordering::Release);
                }
            }
        })?;
        Ok(Self {
            expected,
            bound_addr,
            stop,
            failed,
            #[cfg(test)]
            active_connections,
            #[cfg(test)]
            peak_connections,
            #[cfg(test)]
            rejected_connections,
            worker: Some(worker),
            lease: Some(lease),
        })
    }

    fn is_healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
            && self.lease.is_some()
    }

    fn take_for_restart(&mut self) -> Option<RestartStoppingServerListener> {
        let lease = self.lease.take()?;
        let worker = self.worker.take()?;
        let stop = Arc::clone(&self.stop);
        Some(RestartStoppingServerListener::new(lease, move || {
            stop.store(true, Ordering::Release);
            worker.join().map_err(|_| {
                io::Error::other("workload ingress listener worker panicked during restart stop")
            })
        }))
    }

    fn stop_and_settle(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(lease) = self.lease.take()
            && let Err(error) = lease.settle_after_confirmed_local_close()
        {
            tracing::error!(%error, "failed to settle workload ingress listener");
        }
    }
}

struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl ConnectionPermit {
    fn try_acquire(active: Arc<AtomicUsize>, peak: Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        let observed = active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < limit).then_some(count + 1)
            })
            .ok()?;
        let current = observed + 1;
        peak.fetch_max(current, Ordering::AcqRel);
        Some(Self { active })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Join completed route-owned workers before removing their handles.
///
/// Each outer worker owns and joins its one bidirectional-copy child, so this
/// route-level vector is the transitive ownership root for every thread that
/// can touch the listener lease.
fn reap_finished_connections(connections: &mut Vec<JoinHandle<()>>) -> bool {
    let mut panicked = false;
    let mut index = 0;
    while index < connections.len() {
        if connections[index].is_finished() {
            let connection = connections.swap_remove(index);
            panicked |= connection.join().is_err();
        } else {
            index += 1;
        }
    }
    panicked
}

impl Drop for RunningIngressRoute {
    fn drop(&mut self) {
        self.stop_and_settle();
    }
}

fn proxy_connection(inbound: std::net::TcpStream, upstream: SocketAddr, stop: &Arc<AtomicBool>) {
    let Ok(outbound) = std::net::TcpStream::connect_timeout(&upstream, UPSTREAM_CONNECT_TIMEOUT)
    else {
        return;
    };
    for stream in [&inbound, &outbound] {
        let _ = stream.set_read_timeout(Some(CONNECTION_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT));
    }
    let (Ok(inbound_read), Ok(outbound_write)) = (inbound.try_clone(), outbound.try_clone()) else {
        return;
    };
    // This child is never detached: its route-owned outer worker blocks on
    // `join` before returning, and listener settlement joins that outer worker.
    let request_stop = Arc::clone(stop);
    let forward = thread::spawn(move || {
        copy_until_stopped(inbound_read, outbound_write, &request_stop);
    });
    copy_until_stopped(outbound, inbound, stop);
    let _ = forward.join();
}

fn copy_until_stopped(
    mut reader: std::net::TcpStream,
    mut writer: std::net::TcpStream,
    stop: &AtomicBool,
) {
    let mut buffer = [0_u8; 16 * 1024];
    while !stop.load(Ordering::Acquire) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if !write_all_until_stopped(&mut writer, &buffer[..count], stop) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let _ = writer.shutdown(Shutdown::Write);
}

fn write_all_until_stopped(
    writer: &mut std::net::TcpStream,
    bytes: &[u8],
    stop: &AtomicBool,
) -> bool {
    let mut offset = 0;
    while offset < bytes.len() && !stop.load(Ordering::Acquire) {
        match writer.write(&bytes[offset..]) {
            Ok(0) => return false,
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    }
    offset == bytes.len()
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
