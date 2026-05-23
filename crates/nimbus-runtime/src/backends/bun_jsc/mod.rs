use std::future::Future;
use std::pin::Pin;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::Result;

mod lifecycle;
mod pool;

use self::pool::{BunJscPool, BunJscPoolPolicy};

#[derive(Debug, Default)]
pub(crate) struct BunJscRuntimeBackendFactory;

impl RuntimeBackendFactory for BunJscRuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        Box::new(BunJscRuntimeBackend {
            pool: BunJscPool::new(),
        })
    }
}

struct BunJscRuntimeBackend {
    pool: BunJscPool,
}

impl RuntimeBackend for BunJscRuntimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + 'a>> {
        let pool_policy = BunJscPoolPolicy::from_limits(invocation.policy.limits());
        self.pool.record_admission();
        self.pool.record_disabled_invocation();
        Box::pin(async move {
            pool_policy?;
            BunJscPool::verify_scaffold_contract()?;
            Err(BunJscPool::disabled_error())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::lifecycle::{BunJscLifecycleAck, BunJscLifecycleState};
    use super::pool::{BunJscPoolMode, BunJscPoolPolicy};
    use super::*;
    use crate::limits::{
        RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
        RuntimeBackendTrustTier, RuntimeJavaScriptEvaluationFormat, RuntimeLimits, RuntimePoolKind,
    };

    fn bun_limits_for(
        trust_tier: RuntimeBackendTrustTier,
        lockdown_profile: RuntimeBackendLockdownProfile,
        lifecycle_policy: RuntimeBackendLifecyclePolicy,
        runtime_pool_kind: RuntimePoolKind,
    ) -> RuntimeLimits {
        RuntimeLimits {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: trust_tier,
            backend_lockdown_profile: lockdown_profile,
            backend_lifecycle_policy: lifecycle_policy,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            runtime_pool_kind,
            ..RuntimeLimits::default()
        }
    }

    #[test]
    fn bun_jsc_pool_policy_separates_trusted_retained_from_untrusted_fresh_discard() {
        let trusted = BunJscPoolPolicy::from_limits(&bun_limits_for(
            RuntimeBackendTrustTier::InProcessTrustedOnly,
            RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
            RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            RuntimePoolKind::BunJscTrustedRetained,
        ))
        .expect("trusted generated-wrapper profile should describe retained pool policy");
        assert_eq!(trusted.mode, BunJscPoolMode::TrustedRetained);
        assert!(trusted.retained_vm_reuse_allowed);
        assert!(!trusted.outer_quota_required);
        assert!(!trusted.product_selectable);

        let untrusted = BunJscPoolPolicy::from_limits(&bun_limits_for(
            RuntimeBackendTrustTier::InProcessUntrusted,
            RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
            RuntimePoolKind::BunJscFreshDiscard,
        ))
        .expect("untrusted profile should describe fresh/discard pool policy");
        assert_eq!(untrusted.mode, BunJscPoolMode::FreshDiscardOuterQuota);
        assert!(!untrusted.retained_vm_reuse_allowed);
        assert!(untrusted.outer_quota_required);
        assert!(!untrusted.product_selectable);
    }

    #[test]
    fn bun_jsc_pool_policy_rejects_profile_mismatches_without_productizing_backend() {
        let mismatch = BunJscPoolPolicy::from_limits(&bun_limits_for(
            RuntimeBackendTrustTier::InProcessUntrusted,
            RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            RuntimePoolKind::BunJscTrustedRetained,
        ));
        assert!(mismatch.is_err());

        let product_policy = std::panic::catch_unwind(|| {
            crate::limits::RuntimePolicy::new(bun_limits_for(
                RuntimeBackendTrustTier::InProcessUntrusted,
                RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
                RuntimePoolKind::BunJscFreshDiscard,
            ))
        });
        assert!(
            product_policy.is_err(),
            "Bun/JSC pool scaffold must not make the backend selectable"
        );
    }

    #[test]
    fn bun_jsc_lifecycle_is_ack_driven_and_ordered() {
        let mut pool = BunJscPool::new();
        assert_eq!(pool.lifecycle_state(), BunJscLifecycleState::Created);

        assert!(pool.request_cancellation().is_err());
        assert_eq!(pool.lifecycle_state(), BunJscLifecycleState::Created);

        assert_eq!(
            pool.acknowledge(BunJscLifecycleAck::BootstrapReady)
                .expect("bootstrap ack should advance"),
            BunJscLifecycleState::BootstrapReady
        );
        assert_eq!(
            pool.acknowledge(BunJscLifecycleAck::GuestEntered)
                .expect("guest-entry ack should advance"),
            BunJscLifecycleState::GuestEntered
        );
        pool.record_event_loop_progress();
        pool.request_cancellation()
            .expect("cancellation is valid after guest entry");
        assert_eq!(
            pool.lifecycle_state(),
            BunJscLifecycleState::CancelRequested
        );
        assert_eq!(
            pool.acknowledge(BunJscLifecycleAck::Terminated)
                .expect("termination ack should advance"),
            BunJscLifecycleState::Terminated
        );
        assert_eq!(
            pool.acknowledge(BunJscLifecycleAck::ResetOrDiscarded)
                .expect("reset/discard ack should advance"),
            BunJscLifecycleState::ResetOrDiscarded
        );
        assert_eq!(
            pool.acknowledge(BunJscLifecycleAck::TeardownComplete)
                .expect("teardown ack should advance"),
            BunJscLifecycleState::TeardownComplete
        );

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.cancellation_requests, 1);
        assert_eq!(metrics.event_loop_progress_ticks, 1);
        assert_eq!(metrics.teardown_completions, 1);
        assert_eq!(pool.lifecycle_transition_count(), 6);
    }

    #[test]
    fn bun_jsc_public_pool_envelope_does_not_depend_on_v8_internals() {
        let public_envelope_sources = [include_str!("pool.rs"), include_str!("lifecycle.rs")];
        let forbidden_terms = [
            concat!("crate::backends::", "v8"),
            "JsRuntime",
            "deno_core",
            "V8WorkerRuntimePool",
        ];

        for source in public_envelope_sources {
            for forbidden in forbidden_terms {
                assert!(
                    !source.contains(forbidden),
                    "Bun/JSC pool envelope must not depend on {forbidden}"
                );
            }
        }
    }
}
