use std::future::Future;
use std::pin::Pin;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::Result;
use crate::limits::RuntimeExecutionAdapterState;

mod adapter;
mod lifecycle;
mod pool;

use self::adapter::{
    BunJscExecutionAdapter, BunJscExecutionAdapterFactory, BunJscNoLinkExecutionAdapterFactory,
};
use self::pool::{BunJscPool, BunJscPoolPolicy};

#[derive(Debug, Default)]
pub(crate) struct BunJscRuntimeBackendFactory;

impl RuntimeBackendFactory for BunJscRuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        Box::new(BunJscRuntimeBackend::with_execution_adapter_factory(
            &BunJscNoLinkExecutionAdapterFactory,
        ))
    }
}

struct BunJscRuntimeBackend {
    pool: BunJscPool,
    execution_adapter: Box<dyn BunJscExecutionAdapter>,
}

impl BunJscRuntimeBackend {
    fn with_execution_adapter_factory(factory: &dyn BunJscExecutionAdapterFactory) -> Self {
        Self {
            pool: BunJscPool::new(),
            execution_adapter: factory.create(),
        }
    }

    fn execution_adapter_state(&self) -> RuntimeExecutionAdapterState {
        self.execution_adapter.state()
    }
}

impl RuntimeBackend for BunJscRuntimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + 'a>> {
        let pool_policy = BunJscPoolPolicy::from_limits(invocation.policy.limits());
        self.pool.record_admission();
        if matches!(
            self.execution_adapter_state(),
            RuntimeExecutionAdapterState::NotLinked
        ) {
            self.pool.record_disabled_invocation();
        }
        Box::pin(async move {
            let pool_policy = pool_policy?;
            BunJscPool::verify_scaffold_contract()?;
            self.execution_adapter.invoke(invocation, pool_policy).await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
    use super::lifecycle::{BunJscLifecycleAck, BunJscLifecycleState};
    use super::pool::{BunJscPoolMode, BunJscPoolPolicy};
    use super::*;
    use crate::RuntimeInvocationContext;
    use crate::executor::SharedInvocationPermit;
    use crate::host::{HostBridge, HostCallRequest};
    use crate::limits::{
        RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
        RuntimeBackendTrustTier, RuntimeJavaScriptEvaluationFormat, RuntimeLimits,
        RuntimeMemoryEnforcement, RuntimePoolKind,
    };
    use crate::runtime::{
        InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeHost,
    };
    use crate::watchdog::WatchdogTimer;

    #[derive(Debug)]
    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(crate::error::NimbusRuntimeError::Contract(
                "Bun/JSC no-link tests must not reach the host bridge".to_string(),
            ))
        }
    }

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
            memory_enforcement: RuntimeMemoryEnforcement::OuterQuotaRequired,
            runtime_pool_kind,
            ..RuntimeLimits::default()
        }
    }

    fn bun_policy() -> Arc<crate::limits::RuntimePolicy> {
        Arc::new(crate::limits::RuntimePolicy::new(
            RuntimeLimits::application_bun_jsc(),
        ))
    }

    fn bun_invocation(policy: Arc<crate::limits::RuntimePolicy>) -> RuntimeBackendInvocation {
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:bunProof".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: BTreeMap::new(),
        };
        RuntimeBackendInvocation {
            watchdog: WatchdogTimer::new(),
            host: RuntimeHost::new(Arc::new(NoopHost)),
            policy: policy.clone(),
            bundle: RuntimeBundle::new("unused-bun-jsc-proof-bundle.mjs"),
            request: request.clone(),
            context: RuntimeInvocationContext::top_level(&request),
            cancellation: None,
            permit: SharedInvocationPermit::new(policy, None, None, true, None),
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
        assert!(untrusted.product_selectable);
    }

    #[test]
    fn bun_jsc_pool_policy_rejects_profile_mismatches_before_backend_execution() {
        let mismatch = BunJscPoolPolicy::from_limits(&bun_limits_for(
            RuntimeBackendTrustTier::InProcessUntrusted,
            RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
            RuntimePoolKind::BunJscTrustedRetained,
        ));
        assert!(mismatch.is_err());

        let product_policy = crate::limits::RuntimePolicy::new(crate::RuntimeLimits {
            execution_model: crate::RuntimeExecutionModel::BackendOwnedEventLoop,
            compatibility_target: crate::RuntimeCompatibilityTarget::BunJsc,
            ..bun_limits_for(
                RuntimeBackendTrustTier::InProcessUntrusted,
                RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
                RuntimePoolKind::BunJscFreshDiscard,
            )
        });
        let policy = BunJscPoolPolicy::from_limits(product_policy.limits())
            .expect("proven untrusted Bun/JSC profile should be admissible");
        assert!(policy.product_selectable);
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
    fn bun_jsc_default_runtime_backend_uses_not_linked_adapter() {
        let backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &BunJscNoLinkExecutionAdapterFactory,
        );
        assert_eq!(
            backend.execution_adapter_state(),
            RuntimeExecutionAdapterState::NotLinked
        );
    }

    #[test]
    fn bun_jsc_default_runtime_fails_closed_without_linked_adapter() {
        let policy = bun_policy();
        let runtime = NimbusRuntime::with_policy(Arc::new(NoopHost), policy);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:bunProof".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: BTreeMap::new(),
        };
        let error = runtime
            .invoke_bundle_blocking(
                &RuntimeBundle::new("unused-bun-jsc-proof-bundle.mjs"),
                &request,
            )
            .expect_err("default Bun/JSC runtime must fail closed until an adapter is linked");

        assert!(
            error
                .to_string()
                .contains("does not link a Bun embedder execution adapter yet"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn bun_jsc_runtime_backend_dispatches_through_linked_adapter_seam() {
        #[derive(Debug, Default)]
        struct FakeLinkedExecutionAdapterFactory;

        impl BunJscExecutionAdapterFactory for FakeLinkedExecutionAdapterFactory {
            fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
                Box::new(FakeLinkedExecutionAdapter)
            }
        }

        #[derive(Debug)]
        struct FakeLinkedExecutionAdapter;

        impl BunJscExecutionAdapter for FakeLinkedExecutionAdapter {
            fn state(&self) -> RuntimeExecutionAdapterState {
                RuntimeExecutionAdapterState::Linked
            }

            fn invoke<'a>(
                &'a mut self,
                invocation: RuntimeBackendInvocation,
                pool_policy: BunJscPoolPolicy,
            ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
                assert_eq!(
                    invocation.policy.limits().backend_kind,
                    RuntimeBackendKind::BunJsc
                );
                assert_eq!(pool_policy.mode, BunJscPoolMode::FreshDiscardOuterQuota);
                Box::pin(async { Ok(json!({ "adapter": "linked" })) })
            }
        }

        let policy = bun_policy();
        let mut backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &FakeLinkedExecutionAdapterFactory,
        );
        assert_eq!(
            backend.execution_adapter_state(),
            RuntimeExecutionAdapterState::Linked
        );

        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(backend.invoke(bun_invocation(policy)))
            .expect("fake linked adapter should run");
        assert_eq!(result, json!({ "adapter": "linked" }));
    }

    #[test]
    fn bun_jsc_public_pool_envelope_does_not_depend_on_v8_internals() {
        let public_envelope_sources = [
            include_str!("adapter.rs"),
            include_str!("pool.rs"),
            include_str!("lifecycle.rs"),
        ];
        let forbidden_terms = [
            concat!("crate::backends::", "v8"),
            "JsRuntime",
            "deno_core",
            "V8WorkerRuntimePool",
            concat!("bun_jsc", "::"),
            "bun_runtime",
            "bun_core",
            "VirtualMachine",
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
