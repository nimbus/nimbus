use http::StatusCode;
use nimbus_core::{Error, MutationCap, Retryability};

/// Convex's documented error vocabulary for commit-path failures.
///
/// `code` mirrors Convex's internal `ErrorCode` class and `short_name` mirrors
/// the developer-facing CapitalCamelCase name used by Convex error metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvexCommitErrorVocabulary {
    pub http_status: StatusCode,
    pub code: &'static str,
    pub short_name: &'static str,
    pub retryability: Retryability,
}

#[must_use]
pub fn convex_commit_error_vocabulary(error: &Error) -> Option<ConvexCommitErrorVocabulary> {
    let (http_status, code, short_name) = match error {
        Error::Conflict { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "OCC",
            "OptimisticConcurrencyControlFailure",
        ),
        Error::Overloaded { .. } => (StatusCode::SERVICE_UNAVAILABLE, "Overloaded", "Overloaded"),
        Error::CommitterFull { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Overloaded",
            "CommitterFullError",
        ),
        Error::RejectedBeforeExecution { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "RejectedBeforeExecution",
            "RejectedBeforeExecution",
        ),
        Error::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "RateLimited", "RateLimited"),
        Error::OutOfRetention { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            "OutOfRetention",
            "OutOfRetention",
        ),
        Error::CapExceeded { cap, .. } => (
            StatusCode::BAD_REQUEST,
            "PaginationLimit",
            convex_cap_short_name(*cap),
        ),
        _ => return None,
    };
    Some(ConvexCommitErrorVocabulary {
        http_status,
        code,
        short_name,
        retryability: error.retryability(),
    })
}

fn convex_cap_short_name(cap: MutationCap) -> &'static str {
    match cap {
        MutationCap::ReadBytes => "TooManyBytesRead",
        MutationCap::WriteBytes => "TooManyBytesWritten",
        MutationCap::DocumentsScanned => "TooManyDocumentsRead",
        MutationCap::DocumentsWritten => "TooManyWrites",
        MutationCap::IndexRangeCalls => "TooManyReads",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nimbus_core::{MutationCap, Retryability};

    use super::*;

    #[test]
    fn convex_surfaces_full_commit_taxonomy() {
        let cases = [
            (
                Error::retryable_conflict("race", None),
                503,
                "OCC",
                "OptimisticConcurrencyControlFailure",
                Retryability::Retryable,
            ),
            (
                Error::overloaded("busy"),
                503,
                "Overloaded",
                "Overloaded",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::committer_full("queue full", 128),
                503,
                "Overloaded",
                "CommitterFullError",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::rejected_before_execution("not started"),
                503,
                "RejectedBeforeExecution",
                "RejectedBeforeExecution",
                Retryability::Retryable,
            ),
            (
                Error::rate_limited("hot tenant", Duration::from_millis(250)),
                429,
                "RateLimited",
                "RateLimited",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::out_of_retention("snapshot expired", None),
                503,
                "OutOfRetention",
                "OutOfRetention",
                Retryability::RestartTransaction,
            ),
            (
                Error::cap_exceeded(MutationCap::DocumentsWritten, 17, 16),
                400,
                "PaginationLimit",
                "TooManyWrites",
                Retryability::Terminal,
            ),
        ];

        for (error, status, code, short_name, retryability) in cases {
            let mapped = convex_commit_error_vocabulary(&error)
                .expect("commit taxonomy errors should have Convex vocabulary");
            assert_eq!(mapped.http_status.as_u16(), status, "{error}");
            assert_eq!(mapped.code, code, "{error}");
            assert_eq!(mapped.short_name, short_name, "{error}");
            assert_eq!(mapped.retryability, retryability, "{error}");
        }
    }
}
