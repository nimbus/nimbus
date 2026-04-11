use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexRuntimeResponseEnvelope {
    Ok { value: Value },
    Error { error: ConvexRuntimeEncodedError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexRuntimeEncodedError {
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
    Serialization {
        message: String,
    },
    Internal {
        message: String,
    },
}

impl ConvexRuntimeResponseEnvelope {
    pub(in crate::adapters::convex) fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub(in crate::adapters::convex) fn from_core_error(error: Error) -> Self {
        Self::Error {
            error: ConvexRuntimeEncodedError::from_core_error(error),
        }
    }

    pub(in crate::adapters::convex) fn into_core_result(self) -> Result<Value, Error> {
        match self {
            Self::Ok { value } => Ok(value),
            Self::Error { error } => Err(error.into_core_error()),
        }
    }
}

impl ConvexRuntimeEncodedError {
    pub(in crate::adapters::convex) fn from_core_error(error: Error) -> Self {
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
            Error::Conflict(message) => Self::Conflict { message },
            Error::ResourceExhausted(message) => Self::ResourceExhausted { message },
            Error::PermissionDenied(message) => Self::PermissionDenied { message },
            Error::InvalidInput(message) => Self::InvalidInput { message },
            Error::SchemaValidation(message) => Self::SchemaValidation { message },
            Error::SchemaNotFound(table) => Self::SchemaNotFound {
                table: table.to_string(),
            },
            Error::Storage { kind, message } => Self::Storage {
                storage_kind: kind.as_str().to_string(),
                message,
            },
            Error::Serialization(message) => Self::Serialization { message },
            Error::Internal(message) => Self::Internal { message },
        }
    }

    pub(in crate::adapters::convex) fn into_core_error(self) -> Error {
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
            Self::Conflict { message } => Error::Conflict(message),
            Self::ResourceExhausted { message } => Error::ResourceExhausted(message),
            Self::PermissionDenied { message } => Error::PermissionDenied(message),
            Self::InvalidInput { message } => Error::InvalidInput(message),
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
            Self::Serialization { message } => Error::Serialization(message),
            Self::Internal { message } => Error::Internal(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neovex_core::StorageErrorKind;

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
}
