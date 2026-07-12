use std::cell::RefCell;
use std::rc::Rc;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeAsyncFunctionCallPayload, RuntimeHostCallEnvelope, RuntimeSyncNestedCallPayload,
    RuntimeSyncResolveCalleeLanePayload,
};
use super::shared::{op_nimbus_async_host_call, op_nimbus_sync_host_call};

#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_runtime_enter_nested_call(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncNestedCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::CtxRuntimeEnterNestedCall, payload)
}

/// Callee-lane oracle for the nested `ctx.run*` dispatcher. Returns the host's
/// authoritative runtime lane for `payload.name` (or null when the callee is not
/// a locally dispatchable runtime function). The dispatcher compares the result
/// against this isolate's frozen lane to choose same-isolate local dispatch
/// versus host dispatch — resolving the lane host-side means no guest-reachable
/// JavaScript state can influence that decision.
#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_resolve_callee_lane(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncResolveCalleeLanePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::CtxResolveCalleeLane, payload)
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_ctx_run_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CtxRunQuery, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_ctx_run_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CtxRunMutation, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_ctx_run_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CtxRunAction, payload).await
}
