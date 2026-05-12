use thiserror::Error;

pub type Result<T> = std::result::Result<T, SandboxError>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxError {
    #[error("sandbox spec is invalid: {message}")]
    InvalidSpec { message: String },
    #[error("sandbox backend is unavailable: {message}")]
    BackendUnavailable { message: String },
    #[error("sandbox instance was not found: {sandbox_id}")]
    NotFound { sandbox_id: String },
    #[error("sandbox operation failed: {message}")]
    OperationFailed { message: String },
}
