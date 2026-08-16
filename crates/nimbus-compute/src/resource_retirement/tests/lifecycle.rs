use nimbus_network::{
    EndpointProtocol, NetworkResourceGeneration, PublishedEndpoint, PublishedEndpointHandle,
    PublishedEndpointId,
};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadPublicationIntent, WorkloadRestartStep,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadTeardownStep,
};

use crate::WorkloadTeardownDisposition;
use crate::workload_saga::{
    ExplicitWorkloadRestartRequest, WorkloadRestartCancellationToken,
    WorkloadTeardownCancellationToken, WorkloadTeardownSubmissionError,
};

use super::support::{
    RetirementHarness, SANDBOX_ID, SERVICE_NAME, assert_complete_teardown_order, key,
    run_async_test,
};

fn make_service_routable(harness: &RetirementHarness) {
    let observation = harness
        .manager
        .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
        .expect("started service should have an observed projection");
    let generation = observation.observed_execution_generation;
    let existing_endpoint_handles = observation.published_endpoints;
    let mut routable = observation.handle;
    let endpoint = PublishedEndpoint::new(
        "http",
        EndpointProtocol::Http,
        "127.0.0.1:18080"
            .parse()
            .expect("fixture endpoint should parse"),
    );
    if !routable
        .published_endpoints
        .iter()
        .any(|candidate| candidate.name == endpoint.name)
    {
        routable.published_endpoints.push(endpoint);
    }
    let endpoint_handles = routable
        .published_endpoints
        .iter()
        .map(|endpoint| {
            existing_endpoint_handles
                .iter()
                .find(|candidate| candidate.endpoint().name == endpoint.name)
                .cloned()
                .unwrap_or_else(|| {
                    PublishedEndpointHandle::new(
                        PublishedEndpointId::for_workload_endpoint(
                            "retirement-fixture-service",
                            &endpoint.name,
                        ),
                        NetworkResourceGeneration::new(generation),
                        endpoint.clone(),
                    )
                })
        })
        .collect();
    harness
        .manager
        .project_service_definition_observation(
            harness.context.tenant_id(),
            SERVICE_NAME,
            observation.source_generation,
            observation.execution.attempt_id(),
            routable,
            endpoint_handles,
        )
        .expect("fixture service should become routable");
}

#[test]
fn service_stop_persists_then_observes_complete_teardown_order() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let running = harness.store.record(&key(SERVICE_NAME));
        harness.reset_retirement_evidence();

        let outcome = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "service retirement should converge: {error:?}; events: {:?}",
                    harness.log.entries()
                )
            });

        assert!(outcome.retired_handle.is_some());
        assert_eq!(outcome.disposition(), WorkloadTeardownDisposition::Recorded);
        assert_eq!(
            outcome.terminal_execution_reference(),
            Some(&running.current_execution_reference()),
            "native retirement must return the exact durable execution after projection cleanup"
        );
        let observation = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("terminal service observation should remain truthful");
        assert_eq!(
            observation.handle.status,
            nimbus_sandbox::SandboxStatus::Stopped
        );
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 1);
        assert!(observation.handle.published_endpoints.is_empty());
        let record = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            record.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_terminal_network_successor(&running, &record);
        assert_complete_teardown_order(&harness.log.entries(), &key(SERVICE_NAME));
    });
}

#[test]
fn foreground_service_stop_resumes_waiting_without_duplicate_effect() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.reset_retirement_evidence();
        harness
            .teardown_provider
            .wait_once_at(WorkloadTeardownStep::StopExecution);

        let outcome = harness
            .retire
            .submit_service_teardown_until_terminal(
                &harness.context,
                SERVICE_NAME,
                &WorkloadTeardownCancellationToken::new(),
            )
            .await
            .expect("foreground retirement should resume a safe waiting result");

        assert_eq!(outcome.disposition(), WorkloadTeardownDisposition::Recorded);
        assert_eq!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Recorded
        );
        assert_waiting_step_was_inspected_without_duplicate_effect(
            &harness.log.entries(),
            WorkloadTeardownStep::StopExecution,
        );
    });
}

#[test]
fn cancelled_foreground_service_stop_remains_replayable() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.reset_retirement_evidence();
        let waiting = harness
            .teardown_provider
            .wait_once_at(WorkloadTeardownStep::StopExecution);
        let cancellation = WorkloadTeardownCancellationToken::new();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let waiter_cancellation = cancellation.clone();
        let retirement = tokio::spawn(async move {
            retire
                .submit_service_teardown_until_terminal(
                    &context,
                    SERVICE_NAME,
                    &waiter_cancellation,
                )
                .await
        });
        harness
            .wait_for_signal(
                &waiting,
                "foreground retirement did not reach its durable waiting boundary",
            )
            .await;
        cancellation.cancel();

        let error = tokio::time::timeout(std::time::Duration::from_secs(2), retirement)
            .await
            .expect("cancelled foreground waiter should return")
            .expect("foreground retirement task should join")
            .expect_err("caller cancellation should detach the foreground waiter");
        assert!(matches!(
            error,
            crate::ComputeResourceRetirementError::Teardown(
                WorkloadTeardownSubmissionError::Cancelled
            )
        ));
        assert_ne!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Recorded,
            "cancellation must not invent terminal state"
        );

        let replay = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("a later exact replay should finish retained teardown work");
        assert_eq!(replay.disposition(), WorkloadTeardownDisposition::Recorded);
        assert_waiting_step_was_inspected_without_duplicate_effect(
            &harness.log.entries(),
            WorkloadTeardownStep::StopExecution,
        );
    });
}

#[test]
fn service_resolution_is_fenced_before_awaited_publication_withdrawal() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        make_service_routable(&harness);
        assert!(
            harness
                .manager
                .resolve_service_binding(harness.context.tenant_id(), SERVICE_NAME)
                .expect("ready service resolution should succeed")
                .is_some(),
            "precondition: the ready service should resolve before retirement"
        );
        let (entered, release) = harness
            .teardown_provider
            .install_gate(WorkloadTeardownStep::WithdrawPublication);
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let retirement =
            tokio::spawn(
                async move { retire.submit_service_teardown(&context, SERVICE_NAME).await },
            );
        harness
            .wait_for_signal(
                &entered,
                "service retirement did not start publication withdrawal",
            )
            .await;

        let binding_during_withdrawal = harness
            .manager
            .resolve_service_binding(harness.context.tenant_id(), SERVICE_NAME)
            .expect("fenced service resolution should not error");
        let snapshot_during_withdrawal = harness
            .manager
            .snapshot_for_tenant(harness.context.tenant_id());
        let observation_during_withdrawal = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME);

        release.add_permits(1);
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), retirement)
            .await
            .expect("service retirement should complete after withdrawal release")
            .expect("service retirement task should join")
            .expect("service retirement should converge");

        assert!(
            binding_during_withdrawal.is_none(),
            "service resolution returned a new routable binding after withdrawal started"
        );
        assert!(
            !snapshot_during_withdrawal.contains_key(SERVICE_NAME),
            "runtime snapshot retained a routable service after withdrawal started"
        );
        assert_eq!(
            observation_during_withdrawal
                .expect("withdrawal fence must preserve observed recovery evidence")
                .handle
                .status,
            nimbus_sandbox::SandboxStatus::Ready,
            "resolver fencing must not rewrite observed provider status"
        );
        assert!(outcome.retired_handle.is_some());
    });
}

#[test]
fn service_resolution_stays_fenced_until_restart_publication_is_observed() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_published_service();
        harness.start_service().await;
        make_service_routable(&harness);
        assert!(
            harness
                .manager
                .resolve_service_binding(harness.context.tenant_id(), SERVICE_NAME)
                .expect("ready service resolution should succeed")
                .is_some(),
            "precondition: the ready service should resolve before restart"
        );
        let (entered, release) = harness
            .restart_provider
            .install_gate(WorkloadRestartStep::WithdrawPublication);
        let request = ExplicitWorkloadRestartRequest::new(
            key(SERVICE_NAME),
            WorkloadProvisionSourceIdentity::sandbox_backed_service(SERVICE_NAME)
                .expect("fixture service identity should validate"),
            WorkloadProvisionSourceGeneration::new(1),
            "service-resolution-restart",
        );
        harness
            .restart_runtime
            .submit_explicit(&request, &WorkloadRestartCancellationToken::new())
            .await
            .expect("service restart should submit");
        harness
            .wait_for_signal(
                &entered,
                "service restart did not start publication withdrawal",
            )
            .await;

        let binding_during_withdrawal = harness
            .manager
            .resolve_service_binding(harness.context.tenant_id(), SERVICE_NAME)
            .expect("fenced service resolution should not error");
        let snapshot_during_withdrawal = harness
            .manager
            .snapshot_for_tenant(harness.context.tenant_id());
        let observation_during_withdrawal = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME);

        assert!(
            binding_during_withdrawal.is_none(),
            "service resolution returned a routable binding during restart withdrawal"
        );
        assert!(
            !snapshot_during_withdrawal.contains_key(SERVICE_NAME),
            "runtime snapshot retained a routable service during restart withdrawal"
        );
        assert_eq!(
            observation_during_withdrawal
                .expect("restart fence must preserve observed recovery evidence")
                .handle
                .status,
            nimbus_sandbox::SandboxStatus::Ready,
            "restart fencing must not rewrite observed provider status"
        );

        release.add_permits(1);
        let saga_key = key(SERVICE_NAME);
        harness
            .wait_for_service_restart_resolution(
                &saga_key,
                SERVICE_NAME,
                "service restart should complete after withdrawal release",
            )
            .await;

        assert_eq!(
            harness.restart_provider.execute_call_count()
                + harness.restart_provider.inspect_call_count(),
            9,
            "the fixture should complete the exact published restart sequence"
        );
        assert!(
            harness
                .manager
                .resolve_service_binding(harness.context.tenant_id(), SERVICE_NAME)
                .expect("completed restart resolution should not error")
                .is_some(),
            "service resolution should reopen only after publication observation"
        );
        let completed = harness.store.record(&saga_key);
        let target_attempt = completed
            .restart_state()
            .last_completed()
            .expect("completed restart should retain its target attempt")
            .admission()
            .attempt_id();
        let observation = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("resolution release should retain a truthful target observation");
        assert_eq!(
            observation.execution.attempt_id(),
            target_attempt,
            "resolution must reopen only over the exact target execution attempt"
        );
        assert!(
            !observation.handle.published_endpoints.is_empty(),
            "resolution must reopen only after exact target ingress observation"
        );
    });
}

#[test]
fn unstarted_service_retirement_reports_source_finalized_without_execution_identity() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();

        let outcome = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("an unstarted source should finalize without provider effects");

        assert_eq!(
            outcome.disposition(),
            WorkloadTeardownDisposition::SourceFinalized
        );
        assert!(outcome.terminal_execution_reference().is_none());
        assert!(outcome.retired_handle.is_none());
        assert_eq!(harness.teardown_provider.call_count(), 0);
    });
}

#[test]
fn sandbox_stop_persists_then_observes_complete_teardown_order() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.start_sandbox().await;
        let running = harness.store.record(&key(SANDBOX_ID));
        harness.reset_retirement_evidence();

        let snapshot = harness
            .retire
            .submit_sandbox_teardown(&harness.context, SANDBOX_ID)
            .await
            .expect("sandbox retirement should converge");

        let observation = snapshot
            .observation
            .expect("terminal sandbox observation should remain truthful");
        assert_eq!(
            observation.handle.status,
            nimbus_sandbox::SandboxStatus::Stopped
        );
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 1);
        assert!(observation.handle.published_endpoints.is_empty());
        let record = harness.store.record(&key(SANDBOX_ID));
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            record.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_terminal_network_successor(&running, &record);
        assert_complete_teardown_order(&harness.log.entries(), &key(SANDBOX_ID));
    });
}

fn assert_terminal_network_successor(running: &WorkloadSagaRecord, stopped: &WorkloadSagaRecord) {
    let running = running.active_intent().network().compiled_plan().content();
    let stopped = stopped.active_intent().network().compiled_plan().content();

    assert_eq!(
        stopped.identity().workload_incarnation_key(),
        running.identity().workload_incarnation_key(),
        "retirement must retain the admitted workload-incarnation identity"
    );
    assert_eq!(
        stopped.identity().plan_id(),
        running.identity().plan_id(),
        "retirement must not create a second stable network-plan identity"
    );
    assert_eq!(
        stopped.sovereignty_requirements(),
        running.sovereignty_requirements(),
        "a terminal empty plan must retain the complete admitted sovereignty baseline"
    );
    assert!(stopped.identity().generation() > running.identity().generation());
    assert!(stopped.attachment().is_none());
    assert!(stopped.routes().is_empty());
    assert!(stopped.listeners().is_empty());
    assert!(stopped.dependency_listeners().is_empty());
    assert_eq!(stopped.activation(), WorkloadActivationIntent::PrepareOnly);
    assert_eq!(stopped.publication(), WorkloadPublicationIntent::Withheld);
}

fn assert_waiting_step_was_inspected_without_duplicate_effect(
    events: &[super::support::LifecycleEvent],
    waiting_step: WorkloadTeardownStep,
) {
    use nimbus_workloads::WorkloadTeardownCommandMode;

    for step in [
        WorkloadTeardownStep::WithdrawPublication,
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ] {
        let execute_count = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    super::support::LifecycleEvent::Teardown(_, candidate, WorkloadTeardownCommandMode::Execute)
                        if *candidate == step
                )
            })
            .count();
        assert_eq!(
            execute_count, 1,
            "durable foreground resume must not duplicate the {step:?} effect"
        );
    }
    let inspections = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                super::support::LifecycleEvent::Teardown(_, candidate, WorkloadTeardownCommandMode::Inspect)
                    if *candidate == waiting_step
            )
        })
        .count();
    assert_eq!(
        inspections, 2,
        "the ambiguous effect must be inspected once into Waiting and once into terminal truth"
    );
}
