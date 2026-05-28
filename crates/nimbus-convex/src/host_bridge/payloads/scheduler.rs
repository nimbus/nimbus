use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeSchedulerRunAfterPayload {
    pub delay_ms: u64,
    pub name: String,
    #[serde(default)]
    pub visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub args: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeSchedulerRunAtPayload {
    pub timestamp_ms: u64,
    pub name: String,
    #[serde(default)]
    pub visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    pub args: Value,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeSchedulerCancelPayload {
    pub job_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}
