use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryPayload {
    pub(in crate::adapters::convex) query: ConvexExecutableQuery,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimePaginatedQueryPayload {
    pub(in crate::adapters::convex) query: Query,
    pub(in crate::adapters::convex) page_size: usize,
    #[serde(default)]
    pub(in crate::adapters::convex) cursor: Option<String>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeMutationPayload {
    pub(in crate::adapters::convex) mutation: ConvexExecutableMutation,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeActionPayload {
    pub(in crate::adapters::convex) action: ConvexExecutableAction,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeFunctionCallPayload {
    pub(in crate::adapters::convex) name: String,
    #[serde(default)]
    pub(in crate::adapters::convex) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(in crate::adapters::convex) args: Value,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
    #[serde(default)]
    pub(in crate::adapters::convex) auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeServiceLookupPayload {
    pub(in crate::adapters::convex) service_name: String,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}
