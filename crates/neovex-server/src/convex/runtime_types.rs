use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeHttpRouteInvokePayload {
    pub(super) request: InvocationRequest,
    pub(super) route: ConvexHttpRouteDefinition,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryPayload {
    pub(super) query: ConvexExecutableQuery,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimePaginatedQueryPayload {
    pub(super) query: Query,
    pub(super) page_size: usize,
    #[serde(default)]
    pub(super) cursor: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeMutationPayload {
    pub(super) mutation: ConvexExecutableMutation,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeActionPayload {
    pub(super) action: ConvexExecutableAction,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeFunctionCallPayload {
    pub(super) name: String,
    #[serde(default)]
    pub(super) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default)]
    pub(super) auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeDbGetPayload {
    pub(super) table: TableName,
    pub(super) id: DocumentId,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeDbInsertPayload {
    pub(super) table: TableName,
    pub(super) fields: serde_json::Map<String, Value>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeDbPatchPayload {
    pub(super) table: TableName,
    pub(super) id: DocumentId,
    pub(super) patch: serde_json::Map<String, Value>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeDbDeletePayload {
    pub(super) table: TableName,
    pub(super) id: DocumentId,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryStartPayload {
    pub(super) table: TableName,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryWithIndexPayload {
    pub(super) builder_id: String,
    pub(super) index_name: String,
    #[serde(default)]
    pub(super) filters: Vec<Filter>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryFilterPayload {
    pub(super) builder_id: String,
    #[serde(default)]
    pub(super) filters: Vec<Filter>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryOrderPayload {
    pub(super) builder_id: String,
    pub(super) direction: OrderDirection,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryTerminalPayload {
    pub(super) builder_id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryTakePayload {
    pub(super) builder_id: String,
    pub(super) limit: usize,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeQueryPaginatePayload {
    pub(super) builder_id: String,
    pub(super) page_size: usize,
    #[serde(default)]
    pub(super) cursor: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeSchedulerRunAfterPayload {
    pub(super) delay_ms: u64,
    pub(super) name: String,
    #[serde(default)]
    pub(super) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeSchedulerRunAtPayload {
    pub(super) timestamp_ms: u64,
    pub(super) name: String,
    #[serde(default)]
    pub(super) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexRuntimeSchedulerCancelPayload {
    pub(super) job_id: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum ConvexRuntimeResponseEnvelope {
    Ok { value: Value },
    Error { error: ConvexRuntimeEncodedError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ConvexRuntimeEncodedError {
    Cancelled,
    TenantNotFound { tenant_id: String },
    DocumentNotFound { document_id: String },
    ScheduledJobNotFound { job_id: String },
    AlreadyExists { message: String },
    InvalidInput { message: String },
    SchemaValidation { message: String },
    SchemaNotFound { table: String },
    Storage { message: String },
    Serialization { message: String },
    Internal { message: String },
}

#[derive(Clone)]
pub(super) struct ConvexRuntimeBridge {
    pub(super) service: Arc<neovex_engine::Service>,
    pub(super) registry: Arc<ConvexRegistry>,
    pub(super) tenant_id: TenantId,
    pub(super) server_request_id: Option<String>,
    pub(super) session_id: String,
    pub(super) max_nested_runtime_invocations: usize,
    pub(super) remaining_nested_runtime_invocations: Arc<AtomicUsize>,
    pub(super) query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
    pub(super) read_set: Arc<Mutex<ConvexRuntimeReadSet>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpResponseParts {
    pub(super) kind: ConvexHttpResponseKind,
    pub(super) body: Value,
    #[serde(default)]
    pub(super) status: Option<Value>,
    #[serde(default)]
    pub(super) headers: Option<Value>,
}

#[derive(Debug, Default)]
pub(super) struct ConvexRuntimeQueryBuilders {
    pub(super) next_builder_id: u64,
    pub(super) builders: HashMap<String, ConvexRuntimeQueryBuilderState>,
}

#[derive(Debug, Clone)]
pub(super) struct ConvexRuntimeQueryBuilderState {
    pub(super) table: TableName,
    pub(super) filters: Vec<Filter>,
    pub(super) order: Option<OrderBy>,
    pub(super) order_field_hint: Option<String>,
    pub(super) index_name: Option<String>,
}
