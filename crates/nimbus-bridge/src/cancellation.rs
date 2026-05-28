use nimbus_core::Error;
use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError};

pub fn check_host_cancellation(cancellation: &HostCallCancellation) -> Result<(), Error> {
    if cancellation.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub fn ensure_runtime_host_not_cancelled(
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NimbusRuntimeError> {
    check_host_cancellation(cancellation).map_err(|_| NimbusRuntimeError::Cancelled)
}
