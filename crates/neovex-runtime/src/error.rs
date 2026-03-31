use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NeovexRuntimeError {
    #[error("runtime I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("runtime JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("runtime JavaScript error: {0}")]
    JavaScript(String),

    #[error("runtime contract error: {0}")]
    Contract(String),

    #[error("runtime bundle integrity check failed: {0}")]
    BundleIntegrityMismatch(String),

    #[error("runtime execution timed out after {0:?}")]
    ExecutionTimeout(Duration),

    #[error("runtime heap memory limit exceeded ({0} MB)")]
    HeapLimitExceeded(usize),

    #[error("runtime nested invocation limit exceeded ({0})")]
    NestedInvocationLimitExceeded(usize),

    #[error("runtime host call canceled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, NeovexRuntimeError>;

/// Legacy alias for Convex-shaped integrations.
pub type ConvexRuntimeError = NeovexRuntimeError;
