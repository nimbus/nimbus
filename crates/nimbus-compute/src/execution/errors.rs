use nimbus_core::{Error, RuntimeTimeoutKind};
use nimbus_runtime::NimbusRuntimeError;

pub use nimbus_bridge::cancellation::{check_host_cancellation, ensure_runtime_host_not_cancelled};

pub fn runtime_error_to_core(error: NimbusRuntimeError) -> Error {
    match error {
        NimbusRuntimeError::Cancelled => Error::Cancelled,
        NimbusRuntimeError::ExecutionTimeout(timeout) => {
            Error::runtime_timeout(RuntimeTimeoutKind::Execution, timeout)
        }
        NimbusRuntimeError::SystemTimeout(timeout) => {
            Error::runtime_timeout(RuntimeTimeoutKind::System, timeout)
        }
        NimbusRuntimeError::PromiseStalled => Error::RuntimePromiseStalled,
        NimbusRuntimeError::TenantQueueLimitExceeded { .. } => {
            Error::ResourceExhausted(error.to_string())
        }
        NimbusRuntimeError::CapabilityDenied(message) => {
            Error::InvalidInput(format!("convex runtime capability denied: {message}"))
        }
        other => Error::Internal(format!("convex runtime error: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn runtime_timeouts_preserve_kind_and_duration() {
        let cases = [
            (
                NimbusRuntimeError::ExecutionTimeout(Duration::from_millis(125)),
                RuntimeTimeoutKind::Execution,
                Duration::from_millis(125),
            ),
            (
                NimbusRuntimeError::SystemTimeout(Duration::from_secs(2)),
                RuntimeTimeoutKind::System,
                Duration::from_secs(2),
            ),
        ];

        for (runtime_error, expected_kind, expected_timeout) in cases {
            let core_error = runtime_error_to_core(runtime_error);
            assert!(
                matches!(
                    core_error,
                    Error::RuntimeTimeout { kind, timeout }
                        if kind == expected_kind && timeout == expected_timeout
                ),
                "runtime timeout metadata should survive the compute boundary"
            );
        }
    }

    #[test]
    fn external_runtime_cancellation_remains_distinct_from_timeout() {
        assert!(matches!(
            runtime_error_to_core(NimbusRuntimeError::Cancelled),
            Error::Cancelled
        ));
    }

    #[test]
    fn pending_runtime_promise_preserves_typed_stall() {
        assert!(matches!(
            runtime_error_to_core(NimbusRuntimeError::PromiseStalled),
            Error::RuntimePromiseStalled
        ));
    }
}
