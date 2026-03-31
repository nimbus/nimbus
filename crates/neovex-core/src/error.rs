use thiserror::Error as ThisError;

use crate::types::{DocumentId, TableName, TenantId};

/// Core Neovex error type.
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("operation canceled")]
    Cancelled,

    #[error("tenant not found: {0}")]
    TenantNotFound(TenantId),

    #[error("document not found: {0}")]
    DocumentNotFound(DocumentId),

    #[error("scheduled job not found: {0}")]
    ScheduledJobNotFound(DocumentId),

    #[error("resource already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("schema validation error: {0}")]
    SchemaValidation(String),

    #[error("schema not found for table: {0}")]
    SchemaNotFound(TableName),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Shared result alias.
pub type Result<T> = std::result::Result<T, Error>;
