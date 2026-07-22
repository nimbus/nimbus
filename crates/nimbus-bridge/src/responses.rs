use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::{
    CommitErrorClass, Error, HistoricalReadErrorKind, Result, RuntimeTimeoutKind, StorageErrorKind,
};
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
        let public = RuntimeHostPublicError::from_core_error(error);
        let error = serde_json::to_value(&public).unwrap_or_else(|serialization_error| {
            tracing::error!(
                request_id = %public.request_id,
                error = %error,
                %serialization_error,
                "runtime host error envelope serialization failed"
            );
            json!({
                "code": "service.internal",
                "message": "An internal runtime host error occurred.",
                "requestId": public.request_id,
                "timestamp": public.timestamp,
                "severity": "fatal",
                "retryable": false,
                "detail": null,
                "remediation": {
                    "action": "contact_operator",
                    "message": "Internal runtime host failures require operator investigation."
                }
            })
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
        if let Some(class) = error.commit_class() {
            return match class {
                CommitErrorClass::Conflict => {
                    let Error::Conflict {
                        conflicting_sequence,
                        retryable,
                        attempts,
                        ..
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::new(
                        "op.conflict",
                        error.to_string(),
                        RuntimeHostErrorSeverity::Error,
                        *retryable,
                        conflict_detail(*conflicting_sequence, *attempts),
                        Some(RuntimeHostErrorRemediation::new(
                            "fix_request",
                            "Resolve the conflicting state and retry.",
                        )),
                    )
                }
                CommitErrorClass::Overloaded
                | CommitErrorClass::CommitterFull
                | CommitErrorClass::RejectedBeforeExecution => Self::new(
                    "rate.overloaded",
                    error.to_string(),
                    RuntimeHostErrorSeverity::Error,
                    true,
                    Value::Null,
                    Some(RuntimeHostErrorRemediation::new(
                        "wait_and_retry",
                        "Wait for mutation capacity to recover before retrying.",
                    )),
                ),
                CommitErrorClass::RateLimited => {
                    let Error::RateLimited { retry_after, .. } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::new(
                        "rate.limited",
                        error.to_string(),
                        RuntimeHostErrorSeverity::Error,
                        true,
                        json!({ "retryAfterMs": retry_after.as_millis() }),
                        Some(RuntimeHostErrorRemediation::new(
                            "wait_and_retry",
                            "Retry after the indicated delay.",
                        )),
                    )
                }
                CommitErrorClass::OutOfRetention => {
                    let Error::OutOfRetention {
                        minimum_sequence, ..
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::new(
                        "op.out_of_retention",
                        error.to_string(),
                        RuntimeHostErrorSeverity::Error,
                        true,
                        json!({ "minimumSequence": minimum_sequence.map(|sequence| sequence.0) }),
                        Some(RuntimeHostErrorRemediation::new(
                            "restart_transaction",
                            "Restart the transaction from a fresh snapshot.",
                        )),
                    )
                }
                CommitErrorClass::CapExceeded => {
                    let Error::CapExceeded {
                        cap,
                        observed,
                        limit,
                    } = error
                    else {
                        unreachable!("commit class and error variant must agree")
                    };
                    Self::new(
                        "op.cap_exceeded",
                        error.to_string(),
                        RuntimeHostErrorSeverity::Error,
                        false,
                        json!({ "cap": cap.as_str(), "observed": observed, "limit": limit }),
                        Some(RuntimeHostErrorRemediation::new(
                            "reduce_request",
                            "Reduce the mutation's resource usage before retrying.",
                        )),
                    )
                }
            };
        }

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
            Error::RuntimeTimeout { kind, timeout } => {
                let (code, remediation) = match kind {
                    RuntimeTimeoutKind::Execution => (
                        "runtime.execution_timeout",
                        "Reduce function work or increase the configured execution timeout.",
                    ),
                    RuntimeTimeoutKind::System => (
                        "runtime.system_timeout",
                        "Ensure returned promises settle and background work completes within the configured system timeout.",
                    ),
                };
                Self::new(
                    code,
                    error.to_string(),
                    RuntimeHostErrorSeverity::Error,
                    false,
                    json!({
                        "timeoutKind": kind.as_str(),
                        "timeoutMs": u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    }),
                    Some(RuntimeHostErrorRemediation::new("fix_request", remediation)),
                )
            }
            Error::RuntimePromiseStalled => Self::new(
                "runtime.promise_stalled",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "fix_function",
                    "Ensure every returned promise has a reachable resolution or rejection path.",
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
            Error::PreconditionFailed(_) => Self::new(
                "op.precondition_failed",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "refresh_resource",
                    "Refresh the resource version or generation, then retry.",
                )),
            ),
            Error::MissingIndex { fields } => Self::new(
                "op.missing_index",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                json!({ "fields": fields }),
                Some(RuntimeHostErrorRemediation::new(
                    "create_index",
                    "Create an index covering the required fields, then retry.",
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
            Error::HistoricalRead { kind, .. } => Self::new(
                "op.historical_read",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                matches!(
                    *kind,
                    HistoricalReadErrorKind::UnsupportedBackend
                        | HistoricalReadErrorKind::UnsupportedAdapter
                ),
                json!({ "historicalReadKind": kind.as_str() }),
                Some(RuntimeHostErrorRemediation::new(
                    "fix_request",
                    "Use a supported historical read target and retry within the retained history window.",
                )),
            ),
            Error::Serialization(_) => Self::new(
                "service.serialization",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Internal(_) => Self::internal(error),
            Error::NotFound(_) => Self::new(
                "op.not_found",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Transport(_) => Self::new(
                "service.transport",
                error.to_string(),
                RuntimeHostErrorSeverity::Error,
                true,
                Value::Null,
                Some(RuntimeHostErrorRemediation::new(
                    "retry",
                    "Retry once the transport or connection issue clears.",
                )),
            ),
            _ => Self::internal(error),
        }
    }

    fn internal(error: &Error) -> Self {
        let public = Self::new(
            "service.internal",
            "An internal runtime host error occurred.",
            RuntimeHostErrorSeverity::Fatal,
            false,
            Value::Null,
            Some(RuntimeHostErrorRemediation::new(
                "contact_operator",
                "Internal runtime host failures require operator investigation.",
            )),
        );
        tracing::error!(
            request_id = %public.request_id,
            error = %error,
            "internal error mapped to runtime host envelope"
        );
        public
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

fn conflict_detail(
    conflicting_sequence: Option<nimbus_core::SequenceNumber>,
    attempts: Option<usize>,
) -> Value {
    let mut detail = serde_json::Map::new();
    if let Some(sequence) = conflicting_sequence {
        detail.insert("conflictingSequence".to_string(), json!(sequence.0));
    }
    if let Some(attempts) = attempts {
        detail.insert("attempts".to_string(), json!(attempts));
    }
    Value::Object(detail)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn runtime_host_error_envelope_preserves_historical_read_kind() {
        let error = Error::historical_read(
            HistoricalReadErrorKind::SnapshotUnavailable,
            "serving snapshot does not cover the requested read shape",
        );

        let encoded = serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .expect("error envelope should serialize");

        assert_eq!(encoded["status"], "error");
        assert_eq!(encoded["error"]["code"], "op.historical_read");
        assert_eq!(
            encoded["error"]["detail"]["historicalReadKind"],
            "snapshot_unavailable"
        );
        assert_eq!(encoded["error"]["retryable"], false);
    }

    #[test]
    fn runtime_host_error_envelope_preserves_missing_index_fields() {
        let error = Error::MissingIndex {
            fields: vec!["state".to_string(), "rank".to_string()],
        };

        let encoded = serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .expect("error envelope should serialize");

        assert_eq!(encoded["status"], "error");
        assert_eq!(encoded["error"]["code"], "op.missing_index");
        assert_eq!(
            encoded["error"]["detail"]["fields"],
            serde_json::json!(["state", "rank"])
        );
        assert_eq!(encoded["error"]["retryable"], false);
        assert_eq!(encoded["error"]["remediation"]["action"], "create_index");
    }

    #[test]
    fn runtime_host_timeout_envelope_preserves_safe_metadata() {
        let error = Error::runtime_timeout(RuntimeTimeoutKind::System, Duration::from_millis(250));

        let encoded = serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .expect("error envelope should serialize");

        assert_eq!(encoded["status"], "error");
        assert_eq!(encoded["error"]["code"], "runtime.system_timeout");
        assert_eq!(encoded["error"]["detail"]["timeoutKind"], "system");
        assert_eq!(encoded["error"]["detail"]["timeoutMs"], 250);
        assert_eq!(encoded["error"]["retryable"], false);
    }

    #[test]
    fn runtime_host_stalled_promise_is_safe_and_actionable() {
        let encoded = serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(
            &Error::RuntimePromiseStalled,
        ))
        .expect("error envelope should serialize");

        assert_eq!(encoded["status"], "error");
        assert_eq!(encoded["error"]["code"], "runtime.promise_stalled");
        assert_eq!(
            encoded["error"]["message"],
            "runtime promise cannot settle because the event loop is idle"
        );
        assert_eq!(encoded["error"]["retryable"], false);
        assert_eq!(encoded["error"]["remediation"]["action"], "fix_function");
    }

    #[test]
    fn runtime_host_internal_errors_are_redacted_and_correlated() {
        let error = Error::Internal("sensitive-internal-diagnostic-marker".to_string());

        let encoded = serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .expect("error envelope should serialize");

        assert_eq!(encoded["status"], "error");
        assert_eq!(encoded["error"]["code"], "service.internal");
        assert_eq!(
            encoded["error"]["message"],
            "An internal runtime host error occurred."
        );
        assert!(
            encoded["error"]["requestId"]
                .as_str()
                .is_some_and(|request_id| !request_id.is_empty())
        );
        assert!(
            !encoded
                .to_string()
                .contains("sensitive-internal-diagnostic-marker")
        );
    }
}
