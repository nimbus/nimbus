use nimbus_core::{Result, TenantId};
use nimbus_runtime::{RuntimeBundle, RuntimePolicy};
use nimbus_tenant_context::TenantIsolationContext;
use std::collections::BTreeSet;

use super::{
    RuntimeIsolationTier, RuntimePolicyAdmission, TenantIsolationDecision, TenantIsolationMode,
    TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
    WorkloadAttributes, runtime_admission,
};

/// Admission checks on a [`TenantIsolationContext`] that need runtime and
/// sandbox types. They live here so `nimbus-tenant-context` stays a leaf crate.
pub trait TenantIsolationContextExt {
    fn admit_decision(&self, input: TenantIsolationPolicyInput) -> Result<TenantIsolationDecision>;

    fn ensure_runtime_bundle_matches(&self, bundle: &RuntimeBundle, context: &str) -> Result<()>;
}

impl TenantIsolationContextExt for TenantIsolationContext {
    fn admit_decision(&self, input: TenantIsolationPolicyInput) -> Result<TenantIsolationDecision> {
        TenantIsolationDecision::admit(self, input)
    }

    fn ensure_runtime_bundle_matches(&self, bundle: &RuntimeBundle, context: &str) -> Result<()> {
        let Some(tenant_label) = bundle.identity().tenant_label() else {
            return Ok(());
        };
        let actual = TenantId::new(tenant_label.to_string())?;
        self.ensure_tenant_matches(&actual, context)
    }
}

pub(crate) trait RuntimePolicyAdmissionExt {
    fn admit_runtime_policy(
        &self,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
    ) -> RuntimePolicyAdmission;
}

impl RuntimePolicyAdmissionExt for TenantIsolationContext {
    fn admit_runtime_policy(
        &self,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
    ) -> RuntimePolicyAdmission {
        if !matches!(mode, TenantIsolationMode::Production) {
            return RuntimePolicyAdmission::AdmitInProcess;
        }
        if !matches!(tier, RuntimeIsolationTier::InProcessUntrusted) {
            return RuntimePolicyAdmission::AdmitInProcess;
        }
        match runtime_admission::validate_production_in_process_untrusted_policy(policy.limits()) {
            Ok(()) => RuntimePolicyAdmission::AdmitInProcess,
            Err(rejection) => RuntimePolicyAdmission::Route(rejection.into_route()),
        }
    }
}

pub fn admit_runtime_invocation_decision(
    context: &TenantIsolationContext,
    function_name: &str,
    invocation_id: Option<&str>,
    policy: &RuntimePolicy,
    tier: RuntimeIsolationTier,
    mode: TenantIsolationMode,
    service_names: impl IntoIterator<Item = String>,
) -> Result<TenantIsolationDecision> {
    let mut admitted_services = BTreeSet::new();
    admitted_services.extend(policy.limits().grants.service.iter().cloned());
    admitted_services.extend(service_names);
    let mut workload = WorkloadAttributes::runtime_function(function_name, tier);
    if let Some(invocation_id) = invocation_id {
        workload = workload.with_invocation_id(invocation_id);
    }
    context.admit_decision(
        TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(context, policy, tier, mode)
            .with_services(TenantServiceGrantPolicyDecision::new(admitted_services))
            .with_storage(TenantStoragePolicyDecision::namespace(
                context.tenant_id().as_str(),
            )),
    )
}
