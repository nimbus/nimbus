use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nimbus_core::{
    CommitErrorClass, Error, HistoricalReadErrorKind, RuntimeTimeoutKind, StorageErrorKind,
};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::execution::invocations::next_runtime_server_request_id;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorSeverity {
    Fatal,
    Error,
    Warning,
}

/// The wire surface that is about to carry an error.
///
/// `requestId` correlates an error to the request that caused it. A caller
/// that supplies its own request id (a WebSocket `op` frame carries one) wins;
/// a caller that does not — every HTTP request, and every WebSocket frame sent
/// outside a request — gets a server-minted id instead, and the surface
/// supplies its label so an id read from a log or a support ticket says where
/// it came from. The label is therefore a property of the surface, never a
/// default baked into the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorSurface {
    /// The WebSocket protocol: session frames, and the HTTP response that
    /// rejects an upgrade before a session exists.
    WebSocket,
    /// Any other response on the HTTP listener.
    Http,
}

impl ErrorSurface {
    fn next_request_id(self) -> String {
        next_runtime_server_request_id(match self {
            Self::WebSocket => "ws-protocol",
            Self::Http => "http",
        })
    }
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
                "serverSupports": ["nimbus.v2"],
                "clientOffered": client_offered,
            }),
            Some(ErrorRemediation::new(
                "upgrade_server",
                "Update Nimbus or offer a supported protocol version.",
            )),
            ErrorSurface::WebSocket,
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
            ErrorSurface::WebSocket,
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
            ErrorSurface::WebSocket,
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
            ErrorSurface::WebSocket,
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
            ErrorSurface::WebSocket,
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
            ErrorSurface::WebSocket,
        )
    }

    pub(crate) fn auth_unauthorized(message: impl Into<String>, surface: ErrorSurface) -> Self {
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
            surface,
        )
    }

    pub(crate) fn auth_forbidden(message: impl Into<String>, surface: ErrorSurface) -> Self {
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
            surface,
        )
    }

    /// No route on this listener matches the request. This is a statement
    /// about the server's routing table, not about anything the request
    /// asked for; a request that reached a route and found nothing there is
    /// [`resource_not_found`](Self::resource_not_found).
    pub(crate) fn route_not_found(message: impl Into<String>, surface: ErrorSurface) -> Self {
        Self::new(
            "service.route_not_found",
            message.into(),
            ErrorSeverity::Error,
            false,
            Value::Null,
            None,
            surface,
        )
    }

    /// A route handled the request and the resource it named does not exist.
    /// Shares the code `nimbus_core::Error::NotFound` already maps to, so the
    /// same fact reads the same whichever layer discovered it.
    pub(crate) fn resource_not_found(message: impl Into<String>, surface: ErrorSurface) -> Self {
        Self::new(
            "op.not_found",
            message.into(),
            ErrorSeverity::Error,
            false,
            Value::Null,
            None,
            surface,
        )
    }

    pub(crate) fn from_core_error(error: &Error, surface: ErrorSurface) -> Self {
        // Every arm below reports the same core error on the same surface, so
        // the surface is bound once here rather than repeated 32 times.
        let on_surface = |code: &'static str,
                          message: String,
                          severity: ErrorSeverity,
                          retryable: bool,
                          detail: Value,
                          remediation: Option<ErrorRemediation>| {
            Self::new(
                code,
                message,
                severity,
                retryable,
                detail,
                remediation,
                surface,
            )
        };
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
                    on_surface(
                        "op.conflict",
                        error.to_string(),
                        ErrorSeverity::Error,
                        *retryable,
                        conflict_detail(*conflicting_sequence, *attempts, error.retryability()),
                        Some(ErrorRemediation::new(
                            "fix_request",
                            "Resolve the conflicting state and retry.",
                        )),
                    )
                }
                CommitErrorClass::Overloaded => on_surface(
                    "rate.overloaded",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    retryability_detail(error.retryability()),
                    Some(ErrorRemediation::new(
                        "wait_and_retry",
                        "Wait for mutation capacity to recover before retrying.",
                    )),
                ),
                CommitErrorClass::CommitterFull => {
                    let Error::CommitterFull { capacity, .. } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    on_surface(
                        "rate.committer_full",
                        error.to_string(),
                        ErrorSeverity::Error,
                        true,
                        json!({
                            "capacity": capacity,
                            "retryability": error.retryability(),
                        }),
                        Some(ErrorRemediation::new(
                            "wait_and_retry",
                            "Wait for committer capacity to recover before retrying.",
                        )),
                    )
                }
                CommitErrorClass::RejectedBeforeExecution => on_surface(
                    "rate.rejected_before_execution",
                    error.to_string(),
                    ErrorSeverity::Error,
                    true,
                    retryability_detail(error.retryability()),
                    Some(ErrorRemediation::new(
                        "retry",
                        "The mutation did not start and is safe to retry.",
                    )),
                ),
                CommitErrorClass::RateLimited => {
                    let Error::RateLimited { retry_after, .. } = error else {
                        unreachable!("commit class and error variant must agree")
                    };
                    on_surface(
                        "rate.limited",
                        error.to_string(),
                        ErrorSeverity::Error,
                        true,
                        json!({
                            "retryAfterMs": retry_after.as_millis(),
                            "retryability": error.retryability(),
                        }),
                        Some(ErrorRemediation::new(
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
                    on_surface(
                        "op.out_of_retention",
                        error.to_string(),
                        ErrorSeverity::Error,
                        true,
                        json!({
                            "minimumSequence": minimum_sequence.map(|sequence| sequence.0),
                            "retryability": error.retryability(),
                        }),
                        Some(ErrorRemediation::new(
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
                    on_surface(
                        "op.cap_exceeded",
                        error.to_string(),
                        ErrorSeverity::Error,
                        false,
                        json!({
                            "cap": cap.as_str(),
                            "observed": observed,
                            "limit": limit,
                            "retryability": error.retryability(),
                        }),
                        Some(ErrorRemediation::new(
                            "reduce_request",
                            "Reduce the mutation's resource usage before retrying.",
                        )),
                    )
                }
            };
        }

        match error {
            Error::Cancelled => on_surface(
                "op.cancelled",
                error.to_string(),
                ErrorSeverity::Error,
                true,
                Value::Null,
                Some(ErrorRemediation::new("retry", "Retry the operation.")),
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
                on_surface(
                    code,
                    error.to_string(),
                    ErrorSeverity::Error,
                    false,
                    json!({
                        "timeoutKind": kind.as_str(),
                        "timeoutMs": u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    }),
                    Some(ErrorRemediation::new("fix_request", remediation)),
                )
            }
            Error::FunctionThrown {
                function_path,
                message,
                stack,
            } => on_surface(
                "function.thrown",
                message.clone(),
                ErrorSeverity::Error,
                false,
                json!({ "functionPath": function_path, "stack": stack }),
                Some(ErrorRemediation::new(
                    "fix_function",
                    "Read the message and the stack, then fix the function or the input it received.",
                )),
            ),
            Error::RuntimePromiseStalled => on_surface(
                "runtime.promise_stalled",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "fix_function",
                    "Ensure every returned promise has a reachable resolution or rejection path.",
                )),
            ),
            Error::TenantNotFound(tenant_id) => on_surface(
                "session.tenant_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "tenantId": tenant_id.to_string() }),
                None,
            ),
            Error::DocumentNotFound(document_id) => on_surface(
                "op.document_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "documentId": document_id.to_string() }),
                None,
            ),
            Error::ScheduledJobNotFound(job_id) => on_surface(
                "op.scheduled_job_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "jobId": job_id.to_string() }),
                None,
            ),
            Error::AlreadyExists(_) => on_surface(
                "op.already_exists",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::ResourceExhausted(_) => on_surface(
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
            Error::PermissionDenied(_) => on_surface(
                "auth.permission_denied",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::PreconditionFailed(_) => on_surface(
                "op.precondition_failed",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                Some(ErrorRemediation::new(
                    "refresh_resource",
                    "Refresh the resource, then retry with the latest generation or resource version.",
                )),
            ),
            Error::MissingIndex { fields } => on_surface(
                "op.missing_index",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "fields": fields }),
                Some(ErrorRemediation::new(
                    "create_index",
                    "Create an index covering the required fields, then retry.",
                )),
            ),
            Error::InvalidInput(_) => on_surface(
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
            Error::SchemaValidation(_) => on_surface(
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
            Error::SchemaNotFound(table) => on_surface(
                "op.schema_not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                json!({ "table": table.as_str() }),
                None,
            ),
            Error::Storage { kind, .. } => match kind {
                StorageErrorKind::Busy => on_surface(
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
                StorageErrorKind::Transient => on_surface(
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
                StorageErrorKind::Unavailable => on_surface(
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
                StorageErrorKind::Corruption => on_surface(
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
                StorageErrorKind::Io => on_surface(
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
                StorageErrorKind::Other => on_surface(
                    "service.storage_other",
                    error.to_string(),
                    ErrorSeverity::Error,
                    false,
                    json!({ "storageKind": kind.as_str() }),
                    None,
                ),
            },
            Error::HistoricalRead { kind, .. } => on_surface(
                "op.historical_read",
                error.to_string(),
                ErrorSeverity::Error,
                matches!(
                    *kind,
                    HistoricalReadErrorKind::UnsupportedBackend
                        | HistoricalReadErrorKind::UnsupportedAdapter
                ),
                json!({ "historicalReadKind": kind.as_str() }),
                Some(ErrorRemediation::new(
                    "fix_request",
                    "Use a supported historical read target and retry within the retained history window.",
                )),
            ),
            Error::Serialization(_) => on_surface(
                "service.serialization",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Internal(_) => Self::internal(error, surface),
            Error::NotFound(_) => on_surface(
                "op.not_found",
                error.to_string(),
                ErrorSeverity::Error,
                false,
                Value::Null,
                None,
            ),
            Error::Transport(_) => on_surface(
                "service.transport",
                error.to_string(),
                ErrorSeverity::Error,
                true,
                Value::Null,
                Some(ErrorRemediation::new(
                    "retry",
                    "Retry once the transport or connection issue clears.",
                )),
            ),
            _ => Self::internal(error, surface),
        }
    }

    fn internal(error: &Error, surface: ErrorSurface) -> Self {
        let public = Self::new(
            "service.internal",
            "An internal server error occurred.",
            ErrorSeverity::Fatal,
            false,
            Value::Null,
            Some(ErrorRemediation::new(
                "contact_operator",
                "Internal server failures require operator investigation.",
            )),
            surface,
        );
        tracing::error!(
            request_id = %public.request_id,
            error = %error,
            "internal error mapped to public envelope"
        );
        public
    }

    pub(crate) fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }

    pub(crate) fn websocket_error(
        code: &'static str,
        message: impl Into<String>,
        severity: ErrorSeverity,
        retryable: bool,
        request_id: Option<impl Into<String>>,
    ) -> Self {
        let mut error = Self::new(
            code,
            message,
            severity,
            retryable,
            Value::Null,
            None,
            ErrorSurface::WebSocket,
        );
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
        surface: ErrorSurface,
    ) -> Self {
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        Self {
            code,
            message: message.into(),
            request_id: surface.next_request_id(),
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
        const SURFACE: ErrorSurface = ErrorSurface::Http;
        match error {
            crate::state::AppError::Structured(error) => *error,
            crate::state::AppError::Unauthorized(message) => Self::new(
                StatusCode::UNAUTHORIZED,
                PublicError::auth_unauthorized(message, SURFACE),
            ),
            crate::state::AppError::Forbidden(message) => Self::new(
                StatusCode::FORBIDDEN,
                PublicError::auth_forbidden(message, SURFACE),
            ),
            crate::state::AppError::RouteNotFound(message) => Self::new(
                StatusCode::NOT_FOUND,
                PublicError::route_not_found(message, SURFACE),
            ),
            crate::state::AppError::NotFound(message) => Self::new(
                StatusCode::NOT_FOUND,
                PublicError::resource_not_found(message, SURFACE),
            ),
            crate::state::AppError::Core(error) => {
                let status = if let Some(class) = error.commit_class() {
                    match class {
                        CommitErrorClass::Conflict | CommitErrorClass::OutOfRetention => {
                            StatusCode::CONFLICT
                        }
                        CommitErrorClass::Overloaded
                        | CommitErrorClass::CommitterFull
                        | CommitErrorClass::RejectedBeforeExecution
                        | CommitErrorClass::RateLimited => StatusCode::TOO_MANY_REQUESTS,
                        CommitErrorClass::CapExceeded => StatusCode::BAD_REQUEST,
                    }
                } else {
                    match &error {
                        Error::Cancelled | Error::RuntimeTimeout { .. } => {
                            StatusCode::REQUEST_TIMEOUT
                        }
                        Error::RuntimePromiseStalled | Error::FunctionThrown { .. } => {
                            StatusCode::UNPROCESSABLE_ENTITY
                        }
                        Error::TenantNotFound(_)
                        | Error::DocumentNotFound(_)
                        | Error::ScheduledJobNotFound(_)
                        | Error::SchemaNotFound(_)
                        | Error::NotFound(_) => StatusCode::NOT_FOUND,
                        Error::PreconditionFailed(_) | Error::MissingIndex { .. } => {
                            StatusCode::PRECONDITION_FAILED
                        }
                        Error::ResourceExhausted(_) => StatusCode::TOO_MANY_REQUESTS,
                        Error::PermissionDenied(_) => StatusCode::FORBIDDEN,
                        Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
                        Error::SchemaValidation(_) => StatusCode::UNPROCESSABLE_ENTITY,
                        Error::AlreadyExists(_) => StatusCode::CONFLICT,
                        Error::HistoricalRead { kind, .. } => match kind {
                            HistoricalReadErrorKind::UnsupportedBackend
                            | HistoricalReadErrorKind::UnsupportedAdapter => {
                                StatusCode::NOT_IMPLEMENTED
                            }
                            HistoricalReadErrorKind::PolicySnapshotMissing => StatusCode::FORBIDDEN,
                            HistoricalReadErrorKind::SnapshotUnavailable => {
                                StatusCode::SERVICE_UNAVAILABLE
                            }
                            HistoricalReadErrorKind::CursorMismatch
                            | HistoricalReadErrorKind::FormatMismatch
                            | HistoricalReadErrorKind::RetentionExpired
                            | HistoricalReadErrorKind::TimestampOutOfRange => {
                                StatusCode::BAD_REQUEST
                            }
                        },
                        Error::Transport(_) => StatusCode::SERVICE_UNAVAILABLE,
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
                        _ => StatusCode::INTERNAL_SERVER_ERROR,
                    }
                };
                Self::new(status, PublicError::from_core_error(&error, SURFACE))
            }
        }
    }

    pub(crate) fn from_convex_core_error(error: Error) -> Self {
        let Some(vocabulary) = nimbus_convex::convex_commit_error_vocabulary(&error) else {
            return Self::from_app_error(crate::state::AppError::Core(error));
        };
        let mut public = PublicError::from_core_error(&error, ErrorSurface::Http);
        public.code = vocabulary.code;
        let mut detail = match public.detail {
            Value::Object(detail) => detail,
            Value::Null => serde_json::Map::new(),
            other => serde_json::Map::from_iter([("nimbusDetail".to_string(), other)]),
        };
        detail.insert("shortName".to_string(), json!(vocabulary.short_name));
        detail.insert("retryability".to_string(), json!(vocabulary.retryability));
        public.detail = Value::Object(detail);
        Self::new(vocabulary.http_status, public)
    }
}

fn conflict_detail(
    conflicting_sequence: Option<nimbus_core::SequenceNumber>,
    attempts: Option<usize>,
    retryability: nimbus_core::Retryability,
) -> Value {
    let mut detail = serde_json::Map::new();
    detail.insert("retryability".to_string(), json!(retryability));
    if let Some(sequence) = conflicting_sequence {
        detail.insert("conflictingSequence".to_string(), json!(sequence.0));
    }
    if let Some(attempts) = attempts {
        detail.insert("attempts".to_string(), json!(attempts));
    }
    Value::Object(detail)
}

fn retryability_detail(retryability: nimbus_core::Retryability) -> Value {
    json!({ "retryability": retryability })
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use nimbus_core::{CommitErrorClass, Retryability, RuntimeTimeoutKind};
    use nimbus_testing::commit_taxonomy::assert_commit_taxonomy_mapping;

    /// The wire names the SDK reads.
    ///
    /// `decodeNimbusErrorEnvelope` in `packages/nimbus/src/errors.ts` pulls
    /// `requestId`, `timestamp`, `severity`, and `remediation.{action,message}`
    /// off this struct by exact key, and drops anything whose shape does not
    /// match rather than failing. A rename here would therefore not break a
    /// client loudly — it would quietly hand every caller `undefined` for the
    /// field. This pins the serialized shape so the rename fails here instead.
    #[test]
    fn public_error_serializes_the_key_set_the_sdk_decodes() {
        let public = PublicError::protocol_no_overlap(vec!["nimbus.v1".to_string()]);
        let serialized = serde_json::to_value(&public).expect("public error should serialize");
        let object = serialized
            .as_object()
            .expect("a public error serializes as an object");

        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "code",
                "detail",
                "message",
                "remediation",
                "requestId",
                "retryable",
                "severity",
                "timestamp",
            ],
        );

        let mut remediation_keys: Vec<&str> = serialized["remediation"]
            .as_object()
            .expect("a remediation serializes as an object")
            .keys()
            .map(String::as_str)
            .collect();
        remediation_keys.sort_unstable();
        assert_eq!(remediation_keys, ["action", "message"]);

        // The SDK accepts exactly "fatal", "error", and "warning"; anything
        // else decodes to `undefined`.
        assert_eq!(serialized["severity"], serde_json::json!("fatal"));

        // A remediation is optional on the wire, and its absence must drop the
        // key rather than encode a null the SDK would have to special-case.
        let without = PublicError::route_not_found("no route", ErrorSurface::Http);
        let serialized_without =
            serde_json::to_value(&without).expect("public error should serialize");
        assert!(
            !serialized_without
                .as_object()
                .expect("a public error serializes as an object")
                .contains_key("remediation"),
            "an absent remediation must not serialize as a key"
        );
    }

    #[test]
    fn server_envelope_encodes_full_commit_taxonomy_for_sdk_decoders() {
        #[rustfmt::skip]
        let expectations = [
            (CommitErrorClass::Conflict, ("op.conflict", true, "retryable".to_string(), Retryability::Retryable)),
            (CommitErrorClass::Overloaded, ("rate.overloaded", true, "retryable_after_backoff".to_string(), Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::CommitterFull, ("rate.committer_full", true, "retryable_after_backoff".to_string(), Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::RejectedBeforeExecution, ("rate.rejected_before_execution", true, "retryable".to_string(), Retryability::Retryable)),
            (CommitErrorClass::RateLimited, ("rate.limited", true, "retryable_after_backoff".to_string(), Retryability::RetryableAfterBackoff)),
            (CommitErrorClass::OutOfRetention, ("op.out_of_retention", true, "restart_transaction".to_string(), Retryability::RestartTransaction)),
            (CommitErrorClass::CapExceeded, ("op.cap_exceeded", false, "terminal".to_string(), Retryability::Terminal)),
        ];

        assert_commit_taxonomy_mapping(
            |error| {
                let public = PublicError::from_core_error(error, ErrorSurface::Http);
                (
                    public.code,
                    public.retryable,
                    public.detail["retryability"]
                        .as_str()
                        .expect("commit detail should encode retryability")
                        .to_string(),
                    error.retryability(),
                )
            },
            &expectations,
        );
    }

    #[test]
    fn convex_http_commit_errors_use_convex_vocabulary() {
        let error = Error::overloaded("busy");
        let structured = StructuredHttpError::from_convex_core_error(error);

        assert_eq!(structured.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(structured.envelope.error.code, "Overloaded");
        assert_eq!(
            structured.envelope.error.detail["shortName"],
            json!("Overloaded")
        );
        assert_eq!(
            structured.envelope.error.detail["retryability"],
            json!("retryable_after_backoff")
        );
    }

    #[test]
    fn snapshot_unavailable_historical_read_maps_to_service_unavailable() {
        let response = StructuredHttpError::from_app_error(crate::state::AppError::from(
            Error::historical_read(
                HistoricalReadErrorKind::SnapshotUnavailable,
                "serving snapshot is not available for the requested sequence",
            ),
        ));

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            response
                .message()
                .contains("serving snapshot is not available")
        );
    }

    #[test]
    fn missing_index_maps_to_precondition_failed_with_fields() {
        let response = StructuredHttpError::from_app_error(crate::state::AppError::from(
            Error::MissingIndex {
                fields: vec!["state".to_string(), "rank".to_string()],
            },
        ));

        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
        assert_eq!(response.envelope.error.code, "op.missing_index");
        assert_eq!(
            response.envelope.error.detail["fields"],
            serde_json::json!(["state", "rank"])
        );
    }

    /// A missing route and a missing resource are both 404, and they are not
    /// the same statement. The router says the path names nothing on this
    /// listener; a handler that ran says the thing the path named is not
    /// there. The published taxonomy gives each its own code, so a client can
    /// tell "you called the wrong server" from "that record is gone".
    #[test]
    fn a_missing_route_and_a_missing_resource_carry_different_codes() {
        let route = StructuredHttpError::from_app_error(crate::state::AppError::route_not_found(
            "no route matches GET /nope",
        ));
        let resource =
            StructuredHttpError::from_app_error(crate::state::AppError::not_found("no such run"));

        assert_eq!(route.status(), StatusCode::NOT_FOUND);
        assert_eq!(resource.status(), StatusCode::NOT_FOUND);
        assert_eq!(route.envelope.error.code, "service.route_not_found");
        assert_eq!(resource.envelope.error.code, "op.not_found");
        assert_eq!(
            resource.envelope.error.code,
            PublicError::from_core_error(
                &Error::NotFound("no such run".to_string()),
                ErrorSurface::Http,
            )
            .code,
            "the same fact must read the same whichever layer discovered it"
        );
    }

    /// `requestId` is provenance. The label says which listener minted it, so
    /// an id copied out of a browser console or a support ticket points at the
    /// surface that produced it. An HTTP response labelled `ws-protocol` sends
    /// an operator to the wrong log.
    #[test]
    fn a_request_id_is_labelled_by_the_surface_that_minted_it() {
        let http = StructuredHttpError::from_app_error(crate::state::AppError::route_not_found(
            "no route matches GET /nope",
        ));
        assert!(
            http.envelope.error.request_id.starts_with("http-"),
            "an HTTP error must not borrow the WebSocket's label: {}",
            http.envelope.error.request_id
        );

        let websocket = PublicError::protocol_no_overlap(vec!["nimbus.v1".to_string()]);
        assert!(
            websocket.request_id.starts_with("ws-protocol-"),
            "a WebSocket protocol error keeps its own label: {}",
            websocket.request_id
        );

        // Every surface draws from one counter, so two errors never share an
        // id however they were raised.
        let second = StructuredHttpError::from_app_error(crate::state::AppError::route_not_found(
            "no route matches GET /nope",
        ));
        assert_ne!(
            http.envelope.error.request_id,
            second.envelope.error.request_id
        );
    }

    #[test]
    fn internal_errors_are_redacted_and_correlated() {
        let public = PublicError::from_core_error(
            &Error::Internal("sensitive-internal-diagnostic-marker".to_string()),
            ErrorSurface::Http,
        );

        assert_eq!(public.code, "service.internal");
        assert_eq!(public.message, "An internal server error occurred.");
        assert!(
            !public
                .message
                .contains("sensitive-internal-diagnostic-marker")
        );
        assert!(!public.request_id.is_empty());
        let serialized = serde_json::to_value(&public).expect("public error should serialize");
        assert_eq!(serialized["requestId"], public.request_id);
        assert!(
            !serialized
                .to_string()
                .contains("sensitive-internal-diagnostic-marker")
        );
    }

    #[test]
    fn runtime_timeouts_are_safe_actionable_request_timeouts() {
        let cases = [
            (
                RuntimeTimeoutKind::Execution,
                Duration::from_millis(125),
                "runtime.execution_timeout",
                "runtime execution timed out after 125ms",
            ),
            (
                RuntimeTimeoutKind::System,
                Duration::from_millis(250),
                "runtime.system_timeout",
                "runtime system wall time timed out after 250ms",
            ),
        ];

        for (kind, timeout, expected_code, expected_message) in cases {
            let response = StructuredHttpError::from_app_error(crate::state::AppError::from(
                Error::runtime_timeout(kind, timeout),
            ));

            assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
            assert_eq!(response.envelope.error.code, expected_code);
            assert_eq!(response.envelope.error.message, expected_message);
            assert!(!response.envelope.error.retryable);
            assert_eq!(
                response.envelope.error.detail,
                json!({
                    "timeoutKind": kind.as_str(),
                    "timeoutMs": u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                })
            );
            assert_eq!(
                response
                    .envelope
                    .error
                    .remediation
                    .as_ref()
                    .map(|remediation| remediation.action),
                Some("fix_request")
            );
        }
    }

    #[test]
    fn thrown_function_errors_keep_message_path_and_stack() {
        let response = StructuredHttpError::from_app_error(crate::state::AppError::from(
            Error::function_thrown(
                "messages:send",
                "Message text must not be empty (at messages:12)",
                Some("Error: Message text must not be empty\n    at handler".to_string()),
            ),
        ));

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.envelope.error.code, "function.thrown");
        assert_eq!(
            response.envelope.error.message,
            "Message text must not be empty (at messages:12)"
        );
        assert_eq!(response.envelope.error.severity, ErrorSeverity::Error);
        assert!(!response.envelope.error.retryable);
        assert_eq!(
            response.envelope.error.detail,
            json!({
                "functionPath": "messages:send",
                "stack": "Error: Message text must not be empty\n    at handler",
            })
        );
        assert_eq!(
            response
                .envelope
                .error
                .remediation
                .as_ref()
                .map(|remediation| remediation.action),
            Some("fix_function")
        );
    }

    #[test]
    fn stalled_runtime_promise_is_safe_actionable_user_error() {
        let response = StructuredHttpError::from_app_error(crate::state::AppError::from(
            Error::RuntimePromiseStalled,
        ));

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.envelope.error.code, "runtime.promise_stalled");
        assert_eq!(
            response.envelope.error.message,
            "runtime promise cannot settle because the event loop is idle"
        );
        assert!(!response.envelope.error.retryable);
        assert_eq!(response.envelope.error.detail, Value::Null);
        assert_eq!(
            response
                .envelope
                .error
                .remediation
                .as_ref()
                .map(|remediation| remediation.action),
            Some("fix_function")
        );
    }
}
