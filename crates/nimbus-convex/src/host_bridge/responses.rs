use super::*;

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
        match error {
            Error::Cancelled => Self::Cancelled,
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
            Error::Conflict {
                message,
                conflicting_sequence,
                retryable,
                attempts,
            } => Self::Conflict {
                message,
                conflicting_sequence,
                retryable,
                attempts,
            },
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
            Error::Internal(message) => Self::Internal { message },
        }
    }

    pub fn into_core_error(self) -> Error {
        match self {
            Self::Cancelled => Error::Cancelled,
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
            Self::Internal { message } => Error::Internal(message),
        }
    }
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
}
