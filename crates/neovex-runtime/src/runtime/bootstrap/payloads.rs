use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) use crate::host::{
    RuntimeAsyncActionPayload, RuntimeAsyncDbDeletePayload, RuntimeAsyncDbGetPayload,
    RuntimeAsyncDbInsertPayload, RuntimeAsyncDbPatchPayload, RuntimeAsyncFunctionCallPayload,
    RuntimeAsyncHttpRoutePayload, RuntimeAsyncMutationPayload, RuntimeAsyncPaginatedQueryPayload,
    RuntimeAsyncQueryPaginatePayload, RuntimeAsyncQueryPayload, RuntimeAsyncQueryTakePayload,
    RuntimeAsyncQueryTerminalPayload, RuntimeAsyncSchedulerCancelPayload,
    RuntimeAsyncSchedulerRunAfterPayload, RuntimeAsyncSchedulerRunAtPayload,
    RuntimeAsyncServiceLookupPayload, RuntimeSyncNestedCallPayload, RuntimeSyncQueryFilterPayload,
    RuntimeSyncQueryOrderPayload, RuntimeSyncQueryStartPayload, RuntimeSyncQueryWithIndexPayload,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum RuntimeHostCallEnvelope {
    Ok { value: Value },
    Error { error: Value },
}
