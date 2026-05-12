use nimbus_core::Error;
use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError};

pub(crate) fn runtime_error_to_core(error: NimbusRuntimeError) -> Error {
    match error {
        NimbusRuntimeError::Cancelled | NimbusRuntimeError::ExecutionTimeout(_) => Error::Cancelled,
        NimbusRuntimeError::TenantQueueLimitExceeded { .. } => {
            Error::ResourceExhausted(error.to_string())
        }
        other => Error::Internal(format!("convex runtime error: {other}")),
    }
}

pub(crate) fn check_host_cancellation(cancellation: &HostCallCancellation) -> Result<(), Error> {
    if cancellation.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_runtime_host_not_cancelled(
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NimbusRuntimeError> {
    check_host_cancellation(cancellation).map_err(|_| NimbusRuntimeError::Cancelled)
}
