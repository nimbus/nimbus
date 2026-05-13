use std::cell::RefCell;
use std::rc::Rc;

use crate::RuntimeBundle;
use crate::backends::v8::embedder::{JsErrorBox, OpState, op2, v8};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;

use super::bundle::sync_runtime_test_spawn_file_outputs;
use super::invocation::{
    prepare_runtime_test_spawn_invocation, runtime_test_spawn_envelope,
    runtime_test_spawn_result_from_value,
};
use super::types::RuntimeTestSpawnPayload;

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_test_spawn(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeTestSpawnPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let prepared = prepare_runtime_test_spawn_invocation(state, payload)?;
    let result = runtime_test_spawn_result_from_value(
        prepared
            .runtime
            .invoke_bundle(
                &RuntimeBundle::new(&prepared.bundle_path),
                &prepared.request,
            )
            .await,
    );
    sync_runtime_test_spawn_file_outputs(&prepared.file_output_syncs)?;
    prepared.process_state_snapshot.restore()?;
    runtime_test_spawn_envelope(result?)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_test_spawn_sync(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeTestSpawnPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let prepared = prepare_runtime_test_spawn_invocation(state, payload)?;
    let result = runtime_test_spawn_result_from_value(prepared.runtime.invoke_bundle_blocking(
        &RuntimeBundle::new(&prepared.bundle_path),
        &prepared.request,
    ));
    sync_runtime_test_spawn_file_outputs(&prepared.file_output_syncs)?;
    prepared.process_state_snapshot.restore()?;
    runtime_test_spawn_envelope(result?)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_test_force_gc(
    scope: &mut v8::PinScope,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    scope.low_memory_notification();
    scope.clear_kept_objects();
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::Value::Null,
    })
}
