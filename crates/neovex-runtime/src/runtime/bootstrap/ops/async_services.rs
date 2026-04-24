use std::cell::RefCell;
use std::rc::Rc;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::host::HostCallOperation;

use super::super::payloads::{RuntimeAsyncServiceLookupPayload, RuntimeHostCallEnvelope};
use super::shared::op_neovex_async_host_call;

#[op2]
#[serde]
pub(super) async fn op_neovex_ctx_service_lookup(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncServiceLookupPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxServiceLookup, payload).await
}
