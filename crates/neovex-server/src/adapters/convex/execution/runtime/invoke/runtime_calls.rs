use std::sync::Arc;

use neovex_core::{Error, TenantId};
use neovex_runtime::{HostCallCancellation, InvocationRequest};
use serde_json::Value;

use crate::adapters::convex::host_bridge::{ConvexHostBridge, ConvexRuntimeResponseEnvelope};
use crate::adapters::convex::{ConvexRegistry, RuntimeReadSet};
use crate::runtime::invocations::{
    RuntimeBundleInvocationOptions, RuntimeConcurrencyMode,
    invoke_runtime_bundle_on_worker_with_host_state,
};

use super::super::super::runtime_error_to_core;
use super::super::required_runtime_bundle;

pub(in crate::adapters::convex) async fn invoke_named_convex_function_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        cancellation,
        server_request_id,
    )
    .await
    .map(|(value, _)| value)
}

pub(in crate::adapters::convex) async fn invoke_named_convex_function_with_trace_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<(Value, RuntimeReadSet), Error> {
    let bundle = required_runtime_bundle(registry)?;
    let (response, read_set) = invoke_runtime_bundle_on_worker_with_host_state(
        &registry.runtime_executor(),
        registry.runtime_policy(),
        Arc::new(ConvexHostBridge::new(
            service.clone(),
            registry.clone(),
            tenant_id.clone(),
            server_request_id.clone(),
        )),
        bundle,
        request,
        RuntimeBundleInvocationOptions {
            tenant_id,
            server_request_id: server_request_id.as_deref(),
            cancellation: Some(cancellation),
            concurrency_mode: RuntimeConcurrencyMode::EnforcePolicyLimit,
        },
        |bridge| bridge.snapshot_read_set(),
    )
    .await
    .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, read_set))
}
