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
    TenantNotFound { tenant_id: String },
    DocumentNotFound { document_id: String },
    ScheduledJobNotFound { job_id: String },
    AlreadyExists { message: String },
    ResourceExhausted { message: String },
    PermissionDenied { message: String },
    InvalidInput { message: String },
    SchemaValidation { message: String },
    SchemaNotFound { table: String },
    Storage { message: String },
    Serialization { message: String },
    Internal { message: String },
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
            Error::ResourceExhausted(message) => Self::ResourceExhausted { message },
            Error::PermissionDenied(message) => Self::PermissionDenied { message },
            Error::InvalidInput(message) => Self::InvalidInput { message },
            Error::SchemaValidation(message) => Self::SchemaValidation { message },
            Error::SchemaNotFound(table) => Self::SchemaNotFound {
                table: table.to_string(),
            },
            Error::Storage(message) => Self::Storage { message },
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
            Self::ResourceExhausted { message } => Error::ResourceExhausted(message),
            Self::PermissionDenied { message } => Error::PermissionDenied(message),
            Self::InvalidInput { message } => Error::InvalidInput(message),
            Self::SchemaValidation { message } => Error::SchemaValidation(message),
            Self::SchemaNotFound { table } => TableName::new(table)
                .map(Error::SchemaNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::Storage { message } => Error::Storage(message),
            Self::Serialization { message } => Error::Serialization(message),
            Self::Internal { message } => Error::Internal(message),
        }
    }
}
