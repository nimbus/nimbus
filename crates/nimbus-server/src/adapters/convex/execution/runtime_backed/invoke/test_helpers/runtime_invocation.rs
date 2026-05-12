use std::sync::Arc;

use nimbus_core::{Error, TenantId};
use nimbus_runtime::{HostCallCancellation, InvocationKind, InvocationRequest};
use serde_json::Value;

use crate::adapters::convex::host_bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
    ConvexRuntimeResponseEnvelope,
};
use crate::adapters::convex::{ConvexRegistry, RuntimeReadSet};
use crate::application_auth::normalize_principal_context;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host_state,
};
use crate::service_registry::{RuntimeServiceRegistry, SandboxCatalogRuntimeServiceRegistry};

use super::super::super::runtime_error_to_core;
use super::super::{
    RuntimeInvocationContext,
    runtime_calls::invoke_named_convex_function_with_trace_async_cancellable,
};

pub(super) fn invoke_named_convex_function(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace(service, registry, tenant_id, request)
        .map(|(value, _)| value)
}

fn invoke_named_convex_function_with_trace(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, RuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
    )
}

fn invoke_named_convex_function_with_trace_cancellable(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<(Value, RuntimeReadSet), Error> {
    let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
        SandboxCatalogRuntimeServiceRegistry::new(Arc::new(crate::EmptySandboxCatalog)),
    );
    let bundle = registry.required_runtime_bundle()?;
    let invocation_kind = request.kind.clone();
    let bridge = Arc::new(ConvexHostBridge::build(
        ConvexHostBridgeScope::new(
            service.clone(),
            registry.clone(),
            tenant_id.clone(),
            runtime_service_registry,
        ),
        ConvexHostBridgeInvocation::new(
            request.auth.clone(),
            request.services.clone(),
            normalize_principal_context(request.auth.as_ref()),
            None,
            invocation_kind.clone(),
        ),
    )?);
    let (response, read_set) = invoke_runtime_bundle_blocking_with_host_state(
        &registry.runtime_executor(),
        registry.runtime_policy(),
        bridge.clone(),
        bundle,
        request,
        RuntimeBundleInvocationOptions::enforcing_policy_limit(tenant_id, None, Some(cancellation)),
        |bridge| bridge.snapshot_read_set(),
    )
    .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    let value = envelope.into_core_result()?;
    if matches!(invocation_kind, InvocationKind::Mutation) {
        bridge.commit_mutation_execution_unit()?;
    }
    Ok((value, read_set))
}

#[allow(dead_code)]
async fn invoke_named_convex_function_with_trace_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, RuntimeReadSet), Error> {
    let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
        SandboxCatalogRuntimeServiceRegistry::new(Arc::new(crate::EmptySandboxCatalog)),
    );
    let context =
        RuntimeInvocationContext::new(service, registry, &runtime_service_registry, tenant_id);
    invoke_named_convex_function_with_trace_async_cancellable(
        &context,
        request,
        HostCallCancellation::default(),
        None,
    )
    .await
}
