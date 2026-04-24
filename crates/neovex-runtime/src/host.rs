use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::error::{NeovexRuntimeError, Result};
use crate::runtime::InvocationAuth;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostCallOperation {
    HttpRoute,
    CtxQuery,
    CtxPaginatedQuery,
    CtxMutation,
    CtxAction,
    CtxRunQuery,
    CtxRunMutation,
    CtxRunAction,
    CtxDbGet,
    CtxDbQueryStart,
    CtxDbQueryWithIndex,
    CtxDbQueryFilter,
    CtxDbQueryOrder,
    CtxDbQueryCollect,
    CtxDbQueryTake,
    CtxDbQueryPaginate,
    CtxDbQueryFirst,
    CtxDbQueryUnique,
    CtxDbInsert,
    CtxDbPatch,
    CtxDbDelete,
    CtxSchedulerRunAfter,
    CtxSchedulerRunAt,
    CtxSchedulerCancel,
    CtxServiceLookup,
    CtxRuntimeEnterNestedCall,
}

impl HostCallOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HttpRoute => "http_route",
            Self::CtxQuery => "ctx_query",
            Self::CtxPaginatedQuery => "ctx_paginated_query",
            Self::CtxMutation => "ctx_mutation",
            Self::CtxAction => "ctx_action",
            Self::CtxRunQuery => "ctx_run_query",
            Self::CtxRunMutation => "ctx_run_mutation",
            Self::CtxRunAction => "ctx_run_action",
            Self::CtxDbGet => "ctx_db_get",
            Self::CtxDbQueryStart => "ctx_db_query_start",
            Self::CtxDbQueryWithIndex => "ctx_db_query_with_index",
            Self::CtxDbQueryFilter => "ctx_db_query_filter",
            Self::CtxDbQueryOrder => "ctx_db_query_order",
            Self::CtxDbQueryCollect => "ctx_db_query_collect",
            Self::CtxDbQueryTake => "ctx_db_query_take",
            Self::CtxDbQueryPaginate => "ctx_db_query_paginate",
            Self::CtxDbQueryFirst => "ctx_db_query_first",
            Self::CtxDbQueryUnique => "ctx_db_query_unique",
            Self::CtxDbInsert => "ctx_db_insert",
            Self::CtxDbPatch => "ctx_db_patch",
            Self::CtxDbDelete => "ctx_db_delete",
            Self::CtxSchedulerRunAfter => "ctx_scheduler_run_after",
            Self::CtxSchedulerRunAt => "ctx_scheduler_run_at",
            Self::CtxSchedulerCancel => "ctx_scheduler_cancel",
            Self::CtxServiceLookup => "ctx_service_lookup",
            Self::CtxRuntimeEnterNestedCall => "ctx_runtime_enter_nested_call",
        }
    }
}

impl std::fmt::Display for HostCallOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const HOST_CALL_ABI_VERSION: u16 = 1;

const fn default_host_call_abi_version() -> u16 {
    HOST_CALL_ABI_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryPayload {
    pub query: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncPaginatedQueryPayload {
    pub query: Value,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbGetPayload {
    pub table: String,
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncMutationPayload {
    pub mutation: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncActionPayload {
    pub action: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncHttpRoutePayload {
    pub request: Value,
    pub route: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbInsertPayload {
    pub table: String,
    pub fields: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbPatchPayload {
    pub table: String,
    pub id: String,
    pub patch: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbDeletePayload {
    pub table: String,
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryStartPayload {
    pub table: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryWithIndexPayload {
    pub builder_id: String,
    pub index_name: String,
    #[serde(default)]
    pub filters: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryFilterPayload {
    pub builder_id: String,
    #[serde(default)]
    pub filters: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryOrderPayload {
    pub builder_id: String,
    pub direction: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerRunAfterPayload {
    pub delay_ms: u64,
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerRunAtPayload {
    pub timestamp_ms: u64,
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerCancelPayload {
    pub job_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncServiceLookupPayload {
    pub service_name: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncFunctionCallPayload {
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncNestedCallPayload {
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryTerminalPayload {
    pub builder_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryTakePayload {
    pub builder_id: String,
    pub limit: usize,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryPaginatePayload {
    pub builder_id: String,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum HostCallPayload {
    HttpRoute(RuntimeAsyncHttpRoutePayload),
    CtxQuery(RuntimeAsyncQueryPayload),
    CtxPaginatedQuery(RuntimeAsyncPaginatedQueryPayload),
    CtxMutation(RuntimeAsyncMutationPayload),
    CtxAction(RuntimeAsyncActionPayload),
    CtxRunQuery(RuntimeAsyncFunctionCallPayload),
    CtxRunMutation(RuntimeAsyncFunctionCallPayload),
    CtxRunAction(RuntimeAsyncFunctionCallPayload),
    CtxDbGet(RuntimeAsyncDbGetPayload),
    CtxDbQueryStart(RuntimeSyncQueryStartPayload),
    CtxDbQueryWithIndex(RuntimeSyncQueryWithIndexPayload),
    CtxDbQueryFilter(RuntimeSyncQueryFilterPayload),
    CtxDbQueryOrder(RuntimeSyncQueryOrderPayload),
    CtxDbQueryCollect(RuntimeAsyncQueryTerminalPayload),
    CtxDbQueryTake(RuntimeAsyncQueryTakePayload),
    CtxDbQueryPaginate(RuntimeAsyncQueryPaginatePayload),
    CtxDbQueryFirst(RuntimeAsyncQueryTerminalPayload),
    CtxDbQueryUnique(RuntimeAsyncQueryTerminalPayload),
    CtxDbInsert(RuntimeAsyncDbInsertPayload),
    CtxDbPatch(RuntimeAsyncDbPatchPayload),
    CtxDbDelete(RuntimeAsyncDbDeletePayload),
    CtxSchedulerRunAfter(RuntimeAsyncSchedulerRunAfterPayload),
    CtxSchedulerRunAt(RuntimeAsyncSchedulerRunAtPayload),
    CtxSchedulerCancel(RuntimeAsyncSchedulerCancelPayload),
    CtxServiceLookup(RuntimeAsyncServiceLookupPayload),
    CtxRuntimeEnterNestedCall(RuntimeSyncNestedCallPayload),
}

impl HostCallPayload {
    pub const fn operation(&self) -> HostCallOperation {
        match self {
            Self::HttpRoute(_) => HostCallOperation::HttpRoute,
            Self::CtxQuery(_) => HostCallOperation::CtxQuery,
            Self::CtxPaginatedQuery(_) => HostCallOperation::CtxPaginatedQuery,
            Self::CtxMutation(_) => HostCallOperation::CtxMutation,
            Self::CtxAction(_) => HostCallOperation::CtxAction,
            Self::CtxRunQuery(_) => HostCallOperation::CtxRunQuery,
            Self::CtxRunMutation(_) => HostCallOperation::CtxRunMutation,
            Self::CtxRunAction(_) => HostCallOperation::CtxRunAction,
            Self::CtxDbGet(_) => HostCallOperation::CtxDbGet,
            Self::CtxDbQueryStart(_) => HostCallOperation::CtxDbQueryStart,
            Self::CtxDbQueryWithIndex(_) => HostCallOperation::CtxDbQueryWithIndex,
            Self::CtxDbQueryFilter(_) => HostCallOperation::CtxDbQueryFilter,
            Self::CtxDbQueryOrder(_) => HostCallOperation::CtxDbQueryOrder,
            Self::CtxDbQueryCollect(_) => HostCallOperation::CtxDbQueryCollect,
            Self::CtxDbQueryTake(_) => HostCallOperation::CtxDbQueryTake,
            Self::CtxDbQueryPaginate(_) => HostCallOperation::CtxDbQueryPaginate,
            Self::CtxDbQueryFirst(_) => HostCallOperation::CtxDbQueryFirst,
            Self::CtxDbQueryUnique(_) => HostCallOperation::CtxDbQueryUnique,
            Self::CtxDbInsert(_) => HostCallOperation::CtxDbInsert,
            Self::CtxDbPatch(_) => HostCallOperation::CtxDbPatch,
            Self::CtxDbDelete(_) => HostCallOperation::CtxDbDelete,
            Self::CtxSchedulerRunAfter(_) => HostCallOperation::CtxSchedulerRunAfter,
            Self::CtxSchedulerRunAt(_) => HostCallOperation::CtxSchedulerRunAt,
            Self::CtxSchedulerCancel(_) => HostCallOperation::CtxSchedulerCancel,
            Self::CtxServiceLookup(_) => HostCallOperation::CtxServiceLookup,
            Self::CtxRuntimeEnterNestedCall(_) => HostCallOperation::CtxRuntimeEnterNestedCall,
        }
    }

    pub fn from_parts(operation: HostCallOperation, payload: Value) -> Result<Self> {
        match operation {
            HostCallOperation::HttpRoute => Ok(Self::HttpRoute(serde_json::from_value(payload)?)),
            HostCallOperation::CtxQuery => Ok(Self::CtxQuery(serde_json::from_value(payload)?)),
            HostCallOperation::CtxPaginatedQuery => {
                Ok(Self::CtxPaginatedQuery(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxMutation => {
                Ok(Self::CtxMutation(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxAction => Ok(Self::CtxAction(serde_json::from_value(payload)?)),
            HostCallOperation::CtxRunQuery => {
                Ok(Self::CtxRunQuery(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxRunMutation => {
                Ok(Self::CtxRunMutation(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxRunAction => {
                Ok(Self::CtxRunAction(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbGet => Ok(Self::CtxDbGet(serde_json::from_value(payload)?)),
            HostCallOperation::CtxDbQueryStart => {
                Ok(Self::CtxDbQueryStart(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryWithIndex => {
                Ok(Self::CtxDbQueryWithIndex(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryFilter => {
                Ok(Self::CtxDbQueryFilter(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryOrder => {
                Ok(Self::CtxDbQueryOrder(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryCollect => {
                Ok(Self::CtxDbQueryCollect(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryTake => {
                Ok(Self::CtxDbQueryTake(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryPaginate => {
                Ok(Self::CtxDbQueryPaginate(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryFirst => {
                Ok(Self::CtxDbQueryFirst(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbQueryUnique => {
                Ok(Self::CtxDbQueryUnique(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbInsert => {
                Ok(Self::CtxDbInsert(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxDbPatch => Ok(Self::CtxDbPatch(serde_json::from_value(payload)?)),
            HostCallOperation::CtxDbDelete => {
                Ok(Self::CtxDbDelete(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxSchedulerRunAfter => {
                Ok(Self::CtxSchedulerRunAfter(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxSchedulerRunAt => {
                Ok(Self::CtxSchedulerRunAt(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxSchedulerCancel => {
                Ok(Self::CtxSchedulerCancel(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxServiceLookup => {
                Ok(Self::CtxServiceLookup(serde_json::from_value(payload)?))
            }
            HostCallOperation::CtxRuntimeEnterNestedCall => Ok(Self::CtxRuntimeEnterNestedCall(
                serde_json::from_value(payload)?,
            )),
        }
    }

    pub fn into_value(self) -> Result<Value> {
        serde_json::to_value(self).map_err(NeovexRuntimeError::from)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostCallEnvelope {
    pub abi_version: u16,
    pub payload: HostCallPayload,
}

impl HostCallEnvelope {
    pub const fn operation(&self) -> HostCallOperation {
        self.payload.operation()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostCallRequest {
    #[serde(default = "default_host_call_abi_version")]
    pub abi_version: u16,
    pub operation: HostCallOperation,
    #[serde(default)]
    pub payload: Value,
}

impl HostCallRequest {
    pub fn new(operation: HostCallOperation, payload: Value) -> Self {
        Self {
            abi_version: HOST_CALL_ABI_VERSION,
            operation,
            payload,
        }
    }
}

impl TryFrom<HostCallRequest> for HostCallEnvelope {
    type Error = NeovexRuntimeError;

    fn try_from(request: HostCallRequest) -> Result<Self> {
        if request.abi_version != HOST_CALL_ABI_VERSION {
            return Err(NeovexRuntimeError::Contract(format!(
                "unsupported host call ABI version {}",
                request.abi_version
            )));
        }
        let payload = HostCallPayload::from_parts(request.operation, request.payload)?;
        Ok(Self {
            abi_version: request.abi_version,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        HOST_CALL_ABI_VERSION, HostCallEnvelope, HostCallOperation, HostCallRequest,
        RuntimeAsyncDbGetPayload,
    };

    #[test]
    fn host_call_request_roundtrips_operation_strings_through_serde() {
        let request = HostCallRequest::new(
            HostCallOperation::CtxDbGet,
            json!({ "table": "messages", "id": "doc-1" }),
        );
        assert_eq!(
            serde_json::to_value(&request).expect("host call request should serialize"),
            json!({
                "abi_version": HOST_CALL_ABI_VERSION,
                "operation": "ctx_db_get",
                "payload": { "table": "messages", "id": "doc-1" },
            })
        );
    }

    #[test]
    fn host_call_request_rejects_unknown_operation_names_during_deserialization() {
        let error = serde_json::from_value::<HostCallRequest>(json!({
            "abi_version": HOST_CALL_ABI_VERSION,
            "operation": "ctx_unknown",
            "payload": {},
        }))
        .expect_err("unknown operation names should fail to deserialize");
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn host_call_envelope_rejects_unsupported_abi_versions() {
        let error = HostCallEnvelope::try_from(HostCallRequest {
            abi_version: HOST_CALL_ABI_VERSION + 1,
            operation: HostCallOperation::CtxDbGet,
            payload: json!({ "table": "messages", "id": "doc-1" }),
        })
        .expect_err("unknown host call ABI versions should fail");
        assert!(
            error
                .to_string()
                .contains("unsupported host call ABI version")
        );
    }

    #[test]
    fn host_call_envelope_rejects_operation_payload_mismatches() {
        let error = HostCallEnvelope::try_from(HostCallRequest::new(
            HostCallOperation::CtxDbGet,
            json!({ "mutation": { "table": "messages" } }),
        ))
        .expect_err("mismatched payloads should fail");
        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn host_call_envelope_accepts_matching_operation_payload_pairs() {
        let envelope = HostCallEnvelope::try_from(HostCallRequest::new(
            HostCallOperation::CtxDbGet,
            json!({ "table": "messages", "id": "doc-1" }),
        ))
        .expect("matching payload should parse");
        assert_eq!(envelope.operation(), HostCallOperation::CtxDbGet);
        assert_eq!(
            envelope.payload,
            super::HostCallPayload::CtxDbGet(RuntimeAsyncDbGetPayload {
                table: "messages".to_string(),
                id: "doc-1".to_string(),
                session_id: None,
            })
        );
    }
}

pub type HostBridgeFuture = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostCallCancellationCause {
    Explicit,
    Disconnect,
}

#[derive(Debug, Clone, Default)]
pub struct HostCallCancellation {
    inner: Arc<HostCallCancellationState>,
}

#[derive(Debug, Default)]
struct HostCallCancellationState {
    canceled: AtomicBool,
    cause: AtomicU8,
    notify: Notify,
}

impl HostCallCancellation {
    pub fn cancel(&self) {
        self.cancel_with_cause(HostCallCancellationCause::Explicit);
    }

    pub fn cancel_due_to_disconnect(&self) {
        self.cancel_with_cause(HostCallCancellationCause::Disconnect);
    }

    pub fn cause(&self) -> Option<HostCallCancellationCause> {
        HostCallCancellationCause::from_u8(self.inner.cause.load(Ordering::SeqCst))
    }

    fn cancel_with_cause(&self, cause: HostCallCancellationCause) {
        let _ =
            self.inner
                .cause
                .compare_exchange(0, cause.as_u8(), Ordering::SeqCst, Ordering::SeqCst);
        self.inner.canceled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.canceled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

impl HostCallCancellationCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Disconnect => "disconnect",
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Explicit => 1,
            Self::Disconnect => 2,
        }
    }

    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Explicit),
            2 => Some(Self::Disconnect),
            _ => None,
        }
    }
}

pub trait HostBridge: Send + Sync + 'static {
    fn call(&self, request: HostCallRequest) -> Result<Value>;

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(NeovexRuntimeError::Cancelled);
        }
        self.call(request)
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let result = self.call_cancellable(request, &cancellation);
        Box::pin(async move { result })
    }
}
