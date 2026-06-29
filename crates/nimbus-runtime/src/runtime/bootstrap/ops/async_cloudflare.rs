use std::cell::RefCell;
use std::rc::Rc;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeAsyncCfKvDeletePayload, RuntimeAsyncCfKvGetPayload, RuntimeAsyncCfKvListPayload,
    RuntimeAsyncCfKvPutPayload, RuntimeHostCallEnvelope,
};
use super::shared::op_nimbus_async_host_call;

#[op2]
#[serde]
pub(super) async fn op_nimbus_cf_kv_get(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncCfKvGetPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CfKvGet, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_cf_kv_put(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncCfKvPutPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CfKvPut, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_cf_kv_delete(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncCfKvDeletePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CfKvDelete, payload).await
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_cf_kv_list(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncCfKvListPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_async_host_call(state, HostCallOperation::CfKvList, payload).await
}
