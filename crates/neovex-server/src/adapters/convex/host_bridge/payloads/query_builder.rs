use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryStartPayload {
    pub(in crate::adapters::convex) table: TableName,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryWithIndexPayload {
    pub(in crate::adapters::convex) builder_id: String,
    pub(in crate::adapters::convex) index_name: String,
    #[serde(default)]
    pub(in crate::adapters::convex) filters: Vec<Filter>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryFilterPayload {
    pub(in crate::adapters::convex) builder_id: String,
    #[serde(default)]
    pub(in crate::adapters::convex) filters: Vec<Filter>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryOrderPayload {
    pub(in crate::adapters::convex) builder_id: String,
    pub(in crate::adapters::convex) direction: OrderDirection,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryTerminalPayload {
    pub(in crate::adapters::convex) builder_id: String,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryTakePayload {
    pub(in crate::adapters::convex) builder_id: String,
    pub(in crate::adapters::convex) limit: usize,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeQueryPaginatePayload {
    pub(in crate::adapters::convex) builder_id: String,
    pub(in crate::adapters::convex) page_size: usize,
    #[serde(default)]
    pub(in crate::adapters::convex) cursor: Option<String>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}
