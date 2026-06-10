use nimbus_core::Error;

pub(super) fn map_join_error(error: tokio::task::JoinError) -> Error {
    map_executor_join_error("blocking storage executor", error)
}

pub(super) fn map_permit_error(error: tokio::sync::AcquireError) -> Error {
    map_executor_permit_error("blocking storage executor", error)
}

pub(crate) fn map_executor_join_error(
    context: &'static str,
    error: tokio::task::JoinError,
) -> Error {
    if error.is_cancelled() {
        Error::Cancelled
    } else {
        Error::Internal(format!("{context} join failed: {error}"))
    }
}

pub(crate) fn map_executor_permit_error(
    context: &'static str,
    error: tokio::sync::AcquireError,
) -> Error {
    Error::Internal(format!("{context} permit unavailable: {error}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Semaphore;

    use super::{map_executor_join_error, map_executor_permit_error};
    use nimbus_core::Error;

    #[tokio::test]
    async fn executor_join_mapper_preserves_cancelled_join() {
        let handle = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        handle.abort();
        let error = handle
            .await
            .expect_err("aborted task should produce join error");

        assert!(matches!(
            map_executor_join_error("test executor", error),
            Error::Cancelled
        ));
    }

    #[tokio::test]
    async fn executor_permit_mapper_reports_context_and_closed_semaphore() {
        let semaphore = Arc::new(Semaphore::new(1));
        semaphore.close();
        let error = semaphore
            .acquire_owned()
            .await
            .expect_err("closed semaphore should reject permit acquisition");

        match map_executor_permit_error("test executor", error) {
            Error::Internal(message) => {
                assert!(message.contains("test executor permit unavailable"));
                assert!(message.contains("closed"));
            }
            error => panic!("permit mapper should return internal error, got {error:?}"),
        }
    }
}
