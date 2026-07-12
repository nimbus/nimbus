use super::*;
use nimbus_runtime::{HostCallEnvelope, HostCallPayload};

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => {
                self.dispatch_query_read_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                self.dispatch_document_host_call_async(payload, cancellation)
                    .await
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => {
                self.dispatch_query_read_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                self.dispatch_document_host_call_cancellable(payload, cancellation)
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call(payload)
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call(payload)
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => self.dispatch_query_read_host_call(payload),
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => self.dispatch_document_host_call(payload),
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call(payload)
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => self.dispatch_scheduler_host_call(payload),
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }
}

fn unsupported_adapter_owned_host_call(
    payload: HostCallPayload,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "convex host bridge does not own `{}` runtime compatibility; that host call is adapter-owned",
        convex_host_operation_name(payload.operation())
    )))
}
