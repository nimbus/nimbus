use neovex_core::{Document, DurableMutationRecord, Query, Schema};
use neovex_engine::{MaterializedJournalSnapshot, TenantEngineDiagnosticsSnapshot};
use neovex_runtime::{
    RuntimeBackendKind, RuntimeExecutionModel, RuntimeMetricsSnapshot, RuntimeModuleStateSemantics,
    RuntimePoolKind, RuntimeResetCapabilities, RuntimeRoutingAffinity,
};
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
    pub execution_model: RuntimeExecutionModel,
    pub runtime_pool_kind: RuntimePoolKind,
    pub module_state_semantics: RuntimeModuleStateSemantics,
    pub routing_affinity: RuntimeRoutingAffinity,
    pub routing_affinity_max_entries: usize,
    pub max_retained_runtimes_per_worker: usize,
    pub max_retained_runtimes_per_affinity_key_per_worker: usize,
    pub max_retained_runtime_reuses: usize,
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout_ms: u64,
    pub max_concurrent_isolates: usize,
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
