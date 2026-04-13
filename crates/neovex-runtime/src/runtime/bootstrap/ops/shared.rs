use std::cell::RefCell;
use std::rc::Rc;

use serde::Serialize;
use serde_json::Value;

use crate::backends::v8::embedder::{CancelFuture, JsErrorBox, OpState};
use crate::executor::SharedInvocationPermit;
use crate::host::{HostCallOperation, HostCallRequest};

use super::super::payloads::RuntimeHostCallEnvelope;
use super::super::state::{InstalledRuntimeHostBridge, RuntimeCancellationState};

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

pub(super) async fn op_neovex_async_host_call<T>(
    state: Rc<RefCell<OpState>>,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize + Send + 'static,
{
    let (host_bridge, cancel_handle, cancellation_signal, permit) = {
        let state = state.borrow();
        (
            state.borrow::<InstalledRuntimeHostBridge>().slot.current(),
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
    let host_call = host_bridge
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

pub(super) fn op_neovex_sync_host_call<T>(
    state: &mut OpState,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize,
{
    let host_bridge = state.borrow::<InstalledRuntimeHostBridge>().slot.current();
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let value = host_bridge
        .call(HostCallRequest {
            operation,
            payload: payload_value,
        })
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    normalize_host_call_value(value)
}

fn normalize_host_call_value(
    value: Value,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    match serde_json::from_value::<RuntimeHostCallEnvelope>(value.clone()) {
        Ok(envelope) => Ok(envelope),
        Err(_) => Ok(RuntimeHostCallEnvelope::Ok { value }),
    }
}
