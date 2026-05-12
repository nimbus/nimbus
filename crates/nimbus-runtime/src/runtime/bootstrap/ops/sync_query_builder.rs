use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{
    RuntimeHostCallEnvelope, RuntimeSyncQueryFilterPayload, RuntimeSyncQueryOrderPayload,
    RuntimeSyncQueryStartPayload, RuntimeSyncQueryWithIndexPayload,
};
use super::shared::op_nimbus_sync_host_call;

#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_query_start(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryStartPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::QueryBuilderStart, payload)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_query_with_index(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryWithIndexPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::QueryBuilderWithIndex, payload)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_query_filter(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryFilterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::QueryBuilderFilter, payload)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_ctx_query_order(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryOrderPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_nimbus_sync_host_call(state, HostCallOperation::QueryBuilderOrder, payload)
}
