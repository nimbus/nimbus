use std::future::Future;
use std::pin::Pin;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::{NimbusRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::{RuntimeExecutionAdapterArtifactDiagnostics, RuntimeExecutionAdapterState};

mod adapter;
mod contract;
mod lifecycle;
#[cfg(feature = "bun-jsc-linked-adapter")]
mod linked;
#[cfg(feature = "bun-jsc-linked-adapter")]
mod manifest;
mod pool;

#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
use self::adapter::BunJscNoLinkExecutionAdapterFactory;
use self::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
use self::lifecycle::BunJscLifecycleAck;
use self::pool::{BunJscPool, BunJscPoolPolicy};

#[derive(Debug, Default)]
pub(crate) struct BunJscRuntimeBackendFactory;

impl RuntimeBackendFactory for BunJscRuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        #[cfg(feature = "bun-jsc-linked-adapter")]
        let factory = &linked::BunJscLinkedExecutionAdapterFactory;
        #[cfg(not(feature = "bun-jsc-linked-adapter"))]
        let factory = &BunJscNoLinkExecutionAdapterFactory;

        Box::new(BunJscRuntimeBackend::with_execution_adapter_factory(
            factory,
        ))
    }
}

pub(crate) fn execution_adapter_state() -> RuntimeExecutionAdapterState {
    #[cfg(feature = "bun-jsc-linked-adapter")]
    {
        let factory = &linked::BunJscLinkedExecutionAdapterFactory;
        let adapter = factory.create();
        adapter.state()
    }
    #[cfg(not(feature = "bun-jsc-linked-adapter"))]
    {
        RuntimeExecutionAdapterState::NotLinked
    }
}

pub(crate) fn adapter_artifact_diagnostics() -> RuntimeExecutionAdapterArtifactDiagnostics {
    #[cfg(feature = "bun-jsc-linked-adapter")]
    {
        linked::execution_adapter_artifact_diagnostics()
    }
    #[cfg(not(feature = "bun-jsc-linked-adapter"))]
    {
        contract::disabled_build_diagnostics()
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
        let adapter_state = self.execution_adapter_state();
        self.pool.record_admission();
        if matches!(adapter_state, RuntimeExecutionAdapterState::NotLinked) {
            self.pool.record_disabled_invocation();
        }
        Box::pin(async move {
            let pool_policy = pool_policy?;
            BunJscPool::verify_scaffold_contract()?;
            if invocation
                .cancellation
                .as_ref()
                .is_some_and(HostCallCancellation::is_cancelled)
            {
                return Err(NimbusRuntimeError::Cancelled);
            }
            if !matches!(adapter_state, RuntimeExecutionAdapterState::Linked) {
                return self.execution_adapter.invoke(invocation, pool_policy).await;
            }

            self.pool.begin_invocation();
            self.pool.acknowledge(BunJscLifecycleAck::BootstrapReady)?;
            self.pool.acknowledge(BunJscLifecycleAck::GuestEntered)?;
            let result = self.execution_adapter.invoke(invocation, pool_policy).await;
            if matches!(result, Err(NimbusRuntimeError::Cancelled)) {
                self.pool.request_cancellation()?;
            }
            let teardown = (|| {
                self.pool.acknowledge(BunJscLifecycleAck::Terminated)?;
                self.pool
                    .acknowledge(BunJscLifecycleAck::ResetOrDiscarded)?;
                self.pool
                    .acknowledge(BunJscLifecycleAck::TeardownComplete)?;
                Ok(())
            })();

            match (result, teardown) {
                (Ok(value), Ok(())) => Ok(value),
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
            }
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
    use crate::host::{HostBridge, HostCallCancellation, HostCallRequest};
    use crate::limits::{
        RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
        RuntimeBackendTrustTier, RuntimeJavaScriptEvaluationFormat, RuntimeLimits,
        RuntimeMemoryEnforcement, RuntimePoolKind,
    };
    #[cfg(not(feature = "bun-jsc-linked-adapter"))]
    use crate::limits::{
        RuntimeExecutionAdapterArtifactSource, RuntimeExecutionAdapterArtifactStatus,
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
        bun_invocation_with_cancellation(policy, None)
    }

    fn bun_invocation_with_cancellation(
        policy: Arc<crate::limits::RuntimePolicy>,
        cancellation: Option<HostCallCancellation>,
    ) -> RuntimeBackendInvocation {
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
            cancellation,
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

    #[cfg(not(feature = "bun-jsc-linked-adapter"))]
    #[test]
    fn bun_jsc_default_runtime_backend_reports_sanitized_artifact_diagnostics() {
        let diagnostics = adapter_artifact_diagnostics();

        assert_eq!(
            diagnostics.status,
            RuntimeExecutionAdapterArtifactStatus::NotLinked
        );
        assert_eq!(
            diagnostics.source,
            RuntimeExecutionAdapterArtifactSource::BuildFeatureDisabled
        );
        assert_eq!(diagnostics.reason_code, "linked_adapter_feature_disabled");
        assert_eq!(
            diagnostics
                .expected
                .as_ref()
                .expect("expected Bun/JSC artifact contract should be present")
                .source_ref,
            "bun-v1.4.0-nimbus.5"
        );
        assert!(diagnostics.manifest.is_none());
    }

    #[cfg(not(nimbus_bun_jsc_shared_adapter))]
    #[test]
    fn bun_jsc_default_runtime_fails_closed_without_loaded_shared_adapter() {
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

        #[cfg(feature = "bun-jsc-linked-adapter")]
        let expected_error = "set NIMBUS_BUN_EMBED_SHARED_LIBRARY";
        #[cfg(not(feature = "bun-jsc-linked-adapter"))]
        let expected_error = "does not link a Bun embedder execution adapter yet";
        assert!(
            error.to_string().contains(expected_error),
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
    fn bun_jsc_linked_backend_rejects_pre_cancelled_invocation_before_guest_entry() {
        #[derive(Debug, Default)]
        struct PanicIfInvokedAdapterFactory;

        impl BunJscExecutionAdapterFactory for PanicIfInvokedAdapterFactory {
            fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
                Box::new(PanicIfInvokedAdapter)
            }
        }

        #[derive(Debug)]
        struct PanicIfInvokedAdapter;

        impl BunJscExecutionAdapter for PanicIfInvokedAdapter {
            fn state(&self) -> RuntimeExecutionAdapterState {
                RuntimeExecutionAdapterState::Linked
            }

            fn invoke<'a>(
                &'a mut self,
                _invocation: RuntimeBackendInvocation,
                _pool_policy: BunJscPoolPolicy,
            ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
                panic!("pre-cancelled Bun/JSC invocation must not enter the adapter")
            }
        }

        let cancellation = HostCallCancellation::default();
        cancellation.cancel();
        let policy = bun_policy();
        let mut backend =
            BunJscRuntimeBackend::with_execution_adapter_factory(&PanicIfInvokedAdapterFactory);
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(backend.invoke(bun_invocation_with_cancellation(policy, Some(cancellation))))
            .expect_err("pre-cancelled invocation should fail before guest entry");

        assert!(matches!(error, NimbusRuntimeError::Cancelled));
        assert_eq!(
            backend.pool.lifecycle_state(),
            BunJscLifecycleState::Created
        );
        assert_eq!(backend.pool.lifecycle_transition_count(), 0);
        let metrics = backend.pool.metrics_snapshot();
        assert_eq!(metrics.admitted_invocations, 1);
        assert_eq!(metrics.cancellation_requests, 0);
        assert_eq!(metrics.teardown_completions, 0);
    }

    #[test]
    fn bun_jsc_linked_backend_records_cancelled_adapter_then_tears_down() {
        #[derive(Debug, Default)]
        struct CancellingAdapterFactory;

        impl BunJscExecutionAdapterFactory for CancellingAdapterFactory {
            fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
                Box::new(CancellingAdapter)
            }
        }

        #[derive(Debug)]
        struct CancellingAdapter;

        impl BunJscExecutionAdapter for CancellingAdapter {
            fn state(&self) -> RuntimeExecutionAdapterState {
                RuntimeExecutionAdapterState::Linked
            }

            fn invoke<'a>(
                &'a mut self,
                _invocation: RuntimeBackendInvocation,
                _pool_policy: BunJscPoolPolicy,
            ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
                Box::pin(async { Err(NimbusRuntimeError::Cancelled) })
            }
        }

        let policy = bun_policy();
        let mut backend =
            BunJscRuntimeBackend::with_execution_adapter_factory(&CancellingAdapterFactory);
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(backend.invoke(bun_invocation(policy)))
            .expect_err("cancelled adapter result should surface as cancellation");

        assert!(matches!(error, NimbusRuntimeError::Cancelled));
        assert_eq!(
            backend.pool.lifecycle_state(),
            BunJscLifecycleState::TeardownComplete
        );
        assert_eq!(backend.pool.lifecycle_transition_count(), 6);
        let metrics = backend.pool.metrics_snapshot();
        assert_eq!(metrics.cancellation_requests, 1);
        assert_eq!(metrics.teardown_completions, 1);
    }

    #[cfg(feature = "bun-jsc-linked-adapter")]
    #[test]
    fn bun_jsc_linked_adapter_feature_names_reproducible_bun_source() {
        let contract = linked::BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
        assert_eq!(contract.repository, "https://github.com/nimbus/bun");
        assert_eq!(contract.source_ref, "bun-v1.4.0-nimbus.5");
        assert_eq!(
            contract.git_revision,
            "ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57"
        );
        assert_eq!(contract.proof_target, "check-bun-embed-shared");
        assert_eq!(contract.simdutf_namespace, "nimbus_bun_simdutf");
        assert_eq!(contract.required_exports.len(), 11);
        assert!(
            contract
                .required_exports
                .contains(&"nimbus_bun_embed_probe_program_bundle_host_calls")
        );
        assert!(
            contract
                .required_exports
                .contains(&"nimbus_bun_embed_probe_lifecycle_reuse_stress")
        );
        assert!(
            contract
                .required_exports
                .contains(&"nimbus_bun_embed_invoke_program_wrapper_json")
        );
        assert!(
            contract
                .required_exports
                .contains(&"nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge")
        );
    }

    #[cfg(feature = "bun-jsc-linked-adapter")]
    #[test]
    fn bun_jsc_linked_adapter_feature_reports_shared_library_gated_link_state() {
        let linked_backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &linked::BunJscLinkedExecutionAdapterFactory,
        );
        #[cfg(nimbus_bun_jsc_shared_adapter)]
        let expected_state = RuntimeExecutionAdapterState::Linked;
        #[cfg(not(nimbus_bun_jsc_shared_adapter))]
        let expected_state = RuntimeExecutionAdapterState::NotLinked;
        assert_eq!(linked_backend.execution_adapter_state(), expected_state);
    }

    #[cfg(all(feature = "bun-jsc-linked-adapter", not(nimbus_bun_jsc_shared_adapter)))]
    #[test]
    fn bun_jsc_linked_adapter_feature_requires_explicit_shared_library_for_execution() {
        let policy = bun_policy();
        let mut linked_backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &linked::BunJscLinkedExecutionAdapterFactory,
        );
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(linked_backend.invoke(bun_invocation(policy)))
            .expect_err("linked Bun/JSC execution must require an explicit shared library");
        assert!(
            error
                .to_string()
                .contains("set NIMBUS_BUN_EMBED_SHARED_LIBRARY"),
            "unexpected error: {error}"
        );
    }

    #[cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]
    #[test]
    fn bun_jsc_linked_adapter_executes_pure_program_wrapper_json_through_pool() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let bundle_path = temp_dir.path().join("bun-pure-program-wrapper.js");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__nimbusInvoke = async function(request) {
  return {
    status: "ok",
    value: {
      engine: "bun_jsc",
      kind: request.kind,
      functionName: request.function_name,
      body: request.args.body,
      nested: request.args.nested.value,
    },
  };
};
"#,
        )
        .expect("bundle should be written");

        let policy = bun_policy();
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:bunProof".to_string(),
            args: json!({
                "body": "hello from linked bun",
                "nested": { "value": 42 },
            }),
            page_size: None,
            cursor: None,
            auth: None,
            services: BTreeMap::new(),
        };
        let mut backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &linked::BunJscLinkedExecutionAdapterFactory,
        );

        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(backend.invoke(RuntimeBackendInvocation {
                watchdog: WatchdogTimer::new(),
                host: RuntimeHost::new(Arc::new(NoopHost)),
                policy: policy.clone(),
                bundle: RuntimeBundle::new(&bundle_path),
                request: request.clone(),
                context: RuntimeInvocationContext::top_level(&request),
                cancellation: None,
                permit: SharedInvocationPermit::new(policy, None, None, true, None),
            }))
            .expect("linked Bun/JSC pure invocation should run");

        assert_eq!(
            result,
            json!({
                "status": "ok",
                "value": {
                    "engine": "bun_jsc",
                    "kind": "query",
                    "functionName": "messages:bunProof",
                    "body": "hello from linked bun",
                    "nested": 42,
                },
            })
        );
        assert_eq!(
            backend.pool.lifecycle_state(),
            BunJscLifecycleState::TeardownComplete
        );
        assert_eq!(backend.pool.lifecycle_transition_count(), 5);
        let metrics = backend.pool.metrics_snapshot();
        assert_eq!(metrics.admitted_invocations, 1);
        assert_eq!(metrics.disabled_invocations, 0);
        assert_eq!(metrics.teardown_completions, 1);
    }

    #[cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]
    #[test]
    fn bun_jsc_linked_adapter_coexists_with_v8_backend_in_same_process() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let v8_bundle_path = temp_dir.path().join("v8-bundle.mjs");
        std::fs::write(
            &v8_bundle_path,
            r#"
globalThis.__nimbusInvoke = async function(request) {
  return {
    engine: "v8",
    functionName: request.function_name,
    body: request.args.body,
  };
};

export {};
"#,
        )
        .expect("V8 bundle should be written");
        let bun_bundle_path = temp_dir.path().join("bun-program-wrapper.js");
        std::fs::write(
            &bun_bundle_path,
            r#"
globalThis.__nimbusInvoke = async function(request) {
  return {
    status: "ok",
    value: {
      engine: "bun_jsc",
      functionName: request.function_name,
      body: request.args.body,
    },
  };
};
"#,
        )
        .expect("Bun/JSC bundle should be written");

        let v8_policy = Arc::new(crate::limits::RuntimePolicy::new(
            RuntimeLimits::application_web_standard(),
        ));
        let v8_runtime = NimbusRuntime::with_policy(Arc::new(NoopHost), v8_policy);
        let v8_request = |body: &str| InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:v8Proof".to_string(),
            args: json!({ "body": body }),
            page_size: None,
            cursor: None,
            auth: None,
            services: BTreeMap::new(),
        };

        let first_v8_result = v8_runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&v8_bundle_path), &v8_request("before"))
            .expect("V8 invocation before Bun/JSC should run");
        assert_eq!(
            first_v8_result,
            json!({
                "engine": "v8",
                "functionName": "messages:v8Proof",
                "body": "before",
            })
        );

        let bun_policy = bun_policy();
        let bun_request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:bunProof".to_string(),
            args: json!({ "body": "between" }),
            page_size: None,
            cursor: None,
            auth: None,
            services: BTreeMap::new(),
        };
        let mut bun_backend = BunJscRuntimeBackend::with_execution_adapter_factory(
            &linked::BunJscLinkedExecutionAdapterFactory,
        );
        let bun_result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build")
            .block_on(bun_backend.invoke(RuntimeBackendInvocation {
                watchdog: WatchdogTimer::new(),
                host: RuntimeHost::new(Arc::new(NoopHost)),
                policy: bun_policy.clone(),
                bundle: RuntimeBundle::new(&bun_bundle_path),
                request: bun_request.clone(),
                context: RuntimeInvocationContext::top_level(&bun_request),
                cancellation: None,
                permit: SharedInvocationPermit::new(bun_policy, None, None, true, None),
            }))
            .expect("linked Bun/JSC invocation should run after V8");
        assert_eq!(
            bun_result,
            json!({
                "status": "ok",
                "value": {
                    "engine": "bun_jsc",
                    "functionName": "messages:bunProof",
                    "body": "between",
                },
            })
        );

        let second_v8_result = v8_runtime
            .invoke_bundle_blocking(&RuntimeBundle::new(&v8_bundle_path), &v8_request("after"))
            .expect("V8 invocation after Bun/JSC should still run");
        assert_eq!(
            second_v8_result,
            json!({
                "engine": "v8",
                "functionName": "messages:v8Proof",
                "body": "after",
            })
        );
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
