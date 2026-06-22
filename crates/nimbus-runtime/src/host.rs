use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::error::{NimbusRuntimeError, Result};
use crate::execution_plan::RuntimeEffectClass;
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
    DocumentGet,
    QueryBuilderStart,
    QueryBuilderWithIndex,
    QueryBuilderFilter,
    QueryBuilderOrder,
    QueryReadCollect,
    QueryReadTake,
    QueryReadPaginate,
    QueryReadFirst,
    QueryReadUnique,
    DocumentInsert,
    DocumentPatch,
    DocumentDelete,
    CtxSchedulerRunAfter,
    CtxSchedulerRunAt,
    CtxSchedulerCancel,
    CtxServiceLookup,
    CtxRuntimeEnterNestedCall,
    RuntimeExtensionCall,
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
            Self::DocumentGet => "document_get",
            Self::QueryBuilderStart => "query_builder_start",
            Self::QueryBuilderWithIndex => "query_builder_with_index",
            Self::QueryBuilderFilter => "query_builder_filter",
            Self::QueryBuilderOrder => "query_builder_order",
            Self::QueryReadCollect => "query_read_collect",
            Self::QueryReadTake => "query_read_take",
            Self::QueryReadPaginate => "query_read_paginate",
            Self::QueryReadFirst => "query_read_first",
            Self::QueryReadUnique => "query_read_unique",
            Self::DocumentInsert => "document_insert",
            Self::DocumentPatch => "document_patch",
            Self::DocumentDelete => "document_delete",
            Self::CtxSchedulerRunAfter => "ctx_scheduler_run_after",
            Self::CtxSchedulerRunAt => "ctx_scheduler_run_at",
            Self::CtxSchedulerCancel => "ctx_scheduler_cancel",
            Self::CtxServiceLookup => "ctx_service_lookup",
            Self::CtxRuntimeEnterNestedCall => "ctx_runtime_enter_nested_call",
            Self::RuntimeExtensionCall => "runtime_extension_call",
        }
    }

    pub(crate) const fn runtime_effect_class(self) -> RuntimeEffectClass {
        match self {
            Self::HttpRoute => RuntimeEffectClass::HttpRoute,
            Self::CtxQuery
            | Self::CtxPaginatedQuery
            | Self::DocumentGet
            | Self::QueryBuilderStart
            | Self::QueryBuilderWithIndex
            | Self::QueryBuilderFilter
            | Self::QueryBuilderOrder
            | Self::QueryReadCollect
            | Self::QueryReadTake
            | Self::QueryReadPaginate
            | Self::QueryReadFirst
            | Self::QueryReadUnique => RuntimeEffectClass::ObservableRead,
            Self::CtxMutation
            | Self::DocumentInsert
            | Self::DocumentPatch
            | Self::DocumentDelete => RuntimeEffectClass::Write,
            Self::CtxAction | Self::CtxServiceLookup => RuntimeEffectClass::ServiceExternal,
            Self::CtxRunQuery => RuntimeEffectClass::ObservableRead,
            Self::CtxRunMutation | Self::CtxRunAction | Self::CtxRuntimeEnterNestedCall => {
                RuntimeEffectClass::NestedRuntime
            }
            Self::CtxSchedulerRunAfter | Self::CtxSchedulerRunAt | Self::CtxSchedulerCancel => {
                RuntimeEffectClass::Scheduler
            }
            Self::RuntimeExtensionCall => RuntimeEffectClass::Extension,
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
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncPaginatedQueryPayload {
    pub query: Value,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbGetPayload {
    pub table: String,
    pub id: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncMutationPayload {
    pub mutation: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncActionPayload {
    pub action: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
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
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbPatchPayload {
    pub table: String,
    pub id: String,
    pub patch: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncDbDeletePayload {
    pub table: String,
    pub id: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncExtensionPayload {
    pub namespace: String,
    pub operation: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryStartPayload {
    pub table: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryWithIndexPayload {
    pub builder_id: String,
    pub index_name: String,
    #[serde(default)]
    pub filters: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryFilterPayload {
    pub builder_id: String,
    #[serde(default)]
    pub filters: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncQueryOrderPayload {
    pub builder_id: String,
    pub direction: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerRunAfterPayload {
    pub delay_ms: u64,
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerRunAtPayload {
    pub timestamp_ms: u64,
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncSchedulerCancelPayload {
    pub job_id: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncServiceLookupPayload {
    pub service_name: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncFunctionCallPayload {
    pub name: String,
    pub visibility: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSyncNestedCallPayload {
    pub name: String,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryTerminalPayload {
    pub builder_id: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryTakePayload {
    pub builder_id: String,
    pub limit: usize,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAsyncQueryPaginatePayload {
    pub builder_id: String,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
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
    DocumentGet(RuntimeAsyncDbGetPayload),
    QueryBuilderStart(RuntimeSyncQueryStartPayload),
    QueryBuilderWithIndex(RuntimeSyncQueryWithIndexPayload),
    QueryBuilderFilter(RuntimeSyncQueryFilterPayload),
    QueryBuilderOrder(RuntimeSyncQueryOrderPayload),
    QueryReadCollect(RuntimeAsyncQueryTerminalPayload),
    QueryReadTake(RuntimeAsyncQueryTakePayload),
    QueryReadPaginate(RuntimeAsyncQueryPaginatePayload),
    QueryReadFirst(RuntimeAsyncQueryTerminalPayload),
    QueryReadUnique(RuntimeAsyncQueryTerminalPayload),
    DocumentInsert(RuntimeAsyncDbInsertPayload),
    DocumentPatch(RuntimeAsyncDbPatchPayload),
    DocumentDelete(RuntimeAsyncDbDeletePayload),
    CtxSchedulerRunAfter(RuntimeAsyncSchedulerRunAfterPayload),
    CtxSchedulerRunAt(RuntimeAsyncSchedulerRunAtPayload),
    CtxSchedulerCancel(RuntimeAsyncSchedulerCancelPayload),
    CtxServiceLookup(RuntimeAsyncServiceLookupPayload),
    CtxRuntimeEnterNestedCall(RuntimeSyncNestedCallPayload),
    RuntimeExtensionCall(RuntimeAsyncExtensionPayload),
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
            Self::DocumentGet(_) => HostCallOperation::DocumentGet,
            Self::QueryBuilderStart(_) => HostCallOperation::QueryBuilderStart,
            Self::QueryBuilderWithIndex(_) => HostCallOperation::QueryBuilderWithIndex,
            Self::QueryBuilderFilter(_) => HostCallOperation::QueryBuilderFilter,
            Self::QueryBuilderOrder(_) => HostCallOperation::QueryBuilderOrder,
            Self::QueryReadCollect(_) => HostCallOperation::QueryReadCollect,
            Self::QueryReadTake(_) => HostCallOperation::QueryReadTake,
            Self::QueryReadPaginate(_) => HostCallOperation::QueryReadPaginate,
            Self::QueryReadFirst(_) => HostCallOperation::QueryReadFirst,
            Self::QueryReadUnique(_) => HostCallOperation::QueryReadUnique,
            Self::DocumentInsert(_) => HostCallOperation::DocumentInsert,
            Self::DocumentPatch(_) => HostCallOperation::DocumentPatch,
            Self::DocumentDelete(_) => HostCallOperation::DocumentDelete,
            Self::CtxSchedulerRunAfter(_) => HostCallOperation::CtxSchedulerRunAfter,
            Self::CtxSchedulerRunAt(_) => HostCallOperation::CtxSchedulerRunAt,
            Self::CtxSchedulerCancel(_) => HostCallOperation::CtxSchedulerCancel,
            Self::CtxServiceLookup(_) => HostCallOperation::CtxServiceLookup,
            Self::CtxRuntimeEnterNestedCall(_) => HostCallOperation::CtxRuntimeEnterNestedCall,
            Self::RuntimeExtensionCall(_) => HostCallOperation::RuntimeExtensionCall,
        }
    }

    pub fn host_call_session_id(&self) -> Option<&str> {
        match self {
            Self::HttpRoute(_) | Self::RuntimeExtensionCall(_) => None,
            Self::CtxQuery(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxPaginatedQuery(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxMutation(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxAction(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxRunQuery(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxRunMutation(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxRunAction(payload) => payload.host_call_session_id.as_deref(),
            Self::DocumentGet(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryBuilderStart(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryBuilderWithIndex(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryBuilderFilter(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryBuilderOrder(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryReadCollect(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryReadTake(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryReadPaginate(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryReadFirst(payload) => payload.host_call_session_id.as_deref(),
            Self::QueryReadUnique(payload) => payload.host_call_session_id.as_deref(),
            Self::DocumentInsert(payload) => payload.host_call_session_id.as_deref(),
            Self::DocumentPatch(payload) => payload.host_call_session_id.as_deref(),
            Self::DocumentDelete(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxSchedulerRunAfter(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxSchedulerRunAt(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxSchedulerCancel(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxServiceLookup(payload) => payload.host_call_session_id.as_deref(),
            Self::CtxRuntimeEnterNestedCall(payload) => payload.host_call_session_id.as_deref(),
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
            HostCallOperation::DocumentGet => {
                Ok(Self::DocumentGet(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryBuilderStart => {
                Ok(Self::QueryBuilderStart(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryBuilderWithIndex => Ok(Self::QueryBuilderWithIndex(
                serde_json::from_value(payload)?,
            )),
            HostCallOperation::QueryBuilderFilter => {
                Ok(Self::QueryBuilderFilter(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryBuilderOrder => {
                Ok(Self::QueryBuilderOrder(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryReadCollect => {
                Ok(Self::QueryReadCollect(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryReadTake => {
                Ok(Self::QueryReadTake(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryReadPaginate => {
                Ok(Self::QueryReadPaginate(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryReadFirst => {
                Ok(Self::QueryReadFirst(serde_json::from_value(payload)?))
            }
            HostCallOperation::QueryReadUnique => {
                Ok(Self::QueryReadUnique(serde_json::from_value(payload)?))
            }
            HostCallOperation::DocumentInsert => {
                Ok(Self::DocumentInsert(serde_json::from_value(payload)?))
            }
            HostCallOperation::DocumentPatch => {
                Ok(Self::DocumentPatch(serde_json::from_value(payload)?))
            }
            HostCallOperation::DocumentDelete => {
                Ok(Self::DocumentDelete(serde_json::from_value(payload)?))
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
            HostCallOperation::RuntimeExtensionCall => {
                Ok(Self::RuntimeExtensionCall(serde_json::from_value(payload)?))
            }
        }
    }

    pub fn into_value(self) -> Result<Value> {
        serde_json::to_value(self).map_err(NimbusRuntimeError::from)
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
    type Error = NimbusRuntimeError;

    fn try_from(request: HostCallRequest) -> Result<Self> {
        if request.abi_version != HOST_CALL_ABI_VERSION {
            return Err(NimbusRuntimeError::Contract(format!(
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use crate::execution_plan::RuntimeEffectClass;

    use super::{
        HOST_CALL_ABI_VERSION, HostCallCancellation, HostCallCancellationCause, HostCallEnvelope,
        HostCallOperation, HostCallRequest, RuntimeAsyncDbGetPayload,
    };

    #[test]
    fn host_call_request_roundtrips_operation_strings_through_serde() {
        let request = HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!({ "table": "messages", "id": "doc-1" }),
        );
        assert_eq!(
            serde_json::to_value(&request).expect("host call request should serialize"),
            json!({
                "abi_version": HOST_CALL_ABI_VERSION,
                "operation": "document_get",
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
    fn host_call_operations_have_exhaustive_runtime_effect_classes() {
        for (operation, effect_class) in [
            (HostCallOperation::HttpRoute, RuntimeEffectClass::HttpRoute),
            (
                HostCallOperation::CtxQuery,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::CtxPaginatedQuery,
                RuntimeEffectClass::ObservableRead,
            ),
            (HostCallOperation::CtxMutation, RuntimeEffectClass::Write),
            (
                HostCallOperation::CtxAction,
                RuntimeEffectClass::ServiceExternal,
            ),
            (
                HostCallOperation::CtxRunQuery,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::CtxRunMutation,
                RuntimeEffectClass::NestedRuntime,
            ),
            (
                HostCallOperation::CtxRunAction,
                RuntimeEffectClass::NestedRuntime,
            ),
            (
                HostCallOperation::DocumentGet,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryBuilderStart,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryBuilderWithIndex,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryBuilderFilter,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryBuilderOrder,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryReadCollect,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryReadTake,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryReadPaginate,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryReadFirst,
                RuntimeEffectClass::ObservableRead,
            ),
            (
                HostCallOperation::QueryReadUnique,
                RuntimeEffectClass::ObservableRead,
            ),
            (HostCallOperation::DocumentInsert, RuntimeEffectClass::Write),
            (HostCallOperation::DocumentPatch, RuntimeEffectClass::Write),
            (HostCallOperation::DocumentDelete, RuntimeEffectClass::Write),
            (
                HostCallOperation::CtxSchedulerRunAfter,
                RuntimeEffectClass::Scheduler,
            ),
            (
                HostCallOperation::CtxSchedulerRunAt,
                RuntimeEffectClass::Scheduler,
            ),
            (
                HostCallOperation::CtxSchedulerCancel,
                RuntimeEffectClass::Scheduler,
            ),
            (
                HostCallOperation::CtxServiceLookup,
                RuntimeEffectClass::ServiceExternal,
            ),
            (
                HostCallOperation::CtxRuntimeEnterNestedCall,
                RuntimeEffectClass::NestedRuntime,
            ),
            (
                HostCallOperation::RuntimeExtensionCall,
                RuntimeEffectClass::Extension,
            ),
        ] {
            assert_eq!(
                operation.runtime_effect_class(),
                effect_class,
                "{operation} should classify as {effect_class:?}"
            );
        }
    }

    #[test]
    fn host_call_envelope_rejects_unsupported_abi_versions() {
        let error = HostCallEnvelope::try_from(HostCallRequest {
            abi_version: HOST_CALL_ABI_VERSION + 1,
            operation: HostCallOperation::DocumentGet,
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
            HostCallOperation::DocumentGet,
            json!({ "mutation": { "table": "messages" } }),
        ))
        .expect_err("mismatched payloads should fail");
        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn host_call_envelope_accepts_matching_operation_payload_pairs() {
        let envelope = HostCallEnvelope::try_from(HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!({ "table": "messages", "id": "doc-1" }),
        ))
        .expect("matching payload should parse");
        assert_eq!(envelope.operation(), HostCallOperation::DocumentGet);
        assert_eq!(
            envelope.payload,
            super::HostCallPayload::DocumentGet(RuntimeAsyncDbGetPayload {
                table: "messages".to_string(),
                id: "doc-1".to_string(),
                host_call_session_id: None,
            })
        );
    }

    #[test]
    fn host_call_cancellation_notifies_listeners_once_and_immediate_after_cancel() {
        let cancellation = HostCallCancellation::default();
        let fired = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let fired = fired.clone();
            cancellation.notify_on_cancel(move || {
                fired.fetch_add(1, Ordering::SeqCst);
            });
        }

        cancellation.cancel();
        cancellation.cancel_due_to_disconnect();
        assert_eq!(fired.load(Ordering::SeqCst), 2);
        assert_eq!(
            cancellation.cause(),
            Some(HostCallCancellationCause::Explicit)
        );

        let fired_after_cancel = fired.clone();
        cancellation.notify_on_cancel(move || {
            fired_after_cancel.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(fired.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn host_call_cancellation_waiter_completes_after_cancel() {
        let cancellation = HostCallCancellation::default();
        let waiter_cancellation = cancellation.clone();
        let waiter = tokio::spawn(async move {
            waiter_cancellation.cancelled().await;
            waiter_cancellation.cause()
        });

        tokio::task::yield_now().await;
        cancellation.cancel_due_to_disconnect();

        let cause = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("cancellation waiter should complete after notification")
            .expect("waiter task should not panic");
        assert_eq!(cause, Some(HostCallCancellationCause::Disconnect));
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

type HostCallCancellationListener = Box<dyn FnOnce() + Send + 'static>;

#[derive(Default)]
struct HostCallCancellationState {
    canceled: AtomicBool,
    cause: AtomicU8,
    notify: Notify,
    listeners: Mutex<Vec<HostCallCancellationListener>>,
}

impl std::fmt::Debug for HostCallCancellationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostCallCancellationState")
            .field("canceled", &self.canceled.load(Ordering::SeqCst))
            .field(
                "cause",
                &HostCallCancellationCause::from_u8(self.cause.load(Ordering::SeqCst)),
            )
            .finish_non_exhaustive()
    }
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
        let first_cancel = !self.inner.canceled.swap(true, Ordering::SeqCst);
        let listeners = if first_cancel {
            std::mem::take(
                &mut *self
                    .inner
                    .listeners
                    .lock()
                    .expect("host cancellation listener lock should not be poisoned"),
            )
        } else {
            Vec::new()
        };
        self.inner.notify.notify_waiters();
        for listener in listeners {
            listener();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.canceled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        let notified = self.inner.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }

    pub(crate) fn notify_on_cancel<F>(&self, listener: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let mut listener = Some(Box::new(listener) as HostCallCancellationListener);
        {
            let mut listeners = self
                .inner
                .listeners
                .lock()
                .expect("host cancellation listener lock should not be poisoned");
            if !self.is_cancelled() {
                listeners.push(listener.take().expect("listener should be available"));
                return;
            }
        }
        if let Some(listener) = listener {
            listener();
        }
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
            return Err(NimbusRuntimeError::Cancelled);
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
