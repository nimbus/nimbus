//! Narrow deterministic fixtures for upper-crate substitution tests.

pub use crate::provider_command::{
    ProviderCommandLockTestProbe, with_provider_command_lock_test_probe,
};
pub use crate::provision::test_support::sandbox_provision_network_plan_fixture;

use crate::{
    ProviderCommandClaim, ProviderCommandOperation, SandboxBackendKind,
    SandboxExecutionTeardownCommand, SandboxExecutionTeardownOperation,
    SandboxNetworkTeardownCommand, SandboxNetworkTeardownOperation, SandboxProvisionNetworkPlan,
};

pub fn validate_network_teardown_fixture(
    backend: SandboxBackendKind,
    execution_provider_key: &str,
    attachment_provider_key: &str,
    stopped: &SandboxExecutionTeardownCommand,
    detached: &SandboxNetworkTeardownCommand,
    plan: &SandboxProvisionNetworkPlan,
) -> crate::Result<()> {
    if stopped.provider_registration_key() != execution_provider_key
        || stopped.operation() != SandboxExecutionTeardownOperation::Stop
        || stopped.provider_claim().operation() != ProviderCommandOperation::StopExecution
    {
        return Err(fixture_error(format!(
            "{backend:?} fixture requires its exact StopExecution command"
        )));
    }
    if detached.provider_registration_key() != attachment_provider_key
        || detached.operation() != SandboxNetworkTeardownOperation::Detach
        || detached.provider_claim().operation() != ProviderCommandOperation::DetachNetwork
    {
        return Err(fixture_error(format!(
            "{backend:?} fixture requires its exact DetachNetwork command"
        )));
    }
    let fence_mismatches =
        workload_fence_mismatches(stopped.provider_claim(), detached.provider_claim());
    if stopped.tenant_id() != detached.tenant_id()
        || stopped.sandbox_id() != detached.sandbox_id()
        || stopped.execution_attempt_id() != detached.execution_attempt_id()
        || !fence_mismatches.is_empty()
    {
        return Err(fixture_error(format!(
            "{backend:?} fixture commands do not share one exact workload fence: {}",
            fence_mismatches.join(", ")
        )));
    }
    if plan.tenant_id() != detached.tenant_id()
        || plan.network_plan() != detached.network_plan()
        || plan.attachment_id() != detached.attachment_id()
    {
        return Err(fixture_error(format!(
            "{backend:?} fixture provision plan is crossed with its teardown identity"
        )));
    }
    Ok(())
}

fn workload_fence_mismatches(
    left: &ProviderCommandClaim,
    right: &ProviderCommandClaim,
) -> Vec<&'static str> {
    let mut mismatches = Vec::new();
    for (label, matches) in [
        ("authority", left.authority_id() == right.authority_id()),
        (
            "source attempt",
            left.source_attempt_id() == right.source_attempt_id(),
        ),
        (
            "generation",
            left.workload_generation() == right.workload_generation(),
        ),
        (
            "restart ordinal",
            left.restart_ordinal() == right.restart_ordinal(),
        ),
        (
            "desired digest",
            left.desired_digest() == right.desired_digest(),
        ),
        (
            "source digest",
            left.source_digest() == right.source_digest(),
        ),
        (
            "network plan digest",
            left.network_plan_digest() == right.network_plan_digest(),
        ),
    ] {
        if !matches {
            mismatches.push(label);
        }
    }
    if left.same_lifecycle_fence(right) {
        debug_assert!(mismatches.is_empty());
    }
    mismatches
}

fn fixture_error(message: String) -> crate::SandboxError {
    crate::SandboxError::OperationFailed { message }
}
