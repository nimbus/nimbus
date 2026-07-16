use http::StatusCode;
use nimbus_core::{CommitErrorClass, Error, MutationCap, Retryability};

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
    let class = error.commit_class()?;
    let (http_status, code, short_name) = match class {
        CommitErrorClass::Conflict => (
            StatusCode::SERVICE_UNAVAILABLE,
            "OCC",
            "OptimisticConcurrencyControlFailure",
        ),
        CommitErrorClass::Overloaded => {
            (StatusCode::SERVICE_UNAVAILABLE, "Overloaded", "Overloaded")
        }
        CommitErrorClass::CommitterFull => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Overloaded",
            "CommitterFullError",
        ),
        CommitErrorClass::RejectedBeforeExecution => (
            StatusCode::SERVICE_UNAVAILABLE,
            "RejectedBeforeExecution",
            "RejectedBeforeExecution",
        ),
        CommitErrorClass::RateLimited => {
            (StatusCode::TOO_MANY_REQUESTS, "RateLimited", "RateLimited")
        }
        CommitErrorClass::OutOfRetention => (
            StatusCode::SERVICE_UNAVAILABLE,
            "OutOfRetention",
            "OutOfRetention",
        ),
        CommitErrorClass::CapExceeded => {
            let Error::CapExceeded { cap, .. } = error else {
                unreachable!("commit class and error variant must agree")
            };
            (
                StatusCode::BAD_REQUEST,
                "PaginationLimit",
                convex_cap_short_name(*cap),
            )
        }
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
    use nimbus_core::{CommitErrorClass, Retryability};
    use nimbus_testing::commit_taxonomy::assert_commit_taxonomy_mapping;

    use super::*;

    #[test]
    fn convex_surfaces_full_commit_taxonomy() {
        #[rustfmt::skip]
        let expectations = [
            (CommitErrorClass::Conflict, (503, "OCC", "OptimisticConcurrencyControlFailure", Retryability::Retryable)),
            (CommitErrorClass::Overloaded, (503, "Overloaded", "Overloaded", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::CommitterFull, (503, "Overloaded", "CommitterFullError", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::RejectedBeforeExecution, (503, "RejectedBeforeExecution", "RejectedBeforeExecution", Retryability::Retryable)),
            (CommitErrorClass::RateLimited, (429, "RateLimited", "RateLimited", Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::OutOfRetention, (503, "OutOfRetention", "OutOfRetention", Retryability::RestartTransaction)),
            (CommitErrorClass::CapExceeded, (400, "PaginationLimit", "TooManyWrites", Retryability::Terminal)),
        ];

        assert_commit_taxonomy_mapping(
            |error| {
                let mapped = convex_commit_error_vocabulary(error)
                    .expect("canonical commit error should have Convex vocabulary");
                (
                    mapped.http_status.as_u16(),
                    mapped.code,
                    mapped.short_name,
                    mapped.retryability,
                )
            },
            &expectations,
        );
    }
}
