use super::*;
use neovex_runtime::{HostCallEnvelope, HostCallPayload};

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
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
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)) => {
                self.dispatch_function_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxDbQueryStart(_)
            | HostCallPayload::CtxDbQueryWithIndex(_)
            | HostCallPayload::CtxDbQueryFilter(_)
            | HostCallPayload::CtxDbQueryOrder(_)) => {
                self.dispatch_query_builder_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxDbQueryCollect(_)
            | HostCallPayload::CtxDbQueryTake(_)
            | HostCallPayload::CtxDbQueryPaginate(_)
            | HostCallPayload::CtxDbQueryFirst(_)
            | HostCallPayload::CtxDbQueryUnique(_)) => {
                self.dispatch_query_read_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxDbGet(_)
            | HostCallPayload::CtxDbInsert(_)
            | HostCallPayload::CtxDbPatch(_)
            | HostCallPayload::CtxDbDelete(_)) => {
                self.dispatch_document_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_async(payload, cancellation)
                    .await
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
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
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)) => {
                self.dispatch_function_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxDbQueryStart(_)
            | HostCallPayload::CtxDbQueryWithIndex(_)
            | HostCallPayload::CtxDbQueryFilter(_)
            | HostCallPayload::CtxDbQueryOrder(_)) => {
                self.dispatch_query_builder_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxDbQueryCollect(_)
            | HostCallPayload::CtxDbQueryTake(_)
            | HostCallPayload::CtxDbQueryPaginate(_)
            | HostCallPayload::CtxDbQueryFirst(_)
            | HostCallPayload::CtxDbQueryUnique(_)) => {
                self.dispatch_query_read_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxDbGet(_)
            | HostCallPayload::CtxDbInsert(_)
            | HostCallPayload::CtxDbPatch(_)
            | HostCallPayload::CtxDbDelete(_)) => {
                self.dispatch_document_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_cancellable(payload, cancellation)
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
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
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)) => {
                self.dispatch_function_host_call(payload)
            }
            payload @ (HostCallPayload::CtxDbQueryStart(_)
            | HostCallPayload::CtxDbQueryWithIndex(_)
            | HostCallPayload::CtxDbQueryFilter(_)
            | HostCallPayload::CtxDbQueryOrder(_)) => {
                self.dispatch_query_builder_host_call(payload)
            }
            payload @ (HostCallPayload::CtxDbQueryCollect(_)
            | HostCallPayload::CtxDbQueryTake(_)
            | HostCallPayload::CtxDbQueryPaginate(_)
            | HostCallPayload::CtxDbQueryFirst(_)
            | HostCallPayload::CtxDbQueryUnique(_)) => self.dispatch_query_read_host_call(payload),
            payload @ (HostCallPayload::CtxDbGet(_)
            | HostCallPayload::CtxDbInsert(_)
            | HostCallPayload::CtxDbPatch(_)
            | HostCallPayload::CtxDbDelete(_)) => self.dispatch_document_host_call(payload),
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => self.dispatch_scheduler_host_call(payload),
        }
    }
}
