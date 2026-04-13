use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{RuntimeHostCallEnvelope, RuntimeSyncServiceLookupPayload};
use super::shared::op_neovex_sync_host_call;

#[op2]
#[serde]
pub(super) fn op_neovex_ctx_service_lookup(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncServiceLookupPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxServiceLookup, payload)
}
