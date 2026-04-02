use super::*;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.ctx.db.query.start" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_start(request.payload)
            }
            "convex.ctx.db.query.with_index" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_with_index(request.payload)
            }
            "convex.ctx.db.query.filter" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_filter(request.payload)
            }
            "convex.ctx.db.query.order" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_order(request.payload)
            }
            "convex.http_route" => {
                self.invoke_http_route_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.query" => {
                self.invoke_ctx_query_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.paginated_query" => {
                self.invoke_ctx_paginated_query_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.mutation" => {
                self.invoke_ctx_mutation_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.action" => {
                self.invoke_ctx_action_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.run_query" => {
                self.invoke_ctx_run_query_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.run_mutation" => {
                self.invoke_ctx_run_mutation_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.run_action" => {
                self.invoke_ctx_run_action_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.get" => {
                self.invoke_ctx_db_get_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.insert" => {
                self.invoke_ctx_db_insert_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.patch" => {
                self.invoke_ctx_db_patch_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.delete" => {
                self.invoke_ctx_db_delete_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.query.collect" => {
                self.invoke_ctx_query_collect_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.query.take" => {
                self.invoke_ctx_query_take_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.query.paginate" => {
                self.invoke_ctx_query_paginate_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.query.first" => {
                self.invoke_ctx_query_first_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.db.query.unique" => {
                self.invoke_ctx_query_unique_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.scheduler.run_at" => {
                self.invoke_ctx_scheduler_run_at_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.scheduler.cancel" => {
                self.invoke_ctx_scheduler_cancel_async_cancellable(request.payload, cancellation)
                    .await
            }
            "convex.ctx.runtime.enter_nested_call" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.ctx.db.query.start" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_start(request.payload)
            }
            "convex.ctx.db.query.with_index" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_with_index(request.payload)
            }
            "convex.ctx.db.query.filter" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_filter(request.payload)
            }
            "convex.ctx.db.query.order" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_order(request.payload)
            }
            "convex.http_route" => {
                self.invoke_http_route_cancellable(request.payload, cancellation)
            }
            "convex.ctx.query" => self.invoke_ctx_query_cancellable(request.payload, cancellation),
            "convex.ctx.paginated_query" => {
                self.invoke_ctx_paginated_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.mutation" => {
                self.invoke_ctx_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.action" => {
                self.invoke_ctx_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_query" => {
                self.invoke_ctx_run_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_mutation" => {
                self.invoke_ctx_run_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_action" => {
                self.invoke_ctx_run_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.get" => {
                self.invoke_ctx_db_get_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.insert" => {
                self.invoke_ctx_db_insert_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.patch" => {
                self.invoke_ctx_db_patch_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.delete" => {
                self.invoke_ctx_db_delete_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.collect" => {
                self.invoke_ctx_query_collect_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.take" => {
                self.invoke_ctx_query_take_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.paginate" => {
                self.invoke_ctx_query_paginate_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.first" => {
                self.invoke_ctx_query_first_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.unique" => {
                self.invoke_ctx_query_unique_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_at" => {
                self.invoke_ctx_scheduler_run_at_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.cancel" => {
                self.invoke_ctx_scheduler_cancel_cancellable(request.payload, cancellation)
            }
            "convex.ctx.runtime.enter_nested_call" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.http_route" => self.invoke_http_route(request.payload),
            "convex.ctx.query" => self.invoke_ctx_query(request.payload),
            "convex.ctx.paginated_query" => self.invoke_ctx_paginated_query(request.payload),
            "convex.ctx.mutation" => self.invoke_ctx_mutation(request.payload),
            "convex.ctx.action" => self.invoke_ctx_action(request.payload),
            "convex.ctx.runtime.enter_nested_call" => {
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            "convex.ctx.run_query" => self.invoke_ctx_run_query(request.payload),
            "convex.ctx.run_mutation" => self.invoke_ctx_run_mutation(request.payload),
            "convex.ctx.run_action" => self.invoke_ctx_run_action(request.payload),
            "convex.ctx.db.get" => self.invoke_ctx_db_get(request.payload),
            "convex.ctx.db.query.start" => self.invoke_ctx_query_start(request.payload),
            "convex.ctx.db.query.with_index" => self.invoke_ctx_query_with_index(request.payload),
            "convex.ctx.db.query.filter" => self.invoke_ctx_query_filter(request.payload),
            "convex.ctx.db.query.order" => self.invoke_ctx_query_order(request.payload),
            "convex.ctx.db.query.collect" => self.invoke_ctx_query_collect(request.payload),
            "convex.ctx.db.query.take" => self.invoke_ctx_query_take(request.payload),
            "convex.ctx.db.query.paginate" => self.invoke_ctx_query_paginate(request.payload),
            "convex.ctx.db.query.first" => self.invoke_ctx_query_first(request.payload),
            "convex.ctx.db.query.unique" => self.invoke_ctx_query_unique(request.payload),
            "convex.ctx.db.insert" => self.invoke_ctx_db_insert(request.payload),
            "convex.ctx.db.patch" => self.invoke_ctx_db_patch(request.payload),
            "convex.ctx.db.delete" => self.invoke_ctx_db_delete(request.payload),
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after(request.payload)
            }
            "convex.ctx.scheduler.run_at" => self.invoke_ctx_scheduler_run_at(request.payload),
            "convex.ctx.scheduler.cancel" => self.invoke_ctx_scheduler_cancel(request.payload),
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }
}
