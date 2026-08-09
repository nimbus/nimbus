use std::sync::{Arc, Mutex};

use nimbus_core::Error;
use nimbus_network::NetworkProviderId;
use nimbus_node::{
    DirectProcessBackend, HostExecutable, HostLifecycleBackend, HostLifecycleBackendKind,
    HostLifecycleFuture, HostLifecycleProperty, HostLifecyclePropertySet, HostLifecycleRequest,
    HostRestartPolicy, RuntimePoolTrustClass, SystemdDbusClient, SystemdInspectUnitRequest,
    SystemdStartTransientUnitRequest, SystemdStartTransientUnitResponse, SystemdStopUnitRequest,
    SystemdStopUnitResponse, SystemdStopUnitSubmission, SystemdTransientCapabilities,
    SystemdTransientUnitBackend, SystemdUnitStatus,
};
use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadActivationIntent, WorkloadPublicationIntent,
    WorkloadSagaPhase, WorkloadTeardownDecision, WorkloadTeardownProviderTarget,
    WorkloadTeardownSubjects,
};

use crate::workload_saga::provision_provider::tests::activation_command_for_record;
use crate::workload_saga::recovery::tests::{provision_record, teardown_record};
use crate::workload_saga::teardown_decision::materialize_teardown_candidate;
use crate::workload_saga::teardown_test_support::{
    DurableTeardownStore, RecordingTeardownProvider, StaticSourceAuthority,
    TeardownProviderBehavior, provider_reports,
};
use crate::workload_saga::{
    ConfirmedWorkloadTeardownCommand, IngressTeardownCapabilities,
    NetworkAttachmentTeardownCapabilities, NodeExecutionTeardownAdapter,
    WorkloadExecutionDrainCapability, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadSagaConfirmation, WorkloadSagaCoordinator,
    WorkloadTeardownCancellationToken, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownProviderOutcome, WorkloadTeardownRuntime,
};

fn assert_execution_teardown_substitution<T>()
where
    T: WorkloadExecutionDrainCapability + WorkloadExecutionStopCapability,
{
}

#[test]
fn node_teardown_adapters_substitute_direct_process_and_systemd() {
    assert_execution_teardown_substitution::<
        super::NodeExecutionTeardownAdapter<DirectProcessBackend>,
    >();
    assert_execution_teardown_substitution::<
        super::NodeExecutionTeardownAdapter<SystemdTransientUnitBackend<ComputeSystemdClient>>,
    >();
}

fn direct_request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::DirectProcess,
        HostExecutable::trusted("/bin/nimbus-direct-teardown-test")
            .expect("fixture executable is trusted"),
    )
    .with_properties(HostLifecyclePropertySet::new([
        HostLifecycleProperty::Description("direct teardown substitution".to_owned()),
        HostLifecycleProperty::Restart(HostRestartPolicy::No),
    ]))
    .with_trust_class(RuntimePoolTrustClass::SingleTenant)
}

#[derive(Default)]
struct ComputeSystemdState {
    start_request: Option<SystemdStartTransientUnitRequest>,
    status: Option<SystemdUnitStatus>,
    fail_before_next_stop: bool,
    exact_stop_calls: usize,
    stop_effects: usize,
}

#[derive(Clone, Default)]
struct ComputeSystemdClient {
    state: Arc<Mutex<ComputeSystemdState>>,
}

impl ComputeSystemdClient {
    fn fail_before_next_stop(&self) {
        self.state
            .lock()
            .expect("compute systemd state lock should not be poisoned")
            .fail_before_next_stop = true;
    }

    fn exact_stop_call_count(&self) -> usize {
        self.state
            .lock()
            .expect("compute systemd state lock should not be poisoned")
            .exact_stop_calls
    }

    fn stop_effect_count(&self) -> usize {
        self.state
            .lock()
            .expect("compute systemd state lock should not be poisoned")
            .stop_effects
    }
}

impl SystemdDbusClient for ComputeSystemdClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        SystemdTransientCapabilities::available()
    }

    fn start_transient_unit<'a>(
        &'a self,
        request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            let status = SystemdUnitStatus::for_start_request(&request, "active", "running")?;
            let response = SystemdStartTransientUnitResponse::new(
                request.unit_name().clone(),
                "/org/freedesktop/systemd1/job/601",
            )?;
            let mut state = self
                .state
                .lock()
                .expect("compute systemd state lock should not be poisoned");
            state.start_request = Some(request);
            state.status = Some(status);
            Ok(response)
        })
    }

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            let mut state = self
                .state
                .lock()
                .expect("compute systemd state lock should not be poisoned");
            let start = state.start_request.clone().ok_or_else(|| {
                Error::NotFound("compute systemd fixture has no active start request".to_owned())
            })?;
            if start.execution_id() != request.execution_id()
                || start.unit_name() != request.unit_name()
            {
                return Err(Error::PermissionDenied(
                    "compute systemd stop request is crossed with its exact start".to_owned(),
                ));
            }
            state.stop_effects += 1;
            let status = SystemdUnitStatus::for_start_request(&start, "inactive", "dead")?;
            state.status = Some(status.clone());
            SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/602", status)
        })
    }

    fn stop_unit_exact<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitSubmission> {
        Box::pin(async move {
            let fail_before = {
                let mut state = self
                    .state
                    .lock()
                    .expect("compute systemd state lock should not be poisoned");
                state.exact_stop_calls += 1;
                std::mem::take(&mut state.fail_before_next_stop)
            };
            if fail_before {
                return Ok(SystemdStopUnitSubmission::pre_call_failure(
                    "compute fixture failed before StopUnit",
                ));
            }
            Ok(match self.stop_unit(request).await {
                Ok(response) => SystemdStopUnitSubmission::Terminal(Box::new(response)),
                Err(error) => SystemdStopUnitSubmission::unknown_submission(error.to_string()),
            })
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            let state = self
                .state
                .lock()
                .expect("compute systemd state lock should not be poisoned");
            let Some(status) = state.status.clone() else {
                return SystemdUnitStatus::absent_for_inspect_request(&request);
            };
            if status.execution_id() != request.execution_id()
                || status.unit_name() != request.unit_name()
            {
                return Err(Error::PermissionDenied(
                    "compute systemd inspection is crossed with its exact unit".to_owned(),
                ));
            }
            Ok(status)
        })
    }
}

fn systemd_request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::SystemdTransientUnit,
        HostExecutable::trusted("/bin/nimbus-systemd-teardown-test")
            .expect("fixture executable is trusted"),
    )
    .with_properties(HostLifecyclePropertySet::new([
        HostLifecycleProperty::Description("systemd teardown substitution".to_owned()),
        HostLifecycleProperty::Restart(HostRestartPolicy::No),
    ]))
    .with_trust_class(RuntimePoolTrustClass::SingleTenant)
}

async fn teardown_command(
    label: &str,
    phase: WorkloadSagaPhase,
) -> ConfirmedWorkloadTeardownCommand {
    let loaded = teardown_record(label, phase);
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = loaded
        .decide_teardown()
        .expect("fixture phase is reducible")
    else {
        panic!("fixture phase must require a provider claim");
    };
    let candidate = materialize_teardown_candidate(&loaded, &proposed).expect("claim materializes");
    let confirmed = WorkloadSagaCoordinator::new(DurableTeardownStore::with_record(loaded.clone()))
        .confirm_teardown_transition(&loaded, candidate)
        .await
        .expect("teardown claim confirms");
    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    confirmed
        .command()
        .expect("direct winner receives execute")
        .clone()
}

#[tokio::test]
async fn real_direct_process_adapter_drains_then_stops_one_exact_execution() {
    let label = "node-teardown-direct-substitution";
    let backend = Arc::new(DirectProcessBackend::new());
    let current = provision_record(
        label,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let activation = activation_command_for_record(current).await;
    backend
        .activate_exact(
            activation.execution().clone(),
            activation.claim().clone(),
            direct_request(),
        )
        .await
        .expect("direct process activation succeeds");

    let drain = teardown_command(label, WorkloadSagaPhase::Withdrawn).await;
    let WorkloadTeardownProviderTarget::Execution { provider_id, .. } = drain.provider_target()
    else {
        panic!("drain command must select an execution provider");
    };
    let adapter = super::NodeExecutionTeardownAdapter::new(provider_id.clone(), backend.clone());
    let drained = WorkloadExecutionDrainCapability::execute(&adapter, &drain).await;
    assert!(matches!(
        drained.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));

    let stop = teardown_command(label, WorkloadSagaPhase::Drained).await;
    let stopped = WorkloadExecutionStopCapability::execute(&adapter, &stop).await;
    assert!(matches!(
        stopped.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    let replay = WorkloadExecutionStopCapability::execute(&adapter, &stop).await;
    assert!(matches!(
        replay.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    let WorkloadTeardownSubjects::Execution(execution) = stop.subjects() else {
        panic!("stop command has one exact execution subject");
    };
    assert_eq!(
        backend
            .logs(execution.execution_id())
            .expect("direct process logs exist")
            .iter()
            .filter(|line| line.contains(":stopped:"))
            .count(),
        1
    );
}

#[tokio::test]
async fn real_systemd_adapter_drains_then_stops_one_exact_execution() {
    let label = "node-teardown-systemd-substitution";
    let client = ComputeSystemdClient::default();
    let teardown_state = tempfile::tempdir().expect("systemd teardown state root should exist");
    let backend = Arc::new(
        SystemdTransientUnitBackend::new_with_teardown_state_root(
            client.clone(),
            teardown_state.path(),
        )
        .expect("systemd teardown receipt store should open"),
    );
    let current = provision_record(
        label,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let activation = activation_command_for_record(current).await;
    backend
        .activate_exact(
            activation.execution().clone(),
            activation.claim().clone(),
            systemd_request(),
        )
        .await
        .expect("systemd fixture activation succeeds");

    let drain = teardown_command(label, WorkloadSagaPhase::Withdrawn).await;
    let WorkloadTeardownProviderTarget::Execution { provider_id, .. } = drain.provider_target()
    else {
        panic!("drain command must select an execution provider");
    };
    let adapter = super::NodeExecutionTeardownAdapter::new(provider_id.clone(), backend);
    let drained = WorkloadExecutionDrainCapability::execute(&adapter, &drain).await;
    assert!(matches!(
        drained.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));

    let stop = teardown_command(label, WorkloadSagaPhase::Drained).await;
    let stopped = WorkloadExecutionStopCapability::execute(&adapter, &stop).await;
    assert!(matches!(
        stopped.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    let replay = WorkloadExecutionStopCapability::execute(&adapter, &stop).await;
    assert!(matches!(
        replay.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn real_systemd_runtime_retries_only_after_exact_not_completed_proof() {
    let label = "node-teardown-systemd-runtime-retry";
    let client = ComputeSystemdClient::default();
    let teardown_state = tempfile::tempdir().expect("systemd teardown state root should exist");
    let backend = Arc::new(
        SystemdTransientUnitBackend::new_with_teardown_state_root(
            client.clone(),
            teardown_state.path(),
        )
        .expect("systemd teardown receipt store should open"),
    );
    let activation_record = provision_record(
        label,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let activation = activation_command_for_record(activation_record).await;
    backend
        .activate_exact(
            activation.execution().clone(),
            activation.claim().clone(),
            systemd_request(),
        )
        .await
        .expect("systemd fixture activation succeeds");
    client.fail_before_next_stop();

    let initial = teardown_record(label, WorkloadSagaPhase::Drained);
    let store = DurableTeardownStore::with_record(initial.clone());
    let later = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let execution_provider =
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("fixture-execution");
    let adapter = Arc::new(NodeExecutionTeardownAdapter::new(
        execution_provider.clone(),
        backend,
    ));
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            later.clone(),
            later.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            execution_provider,
            adapter.clone(),
            adapter,
        )],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            later.clone(),
        )],
    )
    .expect("real node teardown registry should validate");
    let runtime = WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store)),
        StaticSourceAuthority::exact(&initial),
        provider_reports(),
        Arc::new(capabilities),
    );

    let completed = runtime
        .submit(
            initial.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
        .expect("pre-call failure should inspect and authorize one next-epoch retry");

    assert_eq!(completed.record().phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(client.exact_stop_call_count(), 2);
    assert_eq!(
        client.stop_effect_count(),
        1,
        "only the reducer-authorized next-epoch retry may reach StopUnit"
    );
    assert_eq!(
        later
            .calls()
            .into_iter()
            .map(|call| call.step)
            .collect::<Vec<_>>(),
        [
            nimbus_workloads::WorkloadTeardownStep::DetachNetwork,
            nimbus_workloads::WorkloadTeardownStep::ReleaseNetwork,
        ]
    );
}
