use nimbus_core::Error;
use nimbus_runtime::NimbusRuntimeError;

pub(crate) use nimbus_bridge::cancellation::{
    check_host_cancellation, ensure_runtime_host_not_cancelled,
};

pub(crate) fn runtime_error_to_core(error: NimbusRuntimeError) -> Error {
    match error {
        NimbusRuntimeError::Cancelled | NimbusRuntimeError::ExecutionTimeout(_) => Error::Cancelled,
        NimbusRuntimeError::TenantQueueLimitExceeded { .. } => {
            Error::ResourceExhausted(error.to_string())
        }
        NimbusRuntimeError::CapabilityDenied(message) => {
            Error::InvalidInput(format!("convex runtime capability denied: {message}"))
        }
        other => Error::Internal(format!("convex runtime error: {other}")),
    }
}
