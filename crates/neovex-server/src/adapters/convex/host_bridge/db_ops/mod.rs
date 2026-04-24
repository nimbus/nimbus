use super::*;
use neovex_runtime::HostCallPayload;

mod documents;
mod query_builder;
mod scheduler;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryStart(payload) => {
                self.invoke_ctx_query_start(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryWithIndex(payload) => {
                self.invoke_ctx_query_with_index(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryFilter(payload) => {
                self.invoke_ctx_query_filter(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryOrder(payload) => {
                self.invoke_ctx_query_order(runtime_host_payload_value(payload)?)
            }
            _ => {
                unreachable!("non-query-builder host operation routed to query-builder dispatcher")
            }
        }
    }

    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_query_builder_host_call(payload)
    }

    pub(in crate::adapters::convex) async fn dispatch_query_builder_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_query_builder_host_call(payload)
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryCollect(payload) => {
                self.invoke_ctx_query_collect(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryTake(payload) => {
                self.invoke_ctx_query_take(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryPaginate(payload) => {
                self.invoke_ctx_query_paginate(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryFirst(payload) => {
                self.invoke_ctx_query_first(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbQueryUnique(payload) => {
                self.invoke_ctx_query_unique(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryCollect(payload) => self
                .invoke_ctx_query_collect_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            HostCallPayload::CtxDbQueryTake(payload) => self.invoke_ctx_query_take_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxDbQueryPaginate(payload) => self
                .invoke_ctx_query_paginate_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            HostCallPayload::CtxDbQueryFirst(payload) => self.invoke_ctx_query_first_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxDbQueryUnique(payload) => self.invoke_ctx_query_unique_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_query_read_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryCollect(payload) => {
                self.invoke_ctx_query_collect_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbQueryTake(payload) => {
                self.invoke_ctx_query_take_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbQueryPaginate(payload) => {
                self.invoke_ctx_query_paginate_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbQueryFirst(payload) => {
                self.invoke_ctx_query_first_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbQueryUnique(payload) => {
                self.invoke_ctx_query_unique_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbGet(payload) => {
                self.invoke_ctx_db_get(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbInsert(payload) => {
                self.invoke_ctx_db_insert(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbPatch(payload) => {
                self.invoke_ctx_db_patch(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxDbDelete(payload) => {
                self.invoke_ctx_db_delete(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbGet(payload) => self
                .invoke_ctx_db_get_cancellable(runtime_host_payload_value(payload)?, cancellation),
            HostCallPayload::CtxDbInsert(payload) => self.invoke_ctx_db_insert_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxDbPatch(payload) => self.invoke_ctx_db_patch_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxDbDelete(payload) => self.invoke_ctx_db_delete_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_document_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbGet(payload) => {
                self.invoke_ctx_db_get_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbInsert(payload) => {
                self.invoke_ctx_db_insert_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbPatch(payload) => {
                self.invoke_ctx_db_patch_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxDbDelete(payload) => {
                self.invoke_ctx_db_delete_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxSchedulerRunAfter(payload) => {
                self.invoke_ctx_scheduler_run_after(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxSchedulerRunAt(payload) => {
                self.invoke_ctx_scheduler_run_at(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxSchedulerCancel(payload) => {
                self.invoke_ctx_scheduler_cancel(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxSchedulerRunAfter(payload) => self
                .invoke_ctx_scheduler_run_after_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            HostCallPayload::CtxSchedulerRunAt(payload) => self
                .invoke_ctx_scheduler_run_at_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            HostCallPayload::CtxSchedulerCancel(payload) => self
                .invoke_ctx_scheduler_cancel_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_scheduler_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxSchedulerRunAfter(payload) => {
                self.invoke_ctx_scheduler_run_after_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxSchedulerRunAt(payload) => {
                self.invoke_ctx_scheduler_run_at_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxSchedulerCancel(payload) => {
                self.invoke_ctx_scheduler_cancel_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }
}
