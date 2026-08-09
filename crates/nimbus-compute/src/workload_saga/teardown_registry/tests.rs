use nimbus_network::NetworkProviderId;
use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadExecutionProviderId, WorkloadSagaPhase,
    WorkloadTeardownDecision, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
};

use super::*;
use crate::workload_saga::recovery::tests::teardown_record;
use crate::workload_saga::teardown_test_support::{
    RecordingTeardownProvider, TeardownProviderBehavior, teardown_capabilities,
};

fn target_for(
    label: &str,
    phase: WorkloadSagaPhase,
) -> (WorkloadTeardownStep, WorkloadTeardownProviderTarget) {
    let record = teardown_record(label, phase);
    let WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
        attempt,
        provider_target,
    }) = record
        .decide_teardown()
        .expect("teardown fixture is reducible")
    else {
        panic!("effectful teardown fixture must yield a claim");
    };
    (attempt.step(), provider_target)
}

#[test]
fn registry_routes_all_five_exact_teardown_capabilities() {
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let registry = teardown_capabilities(provider);
    for (label, phase, expected_step) in [
        (
            "registry-withdraw",
            WorkloadSagaPhase::WithdrawalCommitted,
            WorkloadTeardownStep::WithdrawPublication,
        ),
        (
            "registry-drain",
            WorkloadSagaPhase::Withdrawn,
            WorkloadTeardownStep::DrainExecution,
        ),
        (
            "registry-stop",
            WorkloadSagaPhase::Drained,
            WorkloadTeardownStep::StopExecution,
        ),
        (
            "registry-detach",
            WorkloadSagaPhase::WorkloadStopped,
            WorkloadTeardownStep::DetachNetwork,
        ),
        (
            "registry-release",
            WorkloadSagaPhase::NetworkDetached,
            WorkloadTeardownStep::ReleaseNetwork,
        ),
    ] {
        let (step, target) = target_for(label, phase);
        assert_eq!(step, expected_step);
        assert!(registry.select_for(step, &target).is_ok());
    }
}

#[test]
fn registry_rejects_duplicate_role_provider_registration() {
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let attachment_id = NetworkProviderId::for_registration_key("duplicate-attachment");
    assert!(matches!(
        WorkloadTeardownCapabilityRegistry::new(
            [
                NetworkAttachmentTeardownCapabilities::new(
                    attachment_id.clone(),
                    provider.clone(),
                    provider.clone(),
                ),
                NetworkAttachmentTeardownCapabilities::new(
                    attachment_id.clone(),
                    provider.clone(),
                    provider.clone(),
                ),
            ],
            [],
            [],
        ),
        Err(WorkloadTeardownCapabilityRegistryError::DuplicateAttachmentProvider { .. })
    ));

    let execution_id = WorkloadExecutionProviderId::for_registration_key("duplicate-execution");
    assert!(matches!(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [
                WorkloadExecutionTeardownCapabilities::new(
                    execution_id.clone(),
                    provider.clone(),
                    provider.clone(),
                ),
                WorkloadExecutionTeardownCapabilities::new(
                    execution_id,
                    provider.clone(),
                    provider.clone(),
                ),
            ],
            [],
        ),
        Err(WorkloadTeardownCapabilityRegistryError::DuplicateExecutionProvider { .. })
    ));

    let ingress_id = NetworkProviderId::for_registration_key("duplicate-ingress");
    assert!(matches!(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [],
            [
                IngressTeardownCapabilities::new(ingress_id.clone(), provider.clone()),
                IngressTeardownCapabilities::new(ingress_id, provider.clone()),
            ],
        ),
        Err(WorkloadTeardownCapabilityRegistryError::DuplicateIngressProvider { .. })
    ));
}

#[test]
fn registry_rejects_network_role_conflict() {
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let provider_id = NetworkProviderId::for_registration_key("crossed-network-role");
    assert!(matches!(
        WorkloadTeardownCapabilityRegistry::new(
            [NetworkAttachmentTeardownCapabilities::new(
                provider_id.clone(),
                provider.clone(),
                provider.clone(),
            )],
            [],
            [IngressTeardownCapabilities::new(provider_id, provider)],
        ),
        Err(WorkloadTeardownCapabilityRegistryError::NetworkRoleConflict { .. })
    ));
}

#[test]
fn registry_reports_missing_exact_capability_without_fallback() {
    let registry =
        WorkloadTeardownCapabilityRegistry::new([], [], []).expect("empty exact registry is valid");
    let (step, target) = target_for("registry-missing", WorkloadSagaPhase::WithdrawalCommitted);
    assert!(matches!(
        registry.select_for(step, &target),
        Err(WorkloadTeardownCapabilityRegistryError::MissingExactCapability { .. })
    ));
}

#[test]
fn registry_rejects_crossed_step_target_without_invocation() {
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let registry = teardown_capabilities(provider.clone());
    let (_, execution_target) = target_for("registry-crossed", WorkloadSagaPhase::Withdrawn);
    assert!(matches!(
        registry.select_for(WorkloadTeardownStep::WithdrawPublication, &execution_target),
        Err(WorkloadTeardownCapabilityRegistryError::ProviderTargetMismatch { .. })
    ));
    assert!(provider.calls().is_empty());
}
