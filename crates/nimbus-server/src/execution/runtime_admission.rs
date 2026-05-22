use nimbus_core::{Error, Result};
use nimbus_runtime::RuntimePolicy;

use crate::tenant_isolation::{
    RuntimeIsolationTier, RuntimePolicyAdmission, TenantIsolationContext, TenantIsolationMode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeExecutionAdmission {
    InProcess,
    FallbackUnavailable {
        tier: RuntimeIsolationTier,
        reason: String,
    },
}

impl RuntimeExecutionAdmission {
    pub(crate) fn for_policy(
        isolation: &TenantIsolationContext,
        policy: &RuntimePolicy,
        tier: RuntimeIsolationTier,
        mode: TenantIsolationMode,
    ) -> Self {
        match isolation.admit_runtime_policy(policy, tier, mode) {
            RuntimePolicyAdmission::AdmitInProcess => Self::InProcess,
            RuntimePolicyAdmission::Route(route) => Self::FallbackUnavailable {
                tier: route.recommended_tier(),
                reason: route.reason().to_string(),
            },
        }
    }

    pub(crate) fn ensure_in_process_available(self, context: &str) -> Result<()> {
        match self {
            Self::InProcess => Ok(()),
            Self::FallbackUnavailable { tier, reason } => Err(Error::InvalidInput(format!(
                "runtime fallback route {} for {context} is not configured; fail closed instead of running unsafe in-process policy: {reason}",
                tier.label()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_runtime::{RuntimeGrants, RuntimeLimits, RuntimePolicy};

    use super::*;

    fn test_context() -> TenantIsolationContext {
        TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        )
    }

    #[test]
    fn runtime_execution_admission_allows_production_web_standard_policy() {
        let admission = RuntimeExecutionAdmission::for_policy(
            &test_context(),
            &RuntimePolicy::new(RuntimeLimits::application_web_standard()),
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        );

        assert_eq!(admission, RuntimeExecutionAdmission::InProcess);
        admission
            .ensure_in_process_available("runtime invocation")
            .expect("admitted policy should run in process");
    }

    #[test]
    fn runtime_execution_admission_fails_closed_when_fallback_route_is_unavailable() {
        let policy = RuntimePolicy::new(RuntimeLimits {
            grants: RuntimeGrants {
                run: vec!["npm".to_string()],
                ..RuntimeGrants::application_web_standard()
            },
            ..RuntimeLimits::application_node22()
        });
        let admission = RuntimeExecutionAdmission::for_policy(
            &test_context(),
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        );

        assert!(matches!(
            admission,
            RuntimeExecutionAdmission::FallbackUnavailable {
                tier: RuntimeIsolationTier::MicroVmService,
                ..
            }
        ));
        let error = admission
            .ensure_in_process_available("runtime invocation")
            .expect_err("unavailable fallback route must fail closed");
        assert!(
            error.to_string().contains("fail closed"),
            "error should make fail-closed behavior explicit: {error}"
        );
        assert!(
            error.to_string().contains("microvm_service"),
            "error should name the unavailable fallback tier: {error}"
        );
    }
}
