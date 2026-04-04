use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::error::{NeovexRuntimeError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HostCallOperation {
    #[serde(rename = "convex.http_route")]
    HttpRoute,
    #[serde(rename = "convex.ctx.query")]
    CtxQuery,
    #[serde(rename = "convex.ctx.paginated_query")]
    CtxPaginatedQuery,
    #[serde(rename = "convex.ctx.mutation")]
    CtxMutation,
    #[serde(rename = "convex.ctx.action")]
    CtxAction,
    #[serde(rename = "convex.ctx.run_query")]
    CtxRunQuery,
    #[serde(rename = "convex.ctx.run_mutation")]
    CtxRunMutation,
    #[serde(rename = "convex.ctx.run_action")]
    CtxRunAction,
    #[serde(rename = "convex.ctx.db.get")]
    CtxDbGet,
    #[serde(rename = "convex.ctx.db.query.start")]
    CtxDbQueryStart,
    #[serde(rename = "convex.ctx.db.query.with_index")]
    CtxDbQueryWithIndex,
    #[serde(rename = "convex.ctx.db.query.filter")]
    CtxDbQueryFilter,
    #[serde(rename = "convex.ctx.db.query.order")]
    CtxDbQueryOrder,
    #[serde(rename = "convex.ctx.db.query.collect")]
    CtxDbQueryCollect,
    #[serde(rename = "convex.ctx.db.query.take")]
    CtxDbQueryTake,
    #[serde(rename = "convex.ctx.db.query.paginate")]
    CtxDbQueryPaginate,
    #[serde(rename = "convex.ctx.db.query.first")]
    CtxDbQueryFirst,
    #[serde(rename = "convex.ctx.db.query.unique")]
    CtxDbQueryUnique,
    #[serde(rename = "convex.ctx.db.insert")]
    CtxDbInsert,
    #[serde(rename = "convex.ctx.db.patch")]
    CtxDbPatch,
    #[serde(rename = "convex.ctx.db.delete")]
    CtxDbDelete,
    #[serde(rename = "convex.ctx.scheduler.run_after")]
    CtxSchedulerRunAfter,
    #[serde(rename = "convex.ctx.scheduler.run_at")]
    CtxSchedulerRunAt,
    #[serde(rename = "convex.ctx.scheduler.cancel")]
    CtxSchedulerCancel,
    #[serde(rename = "convex.ctx.runtime.enter_nested_call")]
    CtxRuntimeEnterNestedCall,
}

impl HostCallOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HttpRoute => "convex.http_route",
            Self::CtxQuery => "convex.ctx.query",
            Self::CtxPaginatedQuery => "convex.ctx.paginated_query",
            Self::CtxMutation => "convex.ctx.mutation",
            Self::CtxAction => "convex.ctx.action",
            Self::CtxRunQuery => "convex.ctx.run_query",
            Self::CtxRunMutation => "convex.ctx.run_mutation",
            Self::CtxRunAction => "convex.ctx.run_action",
            Self::CtxDbGet => "convex.ctx.db.get",
            Self::CtxDbQueryStart => "convex.ctx.db.query.start",
            Self::CtxDbQueryWithIndex => "convex.ctx.db.query.with_index",
            Self::CtxDbQueryFilter => "convex.ctx.db.query.filter",
            Self::CtxDbQueryOrder => "convex.ctx.db.query.order",
            Self::CtxDbQueryCollect => "convex.ctx.db.query.collect",
            Self::CtxDbQueryTake => "convex.ctx.db.query.take",
            Self::CtxDbQueryPaginate => "convex.ctx.db.query.paginate",
            Self::CtxDbQueryFirst => "convex.ctx.db.query.first",
            Self::CtxDbQueryUnique => "convex.ctx.db.query.unique",
            Self::CtxDbInsert => "convex.ctx.db.insert",
            Self::CtxDbPatch => "convex.ctx.db.patch",
            Self::CtxDbDelete => "convex.ctx.db.delete",
            Self::CtxSchedulerRunAfter => "convex.ctx.scheduler.run_after",
            Self::CtxSchedulerRunAt => "convex.ctx.scheduler.run_at",
            Self::CtxSchedulerCancel => "convex.ctx.scheduler.cancel",
            Self::CtxRuntimeEnterNestedCall => "convex.ctx.runtime.enter_nested_call",
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
                "operation": "convex.ctx.db.get",
                "payload": { "id": "doc-1" },
            })
        );
    }

    #[test]
    fn host_call_request_rejects_unknown_operation_names_during_deserialization() {
        let error = serde_json::from_value::<HostCallRequest>(json!({
            "operation": "convex.ctx.unknown",
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
