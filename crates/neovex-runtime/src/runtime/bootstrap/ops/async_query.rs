use std::cell::RefCell;
use std::rc::Rc;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeAsyncDbGetPayload, RuntimeAsyncPaginatedQueryPayload, RuntimeAsyncQueryPaginatePayload,
    RuntimeAsyncQueryPayload, RuntimeAsyncQueryTakePayload, RuntimeAsyncQueryTerminalPayload,
    RuntimeHostCallEnvelope,
};
use super::shared::op_neovex_async_host_call;

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxQuery, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_paginated_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncPaginatedQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxPaginatedQuery, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_document_get(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbGetPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::DocumentGet, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query_collect(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::QueryReadCollect, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query_take(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTakePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::QueryReadTake, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query_paginate(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPaginatePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::QueryReadPaginate, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query_first(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::QueryReadFirst, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_query_unique(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::QueryReadUnique, payload).await
}
