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
    // `global.gc()` in the node-compat lane routes here (render.rs injects
    // `globalThis.gc ??= () => __nimbusSyncHostValue("op_nimbus_runtime_test_force_gc")`).
    //
    // A bare low_memory_notification() collects dead objects but does NOT run
    // the destroy() side effects the async-hooks GC fixtures observe. Under the
    // Explicit microtask policy, V8 posts FinalizationRegistry cleanup as a
    // foreground task rather than running it inline; async_hooks' GC-destroy
    // registries (promiseDestroyRegistry and the resource destroy registry) run
    // inside those tasks and call emitDestroy(). A synchronous gc() therefore
    // has to pump that foreground queue itself -- otherwise no destroy is even
    // enqueued before gc() returns. This is what
    // test/async-hooks/test-async-local-storage-gcable.js (the onGC destroy must
    // fire) and test/async-hooks/test-destroy-not-blocked.js L90 (the deferred
    // destroy list must be flooded so emitDestroy's 16384 microtask hatch arms
    // mid await-chain) both depend on.
    //
    // The bounded loop handles cascades: one full GC can free an object whose
    // finalization frees another (e.g. a WeakMap value reachable only through a
    // now-dead key), so repeat until a pass collects and runs nothing new.
    let isolate_key = deno_core::isolate_ptr_to_key(unsafe { scope.as_raw_isolate_ptr() });
    for _ in 0..8 {
        scope.clear_kept_objects();
        scope.low_memory_notification();
        let ran_tasks = deno_core::run_foreground_tasks(isolate_key);
        // Drain any microtasks the cleanups scheduled (e.g. the deferred-destroy
        // hatch). V8 makes this a no-op when a checkpoint is already running, so
        // it is safe even when gc() is called from inside an await chain.
        scope.perform_microtask_checkpoint();
        if !ran_tasks {
            break;
        }
    }
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::Value::Null,
    })
}
