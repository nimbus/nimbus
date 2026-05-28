use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::{Error, Result, StorageErrorKind};
use nimbus_runtime::NimbusRuntimeError;
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RuntimeHostResponseEnvelope {
    Ok { value: Value },
    Error { error: Value },
}

impl RuntimeHostResponseEnvelope {
    pub fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub fn from_core_error(error: &Error) -> Self {
        let error = serde_json::to_value(RuntimeHostPublicError::from_core_error(error))
            .unwrap_or_else(|serialization_error| {
                Value::String(format!(
                    "failed to serialize runtime host error `{error}`: {serialization_error}"
                ))
            });
        Self::Error { error }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeHostErrorSeverity {
    Fatal,
    Error,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeHostErrorRemediation {
    action: &'static str,
    message: String,
}

impl RuntimeHostErrorRemediation {
    fn new(action: &'static str, message: impl Into<String>) -> Self {
        Self {
            action,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeHostPublicError {
    code: &'static str,
    message: String,
    #[serde(rename = "requestId")]
    request_id: String,
    timestamp: String,
    severity: RuntimeHostErrorSeverity,
    retryable: bool,
    detail: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<RuntimeHostErrorRemediation>,
}

impl RuntimeHostPublicError {
    fn from_core_error(error: &Error) -> Self {
        match error {
            Error::Cancelled => Self::new(
                "op.cancelled",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "retry",
                    "Retry the operation.",
                )),
            ),
            Error::TenantNotFound(tenant_id) => Self::new(
                "session.tenant_not_found",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "tenantId": tenant_id.to_string() }),
                None,
            ),
            Error::DocumentNotFound(document_id) => Self::new(
                "op.document_not_found",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "documentId": document_id.to_string() }),
                None,
            ),
            Error::ScheduledJobNotFound(job_id) => Self::new(
                "op.scheduled_job_not_found",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "jobId": job_id.to_string() }),
                None,
            ),
            Error::AlreadyExists(_) => Self::new(
                "op.already_exists",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::ResourceExhausted(_) => Self::new(
                "rate.resource_exhausted",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "wait_and_retry",
                    "Wait for capacity to recover before retrying.",
                )),
            ),
            Error::PermissionDenied(_) => Self::new(
                "auth.permission_denied",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Conflict(_) => Self::new(
                "op.conflict",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "fix_request",
                    "Resolve the conflicting state and retry.",
                )),
            ),
            Error::InvalidInput(_) => Self::new(
                "op.invalid_input",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "fix_request",
                    "Correct the request payload before retrying.",
                )),
            ),
            Error::SchemaValidation(_) => Self::new(
                "op.schema_validation",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "fix_request",
                    "Update the document to satisfy the active schema.",
                )),
            ),
            Error::SchemaNotFound(table) => Self::new(
                "op.schema_not_found",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "table": table.as_str() }),
                None,
            ),
            Error::Storage { kind, .. } => Self::from_storage_error(error, *kind),
            Error::Serialization(_) => Self::new(
                "service.serialization",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Internal(_) => Self::new(
                "service.internal",
                error.to_string(),
                RuntimeHostErrorSeverity::Fatal,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "contact_operator",
                    "Internal server failures require operator investigation.",
                )),
            ),
        }
    }

    fn from_storage_error(error: &Error, kind: StorageErrorKind) -> Self {
        match kind {
            StorageErrorKind::Busy => Self::new(
                "service.storage_busy",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                json!({ "storageKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "wait_and_retry",
                    "Wait briefly and retry the request.",
                )),
            ),
            StorageErrorKind::Transient => Self::new(
                "service.storage_transient",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                json!({ "storageKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "retry",
                    "Retry the request after the transient storage condition clears.",
                )),
            ),
            StorageErrorKind::Unavailable => Self::new(
                "service.unavailable",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                json!({ "storageKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "retry",
                    "Retry once the storage backend becomes available.",
                )),
            ),
            StorageErrorKind::Corruption => Self::new(
                "service.storage_corruption",
                error.to_string(),
                RuntimeHostErrorSeverity::Fatal,
                false,
                json!({ "storageKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "contact_operator",
                    "Storage corruption requires operator intervention.",
                )),
            ),
            StorageErrorKind::Io => Self::new(
                "service.storage_io",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                json!({ "storageKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "retry",
                    "Retry after the storage I/O issue clears.",
                )),
            ),
            StorageErrorKind::Other => Self::new(
                "service.storage_other",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "storageKind": kind.as_str() }),
                None,
            ),
        }
    }

    fn new(
        code: &'static str,
        message: impl Into<String>,
        severity: RuntimeHostErrorSeverity,
        retryable: bool,
        detail: Value,
        remediation: Option<RuntimeHostErrorRemediation>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            request_id: next_runtime_host_request_id(),
            timestamp: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "unknown".to_string()),
            severity,
            retryable,
            detail,
            remediation,
        }
    }
}

fn next_runtime_host_request_id() -> String {
    static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    format!("runtime-host-{id:016x}")
}

pub fn encode_runtime_core_result(
    result: Result<Value>,
) -> std::result::Result<Value, NimbusRuntimeError> {
    match result {
        Ok(value) => {
            serde_json::to_value(RuntimeHostResponseEnvelope::ok(value)).map_err(Into::into)
        }
        Err(Error::Cancelled) => Err(NimbusRuntimeError::Cancelled),
        Err(error) => serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .map_err(Into::into),
    }
}
