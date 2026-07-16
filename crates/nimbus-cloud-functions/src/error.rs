use http::StatusCode;
use nimbus_core::{Error, Retryability};

/// Firebase callable/Google RPC vocabulary for commit-path failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudFunctionsCommitErrorVocabulary {
    pub http_status: StatusCode,
    pub status: &'static str,
    pub retryability: Retryability,
}

#[must_use]
pub fn cloud_functions_commit_error_vocabulary(
    error: &Error,
) -> Option<CloudFunctionsCommitErrorVocabulary> {
    let (http_status, status) = match error {
        Error::Conflict { .. } => (StatusCode::CONFLICT, "ABORTED"),
        Error::Overloaded { .. } | Error::CommitterFull { .. } => {
            (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE")
        }
        Error::RejectedBeforeExecution { .. } => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE"),
        Error::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "RESOURCE_EXHAUSTED"),
        Error::OutOfRetention { .. } => (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION"),
        Error::CapExceeded { .. } => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT"),
        _ => return None,
    };
    Some(CloudFunctionsCommitErrorVocabulary {
        http_status,
        status,
        retryability: error.retryability(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nimbus_core::{MutationCap, Retryability};

    use super::*;

    #[test]
    fn cloud_functions_surfaces_full_commit_taxonomy() {
        let cases = [
            (
                Error::retryable_conflict("race", None),
                409,
                "ABORTED",
                Retryability::Retryable,
            ),
            (
                Error::overloaded("busy"),
                503,
                "UNAVAILABLE",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::committer_full("full", 128),
                503,
                "UNAVAILABLE",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::rejected_before_execution("not started"),
                503,
                "UNAVAILABLE",
                Retryability::Retryable,
            ),
            (
                Error::rate_limited("hot", Duration::from_millis(50)),
                429,
                "RESOURCE_EXHAUSTED",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::out_of_retention("expired", None),
                412,
                "FAILED_PRECONDITION",
                Retryability::RestartTransaction,
            ),
            (
                Error::cap_exceeded(MutationCap::WriteBytes, 2, 1),
                400,
                "INVALID_ARGUMENT",
                Retryability::Terminal,
            ),
        ];

        for (error, status, vocabulary, retryability) in cases {
            let mapped = cloud_functions_commit_error_vocabulary(&error)
                .expect("commit taxonomy errors should have callable vocabulary");
            assert_eq!(mapped.http_status.as_u16(), status, "{error}");
            assert_eq!(mapped.status, vocabulary, "{error}");
            assert_eq!(mapped.retryability, retryability, "{error}");
        }
    }
}
