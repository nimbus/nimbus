use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeSchedulerRunAfterPayload {
    pub(in crate::adapters::convex) delay_ms: u64,
    pub(in crate::adapters::convex) name: String,
    #[serde(default)]
    pub(in crate::adapters::convex) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(in crate::adapters::convex) args: Value,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeSchedulerRunAtPayload {
    pub(in crate::adapters::convex) timestamp_ms: u64,
    pub(in crate::adapters::convex) name: String,
    #[serde(default)]
    pub(in crate::adapters::convex) visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub(in crate::adapters::convex) args: Value,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeSchedulerCancelPayload {
    pub(in crate::adapters::convex) job_id: String,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}
