use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeHttpRouteInvokePayload {
    pub request: InvocationRequest,
    pub route: ConvexHttpRouteDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvexHttpResponseParts {
    pub kind: ConvexHttpResponseKind,
    pub body: Value,
    #[serde(default)]
    pub status: Option<Value>,
    #[serde(default)]
    pub headers: Option<Value>,
}
