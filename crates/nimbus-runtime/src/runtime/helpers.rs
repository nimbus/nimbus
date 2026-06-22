use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::backends::v8::embedder::{JsRuntime, scope, serde_v8, v8};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeLimits;

pub(crate) fn deserialize_json_value(
    runtime: &mut JsRuntime,
    value: v8::Global<v8::Value>,
) -> Result<Value> {
    scope!(scope, runtime);
    let local = v8::Local::new(scope, value);
    serde_v8::from_v8(scope, local)
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))
}

pub(crate) fn ensure_wait_until_drain_succeeded(
    runtime: &mut JsRuntime,
    value: v8::Global<v8::Value>,
) -> Result<()> {
    let value = deserialize_json_value(runtime, value)?;
    let rejected = value
        .get("rejected")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            NimbusRuntimeError::Contract(
                "runtime waitUntil drain result must carry a rejected count".to_string(),
            )
        })?;
    if rejected > 0 {
        return Err(NimbusRuntimeError::JavaScript(format!(
            "Nimbus waitUntil background drain rejected {rejected} promise(s)"
        )));
    }
    Ok(())
}

pub(crate) fn runtime_js_error(error: impl std::fmt::Display) -> NimbusRuntimeError {
    NimbusRuntimeError::JavaScript(error.to_string())
}

pub(crate) fn classify_runtime_error(
    error: NimbusRuntimeError,
    timeout_triggered: &AtomicBool,
    system_timeout_triggered: &AtomicBool,
    heap_limit_triggered: &AtomicBool,
    external_cancellation_triggered: &AtomicBool,
    limits: &RuntimeLimits,
) -> NimbusRuntimeError {
    match error {
        NimbusRuntimeError::JavaScript(message)
            if heap_limit_triggered.load(Ordering::SeqCst)
                && is_execution_terminated_error(&message) =>
        {
            NimbusRuntimeError::HeapLimitExceeded(limits.max_heap_mb)
        }
        NimbusRuntimeError::JavaScript(_message)
            if system_timeout_triggered.load(Ordering::SeqCst) =>
        {
            NimbusRuntimeError::SystemTimeout(limits.system_timeout)
        }
        NimbusRuntimeError::JavaScript(_message) if timeout_triggered.load(Ordering::SeqCst) => {
            NimbusRuntimeError::ExecutionTimeout(limits.execution_timeout)
        }
        NimbusRuntimeError::JavaScript(_message)
            if external_cancellation_triggered.load(Ordering::SeqCst) =>
        {
            NimbusRuntimeError::Cancelled
        }
        NimbusRuntimeError::JavaScript(message) if is_host_call_canceled_error(&message) => {
            NimbusRuntimeError::Cancelled
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
