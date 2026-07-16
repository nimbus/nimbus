//! Error mapping: Nimbus engine/storage errors → DynamoDB error taxonomy.
//!
//! The `__type` + HTTP-status + message envelope is rendered by
//! [`crate::wire::render_error`] from a `DynamoDbError` (D0.1). This module
//! supplies the other direction: when an operation handler calls the Nimbus
//! `Service` and gets a `nimbus_core::Error`, map it to the DynamoDB-canonical
//! `DynamoDbError` so clients see the error code their SDKs expect.
//!
//! Both `nimbus_core::Error` and `extenddb_core::DynamoDbError` are foreign to
//! this crate, so this is a free function (the orphan rule forbids a `From`
//! impl). Commit failures are matched through the closed `CommitErrorClass`
//! taxonomy, so a new commit class is a compile error here while unrelated
//! core errors retain a safe internal-error fallback.

use extenddb_core::error::DynamoDbError;
use nimbus_core::{CommitErrorClass, Error as CoreError};

/// Map a Nimbus core error to the DynamoDB error taxonomy.
#[must_use]
pub fn map_core_error(error: CoreError) -> DynamoDbError {
    let message = error.to_string();
    if let Some(class) = error.commit_class() {
        return match class {
            CommitErrorClass::Conflict | CommitErrorClass::OutOfRetention => {
                DynamoDbError::TransactionConflictException(message)
            }
            CommitErrorClass::Overloaded | CommitErrorClass::CommitterFull => {
                DynamoDbError::ProvisionedThroughputExceededException(message)
            }
            CommitErrorClass::RejectedBeforeExecution => DynamoDbError::ServiceUnavailable(message),
            CommitErrorClass::RateLimited => DynamoDbError::RequestLimitExceeded(message),
            CommitErrorClass::CapExceeded => DynamoDbError::ValidationException(message),
        };
    }

    match error {
        // Missing resources → ResourceNotFoundException (HTTP 400).
        CoreError::DocumentNotFound(_)
        | CoreError::NotFound(_)
        | CoreError::TenantNotFound(_)
        | CoreError::SchemaNotFound(_)
        | CoreError::ScheduledJobNotFound(_) => DynamoDbError::ResourceNotFoundException(message),

        // Resource already exists (e.g. CreateTable on an existing table).
        CoreError::AlreadyExists(_) => DynamoDbError::ResourceInUseException(message),

        // Bad request shape / schema / value → ValidationException.
        CoreError::InvalidInput(_)
        | CoreError::MissingIndex { .. }
        | CoreError::SchemaValidation(_)
        | CoreError::Serialization(_)
        | CoreError::HistoricalRead { .. } => DynamoDbError::ValidationException(message),

        // Authorization.
        CoreError::PermissionDenied(_) => DynamoDbError::AccessDeniedException(message),

        // Failed generation / existence preconditions map to DynamoDB's
        // conditional-write failure class.
        CoreError::PreconditionFailed(_) => {
            DynamoDbError::ConditionalCheckFailedException(message, None)
        }

        // Capacity pressure is the DynamoDB exception that SDKs conventionally
        // retry with exponential backoff.
        CoreError::ResourceExhausted(_) => {
            DynamoDbError::ProvisionedThroughputExceededException(message)
        }

        // Internal/transport/storage faults and cancellation have no client-facing
        // DynamoDB code → InternalServerError (HTTP 500). (Cancellation reporting is
        // hardened in D9.3.)
        CoreError::Cancelled
        | CoreError::Storage { .. }
        | CoreError::Transport(_)
        | CoreError::Internal(_) => DynamoDbError::InternalServerError(message),
        _ => DynamoDbError::InternalServerError(message),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::wire::render_error;
    use nimbus_core::{HistoricalReadErrorKind, MutationCap, Retryability, StorageErrorKind};

    fn code(error: &DynamoDbError) -> &str {
        error.error_type()
    }

    #[test]
    fn dynamodb_surfaces_full_commit_taxonomy() {
        let cases = [
            (
                CoreError::retryable_conflict("race", None),
                "TransactionConflictException",
                400,
                Retryability::Retryable,
            ),
            (
                CoreError::overloaded("busy"),
                "ProvisionedThroughputExceededException",
                400,
                Retryability::RetryableAfterBackoff,
            ),
            (
                CoreError::committer_full("full", 128),
                "ProvisionedThroughputExceededException",
                400,
                Retryability::RetryableAfterBackoff,
            ),
            (
                CoreError::rejected_before_execution("not started"),
                "ServiceUnavailable",
                503,
                Retryability::Retryable,
            ),
            (
                CoreError::rate_limited("hot", Duration::from_millis(100)),
                "RequestLimitExceeded",
                400,
                Retryability::RetryableAfterBackoff,
            ),
            (
                CoreError::out_of_retention("expired", None),
                "TransactionConflictException",
                400,
                Retryability::RestartTransaction,
            ),
            (
                CoreError::cap_exceeded(MutationCap::WriteBytes, 2, 1),
                "ValidationException",
                400,
                Retryability::Terminal,
            ),
        ];

        for (error, expected_code, expected_status, retryability) in cases {
            assert_eq!(error.retryability(), retryability, "{error}");
            let mapped = map_core_error(error);
            assert_eq!(code(&mapped), expected_code);
            assert_eq!(mapped.status_code(), expected_status);
        }
    }

    #[test]
    fn maps_each_core_error_class_to_the_expected_dynamodb_code() {
        assert_eq!(
            code(&map_core_error(CoreError::NotFound("x".into()))),
            "ResourceNotFoundException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::AlreadyExists("t".into()))),
            "ResourceInUseException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::InvalidInput("bad".into()))),
            "ValidationException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::MissingIndex {
                fields: vec!["state".to_string(), "rank".to_string()]
            })),
            "ValidationException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::SchemaValidation("bad".into()))),
            "ValidationException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::historical_read(
                HistoricalReadErrorKind::UnsupportedAdapter,
                "historical reads are not exposed through DynamoDB"
            ))),
            "ValidationException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::PermissionDenied("no".into()))),
            "AccessDeniedException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::conflict("race"))),
            "TransactionConflictException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::PreconditionFailed(
                "stale generation".into()
            ))),
            "ConditionalCheckFailedException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::ResourceExhausted("rate".into()))),
            "ProvisionedThroughputExceededException"
        );
        assert_eq!(
            code(&map_core_error(CoreError::Internal("boom".into()))),
            "InternalServerError"
        );
        assert_eq!(
            code(&map_core_error(CoreError::Cancelled)),
            "InternalServerError"
        );
        assert_eq!(
            code(&map_core_error(CoreError::Storage {
                kind: StorageErrorKind::Io,
                message: "disk".into(),
            })),
            "InternalServerError"
        );
    }

    #[test]
    fn mapped_error_renders_a_well_formed_envelope() {
        // 400 modeled error with the correct __type + message.
        let (status, body) = render_error(&map_core_error(CoreError::InvalidInput(
            "missing the key schema attribute".into(),
        )));
        assert_eq!(status, 400);
        assert!(
            body["__type"]
                .as_str()
                .unwrap()
                .ends_with("ValidationException")
        );
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("missing the key schema attribute")
        );

        // Internal faults are 500.
        let (status, _) = render_error(&map_core_error(CoreError::Internal("x".into())));
        assert_eq!(status, 500);
    }
}
