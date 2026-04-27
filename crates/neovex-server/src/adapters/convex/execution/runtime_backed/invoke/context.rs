use std::sync::Arc;

use neovex_core::{Error, TenantId};
use neovex_runtime::{
    HostCallCancellation, InvocationKind, InvocationRequest, InvocationServices, RuntimeBundle,
};
use serde_json::Value;

use crate::adapters::convex::host_bridge::ConvexRuntimeResponseEnvelope;
use crate::adapters::convex::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope, ConvexRegistry,
    RuntimeReadSet,
};
use crate::application_auth::normalize_principal_context;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_on_worker_with_host_state,
};
use crate::service_registry::RuntimeServiceRegistry;

use super::super::super::runtime_error_to_core;

pub(in crate::adapters::convex) struct RuntimeInvocationContext<'a> {
    service: &'a Arc<neovex_engine::Service>,
    registry: &'a Arc<ConvexRegistry>,
    runtime_service_registry: &'a Arc<dyn RuntimeServiceRegistry>,
    tenant_id: &'a TenantId,
}

impl<'a> RuntimeInvocationContext<'a> {
    pub(in crate::adapters::convex) fn new(
        service: &'a Arc<neovex_engine::Service>,
        registry: &'a Arc<ConvexRegistry>,
        runtime_service_registry: &'a Arc<dyn RuntimeServiceRegistry>,
        tenant_id: &'a TenantId,
    ) -> Self {
        Self {
            service,
            registry,
            runtime_service_registry,
            tenant_id,
        }
    }

    pub(in crate::adapters::convex) fn runtime_services(&self) -> InvocationServices {
        self.runtime_service_registry
            .snapshot_for_tenant(self.tenant_id)
    }

    pub(in crate::adapters::convex) fn required_runtime_bundle(
        &self,
    ) -> Result<RuntimeBundle, Error> {
        self.registry.required_runtime_bundle()
    }

    pub(in crate::adapters::convex) async fn invoke_with_trace_async_cancellable(
        &self,
        request: InvocationRequest,
        cancellation: HostCallCancellation,
        server_request_id: Option<String>,
    ) -> Result<(Value, RuntimeReadSet), Error> {
        let bundle = self.required_runtime_bundle()?;
        let invocation_kind = request.kind.clone();
        let bridge = Arc::new(ConvexHostBridge::build(
            ConvexHostBridgeScope::new(
                self.service.clone(),
                self.registry.clone(),
                self.tenant_id.clone(),
                self.runtime_service_registry.clone(),
            ),
            ConvexHostBridgeInvocation::new(
                request.auth.clone(),
                request.services.clone(),
                normalize_principal_context(request.auth.as_ref()),
                server_request_id.clone(),
                invocation_kind.clone(),
            ),
        )?);
        let (response, read_set) = invoke_runtime_bundle_on_worker_with_host_state(
            &self.registry.runtime_executor(),
            self.registry.runtime_policy(),
            bridge.clone(),
            bundle,
            request,
            RuntimeBundleInvocationOptions::enforcing_policy_limit(
                self.tenant_id,
                server_request_id.as_deref(),
                Some(cancellation),
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
