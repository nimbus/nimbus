use neovex_core::Error;
use neovex_runtime::{HostCallCancellation, NeovexRuntimeError};

pub(crate) fn runtime_error_to_core(error: NeovexRuntimeError) -> Error {
    match error {
        NeovexRuntimeError::Cancelled | NeovexRuntimeError::ExecutionTimeout(_) => Error::Cancelled,
        NeovexRuntimeError::TenantQueueLimitExceeded { .. } => {
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
) -> std::result::Result<(), NeovexRuntimeError> {
    check_host_cancellation(cancellation).map_err(|_| NeovexRuntimeError::Cancelled)
}
