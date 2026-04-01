use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexManifest {
    pub(super) functions: Vec<ConvexFunctionDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexHttpRouteManifest {
    pub(super) routes: Vec<ConvexHttpRouteDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexFunctionDefinition {
    pub(super) name: String,
    pub(super) kind: ConvexFunctionKind,
    #[serde(default)]
    pub(super) visibility: ConvexFunctionVisibility,
    #[serde(default)]
    pub(super) schedulable: bool,
    #[serde(default)]
    pub(super) runtime_handler: Option<String>,
    pub(super) plan: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexHttpRouteDefinition {
    #[serde(default)]
    pub(super) name: Option<String>,
    pub(super) method: ConvexHttpMethod,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) path_prefix: Option<String>,
    pub(super) plan: ConvexHttpActionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpActionPlan {
    #[serde(default)]
    pub(super) operation: Option<Value>,
    pub(super) response: ConvexHttpResponseTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpResponseTemplate {
    pub(super) kind: ConvexHttpResponseKind,
    pub(super) body: Value,
    #[serde(default)]
    pub(super) status: Option<Value>,
    #[serde(default)]
    pub(super) headers: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexHttpResponseKind {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(super) enum ConvexHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexFunctionKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexFunctionVisibility {
    #[default]
    Public,
    Internal,
}
