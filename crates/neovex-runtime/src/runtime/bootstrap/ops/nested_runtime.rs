use std::cell::RefCell;
use std::rc::Rc;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeAsyncFunctionCallPayload, RuntimeHostCallEnvelope, RuntimeSyncNestedCallPayload,
};
use super::shared::{op_neovex_async_host_call, op_neovex_sync_host_call};

#[op2]
#[serde]
pub(super) fn op_neovex_ctx_runtime_enter_nested_call(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncNestedCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxRuntimeEnterNestedCall, payload)
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_run_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunQuery, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_run_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunMutation, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_run_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunAction, payload).await
}
