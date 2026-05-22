use nimbus_core::{Error, Result};

use crate::tenant_isolation::{
    RuntimeIsolationTier, TenantIsolationDecision, TenantRuntimePolicyAdmission,
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
    pub(crate) fn for_decision(decision: &TenantIsolationDecision) -> Self {
        match decision.runtime().admission().clone() {
            TenantRuntimePolicyAdmission::AdmitInProcess => Self::InProcess,
            TenantRuntimePolicyAdmission::Route {
                recommended_tier,
                reason,
            } => Self::FallbackUnavailable {
                tier: recommended_tier,
                reason,
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
    use crate::tenant_isolation::{
        TenantIsolationContext, TenantIsolationMode, admit_runtime_invocation_decision,
    };

    fn test_context() -> TenantIsolationContext {
        TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext::anonymous(),
            "test",
        )
    }

    #[test]
    fn runtime_execution_admission_allows_production_web_standard_policy() {
        let context = test_context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let decision = admit_runtime_invocation_decision(
            &context,
            "runtime_execution",
            None,
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
            std::iter::empty::<String>(),
        )
        .expect("runtime admission decision should build");
        let admission = RuntimeExecutionAdmission::for_decision(&decision);

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
        let context = test_context();
        let decision = admit_runtime_invocation_decision(
            &context,
            "runtime_execution",
            None,
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
            std::iter::empty::<String>(),
        )
        .expect("runtime admission decision should build");
        let admission = RuntimeExecutionAdmission::for_decision(&decision);

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
