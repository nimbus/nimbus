use nimbus_core::TenantId;
use nimbus_sandbox::{
    SandboxBackendKind, SandboxCleanupObservation, SandboxExecutionObservation, SandboxHandle,
    SandboxId, SandboxRestartAssessment, SandboxRestartBlocker, SandboxRestartIneligibility,
    SandboxStatus,
};

use super::{
    authenticated_restart_exit, build_restart_runtime,
    completed_restart_requires_resolution_reconciliation,
};
use crate::workload_saga::test_support;

fn inspection(
    execution: SandboxExecutionObservation,
    restart: SandboxRestartAssessment,
) -> nimbus_sandbox::SandboxInspection {
    let handle = SandboxHandle::new(
        TenantId::new("restart-runtime").expect("tenant ID"),
        SandboxId::new("restart-runtime"),
        "runtime",
        SandboxBackendKind::Container,
        SandboxStatus::Stopping,
        Vec::new(),
    );
    nimbus_sandbox::SandboxInspection::provider_reported(handle.clone()).with_provider_projection(
        handle,
        execution,
        restart,
        SandboxCleanupObservation::Retained,
    )
}

#[test]
fn automatic_restart_requires_unblocked_matching_physical_exit_evidence() {
    let admitted = inspection(
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            blocker: None,
        },
    );
    assert_eq!(authenticated_restart_exit(&admitted), Ok(Some(42)));

    let blocked = inspection(
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        },
    );
    assert_eq!(authenticated_restart_exit(&blocked), Ok(None));

    let shutdown = inspection(
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::ShutdownRequested,
        },
    );
    assert_eq!(authenticated_restart_exit(&shutdown), Ok(None));

    let crossed = inspection(
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 43,
            blocker: None,
        },
    );
    assert_eq!(
        authenticated_restart_exit(&crossed),
        Err("automatic restart inspection crossed exit evidence".to_owned())
    );
}

#[test]
fn automatic_restart_ignores_non_exit_provider_observations() {
    let present = inspection(
        SandboxExecutionObservation::Present,
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::RuntimePresent,
        },
    );
    assert_eq!(authenticated_restart_exit(&present), Ok(None));
}

#[test]
fn completed_restart_without_an_active_epoch_requires_resolution_reconciliation() {
    let observed = test_support::restart_observed_record(
        "runtime-reconcile-observed",
        nimbus_workloads::WorkloadRestartPolicy::Always { max_restarts: 1 },
    );
    assert!(!completed_restart_requires_resolution_reconciliation(
        &observed
    ));

    let completed = test_support::completed_restart_record("runtime-reconcile-completed");
    assert!(completed.restart_state().active().is_none());
    assert!(completed.restart_state().last_completed().is_some());
    assert!(completed_restart_requires_resolution_reconciliation(
        &completed
    ));
}

#[test]
fn dedicated_restart_runtime_drives_tokio_io() {
    let runtime = build_restart_runtime().expect("restart runtime");
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let address = listener.local_addr().expect("loopback address");
        let connect = tokio::net::TcpStream::connect(address);
        let accept = listener.accept();
        let (connected, accepted) = tokio::join!(connect, accept);
        connected.expect("loopback connection");
        accepted.expect("loopback acceptance");
    });
}
