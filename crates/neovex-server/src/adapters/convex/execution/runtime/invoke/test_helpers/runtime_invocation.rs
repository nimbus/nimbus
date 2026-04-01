use std::sync::Arc;

use neovex_core::{Error, TenantId};
use neovex_runtime::{HostCallCancellation, InvocationRequest};
use serde_json::Value;

use crate::adapters::convex::host_bridge::{ConvexHostBridge, ConvexRuntimeResponseEnvelope};
use crate::adapters::convex::{ConvexRegistry, RuntimeReadSet, normalize_principal_context};
use crate::runtime::invocations::{
    RuntimeBundleInvocationOptions, RuntimeConcurrencyMode,
    invoke_runtime_bundle_blocking_with_host_state,
};

use super::super::super::required_runtime_bundle;
use super::super::super::runtime_error_to_core;
use super::super::runtime_calls::invoke_named_convex_function_with_trace_async_cancellable;

pub(super) fn invoke_named_convex_function(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace(service, registry, tenant_id, request)
        .map(|(value, _)| value)
}

fn invoke_named_convex_function_with_trace(
    service: &Arc<neovex_engine::Service>,
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
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<(Value, RuntimeReadSet), Error> {
    let bundle = required_runtime_bundle(registry)?;
    let (response, read_set) = invoke_runtime_bundle_blocking_with_host_state(
        &registry.runtime_executor(),
        registry.runtime_policy(),
        Arc::new(ConvexHostBridge::new(
            service.clone(),
            registry.clone(),
            tenant_id.clone(),
            request.auth.clone(),
            normalize_principal_context(request.auth.as_ref()),
            None,
        )),
        bundle,
        request,
        RuntimeBundleInvocationOptions {
            tenant_id,
            server_request_id: None,
            cancellation: Some(cancellation),
            concurrency_mode: RuntimeConcurrencyMode::EnforcePolicyLimit,
        },
        |bridge| bridge.snapshot_read_set(),
    )
    .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, read_set))
}

#[allow(dead_code)]
async fn invoke_named_convex_function_with_trace_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, RuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
        None,
    )
    .await
}
