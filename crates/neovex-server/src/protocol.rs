use neovex_core::{Document, DurableMutationRecord, Query, Schema};
use neovex_engine::{MaterializedJournalSnapshot, TenantEngineDiagnosticsSnapshot};
use neovex_runtime::{
    RuntimeBackendKind, RuntimeCompatibilityTarget, RuntimeExecutionModel, RuntimeGrants,
    RuntimeLanguage, RuntimeMetricsSnapshot, RuntimeMode, RuntimeModuleStateSemantics,
    RuntimePoolKind, RuntimePreset, RuntimeResetCapabilities, RuntimeRoutingAffinity,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error_envelope::{ErrorSeverity, PublicError};
use crate::ws::NegotiatedWebSocketProtocol;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateTenantRequest {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TenantResponse {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TenantListResponse {
    pub tenants: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InsertDocumentRequest {
    pub table: String,
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DocumentResponse {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduleResponse {
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateDocumentRequest {
    pub patch: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DataResponse {
    pub data: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DocumentDataResponse {
    pub document: Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeDiagnosticsResponse {
    pub limits: RuntimeLimitsResponse,
    pub reset_capabilities: RuntimeResetCapabilities,
    pub metrics: RuntimeMetricsSnapshot,
}

#[derive(Debug, Serialize)]
pub(crate) struct TenantEngineDiagnosticsResponse {
    pub tenant_id: String,
    pub diagnostics: TenantEngineDiagnosticsSnapshot,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeLimitsResponse {
    pub runtime_backend: RuntimeBackendKind,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub execution_model: RuntimeExecutionModel,
    pub runtime_mode: RuntimeMode,
    pub runtime_language: RuntimeLanguage,
    pub runtime_preset: RuntimePreset,
    pub runtime_grants: RuntimeGrants,
    pub runtime_pool_kind: RuntimePoolKind,
    pub module_state_semantics: RuntimeModuleStateSemantics,
    pub routing_affinity: RuntimeRoutingAffinity,
    pub routing_affinity_max_entries: usize,
    pub max_warm_pool_entries_per_worker: usize,
    pub max_warm_reuses: usize,
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout_ms: u64,
    pub max_concurrent_runtime_instances: usize,
    pub worker_threads: usize,
    pub max_active_top_level_invocations_per_tenant: usize,
    pub max_in_flight_top_level_invocations_per_tenant: usize,
    pub max_queued_top_level_invocations_per_tenant: usize,
    pub max_nested_runtime_invocations: usize,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct JournalStreamRequest {
    pub after: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JournalStreamResponse {
    pub records: Vec<DurableMutationRecord>,
    pub next_cursor: u64,
    pub latest_sequence: u64,
    pub cursor_floor: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct MaterializedJournalSnapshotResponse {
    pub version: u16,
    pub applied_sequence: u64,
    pub durable_head: u64,
    pub schema: Schema,
    pub documents: Vec<Document>,
    pub scheduled_execution_ids: Vec<String>,
}

impl From<MaterializedJournalSnapshot> for MaterializedJournalSnapshotResponse {
    fn from(snapshot: MaterializedJournalSnapshot) -> Self {
        Self {
            version: snapshot.version,
            applied_sequence: snapshot.applied_sequence.0,
            durable_head: snapshot.durable_head.0,
            schema: snapshot.schema,
            documents: snapshot.documents,
            scheduled_execution_ids: snapshot.scheduled_execution_ids,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct JournalBootstrapResponse {
    pub snapshot: MaterializedJournalSnapshotResponse,
    pub resume_after_sequence: u64,
    pub bootstrap_cut_sequence: u64,
    pub cursor_floor_sequence: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduledJobsResponse {
    pub jobs: Vec<neovex_core::ScheduledJob>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduledJobResultResponse {
    pub result: neovex_core::ScheduledJobResult,
}

#[derive(Debug, Serialize)]
pub(crate) struct CronJobsResponse {
    pub crons: Vec<neovex_core::CronJob>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ClientMessage {
    #[serde(rename = "authenticate")]
    Authenticate {
        #[serde(rename = "token")]
        _token: String,
    },
    #[serde(rename = "clear_auth")]
    ClearAuth,
    #[serde(rename = "subscribe")]
    Subscribe { request_id: String, query: Query },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { subscription_id: u64 },
}

#[derive(Debug, Clone)]
pub(crate) enum ServerMessage {
    Authenticated {
        is_authenticated: bool,
    },
    AuthError {
        error: PublicError,
    },
    SubscriptionResult {
        subscription_id: u64,
        request_id: Option<String>,
        data: Value,
    },
    Error {
        request_id: Option<String>,
        error: PublicError,
    },
}

impl ServerMessage {
    pub(crate) fn auth_error(message: impl Into<String>) -> Self {
        Self::AuthError {
            error: PublicError::auth_unauthorized(message),
        }
    }

    pub(crate) fn request_error(
        request_id: impl Into<String>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        let request_id = request_id.into();
        Self::Error {
            request_id: Some(request_id.clone()),
            error: PublicError::websocket_error(
                code,
                message,
                ErrorSeverity::Error,
                false,
                Some(request_id),
            ),
        }
    }

    pub(crate) fn session_error(code: &'static str, message: impl Into<String>) -> Self {
        Self::Error {
            request_id: None,
            error: PublicError::websocket_error(
                code,
                message,
                ErrorSeverity::Error,
                false,
                None::<String>,
            ),
        }
    }

    pub(crate) fn session_warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::Error {
            request_id: None,
            error: PublicError::warning(code, message, None::<String>),
        }
    }

    pub(crate) fn to_text(
        &self,
        protocol: NegotiatedWebSocketProtocol,
    ) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_json(protocol))
    }

    fn to_json(&self, protocol: NegotiatedWebSocketProtocol) -> Value {
        match protocol {
            NegotiatedWebSocketProtocol::V2 => self.to_v2_json(),
        }
    }

    fn to_v2_json(&self) -> Value {
        match self {
            Self::Authenticated { is_authenticated } => json!({
                "type": "authenticated",
                "is_authenticated": is_authenticated,
            }),
            Self::AuthError { error } => json!({
                "type": "error",
                "error": error,
            }),
            Self::SubscriptionResult {
                subscription_id,
                request_id,
                data,
            } => {
                let mut body = json!({
                    "type": "subscription_result",
                    "subscription_id": subscription_id,
                    "data": data,
                });
                if let Some(request_id) = request_id {
                    body["request_id"] = Value::String(request_id.clone());
                }
                body
            }
            Self::Error { request_id, error } => match request_id {
                Some(request_id) => json!({
                    "type": "op.error",
                    "id": request_id,
                    "error": error,
                }),
                None => json!({
                    "type": "error",
                    "error": error,
                }),
            },
        }
    }
}
