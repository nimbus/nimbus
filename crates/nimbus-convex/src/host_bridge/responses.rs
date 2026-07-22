use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nimbus_core::RuntimeTimeoutKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConvexRuntimeResponseEnvelope {
    Ok { value: Value },
    Error { error: ConvexRuntimeEncodedError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConvexRuntimeEncodedError {
    Cancelled,
    RuntimeTimeout {
        timeout_kind: RuntimeTimeoutKind,
        timeout: Duration,
    },
    RuntimePromiseStalled,
    TenantNotFound {
        tenant_id: String,
    },
    DocumentNotFound {
        document_id: String,
    },
    ScheduledJobNotFound {
        job_id: String,
    },
    AlreadyExists {
        message: String,
    },
    Conflict {
        message: String,
        conflicting_sequence: Option<nimbus_core::SequenceNumber>,
        retryable: bool,
        attempts: Option<usize>,
    },
    Overloaded {
        message: String,
    },
    CommitterFull {
        message: String,
        capacity: usize,
    },
    RejectedBeforeExecution {
        message: String,
    },
    RateLimited {
        message: String,
        retry_after: Duration,
    },
    OutOfRetention {
        message: String,
        minimum_sequence: Option<nimbus_core::SequenceNumber>,
    },
    CapExceeded {
        cap: nimbus_core::MutationCap,
        observed: u64,
        limit: u64,
    },
    PreconditionFailed {
        message: String,
    },
    ResourceExhausted {
        message: String,
    },
    PermissionDenied {
        message: String,
    },
    InvalidInput {
        message: String,
    },
    MissingIndex {
        fields: Vec<String>,
    },
    SchemaValidation {
        message: String,
    },
    SchemaNotFound {
        table: String,
    },
    Storage {
        storage_kind: String,
        message: String,
    },
    HistoricalRead {
        historical_read_kind: String,
        message: String,
    },
    Serialization {
        message: String,
    },
    NotFound {
        message: String,
    },
    Transport {
        message: String,
    },
    Internal {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

impl ConvexRuntimeResponseEnvelope {
    pub fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub fn from_core_error(error: Error) -> Self {
        Self::Error {
            error: ConvexRuntimeEncodedError::from_core_error(error),
        }
    }

    pub fn into_core_result(self) -> Result<Value, Error> {
        match self {
            Self::Ok { value } => Ok(value),
            Self::Error { error } => Err(error.into_core_error()),
        }
    }
}

impl ConvexRuntimeEncodedError {
    pub fn from_core_error(error: Error) -> Self {
        if let Some(class) = error.commit_class() {
            return match class {
                nimbus_core::CommitErrorClass::Conflict => {
                    let Error::Conflict {
                        message,
                        conflicting_sequence,
                        retryable,
                        attempts,
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::Conflict {
                        message,
                        conflicting_sequence,
                        retryable,
                        attempts,
                    }
                }
                nimbus_core::CommitErrorClass::Overloaded => {
                    let Error::Overloaded { message } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::Overloaded { message }
                }
                nimbus_core::CommitErrorClass::CommitterFull => {
                    let Error::CommitterFull { message, capacity } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::CommitterFull { message, capacity }
                }
                nimbus_core::CommitErrorClass::RejectedBeforeExecution => {
                    let Error::RejectedBeforeExecution { message } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::RejectedBeforeExecution { message }
                }
                nimbus_core::CommitErrorClass::RateLimited => {
                    let Error::RateLimited {
                        message,
                        retry_after,
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::RateLimited {
                        message,
                        retry_after,
                    }
                }
                nimbus_core::CommitErrorClass::OutOfRetention => {
                    let Error::OutOfRetention {
                        message,
                        minimum_sequence,
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::OutOfRetention {
                        message,
                        minimum_sequence,
                    }
                }
                nimbus_core::CommitErrorClass::CapExceeded => {
                    let Error::CapExceeded {
                        cap,
                        observed,
                        limit,
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::CapExceeded {
                        cap,
                        observed,
                        limit,
                    }
                }
            };
        }

        match error {
            Error::Cancelled => Self::Cancelled,
            Error::RuntimeTimeout { kind, timeout } => Self::RuntimeTimeout {
                timeout_kind: kind,
                timeout,
            },
            Error::RuntimePromiseStalled => Self::RuntimePromiseStalled,
            Error::TenantNotFound(tenant_id) => Self::TenantNotFound {
                tenant_id: tenant_id.to_string(),
            },
            Error::DocumentNotFound(document_id) => Self::DocumentNotFound {
                document_id: document_id.to_string(),
            },
            Error::ScheduledJobNotFound(job_id) => Self::ScheduledJobNotFound {
                job_id: job_id.to_string(),
            },
            Error::AlreadyExists(message) => Self::AlreadyExists { message },
            Error::PreconditionFailed(message) => Self::PreconditionFailed { message },
            Error::ResourceExhausted(message) => Self::ResourceExhausted { message },
            Error::PermissionDenied(message) => Self::PermissionDenied { message },
            Error::InvalidInput(message) => Self::InvalidInput { message },
            Error::MissingIndex { fields } => Self::MissingIndex { fields },
            Error::SchemaValidation(message) => Self::SchemaValidation { message },
            Error::SchemaNotFound(table) => Self::SchemaNotFound {
                table: table.to_string(),
            },
            Error::Storage { kind, message } => Self::Storage {
                storage_kind: kind.as_str().to_string(),
                message,
            },
            Error::HistoricalRead { kind, message } => Self::HistoricalRead {
                historical_read_kind: kind.as_str().to_string(),
                message,
            },
            Error::Serialization(message) => Self::Serialization { message },
            Error::NotFound(message) => Self::NotFound { message },
            Error::Transport(message) => Self::Transport { message },
            error @ Error::Internal(_) => Self::internal(error),
            other => Self::internal(other),
        }
    }

    fn internal(error: Error) -> Self {
        let request_id = next_convex_runtime_host_request_id();
        tracing::error!(
            %request_id,
            error = %error,
            "internal error mapped to Convex runtime host response"
        );
        Self::Internal {
            message: "An internal runtime host error occurred.".to_string(),
            request_id: Some(request_id),
        }
    }

    pub fn into_core_error(self) -> Error {
        match self {
            Self::Cancelled => Error::Cancelled,
            Self::RuntimeTimeout {
                timeout_kind,
                timeout,
            } => Error::runtime_timeout(timeout_kind, timeout),
            Self::RuntimePromiseStalled => Error::RuntimePromiseStalled,
            Self::TenantNotFound { tenant_id } => TenantId::new(tenant_id)
                .map(Error::TenantNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::DocumentNotFound { document_id } => document_id
                .parse()
                .map(Error::DocumentNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::ScheduledJobNotFound { job_id } => job_id
                .parse()
                .map(Error::ScheduledJobNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::AlreadyExists { message } => Error::AlreadyExists(message),
            Self::Conflict {
                message,
                conflicting_sequence,
                retryable,
                attempts,
            } => Error::Conflict {
                message,
                conflicting_sequence,
                retryable,
                attempts,
            },
            Self::Overloaded { message } => Error::Overloaded { message },
            Self::CommitterFull { message, capacity } => Error::CommitterFull { message, capacity },
            Self::RejectedBeforeExecution { message } => Error::RejectedBeforeExecution { message },
            Self::RateLimited {
                message,
                retry_after,
            } => Error::RateLimited {
                message,
                retry_after,
            },
            Self::OutOfRetention {
                message,
                minimum_sequence,
            } => Error::OutOfRetention {
                message,
                minimum_sequence,
            },
            Self::CapExceeded {
                cap,
                observed,
                limit,
            } => Error::CapExceeded {
                cap,
                observed,
                limit,
            },
            Self::PreconditionFailed { message } => Error::PreconditionFailed(message),
            Self::ResourceExhausted { message } => Error::ResourceExhausted(message),
            Self::PermissionDenied { message } => Error::PermissionDenied(message),
            Self::InvalidInput { message } => Error::InvalidInput(message),
            Self::MissingIndex { fields } => Error::MissingIndex { fields },
            Self::SchemaValidation { message } => Error::SchemaValidation(message),
            Self::SchemaNotFound { table } => TableName::new(table)
                .map(Error::SchemaNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::Storage {
                storage_kind,
                message,
            } => storage_kind
                .parse()
                .map(|kind| Error::storage(kind, message))
                .unwrap_or_else(Error::Internal),
            Self::HistoricalRead {
                historical_read_kind,
                message,
            } => historical_read_kind_from_str(&historical_read_kind)
                .map(|kind| Error::historical_read(kind, message))
                .unwrap_or_else(|| {
                    Error::Internal(format!(
                        "unknown historical read error kind `{historical_read_kind}`"
                    ))
                }),
            Self::Serialization { message } => Error::Serialization(message),
            Self::NotFound { message } => Error::NotFound(message),
            Self::Transport { message } => Error::Transport(message),
            Self::Internal {
                message,
                request_id,
            } => match request_id {
                Some(request_id) => {
                    Error::Internal(format!("{message} (runtime host request ID: {request_id})"))
                }
                None => Error::Internal(message),
            },
        }
    }
}

fn next_convex_runtime_host_request_id() -> String {
    static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("convex-runtime-host-{id:016x}")
}

fn historical_read_kind_from_str(value: &str) -> Option<nimbus_core::HistoricalReadErrorKind> {
    match value {
        "cursor_mismatch" => Some(nimbus_core::HistoricalReadErrorKind::CursorMismatch),
        "format_mismatch" => Some(nimbus_core::HistoricalReadErrorKind::FormatMismatch),
        "policy_snapshot_missing" => {
            Some(nimbus_core::HistoricalReadErrorKind::PolicySnapshotMissing)
        }
        "retention_expired" => Some(nimbus_core::HistoricalReadErrorKind::RetentionExpired),
        "snapshot_unavailable" => Some(nimbus_core::HistoricalReadErrorKind::SnapshotUnavailable),
        "timestamp_out_of_range" => Some(nimbus_core::HistoricalReadErrorKind::TimestampOutOfRange),
        "unsupported_adapter" => Some(nimbus_core::HistoricalReadErrorKind::UnsupportedAdapter),
        "unsupported_backend" => Some(nimbus_core::HistoricalReadErrorKind::UnsupportedBackend),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::{HistoricalReadErrorKind, StorageErrorKind};

    #[test]
    fn storage_error_round_trips_through_runtime_encoding() {
        let encoded = ConvexRuntimeEncodedError::from_core_error(Error::storage(
            StorageErrorKind::Unavailable,
            "replica cache unavailable",
        ));

        let decoded = encoded.into_core_error();
        match decoded {
            Error::Storage { kind, message } => {
                assert_eq!(kind, StorageErrorKind::Unavailable);
                assert_eq!(message, "replica cache unavailable");
            }
            other => panic!("expected storage error, got {other:?}"),
        }
    }

    #[test]
    fn historical_read_error_round_trips_through_runtime_encoding() {
        let encoded = ConvexRuntimeEncodedError::from_core_error(Error::historical_read(
            HistoricalReadErrorKind::SnapshotUnavailable,
            "serving snapshot does not cover the requested read shape",
        ));

        let decoded = encoded.into_core_error();
        match decoded {
            Error::HistoricalRead { kind, message } => {
                assert_eq!(kind, HistoricalReadErrorKind::SnapshotUnavailable);
                assert_eq!(
                    message,
                    "serving snapshot does not cover the requested read shape"
                );
            }
            other => panic!("expected historical read error, got {other:?}"),
        }
    }

    #[test]
    fn precondition_failed_error_round_trips_through_runtime_encoding() {
        let encoded = ConvexRuntimeEncodedError::from_core_error(Error::PreconditionFailed(
            "stale generation".to_owned(),
        ));

        let decoded = encoded.into_core_error();
        assert!(matches!(
            decoded,
            Error::PreconditionFailed(message) if message == "stale generation"
        ));
    }

    #[test]
    fn missing_index_error_round_trips_through_runtime_encoding() {
        let encoded = ConvexRuntimeEncodedError::from_core_error(Error::MissingIndex {
            fields: vec!["state".to_string(), "rank".to_string()],
        });

        let decoded = encoded.into_core_error();
        assert!(matches!(
            decoded,
            Error::MissingIndex { fields } if fields == vec!["state".to_string(), "rank".to_string()]
        ));
    }

    #[test]
    fn commit_taxonomy_round_trips_through_runtime_encoding() {
        let errors = [
            Error::overloaded("node pressure"),
            Error::committer_full("inbox full", 64),
            Error::rejected_before_execution("admission shed"),
            Error::rate_limited("tenant rate", Duration::from_millis(250)),
            Error::out_of_retention("snapshot expired", Some(nimbus_core::SequenceNumber(9))),
            Error::cap_exceeded(nimbus_core::MutationCap::WriteBytes, 11, 10),
        ];

        for error in errors {
            let expected = error.to_string();
            let decoded = ConvexRuntimeEncodedError::from_core_error(error).into_core_error();
            assert_eq!(decoded.to_string(), expected);
        }
    }

    #[test]
    fn runtime_timeout_round_trips_through_runtime_encoding() {
        let error = Error::runtime_timeout(RuntimeTimeoutKind::System, Duration::from_millis(250));

        let decoded = ConvexRuntimeEncodedError::from_core_error(error).into_core_error();
        assert!(matches!(
            decoded,
            Error::RuntimeTimeout {
                kind: RuntimeTimeoutKind::System,
                timeout,
            } if timeout == Duration::from_millis(250)
        ));
    }

    #[test]
    fn stalled_runtime_promise_round_trips_through_runtime_encoding() {
        let decoded = ConvexRuntimeEncodedError::from_core_error(Error::RuntimePromiseStalled)
            .into_core_error();

        assert!(matches!(decoded, Error::RuntimePromiseStalled));
    }

    #[test]
    fn internal_runtime_host_errors_are_redacted_and_correlated() {
        let encoded = ConvexRuntimeEncodedError::from_core_error(Error::Internal(
            "sensitive-internal-diagnostic-marker".to_string(),
        ));
        let serialized = serde_json::to_value(&encoded).expect("encoded error should serialize");

        assert_eq!(serialized["kind"], "internal");
        assert_eq!(
            serialized["message"],
            "An internal runtime host error occurred."
        );
        assert!(
            serialized["request_id"]
                .as_str()
                .is_some_and(|request_id| !request_id.is_empty())
        );
        assert!(
            !serialized
                .to_string()
                .contains("sensitive-internal-diagnostic-marker")
        );

        let decoded = encoded.into_core_error();
        assert!(
            !decoded
                .to_string()
                .contains("sensitive-internal-diagnostic-marker")
        );
        assert!(decoded.to_string().contains("runtime host request ID"));
    }
}
