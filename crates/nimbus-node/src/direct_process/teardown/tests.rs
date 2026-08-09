use nimbus_workloads::{WorkloadTeardownCommandMode, WorkloadTeardownStep};

use super::super::*;
use crate::host_lifecycle::teardown_fail_before_tests::{fixture, input, inspection_fixture};
use crate::{
    HostExecutable, HostExecutionDrainProvider, HostExecutionStopProvider,
    HostLifecycleBackendKind, HostLifecycleRequest, HostLifecycleStatusReason,
    HostTeardownExecuteClaim, HostTeardownExecuteObservation, HostTeardownInspectClaim,
    HostTeardownInspectObservation, RuntimePoolTrustClass,
};

fn request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::DirectProcess,
        HostExecutable::trusted("/bin/nimbus-direct-teardown-test")
            .expect("test executable should validate"),
    )
    .with_args(["--teardown-test"])
    .expect("test args should validate")
    .with_trust_class(RuntimePoolTrustClass::SingleTenant)
}

async fn activate(
    backend: &DirectProcessBackend,
    fixture: &crate::host_lifecycle::teardown_fail_before_tests::Fixture,
) {
    backend
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect("fixture process should activate");
}

#[tokio::test]
async fn direct_process_exact_drain_keeps_execution_running() {
    let backend = DirectProcessBackend::new();
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    activate(&backend, &fixture).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");

    let observed = backend.execute_drain(claim).await;
    assert!(matches!(
        observed,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(
        backend
            .inspect(fixture.execution.execution_id().clone())
            .await
            .expect("process should remain inspectable")
            .reason(),
        HostLifecycleStatusReason::Running,
        "drain must not stop the process-local execution"
    );
    assert_eq!(
        backend
            .logs(fixture.execution.execution_id())
            .expect("logs should remain available")
            .iter()
            .filter(|line| line.contains(":stopped:"))
            .count(),
        0
    );
}

#[tokio::test]
async fn direct_process_exact_stop_replay_records_one_terminal_effect() {
    let backend = DirectProcessBackend::new();
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &fixture).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("stop claim should validate");

    let first = backend.execute_stop(claim.clone()).await;
    let replay = backend.execute_stop(claim).await;
    assert_eq!(
        first, replay,
        "exact replay should adopt canonical evidence"
    );
    assert!(matches!(
        first,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(
        backend
            .logs(fixture.execution.execution_id())
            .expect("logs should remain available")
            .iter()
            .filter(|line| line.contains(":stopped:"))
            .count(),
        1,
        "exact replay must record one terminal stop effect"
    );
}

#[tokio::test]
async fn direct_process_crossed_confirmation_cannot_adopt_prior_success() {
    let backend = DirectProcessBackend::new();
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    let original =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("original stop claim should validate");
    assert!(matches!(
        backend.execute_stop(original).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));

    let inspection = inspection_fixture(&primary, "7a");
    let inspection_claim =
        HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
            .expect("corresponding inspection claim should validate");
    assert!(matches!(
        backend.inspect_stop(inspection_claim).await,
        HostTeardownInspectObservation::Satisfied(_)
    ));

    let mut crossed_fixture = fixture(WorkloadTeardownStep::StopExecution);
    crossed_fixture.confirmed_transition_id = format!("wst_{}", "7b".repeat(32))
        .parse()
        .expect("crossed confirmation should validate");
    let crossed_claim = HostTeardownExecuteClaim::new(input(
        &crossed_fixture,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("internally consistent crossed claim should validate");
    let execution_id = crossed_claim.execution().execution_id().clone();

    assert!(matches!(
        backend.execute_stop(crossed_claim).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(
        backend
            .logs(&execution_id)
            .expect("logs should remain available")
            .iter()
            .filter(|line| line.contains(":stopped:"))
            .count(),
        1,
        "crossed confirmation must not create or adopt another stop effect"
    );

    crossed_fixture.confirmed_revision = crossed_fixture
        .confirmed_revision
        .checked_next()
        .expect("crossed inspection revision should advance");
    let crossed_inspection = HostTeardownInspectClaim::new(input(
        &crossed_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("internally consistent crossed inspection should validate");
    assert!(matches!(
        backend.inspect_stop(crossed_inspection).await,
        HostTeardownInspectObservation::DefiniteFailure(_)
    ));
}

#[tokio::test]
async fn direct_process_fresh_authority_missing_state_is_not_false_absence() {
    let backend = DirectProcessBackend::new();
    let mut fixture = fixture(WorkloadTeardownStep::StopExecution);
    fixture.confirmed_revision = fixture
        .confirmed_revision
        .checked_next()
        .expect("fixture revision should advance");
    let claim =
        HostTeardownInspectClaim::new(input(&fixture, WorkloadTeardownCommandMode::Inspect))
            .expect("inspection claim should validate");

    assert_eq!(
        backend.inspect_stop(claim).await,
        HostTeardownInspectObservation::Ambiguous,
        "an empty process-local map cannot prove a prior live authority is absent"
    );
}
