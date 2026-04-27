use neovex_runtime::{HOST_CALL_ABI_VERSION, HostCallOperation, HostCallRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const fn default_convex_host_call_abi_version() -> u16 {
    HOST_CALL_ABI_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(in crate::adapters::convex) enum ConvexHostCallOperation {
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
    #[serde(rename = "convex.ctx.service.lookup")]
    CtxServiceLookup,
    #[serde(rename = "convex.ctx.runtime.enter_nested_call")]
    CtxRuntimeEnterNestedCall,
    #[serde(rename = "adapter.runtime_extension_call")]
    RuntimeExtensionCall,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::adapters::convex) enum ConvexHostCallFamily {
    Function,
    QueryBuilder,
    QueryRead,
    Document,
    Scheduler,
    AdapterExtension,
}

impl ConvexHostCallOperation {
    #[cfg(test)]
    pub(in crate::adapters::convex) const fn family(self) -> ConvexHostCallFamily {
        match self {
            Self::HttpRoute
            | Self::CtxQuery
            | Self::CtxPaginatedQuery
            | Self::CtxMutation
            | Self::CtxAction
            | Self::CtxRunQuery
            | Self::CtxRunMutation
            | Self::CtxRunAction
            | Self::CtxServiceLookup
            | Self::CtxRuntimeEnterNestedCall => ConvexHostCallFamily::Function,
            Self::CtxDbQueryStart
            | Self::CtxDbQueryWithIndex
            | Self::CtxDbQueryFilter
            | Self::CtxDbQueryOrder => ConvexHostCallFamily::QueryBuilder,
            Self::CtxDbQueryCollect
            | Self::CtxDbQueryTake
            | Self::CtxDbQueryPaginate
            | Self::CtxDbQueryFirst
            | Self::CtxDbQueryUnique => ConvexHostCallFamily::QueryRead,
            Self::CtxDbGet | Self::CtxDbInsert | Self::CtxDbPatch | Self::CtxDbDelete => {
                ConvexHostCallFamily::Document
            }
            Self::CtxSchedulerRunAfter | Self::CtxSchedulerRunAt | Self::CtxSchedulerCancel => {
                ConvexHostCallFamily::Scheduler
            }
            Self::RuntimeExtensionCall => ConvexHostCallFamily::AdapterExtension,
        }
    }

    pub(in crate::adapters::convex) const fn as_str(self) -> &'static str {
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
            Self::CtxServiceLookup => "convex.ctx.service.lookup",
            Self::CtxRuntimeEnterNestedCall => "convex.ctx.runtime.enter_nested_call",
            Self::RuntimeExtensionCall => "adapter.runtime_extension_call",
        }
    }
}

impl From<HostCallOperation> for ConvexHostCallOperation {
    fn from(operation: HostCallOperation) -> Self {
        match operation {
            HostCallOperation::HttpRoute => Self::HttpRoute,
            HostCallOperation::CtxQuery => Self::CtxQuery,
            HostCallOperation::CtxPaginatedQuery => Self::CtxPaginatedQuery,
            HostCallOperation::CtxMutation => Self::CtxMutation,
            HostCallOperation::CtxAction => Self::CtxAction,
            HostCallOperation::CtxRunQuery => Self::CtxRunQuery,
            HostCallOperation::CtxRunMutation => Self::CtxRunMutation,
            HostCallOperation::CtxRunAction => Self::CtxRunAction,
            HostCallOperation::DocumentGet => Self::CtxDbGet,
            HostCallOperation::QueryBuilderStart => Self::CtxDbQueryStart,
            HostCallOperation::QueryBuilderWithIndex => Self::CtxDbQueryWithIndex,
            HostCallOperation::QueryBuilderFilter => Self::CtxDbQueryFilter,
            HostCallOperation::QueryBuilderOrder => Self::CtxDbQueryOrder,
            HostCallOperation::QueryReadCollect => Self::CtxDbQueryCollect,
            HostCallOperation::QueryReadTake => Self::CtxDbQueryTake,
            HostCallOperation::QueryReadPaginate => Self::CtxDbQueryPaginate,
            HostCallOperation::QueryReadFirst => Self::CtxDbQueryFirst,
            HostCallOperation::QueryReadUnique => Self::CtxDbQueryUnique,
            HostCallOperation::DocumentInsert => Self::CtxDbInsert,
            HostCallOperation::DocumentPatch => Self::CtxDbPatch,
            HostCallOperation::DocumentDelete => Self::CtxDbDelete,
            HostCallOperation::CtxSchedulerRunAfter => Self::CtxSchedulerRunAfter,
            HostCallOperation::CtxSchedulerRunAt => Self::CtxSchedulerRunAt,
            HostCallOperation::CtxSchedulerCancel => Self::CtxSchedulerCancel,
            HostCallOperation::CtxServiceLookup => Self::CtxServiceLookup,
            HostCallOperation::CtxRuntimeEnterNestedCall => Self::CtxRuntimeEnterNestedCall,
            HostCallOperation::RuntimeExtensionCall => Self::RuntimeExtensionCall,
        }
    }
}

impl From<ConvexHostCallOperation> for HostCallOperation {
    fn from(operation: ConvexHostCallOperation) -> Self {
        match operation {
            ConvexHostCallOperation::HttpRoute => Self::HttpRoute,
            ConvexHostCallOperation::CtxQuery => Self::CtxQuery,
            ConvexHostCallOperation::CtxPaginatedQuery => Self::CtxPaginatedQuery,
            ConvexHostCallOperation::CtxMutation => Self::CtxMutation,
            ConvexHostCallOperation::CtxAction => Self::CtxAction,
            ConvexHostCallOperation::CtxRunQuery => Self::CtxRunQuery,
            ConvexHostCallOperation::CtxRunMutation => Self::CtxRunMutation,
            ConvexHostCallOperation::CtxRunAction => Self::CtxRunAction,
            ConvexHostCallOperation::CtxDbGet => Self::DocumentGet,
            ConvexHostCallOperation::CtxDbQueryStart => Self::QueryBuilderStart,
            ConvexHostCallOperation::CtxDbQueryWithIndex => Self::QueryBuilderWithIndex,
            ConvexHostCallOperation::CtxDbQueryFilter => Self::QueryBuilderFilter,
            ConvexHostCallOperation::CtxDbQueryOrder => Self::QueryBuilderOrder,
            ConvexHostCallOperation::CtxDbQueryCollect => Self::QueryReadCollect,
            ConvexHostCallOperation::CtxDbQueryTake => Self::QueryReadTake,
            ConvexHostCallOperation::CtxDbQueryPaginate => Self::QueryReadPaginate,
            ConvexHostCallOperation::CtxDbQueryFirst => Self::QueryReadFirst,
            ConvexHostCallOperation::CtxDbQueryUnique => Self::QueryReadUnique,
            ConvexHostCallOperation::CtxDbInsert => Self::DocumentInsert,
            ConvexHostCallOperation::CtxDbPatch => Self::DocumentPatch,
            ConvexHostCallOperation::CtxDbDelete => Self::DocumentDelete,
            ConvexHostCallOperation::CtxSchedulerRunAfter => Self::CtxSchedulerRunAfter,
            ConvexHostCallOperation::CtxSchedulerRunAt => Self::CtxSchedulerRunAt,
            ConvexHostCallOperation::CtxSchedulerCancel => Self::CtxSchedulerCancel,
            ConvexHostCallOperation::CtxServiceLookup => Self::CtxServiceLookup,
            ConvexHostCallOperation::CtxRuntimeEnterNestedCall => Self::CtxRuntimeEnterNestedCall,
            ConvexHostCallOperation::RuntimeExtensionCall => Self::RuntimeExtensionCall,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(in crate::adapters::convex) struct ConvexHostCallRequest {
    #[serde(default = "default_convex_host_call_abi_version")]
    pub(in crate::adapters::convex) abi_version: u16,
    pub(in crate::adapters::convex) operation: ConvexHostCallOperation,
    #[serde(default)]
    pub(in crate::adapters::convex) payload: Value,
}

impl From<HostCallRequest> for ConvexHostCallRequest {
    fn from(request: HostCallRequest) -> Self {
        Self {
            abi_version: request.abi_version,
            operation: request.operation.into(),
            payload: request.payload,
        }
    }
}

impl From<ConvexHostCallRequest> for HostCallRequest {
    fn from(request: ConvexHostCallRequest) -> Self {
        Self {
            abi_version: request.abi_version,
            operation: request.operation.into(),
            payload: request.payload,
        }
    }
}

pub(in crate::adapters::convex) fn convex_host_operation_name(
    operation: HostCallOperation,
) -> &'static str {
    ConvexHostCallOperation::from(operation).as_str()
}
