use super::*;

trait ConvexHostOperationDispatch {
    fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError>;

    fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError>;

    async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError>;
}

impl ConvexHostOperationDispatch for HostCallOperation {
    fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::HttpRoute => bridge.invoke_http_route(payload),
            Self::CtxQuery => bridge.invoke_ctx_query(payload),
            Self::CtxPaginatedQuery => bridge.invoke_ctx_paginated_query(payload),
            Self::CtxMutation => bridge.invoke_ctx_mutation(payload),
            Self::CtxAction => bridge.invoke_ctx_action(payload),
            Self::CtxRunQuery => bridge.invoke_ctx_run_query(payload),
            Self::CtxRunMutation => bridge.invoke_ctx_run_mutation(payload),
            Self::CtxRunAction => bridge.invoke_ctx_run_action(payload),
            Self::CtxDbGet => bridge.invoke_ctx_db_get(payload),
            Self::CtxDbQueryStart => bridge.invoke_ctx_query_start(payload),
            Self::CtxDbQueryWithIndex => bridge.invoke_ctx_query_with_index(payload),
            Self::CtxDbQueryFilter => bridge.invoke_ctx_query_filter(payload),
            Self::CtxDbQueryOrder => bridge.invoke_ctx_query_order(payload),
            Self::CtxDbQueryCollect => bridge.invoke_ctx_query_collect(payload),
            Self::CtxDbQueryTake => bridge.invoke_ctx_query_take(payload),
            Self::CtxDbQueryPaginate => bridge.invoke_ctx_query_paginate(payload),
            Self::CtxDbQueryFirst => bridge.invoke_ctx_query_first(payload),
            Self::CtxDbQueryUnique => bridge.invoke_ctx_query_unique(payload),
            Self::CtxDbInsert => bridge.invoke_ctx_db_insert(payload),
            Self::CtxDbPatch => bridge.invoke_ctx_db_patch(payload),
            Self::CtxDbDelete => bridge.invoke_ctx_db_delete(payload),
            Self::CtxSchedulerRunAfter => bridge.invoke_ctx_scheduler_run_after(payload),
            Self::CtxSchedulerRunAt => bridge.invoke_ctx_scheduler_run_at(payload),
            Self::CtxSchedulerCancel => bridge.invoke_ctx_scheduler_cancel(payload),
            Self::CtxRuntimeEnterNestedCall => bridge.invoke_ctx_runtime_enter_nested_call(payload),
        }
    }

    fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::HttpRoute => bridge.invoke_http_route_cancellable(payload, cancellation),
            Self::CtxQuery => bridge.invoke_ctx_query_cancellable(payload, cancellation),
            Self::CtxPaginatedQuery => {
                bridge.invoke_ctx_paginated_query_cancellable(payload, cancellation)
            }
            Self::CtxMutation => bridge.invoke_ctx_mutation_cancellable(payload, cancellation),
            Self::CtxAction => bridge.invoke_ctx_action_cancellable(payload, cancellation),
            Self::CtxRunQuery => bridge.invoke_ctx_run_query_cancellable(payload, cancellation),
            Self::CtxRunMutation => {
                bridge.invoke_ctx_run_mutation_cancellable(payload, cancellation)
            }
            Self::CtxRunAction => bridge.invoke_ctx_run_action_cancellable(payload, cancellation),
            Self::CtxDbGet => bridge.invoke_ctx_db_get_cancellable(payload, cancellation),
            Self::CtxDbQueryCollect => {
                bridge.invoke_ctx_query_collect_cancellable(payload, cancellation)
            }
            Self::CtxDbQueryTake => bridge.invoke_ctx_query_take_cancellable(payload, cancellation),
            Self::CtxDbQueryPaginate => {
                bridge.invoke_ctx_query_paginate_cancellable(payload, cancellation)
            }
            Self::CtxDbQueryFirst => {
                bridge.invoke_ctx_query_first_cancellable(payload, cancellation)
            }
            Self::CtxDbQueryUnique => {
                bridge.invoke_ctx_query_unique_cancellable(payload, cancellation)
            }
            Self::CtxDbInsert => bridge.invoke_ctx_db_insert_cancellable(payload, cancellation),
            Self::CtxDbPatch => bridge.invoke_ctx_db_patch_cancellable(payload, cancellation),
            Self::CtxDbDelete => bridge.invoke_ctx_db_delete_cancellable(payload, cancellation),
            Self::CtxSchedulerRunAfter => {
                bridge.invoke_ctx_scheduler_run_after_cancellable(payload, cancellation)
            }
            Self::CtxSchedulerRunAt => {
                bridge.invoke_ctx_scheduler_run_at_cancellable(payload, cancellation)
            }
            Self::CtxSchedulerCancel => {
                bridge.invoke_ctx_scheduler_cancel_cancellable(payload, cancellation)
            }
            Self::CtxDbQueryStart => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_start(payload)
            }
            Self::CtxDbQueryWithIndex => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_with_index(payload)
            }
            Self::CtxDbQueryFilter => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_filter(payload)
            }
            Self::CtxDbQueryOrder => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_order(payload)
            }
            Self::CtxRuntimeEnterNestedCall => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_runtime_enter_nested_call(payload)
            }
        }
    }

    async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::HttpRoute => {
                bridge
                    .invoke_http_route_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxQuery => {
                bridge
                    .invoke_ctx_query_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxPaginatedQuery => {
                bridge
                    .invoke_ctx_paginated_query_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxMutation => {
                bridge
                    .invoke_ctx_mutation_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxAction => {
                bridge
                    .invoke_ctx_action_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxRunQuery => {
                bridge
                    .invoke_ctx_run_query_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxRunMutation => {
                bridge
                    .invoke_ctx_run_mutation_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxRunAction => {
                bridge
                    .invoke_ctx_run_action_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbGet => {
                bridge
                    .invoke_ctx_db_get_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryCollect => {
                bridge
                    .invoke_ctx_query_collect_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryTake => {
                bridge
                    .invoke_ctx_query_take_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryPaginate => {
                bridge
                    .invoke_ctx_query_paginate_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryFirst => {
                bridge
                    .invoke_ctx_query_first_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryUnique => {
                bridge
                    .invoke_ctx_query_unique_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbInsert => {
                bridge
                    .invoke_ctx_db_insert_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbPatch => {
                bridge
                    .invoke_ctx_db_patch_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbDelete => {
                bridge
                    .invoke_ctx_db_delete_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxSchedulerRunAfter => {
                bridge
                    .invoke_ctx_scheduler_run_after_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxSchedulerRunAt => {
                bridge
                    .invoke_ctx_scheduler_run_at_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxSchedulerCancel => {
                bridge
                    .invoke_ctx_scheduler_cancel_async_cancellable(payload, cancellation)
                    .await
            }
            Self::CtxDbQueryStart => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_start(payload)
            }
            Self::CtxDbQueryWithIndex => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_with_index(payload)
            }
            Self::CtxDbQueryFilter => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_filter(payload)
            }
            Self::CtxDbQueryOrder => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_query_order(payload)
            }
            Self::CtxRuntimeEnterNestedCall => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                bridge.invoke_ctx_runtime_enter_nested_call(payload)
            }
        }
    }
}

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        request
            .operation
            .dispatch_async(self, request.payload, cancellation)
            .await
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        request
            .operation
            .dispatch_cancellable(self, request.payload, cancellation)
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        request.operation.dispatch_sync(self, request.payload)
    }
}
