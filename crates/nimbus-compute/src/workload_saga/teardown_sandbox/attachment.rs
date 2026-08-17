//! Exact lowering for sandbox-owned network attachment teardown.

use nimbus_sandbox::backends::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
};
use nimbus_sandbox::{
    ProviderCommandOperation, SandboxBackendKind, SandboxExecutionAttemptId, SandboxId,
    SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownOperation, sandbox_network_plan_requirements,
};
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects,
};

use super::{ConfirmedWorkloadTeardownCommand, crossed_command_failure, invalid_command_failure};
use crate::workload_saga::provision_sandbox::sandbox_execution_provider_id;
use crate::workload_saga::teardown_provider_command::ConfirmedTeardownProviderCommand;

/// Validated lower command for one exact attachment detach or release.
#[derive(Debug)]
pub(super) struct ValidatedSandboxNetworkTeardownCommand {
    sandbox_command: SandboxNetworkTeardownCommand,
    provider_command: ConfirmedTeardownProviderCommand,
}

impl ValidatedSandboxNetworkTeardownCommand {
    pub(super) fn sandbox_command(&self) -> &SandboxNetworkTeardownCommand {
        &self.sandbox_command
    }

    pub(super) fn provider_command(&self) -> &ConfirmedTeardownProviderCommand {
        &self.provider_command
    }
}

/// Authenticate and lower one compute-confirmed attachment teardown command.
pub(super) fn validate_sandbox_network_teardown_command(
    command: &ConfirmedWorkloadTeardownCommand,
    backend: SandboxBackendKind,
) -> Result<ValidatedSandboxNetworkTeardownCommand, WorkloadFailureEvidence> {
    let requirements = sandbox_network_plan_requirements(backend);
    let expected_provider = requirements.required_attachment_provider_id();
    let WorkloadTeardownProviderTarget::Attachment {
        provider_id,
        provider_source_digest,
    } = command.provider_target()
    else {
        return Err(invalid_command_failure(
            "sandbox network teardown requires an attachment provider target",
        ));
    };

    let compiled = command.compiled_network_plan();
    let content = compiled.content();
    let Some(selection) = content.capability_selection() else {
        return Err(invalid_command_failure(
            "sandbox network teardown requires an admitted capability selection",
        ));
    };
    let Some(selection_evidence) = content.capability_selection_evidence() else {
        return Err(invalid_command_failure(
            "sandbox network teardown requires capability source evidence",
        ));
    };
    if command.selection_evidence() != Some(selection_evidence)
        || selection_evidence.selection() != selection
        || selection.attachment_provider_id() != expected_provider
        || provider_id != expected_provider
        || *provider_source_digest != selection_evidence.source_digest()
    {
        return Err(crossed_command_failure(
            "sandbox attachment provider is crossed with confirmed capability evidence",
        ));
    }
    if command.source().execution_provider_id() != &sandbox_execution_provider_id(backend) {
        return Err(crossed_command_failure(
            "sandbox attachment backend is crossed with execution source evidence",
        ));
    }

    let operation = match command.step() {
        WorkloadTeardownStep::DetachNetwork => SandboxNetworkTeardownOperation::Detach,
        WorkloadTeardownStep::ReleaseNetwork => SandboxNetworkTeardownOperation::Release,
        _ => {
            return Err(invalid_command_failure(
                "sandbox network teardown supports only detach and release",
            ));
        }
    };
    let WorkloadTeardownSubjects::Network(subject) = command.subjects() else {
        return Err(invalid_command_failure(
            "sandbox network teardown requires a network subject",
        ));
    };
    if subject.plan_id() != compiled.plan().plan_id()
        || subject.generation() != compiled.plan().generation()
        || subject.digest() != compiled.plan().digest()
        || command.network_plan_digest() != compiled.plan().digest()
        || content.identity().tenant_id() != command.key().tenant_id()
        || content.identity().generation() != compiled.plan().generation()
        || command.generation().as_u64() != compiled.plan().generation().as_u64()
    {
        return Err(crossed_command_failure(
            "sandbox network subject is crossed with confirmed plan identity",
        ));
    }
    let Some(attachment) = content.attachment() else {
        return Err(invalid_command_failure(
            "sandbox network teardown requires one compiled attachment",
        ));
    };

    let locator = command.execution_locator();
    if locator.node_identity() != command.required_node()
        || locator.generation() != command.generation()
        || locator.desired_digest() != command.desired_digest()
    {
        return Err(crossed_command_failure(
            "sandbox network execution locator is crossed with confirmed command fences",
        ));
    }

    let provider_operation = match operation {
        SandboxNetworkTeardownOperation::Detach => ProviderCommandOperation::DetachNetwork,
        SandboxNetworkTeardownOperation::Release => ProviderCommandOperation::ReleaseNetwork,
    };
    let provider_key = match backend {
        SandboxBackendKind::Container => CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        SandboxBackendKind::Krun => KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
    };
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: command.key().tenant_id().clone(),
        sandbox_id: SandboxId::new(locator.execution_id().as_str()),
        execution_attempt_id: SandboxExecutionAttemptId::new(locator.attempt_id().to_string())
            .map_err(|error| invalid_command_failure(error.to_string()))?,
        attachment_id: attachment.attachment_id().clone(),
        network_plan: compiled.plan().clone(),
        provider_registration_key: provider_key.to_owned(),
        provider_source_digest: *provider_source_digest,
    })
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    let provider_command = ConfirmedTeardownProviderCommand::new(
        command,
        identity.provider_effect_subject(),
        identity.provider_target_digest(),
    )
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    if provider_command.claim().operation() != provider_operation {
        return Err(invalid_command_failure(
            "sandbox network teardown operation crosses the confirmed provider operation",
        ));
    }
    let sandbox_command = SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim: provider_command.claim().clone(),
    })
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    Ok(ValidatedSandboxNetworkTeardownCommand {
        sandbox_command,
        provider_command,
    })
}
