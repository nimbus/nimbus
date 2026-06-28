use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::backends::v8::embedder::{JsRuntime, scope, serde_v8, v8};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeLimits;

const PENDING_PROMISE_WITH_RESOLVED_EVENT_LOOP: &str =
    "Promise resolution is still pending but the event loop has already resolved";

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

pub(crate) fn classify_wait_until_drain_error(
    error: NimbusRuntimeError,
    timeout_triggered: &AtomicBool,
    system_timeout_triggered: &AtomicBool,
    limits: &RuntimeLimits,
) -> NimbusRuntimeError {
    if system_timeout_triggered.load(Ordering::SeqCst) {
        return NimbusRuntimeError::SystemTimeout(limits.system_timeout);
    }
    if timeout_triggered.load(Ordering::SeqCst) {
        return NimbusRuntimeError::ExecutionTimeout(limits.execution_timeout);
    }

    match error {
        NimbusRuntimeError::JavaScript(message)
            if is_pending_promise_with_resolved_event_loop(&message) =>
        {
            match wait_until_pending_timeout_error(limits) {
                Some(NimbusRuntimeError::SystemTimeout(timeout)) => {
                    system_timeout_triggered.store(true, Ordering::SeqCst);
                    NimbusRuntimeError::SystemTimeout(timeout)
                }
                Some(NimbusRuntimeError::ExecutionTimeout(timeout)) => {
                    timeout_triggered.store(true, Ordering::SeqCst);
                    NimbusRuntimeError::ExecutionTimeout(timeout)
                }
                Some(other) => other,
                None => NimbusRuntimeError::JavaScript(message),
            }
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

fn is_pending_promise_with_resolved_event_loop(message: &str) -> bool {
    message.contains(PENDING_PROMISE_WITH_RESOLVED_EVENT_LOOP)
}

fn wait_until_pending_timeout_error(limits: &RuntimeLimits) -> Option<NimbusRuntimeError> {
    match (
        limits.system_timeout.is_zero(),
        limits.execution_timeout.is_zero(),
    ) {
        (true, true) => None,
        (false, true) => Some(NimbusRuntimeError::SystemTimeout(limits.system_timeout)),
        (true, false) => Some(NimbusRuntimeError::ExecutionTimeout(
            limits.execution_timeout,
        )),
        (false, false) if limits.system_timeout <= limits.execution_timeout => {
            Some(NimbusRuntimeError::SystemTimeout(limits.system_timeout))
        }
        (false, false) => Some(NimbusRuntimeError::ExecutionTimeout(
            limits.execution_timeout,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use crate::error::NimbusRuntimeError;
    use crate::limits::RuntimeLimits;

    use super::classify_wait_until_drain_error;

    #[test]
    fn wait_until_pending_promise_race_maps_to_system_timeout() {
        let timeout_triggered = AtomicBool::new(false);
        let system_timeout_triggered = AtomicBool::new(false);
        let mut limits = RuntimeLimits::default();
        limits.execution_timeout = Duration::from_secs(5);
        limits.system_timeout = Duration::from_millis(300);

        let error = classify_wait_until_drain_error(
            NimbusRuntimeError::JavaScript(
                "Promise resolution is still pending but the event loop has already resolved"
                    .to_string(),
            ),
            &timeout_triggered,
            &system_timeout_triggered,
            &limits,
        );

        match error {
            NimbusRuntimeError::SystemTimeout(timeout) => {
                assert_eq!(timeout, Duration::from_millis(300));
            }
            other => panic!("unexpected waitUntil drain classification: {other}"),
        }
        assert!(!timeout_triggered.load(Ordering::SeqCst));
        assert!(system_timeout_triggered.load(Ordering::SeqCst));
    }

    #[test]
    fn wait_until_pending_promise_race_uses_shorter_execution_timeout() {
        let timeout_triggered = AtomicBool::new(false);
        let system_timeout_triggered = AtomicBool::new(false);
        let mut limits = RuntimeLimits::default();
        limits.execution_timeout = Duration::from_millis(100);
        limits.system_timeout = Duration::from_secs(5);

        let error = classify_wait_until_drain_error(
            NimbusRuntimeError::JavaScript(
                "Promise resolution is still pending but the event loop has already resolved"
                    .to_string(),
            ),
            &timeout_triggered,
            &system_timeout_triggered,
            &limits,
        );

        match error {
            NimbusRuntimeError::ExecutionTimeout(timeout) => {
                assert_eq!(timeout, Duration::from_millis(100));
            }
            other => panic!("unexpected waitUntil drain classification: {other}"),
        }
        assert!(timeout_triggered.load(Ordering::SeqCst));
        assert!(!system_timeout_triggered.load(Ordering::SeqCst));
    }

    #[test]
    fn wait_until_classifier_preserves_unrelated_javascript_errors() {
        let timeout_triggered = AtomicBool::new(false);
        let system_timeout_triggered = AtomicBool::new(false);
        let limits = RuntimeLimits::default();

        let error = classify_wait_until_drain_error(
            NimbusRuntimeError::JavaScript("ordinary rejection".to_string()),
            &timeout_triggered,
            &system_timeout_triggered,
            &limits,
        );

        match error {
            NimbusRuntimeError::JavaScript(message) => {
                assert_eq!(message, "ordinary rejection");
            }
            other => panic!("unexpected waitUntil drain classification: {other}"),
        }
        assert!(!timeout_triggered.load(Ordering::SeqCst));
        assert!(!system_timeout_triggered.load(Ordering::SeqCst));
    }
}
