use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use neovex_core::{Error, StorageErrorKind};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::execution::invocations::next_runtime_server_request_id;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorSeverity {
    Fatal,
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ErrorRemediation {
    action: &'static str,
    message: String,
}

impl ErrorRemediation {
    pub(crate) fn new(action: &'static str, message: impl Into<String>) -> Self {
        Self {
            action,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PublicError {
    code: &'static str,
    message: String,
    #[serde(rename = "requestId")]
    request_id: String,
    timestamp: String,
    severity: ErrorSeverity,
    retryable: bool,
    detail: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<ErrorRemediation>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PublicErrorEnvelope {
    pub(crate) error: PublicError,
}

#[derive(Debug)]
pub(crate) struct StructuredHttpError {
    status: StatusCode,
    envelope: PublicErrorEnvelope,
}

#[derive(Debug, Clone, Serialize)]
struct FatalErrorFrame {
    #[serde(rename = "type")]
    frame_type: &'static str,
    error: PublicError,
}

impl PublicError {
    pub(crate) fn protocol_no_overlap(client_offered: Vec<String>) -> Self {
        Self::new(
            "protocol.no_overlap",
            "Server does not support any of the offered WebSocket protocols.",
            ErrorSeverity::Fatal,
            false,
            json!({
                "serverSupports": ["neovex.v2"],
                "clientOffered": client_offered,
            }),
            Some(ErrorRemediation::new(
                "upgrade_server",
                "Update Neovex or offer a supported protocol version.",
            )),
        )
    }

    pub(crate) fn protocol_hello_timeout(timeout_ms: u64) -> Self {
        Self::new(
            "protocol.hello_timeout",
            format!("Client did not send client_hello within {timeout_ms} ms."),
            ErrorSeverity::Fatal,
            true,
            json!({ "timeoutMs": timeout_ms }),
            Some(ErrorRemediation::new(
                "retry",
                "Reconnect and send client_hello immediately after hello.",
            )),
        )
    }

    pub(crate) fn protocol_invalid_json(message: impl Into<String>) -> Self {
        Self::new(
            "protocol.invalid_json",
            message.into(),
            ErrorSeverity::Fatal,
            false,
            Value::Null,
            Some(ErrorRemediation::new(
                "fix_request",
                "Send a valid JSON text frame for the negotiated protocol.",
            )),
        )
    }

    pub(crate) fn protocol_unsupported_message_type(message_type: Option<&str>) -> Self {
        Self::new(
            "protocol.unsupported_message_type",
            "Received an unexpected WebSocket message type during protocol handshake.",
            ErrorSeverity::Fatal,
            false,
            json!({
                "receivedType": message_type,
                "expectedType": "client_hello",
            }),
            Some(ErrorRemediation::new(
                "fix_request",
                "Reply to hello with a client_hello frame first.",
            )),
        )
    }

    pub(crate) fn protocol_unsupported_version(protocol: impl Into<String>) -> Self {
        Self::new(
            "protocol.unsupported_version",
            "Client hello referenced an unsupported protocol version.",
            ErrorSeverity::Fatal,
            false,
            json!({ "receivedProtocol": protocol.into() }),
            Some(ErrorRemediation::new(
                "upgrade_client",
                "Use the negotiated protocol version echoed by the server.",
            )),
        )
    }

    pub(crate) fn protocol_unsupported_binary() -> Self {
        Self::new(
            "protocol.unsupported_binary",
            "Binary WebSocket frames are not supported during the negotiated handshake.",
            ErrorSeverity::Fatal,
            false,
            Value::Null,
            Some(ErrorRemediation::new(
                "fix_request",
                "Send a JSON client_hello text frame instead of binary data.",
            )),
        )
    }

    pub(crate) fn auth_unauthorized(message: impl Into<String>) -> Self {
        Self::new(
            "auth.unauthorized",
            message.into(),
            ErrorSeverity::Error,
            false,
            Value::Null,
            Some(ErrorRemediation::new(
                "reauthenticate",
                "Present a valid authentication token and retry.",
            )),
        )
    }

    pub(crate) fn auth_forbidden(message: impl Into<String>) -> Self {
        Self::new(
            "auth.forbidden",
            message.into(),
            ErrorSeverity::Error,
            false,
            Value::Null,
            Some(ErrorRemediation::new(
                "contact_operator",
                "Update access policy or use an allowed principal.",
            )),
        )
    }

    pub(crate) fn route_not_found(message: impl Into<String>) -> Self {
        Self::new(
            "service.route_not_found",
            message.into(),
            ErrorSeverity::Error,
            false,
            Value::Null,
            None,
        )
    }

    pub(crate) fn from_core_error(error: &Error) -> Self {
        match error {
            Error::Cancelled => Self::new(
                "op.cancelled",
                error.to_string(),
                ErrorSeverity::Error,
                true,
                Value::Null,
                Some(ErrorRemediation::new("retry", "Retry the operation.")),
            ),
            Error::TenantNotFound(tenant_id) => Self::new(
                "session.tenant_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "tenantId": tenant_id.to_string() }),
                None,
            ),
            Error::DocumentNotFound(document_id) => Self::new(
                "op.document_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "documentId": document_id.to_string() }),
                None,
            ),
            Error::ScheduledJobNotFound(job_id) => Self::new(
                "op.scheduled_job_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "jobId": job_id.to_string() }),
                None,
            ),
            Error::AlreadyExists(_) => Self::new(
                "op.already_exists",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::ResourceExhausted(_) => Self::new(
                "rate.resource_exhausted",
                error.to_string(),
                ErrorSeverity::Error,
                true,
                Value::Null,
                Some(ErrorRemediation::new(
                    "wait_and_retry",
                    "Wait for capacity to recover before retrying.",
                )),
            ),
            Error::PermissionDenied(_) => Self::new(
                "auth.permission_denied",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Conflict(_) => Self::new(
                "op.conflict",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "fix_request",
                    "Resolve the conflicting state and retry.",
                )),
            ),
            Error::InvalidInput(_) => Self::new(
                "op.invalid_input",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "fix_request",
                    "Correct the request payload before retrying.",
                )),
            ),
            Error::SchemaValidation(_) => Self::new(
                "op.schema_validation",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "fix_request",
                    "Update the document to satisfy the active schema.",
                )),
            ),
            Error::SchemaNotFound(table) => Self::new(
                "op.schema_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "table": table.as_str() }),
                None,
            ),
            Error::Storage { kind, .. } => match kind {
                StorageErrorKind::Busy => Self::new(
                    "service.storage_busy",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    json!({ "storageKind": kind.as_str() }),
                    Some(ErrorRemediation::new(
                        "wait_and_retry",
                        "Wait briefly and retry the request.",
                    )),
                ),
                StorageErrorKind::Transient => Self::new(
                    "service.storage_transient",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    json!({ "storageKind": kind.as_str() }),
                    Some(ErrorRemediation::new(
                        "retry",
                        "Retry the request after the transient storage condition clears.",
                    )),
                ),
                StorageErrorKind::Unavailable => Self::new(
                    "service.unavailable",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    json!({ "storageKind": kind.as_str() }),
                    Some(ErrorRemediation::new(
                        "retry",
                        "Retry once the storage backend becomes available.",
                    )),
                ),
                StorageErrorKind::Corruption => Self::new(
                    "service.storage_corruption",
                    error.to_string(),
                    ErrorSeverity::Fatal,
                    false,
                    json!({ "storageKind": kind.as_str() }),
                    Some(ErrorRemediation::new(
                        "contact_operator",
                        "Storage corruption requires operator intervention.",
                    )),
                ),
                StorageErrorKind::Io => Self::new(
                    "service.storage_io",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    json!({ "storageKind": kind.as_str() }),
                    Some(ErrorRemediation::new(
                        "retry",
                        "Retry after the storage I/O issue clears.",
                    )),
                ),
                StorageErrorKind::Other => Self::new(
                    "service.storage_other",
                    error.to_string(),
                    ErrorSeverity::Error,
                    false,
                    json!({ "storageKind": kind.as_str() }),
                    None,
                ),
            },
            Error::Serialization(_) => Self::new(
                "service.serialization",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Internal(_) => Self::new(
                "service.internal",
                error.to_string(),
                ErrorSeverity::Fatal,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "contact_operator",
                    "Internal server failures require operator investigation.",
                )),
            ),
        }
    }

    pub(crate) fn websocket_error(
        code: &'static str,
        message: impl Into<String>,
        severity: ErrorSeverity,
        retryable: bool,
        request_id: Option<impl Into<String>>,
    ) -> Self {
        let mut error = Self::new(code, message, severity, retryable, Value::Null, None);
        if let Some(request_id) = request_id {
            error.request_id = request_id.into();
        }
        error
    }

    pub(crate) fn warning(
        code: &'static str,
        message: impl Into<String>,
        request_id: Option<impl Into<String>>,
    ) -> Self {
        Self::websocket_error(code, message, ErrorSeverity::Warning, true, request_id)
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        self.message.as_str()
    }

    fn new(
        code: &'static str,
        message: impl Into<String>,
        severity: ErrorSeverity,
        retryable: bool,
        detail: Value,
        remediation: Option<ErrorRemediation>,
    ) -> Self {
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        Self {
            code,
            message: message.into(),
            request_id: next_runtime_server_request_id("ws-protocol"),
            timestamp,
            severity,
            retryable,
            detail,
            remediation,
        }
    }
}

impl PublicErrorEnvelope {
    pub(crate) fn new(error: PublicError) -> Self {
        Self { error }
    }
}

impl StructuredHttpError {
    pub(crate) fn new(status: StatusCode, error: PublicError) -> Self {
        Self {
            status,
            envelope: PublicErrorEnvelope::new(error),
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }

    pub(crate) fn message(&self) -> &str {
        self.envelope.error.message.as_str()
    }

    pub(crate) fn from_app_error(error: crate::state::AppError) -> Self {
        match error {
            crate::state::AppError::Structured(error) => *error,
            crate::state::AppError::Unauthorized(message) => Self::new(
                StatusCode::UNAUTHORIZED,
                PublicError::auth_unauthorized(message),
            ),
            crate::state::AppError::Forbidden(message) => {
                Self::new(StatusCode::FORBIDDEN, PublicError::auth_forbidden(message))
            }
            crate::state::AppError::NotFound(message) => {
                Self::new(StatusCode::NOT_FOUND, PublicError::route_not_found(message))
            }
            crate::state::AppError::Core(error) => {
                let status = match &error {
                    Error::Cancelled => StatusCode::REQUEST_TIMEOUT,
                    Error::TenantNotFound(_)
                    | Error::DocumentNotFound(_)
                    | Error::ScheduledJobNotFound(_)
                    | Error::SchemaNotFound(_) => StatusCode::NOT_FOUND,
                    Error::Conflict(_) => StatusCode::CONFLICT,
                    Error::ResourceExhausted(_) => StatusCode::TOO_MANY_REQUESTS,
                    Error::PermissionDenied(_) => StatusCode::FORBIDDEN,
                    Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    Error::SchemaValidation(_) => StatusCode::UNPROCESSABLE_ENTITY,
                    Error::AlreadyExists(_) => StatusCode::CONFLICT,
                    Error::Storage { kind, .. } => match kind {
                        StorageErrorKind::Busy
                        | StorageErrorKind::Transient
                        | StorageErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                        StorageErrorKind::Corruption
                        | StorageErrorKind::Io
                        | StorageErrorKind::Other => StatusCode::INTERNAL_SERVER_ERROR,
                    },
                    Error::Serialization(_) | Error::Internal(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                Self::new(status, PublicError::from_core_error(&error))
            }
        }
    }
}

impl std::fmt::Display for StructuredHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.envelope.error.message)
    }
}

impl IntoResponse for StructuredHttpError {
    fn into_response(self) -> Response {
        (self.status, axum::Json(self.envelope)).into_response()
    }
}

pub(crate) async fn send_fatal_error_and_close(
    socket: &mut WebSocket,
    error: PublicError,
    close_code_value: u16,
) {
    let fatal_frame = FatalErrorFrame {
        frame_type: "fatal_error",
        error: error.clone(),
    };
    if let Ok(text) = serde_json::to_string(&fatal_frame) {
        let _ = socket.send(Message::Text(text.into())).await;
    }
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: close_code_value,
            reason: error.code.into(),
        })))
        .await;
}

pub(crate) const FATAL_PROTOCOL_CLOSE_CODE: u16 = close_code::POLICY;
