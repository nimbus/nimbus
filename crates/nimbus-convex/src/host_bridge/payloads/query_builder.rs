use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryStartPayload {
    pub table: TableName,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryWithIndexPayload {
    pub builder_id: String,
    pub index_name: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryFilterPayload {
    pub builder_id: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryOrderPayload {
    pub builder_id: String,
    pub direction: OrderDirection,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryTerminalPayload {
    pub builder_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryTakePayload {
    pub builder_id: String,
    pub limit: usize,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeQueryPaginatePayload {
    pub builder_id: String,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}
