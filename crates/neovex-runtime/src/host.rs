use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::error::{NeovexRuntimeError, Result};

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
            Self::CtxRuntimeEnterNestedCall => "ctx_runtime_enter_nested_call",
        }
    }
}

impl std::fmt::Display for HostCallOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostCallRequest {
    pub operation: HostCallOperation,
    #[serde(default)]
    pub payload: Value,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{HostCallOperation, HostCallRequest};

    #[test]
    fn host_call_request_roundtrips_operation_strings_through_serde() {
        let request = HostCallRequest {
            operation: HostCallOperation::CtxDbGet,
            payload: json!({ "id": "doc-1" }),
        };
        assert_eq!(
            serde_json::to_value(&request).expect("host call request should serialize"),
            json!({
                "operation": "ctx_db_get",
                "payload": { "id": "doc-1" },
            })
        );
    }

    #[test]
    fn host_call_request_rejects_unknown_operation_names_during_deserialization() {
        let error = serde_json::from_value::<HostCallRequest>(json!({
            "operation": "ctx_unknown",
            "payload": {},
        }))
        .expect_err("unknown operation names should fail to deserialize");
        assert!(error.to_string().contains("unknown variant"));
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
