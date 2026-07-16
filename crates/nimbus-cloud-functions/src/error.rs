use http::StatusCode;
use nimbus_core::{CommitErrorClass, Error, Retryability};

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
    let class = error.commit_class()?;
    let (http_status, status) = match class {
        CommitErrorClass::Conflict => (StatusCode::CONFLICT, "ABORTED"),
        CommitErrorClass::Overloaded | CommitErrorClass::CommitterFull => {
            (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE")
        }
        CommitErrorClass::RejectedBeforeExecution => {
            (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE")
        }
        CommitErrorClass::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "RESOURCE_EXHAUSTED"),
        CommitErrorClass::OutOfRetention => {
            (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION")
        }
        CommitErrorClass::CapExceeded => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT"),
    };
    Some(CloudFunctionsCommitErrorVocabulary {
        http_status,
        status,
        retryability: error.retryability(),
    })
}

#[cfg(test)]
mod tests {
    use nimbus_core::{CommitErrorClass, Retryability};
    use nimbus_testing::commit_taxonomy::assert_commit_taxonomy_mapping;

    use super::*;

    #[test]
    fn cloud_functions_surfaces_full_commit_taxonomy() {
        #[rustfmt::skip]
        let expectations = [
            (CommitErrorClass::Conflict, (409, "ABORTED", Retryability::Retryable)),
            (CommitErrorClass::Overloaded, (503, "UNAVAILABLE", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::CommitterFull, (503, "UNAVAILABLE", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::RejectedBeforeExecution, (503, "UNAVAILABLE", Retryability::Retryable)),
            (CommitErrorClass::RateLimited, (429, "RESOURCE_EXHAUSTED", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::OutOfRetention, (412, "FAILED_PRECONDITION", Retryability::RestartTransaction)),
            (CommitErrorClass::CapExceeded, (400, "INVALID_ARGUMENT", Retryability::Terminal)),
        ];

        assert_commit_taxonomy_mapping(
            |error| {
                let mapped = cloud_functions_commit_error_vocabulary(error)
                    .expect("canonical commit error should have callable vocabulary");
                (
                    mapped.http_status.as_u16(),
                    mapped.status,
                    mapped.retryability,
                )
            },
            &expectations,
        );
    }
}
