use std::sync::Arc;

use nimbus_core::{Error, InvocationAuth};
use nimbus_runtime::{
    HostCallCancellation, InvocationKind, InvocationRequest, InvocationServices, RuntimeBundle,
};
use serde_json::Value;

use crate::adapters::convex::host_bridge::ConvexRuntimeResponseEnvelope;
use crate::adapters::convex::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope, ConvexRegistry,
    RuntimeReadSet,
};
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_on_worker_with_host_state,
};
use nimbus_auth::normalize_principal_context;
use nimbus_bridge::admission::RuntimeExecutionAdmission;
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
    admit_runtime_invocation_decision,
};

use super::super::super::runtime_error_to_core;

pub(in crate::adapters::convex) struct RuntimeInvocationContext<'a> {
    engine: &'a Arc<nimbus_engine::Engine>,
    registry: &'a Arc<ConvexRegistry>,
    runtime_service_registry: &'a Arc<dyn RuntimeServiceRegistry>,
    isolation: TenantIsolationContext,
    tenant_isolation_mode: TenantIsolationMode,
}

impl<'a> RuntimeInvocationContext<'a> {
    pub(in crate::adapters::convex) fn new(
        engine: &'a Arc<nimbus_engine::Engine>,
        registry: &'a Arc<ConvexRegistry>,
        runtime_service_registry: &'a Arc<dyn RuntimeServiceRegistry>,
        isolation: TenantIsolationContext,
        tenant_isolation_mode: TenantIsolationMode,
    ) -> Self {
        Self {
            engine,
            registry,
            runtime_service_registry,
            isolation,
            tenant_isolation_mode,
        }
    }

    pub(in crate::adapters::convex) fn runtime_services(&self) -> InvocationServices {
        self.runtime_service_registry
            .snapshot_for_tenant(self.isolation.tenant_id())
    }

    pub(in crate::adapters::convex) fn required_runtime_bundle_for_function(
        &self,
        function_name: &str,
    ) -> Result<RuntimeBundle, Error> {
        let bundle = self
            .registry
            .required_runtime_bundle_for_function(function_name)?;
        self.isolation
            .ensure_runtime_bundle_matches(&bundle, "convex runtime bundle")?;
        Ok(bundle)
    }

    pub(in crate::adapters::convex) async fn invoke_with_trace_async_cancellable(
        &self,
        request: InvocationRequest,
        cancellation: HostCallCancellation,
        server_request_id: Option<String>,
    ) -> Result<(Value, RuntimeReadSet), Error> {
        let bundle = self.required_runtime_bundle_for_function(&request.function_name)?;
        let invocation_kind = request.kind.clone();
        let (runtime_executor, runtime_policy) = self
            .registry
            .runtime_lane_for_function(&request.function_name)?;
        let decision = admit_runtime_invocation_decision(
            &self.isolation,
            &request.function_name,
            server_request_id.as_deref(),
            &runtime_policy,
            RuntimeIsolationTier::InProcessUntrusted,
            self.tenant_isolation_mode,
            request.services.keys().cloned(),
        )
        .map_err(|error| {
            Error::InvalidInput(format!(
                "tenant isolation decision rejected convex runtime invocation: {error}"
            ))
        })?;
        decision.ensure_runtime_bundle_matches(&bundle, "convex runtime bundle")?;
        RuntimeExecutionAdmission::for_decision(&decision)
            .ensure_in_process_available("convex runtime invocation")?;
        let auth = request
            .auth
            .clone()
            .map(serde_json::from_value::<InvocationAuth>)
            .transpose()
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let bridge = Arc::new(ConvexHostBridge::build(
            ConvexHostBridgeScope::new(
                self.engine.clone(),
                self.registry.clone(),
                decision.clone(),
                self.runtime_service_registry.clone(),
            ),
            ConvexHostBridgeInvocation::new(
                auth.clone(),
                request.services.clone(),
                normalize_principal_context(auth.as_ref()),
                server_request_id.clone(),
                invocation_kind.clone(),
                request.function_name.clone(),
            ),
        )?);
        let (response, read_set) = invoke_runtime_bundle_on_worker_with_host_state(
            &runtime_executor,
            runtime_policy,
            bridge.clone(),
            bundle,
            request,
            RuntimeBundleInvocationOptions::enforcing_policy_limit(
                decision.tenant_id(),
                server_request_id.as_deref(),
                Some(cancellation),
            )
            .with_optional_runtime_bundle_provenance_gate(
                self.registry.runtime_bundle_provenance(),
            ),
            |bridge| bridge.snapshot_read_set(),
        )
        .await
        .map_err(runtime_error_to_core)?;
        let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let value = envelope.into_core_result()?;
        if matches!(invocation_kind, InvocationKind::Mutation) {
            bridge.commit_mutation_execution_unit()?;
        }
        Ok((value, read_set))
    }
}
