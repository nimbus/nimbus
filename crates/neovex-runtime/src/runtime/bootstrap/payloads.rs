use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::InvocationAuth;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum RuntimeHostCallEnvelope {
    Ok { value: Value },
    Error { error: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncQueryPayload {
    pub(super) query: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncPaginatedQueryPayload {
    pub(super) query: Value,
    pub(super) page_size: usize,
    #[serde(default)]
    pub(super) cursor: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncDbGetPayload {
    pub(super) table: String,
    pub(super) id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncMutationPayload {
    pub(super) mutation: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncActionPayload {
    pub(super) action: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncHttpRoutePayload {
    pub(super) request: Value,
    pub(super) route: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncDbInsertPayload {
    pub(super) table: String,
    pub(super) fields: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncDbPatchPayload {
    pub(super) table: String,
    pub(super) id: String,
    pub(super) patch: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncDbDeletePayload {
    pub(super) table: String,
    pub(super) id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncQueryStartPayload {
    pub(super) table: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncQueryWithIndexPayload {
    pub(super) builder_id: String,
    pub(super) index_name: String,
    #[serde(default)]
    pub(super) filters: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncQueryFilterPayload {
    pub(super) builder_id: String,
    #[serde(default)]
    pub(super) filters: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncQueryOrderPayload {
    pub(super) builder_id: String,
    pub(super) direction: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncSchedulerRunAfterPayload {
    pub(super) delay_ms: u64,
    pub(super) name: String,
    pub(super) visibility: String,
    #[serde(default)]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncSchedulerRunAtPayload {
    pub(super) timestamp_ms: u64,
    pub(super) name: String,
    pub(super) visibility: String,
    #[serde(default)]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncSchedulerCancelPayload {
    pub(super) job_id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncServiceLookupPayload {
    pub(super) service_name: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncFunctionCallPayload {
    pub(super) name: String,
    pub(super) visibility: String,
    #[serde(default)]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeSyncNestedCallPayload {
    pub(super) name: String,
    pub(super) visibility: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncQueryTerminalPayload {
    pub(super) builder_id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncQueryTakePayload {
    pub(super) builder_id: String,
    pub(super) limit: usize,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RuntimeAsyncQueryPaginatePayload {
    pub(super) builder_id: String,
    pub(super) page_size: usize,
    #[serde(default)]
    pub(super) cursor: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}
