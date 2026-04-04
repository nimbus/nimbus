use std::cell::RefCell;
use std::rc::Rc;

use deno_core::{CancelFuture, OpState, extension, op2};
use deno_error::JsErrorBox;
use serde::Serialize;
use serde_json::Value;

use crate::executor::SharedInvocationPermit;
use crate::host::{HostCallOperation, HostCallRequest};

use super::payloads::{
    RuntimeAsyncActionPayload, RuntimeAsyncDbDeletePayload, RuntimeAsyncDbGetPayload,
    RuntimeAsyncDbInsertPayload, RuntimeAsyncDbPatchPayload, RuntimeAsyncFunctionCallPayload,
    RuntimeAsyncHttpRoutePayload, RuntimeAsyncMutationPayload, RuntimeAsyncPaginatedQueryPayload,
    RuntimeAsyncQueryPaginatePayload, RuntimeAsyncQueryPayload, RuntimeAsyncQueryTakePayload,
    RuntimeAsyncQueryTerminalPayload, RuntimeAsyncSchedulerCancelPayload,
    RuntimeAsyncSchedulerRunAfterPayload, RuntimeAsyncSchedulerRunAtPayload,
    RuntimeHostCallEnvelope, RuntimeSyncNestedCallPayload, RuntimeSyncQueryFilterPayload,
    RuntimeSyncQueryOrderPayload, RuntimeSyncQueryStartPayload, RuntimeSyncQueryWithIndexPayload,
};
use super::state::{RuntimeCancellationState, RuntimeHostState};

extension!(
    neovex_runtime_ext,
    ops = [
        op_neovex_ctx_query_start,
        op_neovex_ctx_query_with_index,
        op_neovex_ctx_query_filter,
        op_neovex_ctx_query_order,
        op_neovex_ctx_query,
        op_neovex_ctx_paginated_query,
        op_neovex_ctx_mutation,
        op_neovex_ctx_action,
        op_neovex_http_route,
        op_neovex_ctx_db_get,
        op_neovex_ctx_db_insert,
        op_neovex_ctx_db_patch,
        op_neovex_ctx_db_delete,
        op_neovex_ctx_query_collect,
        op_neovex_ctx_query_take,
        op_neovex_ctx_query_paginate,
        op_neovex_ctx_query_first,
        op_neovex_ctx_query_unique,
        op_neovex_ctx_scheduler_run_after,
        op_neovex_ctx_scheduler_run_at,
        op_neovex_ctx_scheduler_cancel,
        op_neovex_ctx_runtime_enter_nested_call,
        op_neovex_ctx_run_query,
        op_neovex_ctx_run_mutation,
        op_neovex_ctx_run_action
    ],
);

#[op2]
#[serde]
fn op_neovex_ctx_query_start(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryStartPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryStart, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_with_index(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryWithIndexPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryWithIndex, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_filter(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryFilterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryFilter, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_order(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryOrderPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryOrder, payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_paginated_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncPaginatedQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxPaginatedQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncMutationPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxMutation, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncActionPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxAction, payload).await
}

#[op2]
#[serde]
async fn op_neovex_http_route(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncHttpRoutePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::HttpRoute, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_get(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbGetPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbGet, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_insert(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbInsertPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbInsert, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_patch(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbPatchPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbPatch, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_delete(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbDeletePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbDelete, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_collect(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryCollect, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_take(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTakePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryTake, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_paginate(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPaginatePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryPaginate, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_first(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryFirst, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_unique(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryUnique, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_after(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAfterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAfter, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_at(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAtPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAt, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_cancel(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerCancelPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerCancel, payload).await
}

#[op2]
#[serde]
fn op_neovex_ctx_runtime_enter_nested_call(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncNestedCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxRuntimeEnterNestedCall, payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunMutation, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunAction, payload).await
}

async fn op_neovex_async_host_call<T>(
    state: Rc<RefCell<OpState>>,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize + Send + 'static,
{
    struct HostCallPermitLease {
        permit: SharedInvocationPermit,
        completed: bool,
    }

    impl HostCallPermitLease {
        fn new(permit: SharedInvocationPermit) -> Self {
            permit.begin_async_host_call();
            Self {
                permit,
                completed: false,
            }
        }

        async fn complete(&mut self) -> std::result::Result<(), JsErrorBox> {
            self.completed = true;
            self.permit
                .complete_async_host_call()
                .await
                .map_err(|error| JsErrorBox::generic(error.to_string()))
        }
    }

    impl Drop for HostCallPermitLease {
        fn drop(&mut self) {
            if !self.completed {
                self.permit.drop_async_host_call();
            }
        }
    }

    let (host_state, cancel_handle, cancellation_signal, permit) = {
        let state = state.borrow();
        (
            state.borrow::<RuntimeHostState>().clone(),
            state
                .borrow::<RuntimeCancellationState>()
                .cancel_handle
                .clone(),
            state.borrow::<RuntimeCancellationState>().signal.clone(),
            state.borrow::<SharedInvocationPermit>().clone(),
        )
    };
    let mut permit_lease = HostCallPermitLease::new(permit);
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let host_call = host_state
        .bridge
        .call_async(
            HostCallRequest {
                operation,
                payload: payload_value,
            },
            cancellation_signal.clone(),
        )
        .or_cancel(cancel_handle.clone());
    tokio::pin!(host_call);

    tokio::select! {
        result = &mut host_call => {
            let result = normalize_host_call_value(
                result
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            );
            permit_lease.complete().await?;
            result
        }
        _ = cancellation_signal.cancelled() => {
            cancel_handle.cancel();
            let result = normalize_host_call_value(
                host_call
                .await
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            );
            permit_lease.complete().await?;
            result
        }
    }
}

fn normalize_host_call_value(
    value: Value,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    match serde_json::from_value::<RuntimeHostCallEnvelope>(value.clone()) {
        Ok(envelope) => Ok(envelope),
        Err(_) => Ok(RuntimeHostCallEnvelope::Ok { value }),
    }
}

fn op_neovex_sync_host_call<T>(
    state: &mut OpState,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize,
{
    let host_state = state.borrow::<RuntimeHostState>().clone();
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let value = host_state
        .bridge
        .call(HostCallRequest {
            operation,
            payload: payload_value,
        })
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    normalize_host_call_value(value)
}

pub(crate) fn runtime_extension() -> deno_core::Extension {
    neovex_runtime_ext::init()
}
