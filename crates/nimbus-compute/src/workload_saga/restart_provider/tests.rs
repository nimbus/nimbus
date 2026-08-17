use super::*;
use nimbus_network::NetworkProviderId;

struct InertRestartProvider;

fn succeeded(
    command: &ConfirmedWorkloadRestartCommand,
) -> WorkloadRestartCapabilityFuture<'static> {
    let observation = exact_observation(
        command,
        WorkloadRestartCommandOutcome::Succeeded {
            evidence: nimbus_workloads::WorkloadRestartEvidenceDigest::sha256("inert-provider"),
        },
    );
    Box::pin(async move { observation })
}

fn exact_observation(
    command: &ConfirmedWorkloadRestartCommand,
    outcome: WorkloadRestartCommandOutcome,
) -> WorkloadRestartProviderObservation {
    WorkloadRestartProviderObservation::new(WorkloadRestartProviderObservationInput {
        command_id: command.command_id().clone(),
        transition_id: command.transition_id().clone(),
        generation: command.generation(),
        desired_digest: command.desired_digest(),
        request_id: command.request_id().clone(),
        source_attempt_id: command.source_attempt_id().clone(),
        attempt_id: command.attempt_id().clone(),
        restart_epoch: command.restart_epoch(),
        dispatch_epoch: command.dispatch_epoch(),
        provider_selection: command.provider_selection().clone(),
        outcome,
    })
}

macro_rules! effect_capability {
    ($capability:ident) => {
        impl $capability for InertRestartProvider {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                succeeded(command)
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                succeeded(command)
            }
        }
    };
}

macro_rules! inspection_capability {
    ($capability:ident) => {
        impl $capability for InertRestartProvider {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                succeeded(command)
            }
        }
    };
}

effect_capability!(RestartPublicationWithdrawalCapability);
effect_capability!(WorkloadExecutionQuiescenceCapability);
effect_capability!(WorkloadRestartPreparationCapability);
effect_capability!(NetworkRestartAttachmentCapability);
inspection_capability!(WorkloadRestartActivationPrerequisiteCapability);
effect_capability!(WorkloadRestartActivationCapability);
inspection_capability!(WorkloadRestartReadinessCapability);
effect_capability!(RestartPublicationCapability);
inspection_capability!(RestartPublicationObservationCapability);

fn provider_id(label: &str) -> WorkloadExecutionProviderId {
    WorkloadExecutionProviderId::for_registration_key(label)
}

fn capabilities(execution_provider_id: WorkloadExecutionProviderId) -> WorkloadRestartCapabilities {
    capabilities_for_realm(execution_provider_id, None)
}

fn capabilities_for_realm(
    execution_provider_id: WorkloadExecutionProviderId,
    network_selection: Option<NetworkCapabilitySelection>,
) -> WorkloadRestartCapabilities {
    WorkloadRestartCapabilities::new(
        execution_provider_id,
        network_selection,
        Arc::new(InertRestartProvider),
        Arc::new(InertRestartProvider),
        Arc::new(InertRestartProvider),
    )
}

#[test]
fn restart_registry_rejects_duplicate_provider_selection() {
    let duplicate = provider_id("restart-duplicate");
    let result = WorkloadRestartCapabilityRegistry::new([
        capabilities(duplicate.clone()),
        capabilities(duplicate.clone()),
    ]);

    assert!(matches!(
        result,
        Err(
            WorkloadRestartCapabilityRegistryError::DuplicateProviderSelection {
                execution_provider_id,
                network_selection: None,
            }
        ) if execution_provider_id == duplicate
    ));
}

#[test]
fn same_execution_provider_can_serve_distinct_exact_network_realms() {
    let execution = provider_id("restart-multi-realm");
    let left = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("left-attachment"),
        NetworkProviderId::for_registration_key("left-ingress"),
    );
    let right = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("right-attachment"),
        NetworkProviderId::for_registration_key("right-ingress"),
    );
    let registry = WorkloadRestartCapabilityRegistry::new([
        capabilities_for_realm(execution.clone(), Some(left.clone())),
        capabilities_for_realm(execution.clone(), Some(right.clone())),
    ])
    .expect("the complete provider realm, not execution alone, is unique");

    assert!(
        registry
            .providers
            .contains_key(&(execution.clone(), Some(left)))
    );
    assert!(registry.providers.contains_key(&(execution, Some(right))));
}

#[test]
fn restart_registry_has_no_first_available_fallback() {
    let registered = provider_id("restart-registered");
    let missing = provider_id("restart-missing");
    let registry = WorkloadRestartCapabilityRegistry::new([capabilities(registered)])
        .expect("one exact restart provider should register");
    assert!(!registry.providers.contains_key(&(missing, None)));
}
