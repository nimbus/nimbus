use super::*;
use neovex_runtime::HostCallPayload;

pub(super) enum QueryBuilderHostCall {
    Start(Value),
    WithIndex(Value),
    Filter(Value),
    Order(Value),
}

impl QueryBuilderHostCall {
    pub(super) fn from_payload(
        payload: HostCallPayload,
    ) -> std::result::Result<Self, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryStart(payload) => {
                Ok(Self::Start(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryWithIndex(payload) => {
                Ok(Self::WithIndex(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryFilter(payload) => {
                Ok(Self::Filter(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryOrder(payload) => {
                Ok(Self::Order(runtime_host_payload_value(payload)?))
            }
            _ => {
                unreachable!("non-query-builder host operation routed to query-builder dispatcher")
            }
        }
    }

    pub(super) fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Start(payload) => bridge.invoke_ctx_query_start(payload),
            Self::WithIndex(payload) => bridge.invoke_ctx_query_with_index(payload),
            Self::Filter(payload) => bridge.invoke_ctx_query_filter(payload),
            Self::Order(payload) => bridge.invoke_ctx_query_order(payload),
        }
    }

    pub(super) fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_sync(bridge)
    }

    pub(super) async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        ensure_runtime_host_not_cancelled(cancellation)?;
        self.dispatch_sync(bridge)
    }
}

pub(super) enum QueryReadHostCall {
    Collect(Value),
    Take(Value),
    Paginate(Value),
    First(Value),
    Unique(Value),
}

impl QueryReadHostCall {
    pub(super) fn from_payload(
        payload: HostCallPayload,
    ) -> std::result::Result<Self, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbQueryCollect(payload) => {
                Ok(Self::Collect(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryTake(payload) => {
                Ok(Self::Take(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryPaginate(payload) => {
                Ok(Self::Paginate(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryFirst(payload) => {
                Ok(Self::First(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbQueryUnique(payload) => {
                Ok(Self::Unique(runtime_host_payload_value(payload)?))
            }
            _ => unreachable!("non-query-read host operation routed to query-read dispatcher"),
        }
    }

    pub(super) fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Collect(payload) => bridge.invoke_ctx_query_collect(payload),
            Self::Take(payload) => bridge.invoke_ctx_query_take(payload),
            Self::Paginate(payload) => bridge.invoke_ctx_query_paginate(payload),
            Self::First(payload) => bridge.invoke_ctx_query_first(payload),
            Self::Unique(payload) => bridge.invoke_ctx_query_unique(payload),
        }
    }

    pub(super) fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Collect(payload) => {
                bridge.invoke_ctx_query_collect_cancellable(payload, cancellation)
            }
            Self::Take(payload) => bridge.invoke_ctx_query_take_cancellable(payload, cancellation),
            Self::Paginate(payload) => {
                bridge.invoke_ctx_query_paginate_cancellable(payload, cancellation)
            }
            Self::First(payload) => {
                bridge.invoke_ctx_query_first_cancellable(payload, cancellation)
            }
            Self::Unique(payload) => {
                bridge.invoke_ctx_query_unique_cancellable(payload, cancellation)
            }
        }
    }

    pub(super) async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Collect(payload) => {
                bridge
                    .invoke_ctx_query_collect_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Take(payload) => {
                bridge
                    .invoke_ctx_query_take_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Paginate(payload) => {
                bridge
                    .invoke_ctx_query_paginate_async_cancellable(payload, cancellation)
                    .await
            }
            Self::First(payload) => {
                bridge
                    .invoke_ctx_query_first_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Unique(payload) => {
                bridge
                    .invoke_ctx_query_unique_async_cancellable(payload, cancellation)
                    .await
            }
        }
    }
}

pub(super) enum DocumentHostCall {
    Get(Value),
    Insert(Value),
    Patch(Value),
    Delete(Value),
}

impl DocumentHostCall {
    pub(super) fn from_payload(
        payload: HostCallPayload,
    ) -> std::result::Result<Self, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxDbGet(payload) => {
                Ok(Self::Get(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbInsert(payload) => {
                Ok(Self::Insert(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbPatch(payload) => {
                Ok(Self::Patch(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxDbDelete(payload) => {
                Ok(Self::Delete(runtime_host_payload_value(payload)?))
            }
            _ => unreachable!("non-document host operation routed to document dispatcher"),
        }
    }

    pub(super) fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Get(payload) => bridge.invoke_ctx_db_get(payload),
            Self::Insert(payload) => bridge.invoke_ctx_db_insert(payload),
            Self::Patch(payload) => bridge.invoke_ctx_db_patch(payload),
            Self::Delete(payload) => bridge.invoke_ctx_db_delete(payload),
        }
    }

    pub(super) fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Get(payload) => bridge.invoke_ctx_db_get_cancellable(payload, cancellation),
            Self::Insert(payload) => bridge.invoke_ctx_db_insert_cancellable(payload, cancellation),
            Self::Patch(payload) => bridge.invoke_ctx_db_patch_cancellable(payload, cancellation),
            Self::Delete(payload) => bridge.invoke_ctx_db_delete_cancellable(payload, cancellation),
        }
    }

    pub(super) async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::Get(payload) => {
                bridge
                    .invoke_ctx_db_get_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Insert(payload) => {
                bridge
                    .invoke_ctx_db_insert_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Patch(payload) => {
                bridge
                    .invoke_ctx_db_patch_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Delete(payload) => {
                bridge
                    .invoke_ctx_db_delete_async_cancellable(payload, cancellation)
                    .await
            }
        }
    }
}

pub(super) enum SchedulerHostCall {
    RunAfter(Value),
    RunAt(Value),
    Cancel(Value),
}

impl SchedulerHostCall {
    pub(super) fn from_payload(
        payload: HostCallPayload,
    ) -> std::result::Result<Self, NeovexRuntimeError> {
        match payload {
            HostCallPayload::CtxSchedulerRunAfter(payload) => {
                Ok(Self::RunAfter(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxSchedulerRunAt(payload) => {
                Ok(Self::RunAt(runtime_host_payload_value(payload)?))
            }
            HostCallPayload::CtxSchedulerCancel(payload) => {
                Ok(Self::Cancel(runtime_host_payload_value(payload)?))
            }
            _ => unreachable!("non-scheduler host operation routed to scheduler dispatcher"),
        }
    }

    pub(super) fn dispatch_sync(
        self,
        bridge: &ConvexHostBridge,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::RunAfter(payload) => bridge.invoke_ctx_scheduler_run_after(payload),
            Self::RunAt(payload) => bridge.invoke_ctx_scheduler_run_at(payload),
            Self::Cancel(payload) => bridge.invoke_ctx_scheduler_cancel(payload),
        }
    }

    pub(super) fn dispatch_cancellable(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::RunAfter(payload) => {
                bridge.invoke_ctx_scheduler_run_after_cancellable(payload, cancellation)
            }
            Self::RunAt(payload) => {
                bridge.invoke_ctx_scheduler_run_at_cancellable(payload, cancellation)
            }
            Self::Cancel(payload) => {
                bridge.invoke_ctx_scheduler_cancel_cancellable(payload, cancellation)
            }
        }
    }

    pub(super) async fn dispatch_async(
        self,
        bridge: &ConvexHostBridge,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match self {
            Self::RunAfter(payload) => {
                bridge
                    .invoke_ctx_scheduler_run_after_async_cancellable(payload, cancellation)
                    .await
            }
            Self::RunAt(payload) => {
                bridge
                    .invoke_ctx_scheduler_run_at_async_cancellable(payload, cancellation)
                    .await
            }
            Self::Cancel(payload) => {
                bridge
                    .invoke_ctx_scheduler_cancel_async_cancellable(payload, cancellation)
                    .await
            }
        }
    }
}
