use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeHttpRouteInvokePayload {
    pub(in crate::adapters::convex) request: InvocationRequest,
    pub(in crate::adapters::convex) route: ConvexHttpRouteDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::adapters::convex) struct ConvexHttpResponseParts {
    pub(in crate::adapters::convex) kind: ConvexHttpResponseKind,
    pub(in crate::adapters::convex) body: Value,
    #[serde(default)]
    pub(in crate::adapters::convex) status: Option<Value>,
    #[serde(default)]
    pub(in crate::adapters::convex) headers: Option<Value>,
}
