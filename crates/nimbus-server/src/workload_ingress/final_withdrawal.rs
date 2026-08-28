//! Final ingress withdrawal for one exact published workload identity.
//!
//! This adapter validates the complete compute-confirmed command before it
//! mutates durable lease state. Execute fences the exact listener subset,
//! stops and joins its transitive process-bound effects, then releases it.
//! Inspect only reads synchronized live ownership and durable lease evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use nimbus_compute::workload_saga::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};
use nimbus_network::{
    ListenerId, NetworkCapabilitySourceDigest, NetworkResourceId, PortLeaseAccounting,
    PortLeaseError, PortLeaseId, PortLeasePhase, PortLeaseRecord, PortLeaseRequest,
    PublishedEndpointId,
};
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest, WorkloadProvisionSourceDigest,
    WorkloadPublicationReference, WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget,
    WorkloadTeardownStep, WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::route_workers::{RunningIngressRoute, cancel_and_join_ingress_workers};
use super::{
    RunningIngressBatch, ServerIngressPublicationAdapter, nimbus_owned_local_ingress_provider_id,
};
use crate::listener_lease::{
    TerminalStoppingServerListener, settle_exact_listener_leases,
    stop_server_listeners_for_final_withdrawal, withdraw_server_listeners_for_final_withdrawal,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PublicationKey {
    pub(super) saga_id: String,
    pub(super) attempt_id: String,
    pub(super) execution_id: String,
    pub(super) generation: u64,
    pub(super) network_plan_digest: String,
}

/// Exact portable publication and admitted ingress-source identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PublishedIngressAuthority {
    reference: Option<WorkloadPublicationReference>,
    provider_source_digest: Option<NetworkCapabilitySourceDigest>,
    workload_source_digest: Option<WorkloadProvisionSourceDigest>,
}

impl PublishedIngressAuthority {
    pub(super) fn new(
        reference: WorkloadPublicationReference,
        provider_source_digest: NetworkCapabilitySourceDigest,
        workload_source_digest: WorkloadProvisionSourceDigest,
    ) -> Self {
        Self {
            reference: Some(reference),
            provider_source_digest: Some(provider_source_digest),
            workload_source_digest: Some(workload_source_digest),
        }
    }

    fn matches(
        &self,
        reference: &WorkloadPublicationReference,
        provider_source_digest: NetworkCapabilitySourceDigest,
        workload_source_digest: WorkloadProvisionSourceDigest,
    ) -> bool {
        self.reference.as_ref() == Some(reference)
            && self.provider_source_digest == Some(provider_source_digest)
            && self.workload_source_digest == Some(workload_source_digest)
    }

    #[cfg(test)]
    pub(super) const fn direct_fixture() -> Self {
        Self {
            reference: None,
            provider_source_digest: None,
            workload_source_digest: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FinalIngressPhase {
    Published,
    Withdrawing,
    Released,
}

struct ValidatedFinalWithdrawal {
    reference: WorkloadPublicationReference,
    provider_source_digest: NetworkCapabilitySourceDigest,
    tenant_id: nimbus_core::TenantId,
    exact_listener_leases: BTreeMap<PortLeaseId, ListenerId>,
}

enum FinalIngressReconciliationError {
    CrossedMembership(&'static str),
    Authority,
}

struct AuthenticatedDurableIngressPlan {
    plan_members: Vec<PortLeaseRequest>,
    ingress_records: Vec<PortLeaseRecord>,
}

enum AuthenticatedAbsentIngressInspection {
    Satisfied(Vec<PortLeaseRecord>),
    RetryRequired(Vec<PortLeaseRecord>),
    InProgress(Vec<PortLeaseRecord>),
}

impl From<PortLeaseError> for FinalIngressReconciliationError {
    fn from(_: PortLeaseError) -> Self {
        Self::Authority
    }
}

impl ServerIngressPublicationAdapter {
    fn validate_final_withdrawal(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        required_mode: WorkloadTeardownCommandMode,
    ) -> Result<ValidatedFinalWithdrawal, WorkloadFailureEvidence> {
        if command.mode() != required_mode
            || command.step() != WorkloadTeardownStep::WithdrawPublication
        {
            return Err(invalid_command_failure(
                "server final ingress command mode or step is crossed",
            ));
        }
        let WorkloadTeardownProviderTarget::Ingress {
            provider_id,
            provider_source_digest,
        } = command.provider_target()
        else {
            return Err(invalid_command_failure(
                "server final ingress command requires an ingress target",
            ));
        };
        let expected_provider = nimbus_owned_local_ingress_provider_id();
        if provider_id != &expected_provider {
            return Err(invalid_command_failure(
                "server final ingress provider ID is crossed",
            ));
        }
        let Some(selection) = command.selection_evidence() else {
            return Err(invalid_command_failure(
                "server final ingress command omits selection evidence",
            ));
        };
        if selection.selection().ingress_provider_id() != &expected_provider
            || selection.source_digest() != *provider_source_digest
        {
            return Err(invalid_command_failure(
                "server final ingress provider source is crossed",
            ));
        }
        let WorkloadTeardownSubjects::Publication(reference) = command.subjects() else {
            return Err(invalid_command_failure(
                "server final ingress command requires a publication subject",
            ));
        };
        if reference.network().digest() != command.network_plan_digest()
            || reference.network().generation().as_u64() != command.generation().as_u64()
            || reference.execution().generation() != command.generation()
            || reference.execution().desired_digest() != command.desired_digest()
            || reference.execution().node_identity() != command.required_node()
        {
            return Err(invalid_command_failure(
                "server final ingress publication fences are crossed",
            ));
        }
        let compiled_plan = command.compiled_network_plan();
        let content = compiled_plan.content();
        let identity = content.identity();
        if compiled_plan.plan().digest() != command.network_plan_digest()
            || compiled_plan.plan().plan_id() != reference.network().plan_id()
            || compiled_plan.plan().generation() != reference.network().generation()
            || identity.plan_id() != *reference.network().plan_id()
            || identity.tenant_id() != command.key().tenant_id()
            || identity.generation() != reference.network().generation()
        {
            return Err(invalid_command_failure(
                "server final ingress compiled plan is crossed with publication fences",
            ));
        }
        let expected_endpoints = reference
            .endpoints()
            .iter()
            .cloned()
            .collect::<BTreeSet<PublishedEndpointId>>();
        let compiled_endpoints = content
            .listeners()
            .iter()
            .map(|listener| listener.endpoint_id().clone())
            .collect::<BTreeSet<_>>();
        // A withheld or never-published workload has no public endpoints. The
        // exact empty membership remains authenticated by the compiled plan;
        // its attachment and internal PEP resources are released by later
        // teardown steps.
        let mut exact_listener_leases = BTreeMap::new();
        for listener in content.listeners() {
            if exact_listener_leases
                .insert(
                    listener.port_lease_id().clone(),
                    listener.listener_id().clone(),
                )
                .is_some()
            {
                return Err(invalid_command_failure(
                    "server final ingress compiled plan repeats a listener lease",
                ));
            }
        }
        if expected_endpoints != compiled_endpoints
            || exact_listener_leases.len() != content.listeners().len()
        {
            return Err(invalid_command_failure(
                "server final ingress endpoint membership is crossed with compiled listeners",
            ));
        }
        Ok(ValidatedFinalWithdrawal {
            reference: reference.clone(),
            provider_source_digest: *provider_source_digest,
            tenant_id: command.key().tenant_id().clone(),
            exact_listener_leases,
        })
    }

    fn execute_exact_final_withdrawal(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let validated =
            match self.validate_final_withdrawal(command, WorkloadTeardownCommandMode::Execute) {
                Ok(validated) => validated,
                Err(failure) => return execute_failure(command, failure),
            };
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return execute_ambiguous(command),
        };
        let matching = running
            .iter_mut()
            .filter(|(key, _)| key.saga_id == command.saga_id().as_str())
            .collect::<Vec<_>>();
        let exact_count = matching
            .iter()
            .filter(|(_, batch)| {
                batch.publication.matches(
                    &validated.reference,
                    validated.provider_source_digest,
                    command.source_digest(),
                )
            })
            .count();
        if exact_count > 1 {
            return execute_ambiguous(command);
        }
        if let Some((_, batch)) = matching.into_iter().find(|(_, batch)| {
            batch.publication.matches(
                &validated.reference,
                validated.provider_source_digest,
                command.source_digest(),
            )
        }) {
            let result = match batch.final_phase {
                FinalIngressPhase::Published => {
                    batch.stop_and_release_for_final_withdrawal().map(|_| ())
                }
                FinalIngressPhase::Withdrawing => self
                    .recover_dead_batch_after_final_withdrawal(batch)
                    .map(|_| ())
                    .map_err(|error| io::Error::other(error.to_string())),
                FinalIngressPhase::Released => Ok(()),
            };
            return match result {
                Ok(()) => execute_succeeded(command, &validated.reference, batch),
                Err(_) => execute_ambiguous(command),
            };
        }
        if running
            .keys()
            .any(|key| key.saga_id == command.saga_id().as_str())
        {
            return execute_failure(
                command,
                invalid_command_failure(
                    "live server ingress publication is crossed with final withdrawal",
                ),
            );
        }
        drop(running);
        match self.reconcile_absent_live_batch_after_owner_death(&validated) {
            Ok(records) => execute_succeeded_from_records(command, &validated.reference, &records),
            Err(FinalIngressReconciliationError::CrossedMembership(reason)) => {
                execute_failure(command, invalid_command_failure(reason))
            }
            Err(FinalIngressReconciliationError::Authority) => execute_ambiguous(command),
        }
    }

    fn inspect_exact_final_withdrawal(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let validated =
            match self.validate_final_withdrawal(command, WorkloadTeardownCommandMode::Inspect) {
                Ok(validated) => validated,
                Err(failure) => return inspect_failure(command, failure),
            };
        let mut running = match self.running.lock() {
            Ok(running) => running,
            Err(_) => return inspect_ambiguous(command),
        };
        let matching = running
            .iter()
            .filter(|(key, _)| key.saga_id == command.saga_id().as_str())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let exact = matching
            .iter()
            .filter(|key| {
                let batch = running
                    .get(key)
                    .expect("retained final ingress key must resolve under the same lock");
                batch.publication.matches(
                    &validated.reference,
                    validated.provider_source_digest,
                    command.source_digest(),
                )
            })
            .collect::<Vec<_>>();
        if exact.len() > 1 {
            return inspect_ambiguous(command);
        }
        if let Some(key) = exact.first() {
            let batch = running
                .get_mut(key)
                .expect("exact final ingress key must resolve under the same lock");
            return match batch.final_phase {
                FinalIngressPhase::Published => inspect_not_completed(command, batch.evidence()),
                FinalIngressPhase::Withdrawing => {
                    drop(running);
                    self.inspect_exact_durable_final_withdrawal(command, &validated)
                }
                FinalIngressPhase::Released => {
                    inspect_satisfied(command, &validated.reference, batch.evidence())
                }
            };
        }
        if !matching.is_empty() {
            return inspect_failure(
                command,
                invalid_command_failure(
                    "live server ingress publication is crossed with final inspection",
                ),
            );
        }
        drop(running);
        self.inspect_exact_durable_final_withdrawal(command, &validated)
    }

    fn inspect_exact_durable_final_withdrawal(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &ValidatedFinalWithdrawal,
    ) -> WorkloadTeardownProviderObservation {
        let authenticated = match self.exact_durable_ingress_records(validated) {
            Ok(authenticated) => authenticated,
            Err(FinalIngressReconciliationError::CrossedMembership(reason)) => {
                return inspect_failure(command, invalid_command_failure(reason));
            }
            Err(FinalIngressReconciliationError::Authority) => {
                return inspect_ambiguous(command);
            }
        };
        match inspect_authenticated_absent_ingress(&self.port_leases, authenticated) {
            Ok(AuthenticatedAbsentIngressInspection::Satisfied(records)) => {
                prove_exact_ingress_absence(
                    command,
                    &validated.reference,
                    &records,
                    b"durable server ingress is released",
                )
            }
            Ok(AuthenticatedAbsentIngressInspection::RetryRequired(records)) => {
                inspect_not_completed_digest(
                    command,
                    record_evidence(&validated.reference, &records),
                )
            }
            Ok(AuthenticatedAbsentIngressInspection::InProgress(records)) => {
                inspect_in_progress_digest(command, record_evidence(&validated.reference, &records))
            }
            Err(FinalIngressReconciliationError::CrossedMembership(reason)) => {
                inspect_failure(command, invalid_command_failure(reason))
            }
            Err(FinalIngressReconciliationError::Authority) => inspect_ambiguous(command),
        }
    }

    fn recover_dead_batch_after_final_withdrawal(
        &self,
        batch: &mut RunningIngressBatch,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let requests = batch
            .routes
            .iter()
            .map(|route| route.expected.request.clone())
            .collect::<Vec<_>>();
        let recoveries = self
            .port_leases
            .recover_dead_plan_members(&batch.plan_members, &requests)?;
        let records = self
            .port_leases
            .release_process_bound_plan_members_after_owner_death(
                &batch.plan_members,
                &requests,
                &recoveries,
            )?;
        batch.final_phase = FinalIngressPhase::Released;
        Ok(records)
    }

    fn reconcile_absent_live_batch_after_owner_death(
        &self,
        validated: &ValidatedFinalWithdrawal,
    ) -> Result<Vec<PortLeaseRecord>, FinalIngressReconciliationError> {
        let authenticated = self.exact_durable_ingress_records(validated)?;
        settle_authenticated_absent_ingress(&self.port_leases, authenticated)
    }

    fn exact_durable_ingress_records(
        &self,
        validated: &ValidatedFinalWithdrawal,
    ) -> Result<AuthenticatedDurableIngressPlan, FinalIngressReconciliationError> {
        let plan_records = self
            .port_leases
            .list_plan(validated.reference.network().plan_id())?;
        let plan_members = plan_records
            .iter()
            .map(|record| record.request().clone())
            .collect::<Vec<_>>();
        let mut ingress_records = plan_records
            .into_iter()
            .filter(|record| record.request().accounting() == PortLeaseAccounting::TenantPublished)
            .map(|record| {
                let request = record.request();
                let Some(listener_id) = validated.exact_listener_leases.get(request.lease_id())
                else {
                    return Err(FinalIngressReconciliationError::CrossedMembership(
                        "durable ingress contains an extra published listener lease",
                    ));
                };
                if request.plan_id() != Some(validated.reference.network().plan_id())
                    || request.tenant_id() != Some(&validated.tenant_id)
                    || request.generation() != validated.reference.network().generation()
                    || !matches!(
                        request.owner_id(),
                        NetworkResourceId::Listener(candidate) if candidate == listener_id
                    )
                {
                    return Err(FinalIngressReconciliationError::CrossedMembership(
                        "durable ingress listener lease is crossed with compiled membership",
                    ));
                }
                Ok(record)
            })
            .collect::<Result<Vec<_>, _>>()?;
        ingress_records
            .sort_by(|left, right| left.request().lease_id().cmp(right.request().lease_id()));
        if ingress_records.len() != validated.exact_listener_leases.len() {
            return Err(FinalIngressReconciliationError::CrossedMembership(
                "durable ingress omits a compiled published listener lease",
            ));
        }
        Ok(AuthenticatedDurableIngressPlan {
            plan_members,
            ingress_records,
        })
    }
}

fn settle_authenticated_absent_ingress(
    port_leases: &nimbus_network::LocalPortLeaseAuthority,
    authenticated: AuthenticatedDurableIngressPlan,
) -> Result<Vec<PortLeaseRecord>, FinalIngressReconciliationError> {
    if authenticated
        .ingress_records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Released)
    {
        return Ok(authenticated.ingress_records);
    }
    let requests = authenticated
        .ingress_records
        .iter()
        .map(|record| record.request().clone())
        .collect::<Vec<_>>();
    if authenticated
        .ingress_records
        .iter()
        .all(is_restart_retained_after_confirmed_stop)
    {
        return Ok(port_leases
            .release_plan_members_after_confirmed_stop(&authenticated.plan_members, &requests)?);
    }
    let recoveries =
        port_leases.recover_dead_plan_members(&authenticated.plan_members, &requests)?;
    Ok(
        port_leases.release_process_bound_plan_members_after_owner_death(
            &authenticated.plan_members,
            &requests,
            &recoveries,
        )?,
    )
}

fn inspect_authenticated_absent_ingress(
    port_leases: &nimbus_network::LocalPortLeaseAuthority,
    authenticated: AuthenticatedDurableIngressPlan,
) -> Result<AuthenticatedAbsentIngressInspection, FinalIngressReconciliationError> {
    if authenticated
        .ingress_records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Released)
    {
        return Ok(AuthenticatedAbsentIngressInspection::Satisfied(
            authenticated.ingress_records,
        ));
    }
    if authenticated
        .ingress_records
        .iter()
        .all(is_restart_retained_after_confirmed_stop)
    {
        return Ok(AuthenticatedAbsentIngressInspection::RetryRequired(
            authenticated.ingress_records,
        ));
    }
    let requests = authenticated
        .ingress_records
        .iter()
        .map(|record| record.request().clone())
        .collect::<Vec<_>>();
    match port_leases.recover_dead_plan_members(&authenticated.plan_members, &requests) {
        Ok(recoveries) => {
            drop(recoveries);
            Ok(AuthenticatedAbsentIngressInspection::RetryRequired(
                authenticated.ingress_records,
            ))
        }
        Err(PortLeaseError::LifetimeOwnerLive { .. }) => Ok(
            AuthenticatedAbsentIngressInspection::InProgress(authenticated.ingress_records),
        ),
        Err(_) => Err(FinalIngressReconciliationError::Authority),
    }
}

fn is_restart_retained_after_confirmed_stop(record: &PortLeaseRecord) -> bool {
    record.phase() == PortLeasePhase::Reserved
        && record.reservation_claim().is_none()
        && record.bind_claim().is_none()
        && record.adoption_claim().is_none()
        && record.binding().is_none()
        && record.confirmed_stopped_binding().is_some()
        && record.failure().is_none()
        && record.active_lifetime().is_none()
}

impl RunningIngressBatch {
    pub(super) fn stop_and_release_for_final_withdrawal(&mut self) -> io::Result<Vec<u8>> {
        if self.final_phase == FinalIngressPhase::Released {
            return Ok(self.evidence());
        }
        if self.final_phase != FinalIngressPhase::Published {
            return Err(io::Error::other(
                "final workload ingress settlement requires dead-owner reconciliation",
            ));
        }
        let leases = self
            .routes
            .iter()
            .map(|route| {
                route.lease.as_ref().ok_or_else(|| {
                    io::Error::other(
                        "workload ingress route lost its listener authority before withdrawal",
                    )
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        propagate_listener_settlement_failure(withdraw_server_listeners_for_final_withdrawal(
            &self.plan_members,
            &leases,
        ))?;
        self.final_phase = FinalIngressPhase::Withdrawing;

        let records = close_exact_ingress_routes(&self.plan_members, &mut self.routes)?;
        self.final_phase = FinalIngressPhase::Released;
        let leases = records
            .iter()
            .map(|record| record.request().lease_id().to_string())
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            "tenant={};plan={};generation={};attachment={};released={leases}",
            self.tenant_id,
            self.plan_id,
            self.generation.as_u64(),
            self.attachment_id
        )
        .into_bytes())
    }
}

impl RunningIngressRoute {
    fn take_for_final_withdrawal(&mut self) -> Option<TerminalStoppingServerListener> {
        if self.lease.is_none() || self.worker.is_none() {
            return None;
        }
        let lease = self.lease.take()?;
        let worker = self.worker.take()?;
        let stop = std::sync::Arc::clone(&self.stop);
        let connections = std::sync::Arc::clone(&self.connections);
        #[cfg(test)]
        let inject_failure = std::sync::Arc::clone(&self.final_join_failure);
        Some(TerminalStoppingServerListener::new(lease, move || {
            cancel_and_join_ingress_workers(
                &stop,
                worker,
                &connections,
                "workload ingress listener worker panicked during terminal stop",
            )?;
            #[cfg(test)]
            if inject_failure.load(std::sync::atomic::Ordering::Acquire) {
                return Err(io::Error::other(
                    "injected ambiguity after terminal worker joins",
                ));
            }
            Ok(())
        }))
    }

    #[cfg(test)]
    pub(super) fn inject_final_join_failure_for_test(&self) {
        self.final_join_failure
            .store(true, std::sync::atomic::Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn abandon_final_worker_owner_for_test(&mut self) {
        drop(self.worker.take());
    }
}

fn close_exact_ingress_routes(
    plan_members: &[nimbus_network::PortLeaseRequest],
    routes: &mut [RunningIngressRoute],
) -> io::Result<Vec<PortLeaseRecord>> {
    let mut stopping = Vec::with_capacity(routes.len());
    let mut missing_owners = Vec::new();
    for route in routes {
        match route.take_for_final_withdrawal() {
            Some(listener) => stopping.push(listener),
            None => missing_owners.push(route.expected.listener_id.to_string()),
        }
    }
    if !missing_owners.is_empty() {
        let stop_error = stop_server_listeners_for_final_withdrawal(&mut stopping)
            .err()
            .map(|error| format!("; sibling stop/join also failed: {error}"))
            .unwrap_or_default();
        return Err(io::Error::other(format!(
            "withdrawn workload ingress routes lost terminal effect ownership: {}{stop_error}",
            missing_owners.join(",")
        )));
    }
    propagate_listener_settlement_failure(settle_exact_listener_leases(plan_members, stopping))
}

fn propagate_listener_settlement_failure<T, E>(result: Result<T, E>) -> io::Result<T>
where
    E: std::fmt::Display,
{
    result.map_err(|error| io::Error::other(error.to_string()))
}

impl FinalIngressWithdrawalCapability for ServerIngressPublicationAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move { self.execute_exact_final_withdrawal(command) })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_exact_final_withdrawal(command) })
    }
}

fn prove_exact_ingress_absence(
    command: &ConfirmedWorkloadTeardownCommand,
    reference: &WorkloadPublicationReference,
    records: &[PortLeaseRecord],
    evidence: impl AsRef<[u8]>,
) -> WorkloadTeardownProviderObservation {
    if records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Released)
    {
        return inspect_satisfied_digest(command, reference, record_evidence(reference, records));
    }
    if records
        .iter()
        .all(|record| record.phase() == PortLeasePhase::Active)
    {
        return inspect_not_completed(command, evidence);
    }
    if records.iter().all(|record| {
        matches!(
            record.phase(),
            PortLeasePhase::Withdrawing | PortLeasePhase::CleanupPending
        )
    }) {
        return WorkloadTeardownProviderObservation::for_command(
            command,
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::InProgress(
                record_evidence(reference, records),
            )),
        );
    }
    inspect_ambiguous(command)
}

fn execute_succeeded(
    command: &ConfirmedWorkloadTeardownCommand,
    reference: &WorkloadPublicationReference,
    batch: &RunningIngressBatch,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
            Box::new(WorkloadTeardownSuccessEvidence::PublicationAbsent {
                reference: reference.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256(batch.evidence()),
            }),
        )),
    )
}

fn execute_succeeded_from_records(
    command: &ConfirmedWorkloadTeardownCommand,
    reference: &WorkloadPublicationReference,
    records: &[PortLeaseRecord],
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
            Box::new(WorkloadTeardownSuccessEvidence::PublicationAbsent {
                reference: reference.clone(),
                evidence: record_evidence(reference, records),
            }),
        )),
    )
}

fn inspect_satisfied(
    command: &ConfirmedWorkloadTeardownCommand,
    reference: &WorkloadPublicationReference,
    evidence: impl AsRef<[u8]>,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(
            Box::new(WorkloadTeardownSuccessEvidence::PublicationAbsent {
                reference: reference.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256(evidence),
            }),
        )),
    )
}

fn inspect_satisfied_digest(
    command: &ConfirmedWorkloadTeardownCommand,
    reference: &WorkloadPublicationReference,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(
            Box::new(WorkloadTeardownSuccessEvidence::PublicationAbsent {
                reference: reference.clone(),
                evidence,
            }),
        )),
    )
}

fn inspect_not_completed(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: impl AsRef<[u8]>,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::NotCompleted(
            WorkloadOwnerEvidenceDigest::sha256(evidence),
        )),
    )
}

fn inspect_not_completed_digest(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::NotCompleted(
            evidence,
        )),
    )
}

fn inspect_in_progress_digest(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::InProgress(
            evidence,
        )),
    )
}

fn execute_failure(
    command: &ConfirmedWorkloadTeardownCommand,
    failure: WorkloadFailureEvidence,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::DefiniteFailure(
            failure,
        )),
    )
}

fn inspect_failure(
    command: &ConfirmedWorkloadTeardownCommand,
    failure: WorkloadFailureEvidence,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::DefiniteFailure(
            failure,
        )),
    )
}

fn execute_ambiguous(
    command: &ConfirmedWorkloadTeardownCommand,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous),
    )
}

fn inspect_ambiguous(
    command: &ConfirmedWorkloadTeardownCommand,
) -> WorkloadTeardownProviderObservation {
    WorkloadTeardownProviderObservation::for_command(
        command,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous),
    )
}

fn invalid_command_failure(message: impl AsRef<str>) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(
        "server_final_ingress_command_invalid",
        WorkloadOwnerEvidenceDigest::sha256(message.as_ref()),
    )
    .expect("static server final ingress failure code is valid")
}

fn record_evidence(
    reference: &WorkloadPublicationReference,
    records: &[PortLeaseRecord],
) -> WorkloadOwnerEvidenceDigest {
    let leases = records
        .iter()
        .map(|record| format!("{}:{:?}", record.request().lease_id(), record.phase()))
        .collect::<Vec<_>>()
        .join(",");
    WorkloadOwnerEvidenceDigest::sha256(format!(
        "nimbus.server.final-ingress.v1:{}:{}:{leases}",
        reference.network().plan_id(),
        reference.execution().attempt_id(),
    ))
}

#[cfg(test)]
#[path = "final_withdrawal/tests.rs"]
mod tests;
