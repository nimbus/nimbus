use neovex_core::Query;
use neovex_runtime::RuntimeMetricsSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub metrics: RuntimeMetricsSnapshot,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeLimitsResponse {
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout_ms: u64,
    pub max_concurrent_isolates: usize,
    pub max_top_level_invocations_per_tenant: usize,
    pub max_queued_top_level_invocations_per_tenant: usize,
    pub max_nested_runtime_invocations: usize,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CommitLogRequest {
    pub after: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CommitLogResponse {
    pub commits: Vec<neovex_core::CommitEntry>,
    pub latest_sequence: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ServerMessage {
    #[serde(rename = "authenticated")]
    Authenticated { is_authenticated: bool },
    #[serde(rename = "auth_error")]
    AuthError { message: String },
    #[serde(rename = "subscription_result")]
    SubscriptionResult {
        subscription_id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        data: Value,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        message: String,
    },
}
