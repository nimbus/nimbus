use std::sync::atomic::{AtomicBool, Ordering};

use deno_core::{JsRuntime, scope, serde_v8, v8};
use serde_json::Value;

use crate::error::{NeovexRuntimeError, Result};
use crate::limits::RuntimeLimits;

pub(crate) fn deserialize_json_value(
    runtime: &mut JsRuntime,
    value: v8::Global<v8::Value>,
) -> Result<Value> {
    scope!(scope, runtime);
    let local = v8::Local::new(scope, value);
    serde_v8::from_v8(scope, local)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))
}

pub(crate) fn runtime_js_error(error: impl std::fmt::Display) -> NeovexRuntimeError {
    NeovexRuntimeError::JavaScript(error.to_string())
}

pub(crate) fn classify_runtime_error(
    error: NeovexRuntimeError,
    timeout_triggered: &AtomicBool,
    heap_limit_triggered: &AtomicBool,
    external_cancellation_triggered: &AtomicBool,
    limits: &RuntimeLimits,
) -> NeovexRuntimeError {
    match error {
        NeovexRuntimeError::JavaScript(message)
            if heap_limit_triggered.load(Ordering::SeqCst)
                && is_execution_terminated_error(&message) =>
        {
            NeovexRuntimeError::HeapLimitExceeded(limits.max_heap_mb)
        }
        NeovexRuntimeError::JavaScript(message) if is_host_call_canceled_error(&message) => {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message)
            if external_cancellation_triggered.load(Ordering::SeqCst) =>
        {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message) if timeout_triggered.load(Ordering::SeqCst) => {
            NeovexRuntimeError::ExecutionTimeout(limits.execution_timeout)
        }
        other => other,
    }
}

fn is_execution_terminated_error(message: &str) -> bool {
    message.contains("execution terminated")
}

fn is_host_call_canceled_error(message: &str) -> bool {
    message.contains("runtime host call canceled")
}
