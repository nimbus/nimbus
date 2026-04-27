use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neovex_core::Result;
use neovex_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallEnvelope, HostCallPayload,
    HostCallRequest, NeovexRuntimeError, RuntimePolicy,
};
use serde_json::Value;

use super::runtime_api;
use crate::execution::host_calls::{
    RuntimeAsyncHostCallTrace, execute_async_host_call, execute_host_call,
    execute_host_call_cancellable,
};
use crate::runtime_host::{RuntimeHostContext, RuntimeHostInvocation, RuntimeHostScope, abi};

#[derive(Clone)]
pub(crate) struct CloudFunctionsHostBridge {
    context: RuntimeHostContext,
    runtime_policy: Arc<RuntimePolicy>,
}

impl CloudFunctionsHostBridge {
    pub(crate) fn build(
        scope: RuntimeHostScope,
        invocation: RuntimeHostInvocation,
    ) -> Result<Self> {
        let runtime_policy = scope.runtime_policy().clone();
        let context =
            RuntimeHostContext::build(scope, invocation, "cloud-functions-runtime-session")?;
        Ok(Self {
            context,
            runtime_policy,
        })
    }

    pub(crate) fn commit_mutation_execution_unit(&self) -> Result<()> {
        self.context.commit_mutation_execution_unit()
    }

    async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call_async(
                    &self.context,
                    payload,
                    cancellation,
                )
                .await
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call_async(
                    &self.context,
                    payload,
                    cancellation,
                )
                .await
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }

    fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call_cancellable(
                    &self.context,
                    payload,
                    cancellation,
                )
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call_cancellable(
                    &self.context,
                    payload,
                    cancellation,
                )
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }

    fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call(&self.context, payload)
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call(&self.context, payload)
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }
}

impl HostBridge for CloudFunctionsHostBridge {
    fn call(&self, request: HostCallRequest) -> std::result::Result<Value, NeovexRuntimeError> {
        let operation = request.operation.as_str();
        execute_host_call(self.runtime_policy.metrics().as_ref(), operation, || {
            self.dispatch_host_call(request)
        })
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let operation = request.operation.as_str();
        execute_host_call_cancellable(
            self.runtime_policy.metrics().as_ref(),
            operation,
            cancellation,
            || self.dispatch_host_call_cancellable(request, cancellation),
        )
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let bridge = self.clone();
        static NEXT_ASYNC_HOST_CALL_ID: AtomicU64 = AtomicU64::new(1);
        let trace = RuntimeAsyncHostCallTrace::new(
            tracing::debug_span!(
                "cloud_functions_runtime_async_host_call",
                tenant = %bridge.context.tenant_id(),
                server_request_id = ?bridge.context.server_request_id(),
                session_id = %bridge.context.session_id(),
                operation = %request.operation.as_str(),
                host_call_id = NEXT_ASYNC_HOST_CALL_ID.fetch_add(1, Ordering::Relaxed),
            ),
            "cloud functions runtime async host call",
        );
        let metrics = bridge.runtime_policy.metrics();
        let operation = request.operation.as_str();
        Box::pin(execute_async_host_call(
            trace,
            metrics,
            operation,
            cancellation.clone(),
            async move {
                bridge
                    .dispatch_host_call_async(request, &cancellation)
                    .await
            },
        ))
    }
}

fn unsupported_cloud_functions_host_operation(
    operation: &str,
) -> std::result::Result<Value, NeovexRuntimeError> {
    Err(NeovexRuntimeError::Contract(format!(
        "cloud functions runtime host does not support operation `{operation}`"
    )))
}
