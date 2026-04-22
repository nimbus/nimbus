use super::*;

mod documents;
mod query_builder;
mod scheduler;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbQueryStart => self.invoke_ctx_query_start(payload),
            ConvexHostCallOperation::CtxDbQueryWithIndex => {
                self.invoke_ctx_query_with_index(payload)
            }
            ConvexHostCallOperation::CtxDbQueryFilter => self.invoke_ctx_query_filter(payload),
            ConvexHostCallOperation::CtxDbQueryOrder => self.invoke_ctx_query_order(payload),
            _ => {
                unreachable!("non-query-builder host operation routed to query-builder dispatcher")
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call_cancellable(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_query_builder_host_call(operation, payload)
    }

    pub(in crate::adapters::convex) async fn dispatch_query_builder_host_call_async(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_query_builder_host_call(operation, payload)
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbQueryCollect => self.invoke_ctx_query_collect(payload),
            ConvexHostCallOperation::CtxDbQueryTake => self.invoke_ctx_query_take(payload),
            ConvexHostCallOperation::CtxDbQueryPaginate => self.invoke_ctx_query_paginate(payload),
            ConvexHostCallOperation::CtxDbQueryFirst => self.invoke_ctx_query_first(payload),
            ConvexHostCallOperation::CtxDbQueryUnique => self.invoke_ctx_query_unique(payload),
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call_cancellable(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbQueryCollect => {
                self.invoke_ctx_query_collect_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbQueryTake => {
                self.invoke_ctx_query_take_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbQueryPaginate => {
                self.invoke_ctx_query_paginate_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbQueryFirst => {
                self.invoke_ctx_query_first_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbQueryUnique => {
                self.invoke_ctx_query_unique_cancellable(payload, cancellation)
            }
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_query_read_host_call_async(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbQueryCollect => {
                self.invoke_ctx_query_collect_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbQueryTake => {
                self.invoke_ctx_query_take_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbQueryPaginate => {
                self.invoke_ctx_query_paginate_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbQueryFirst => {
                self.invoke_ctx_query_first_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbQueryUnique => {
                self.invoke_ctx_query_unique_async_cancellable(payload, cancellation)
                    .await
            }
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbGet => self.invoke_ctx_db_get(payload),
            ConvexHostCallOperation::CtxDbInsert => self.invoke_ctx_db_insert(payload),
            ConvexHostCallOperation::CtxDbPatch => self.invoke_ctx_db_patch(payload),
            ConvexHostCallOperation::CtxDbDelete => self.invoke_ctx_db_delete(payload),
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call_cancellable(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbGet => {
                self.invoke_ctx_db_get_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbInsert => {
                self.invoke_ctx_db_insert_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbPatch => {
                self.invoke_ctx_db_patch_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxDbDelete => {
                self.invoke_ctx_db_delete_cancellable(payload, cancellation)
            }
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_document_host_call_async(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxDbGet => {
                self.invoke_ctx_db_get_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbInsert => {
                self.invoke_ctx_db_insert_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbPatch => {
                self.invoke_ctx_db_patch_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxDbDelete => {
                self.invoke_ctx_db_delete_async_cancellable(payload, cancellation)
                    .await
            }
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxSchedulerRunAfter => {
                self.invoke_ctx_scheduler_run_after(payload)
            }
            ConvexHostCallOperation::CtxSchedulerRunAt => self.invoke_ctx_scheduler_run_at(payload),
            ConvexHostCallOperation::CtxSchedulerCancel => {
                self.invoke_ctx_scheduler_cancel(payload)
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call_cancellable(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxSchedulerRunAfter => {
                self.invoke_ctx_scheduler_run_after_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxSchedulerRunAt => {
                self.invoke_ctx_scheduler_run_at_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxSchedulerCancel => {
                self.invoke_ctx_scheduler_cancel_cancellable(payload, cancellation)
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_scheduler_host_call_async(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::CtxSchedulerRunAfter => {
                self.invoke_ctx_scheduler_run_after_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxSchedulerRunAt => {
                self.invoke_ctx_scheduler_run_at_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxSchedulerCancel => {
                self.invoke_ctx_scheduler_cancel_async_cancellable(payload, cancellation)
                    .await
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }
}
