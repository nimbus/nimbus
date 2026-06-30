use super::*;
use nimbus_core::InvocationAuth;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryPayload {
    pub query: ConvexExecutableQuery,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimePaginatedQueryPayload {
    pub query: Query,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeMutationPayload {
    pub mutation: ConvexExecutableMutation,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeActionPayload {
    pub action: ConvexExecutableAction,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeFunctionCallPayload {
    pub name: String,
    #[serde(default)]
    pub visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub args: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
    #[serde(default)]
    pub auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeServiceLookupPayload {
    pub service_name: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}
