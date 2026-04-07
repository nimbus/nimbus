use std::cell::RefCell;
use std::rc::Rc;

use deno_core::{OpState, op2};
use deno_error::JsErrorBox;

use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeAsyncActionPayload, RuntimeAsyncDbDeletePayload, RuntimeAsyncDbInsertPayload,
    RuntimeAsyncDbPatchPayload, RuntimeAsyncHttpRoutePayload, RuntimeAsyncMutationPayload,
    RuntimeAsyncSchedulerCancelPayload, RuntimeAsyncSchedulerRunAfterPayload,
    RuntimeAsyncSchedulerRunAtPayload, RuntimeHostCallEnvelope,
};
use super::shared::op_neovex_async_host_call;

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncMutationPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxMutation, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncActionPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxAction, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_http_route(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncHttpRoutePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::HttpRoute, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_db_insert(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbInsertPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbInsert, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_db_patch(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbPatchPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbPatch, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_db_delete(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbDeletePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbDelete, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_scheduler_run_after(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAfterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAfter, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_scheduler_run_at(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAtPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAt, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_scheduler_cancel(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerCancelPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerCancel, payload).await
}
