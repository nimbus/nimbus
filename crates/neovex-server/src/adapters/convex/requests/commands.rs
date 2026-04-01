use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(in crate::adapters::convex) enum ConvexExecutableQuery {
    Query(Query),
    Read(ConvexReadCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexReadCommand {
    Get { table: TableName, id: DocumentId },
    First { query: Query },
    Unique { query: Query },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(in crate::adapters::convex) enum ConvexExecutableMutation {
    Mutation(Mutation),
    Query(ConvexExecutableQuery),
    Scheduled(ConvexScheduledCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(in crate::adapters::convex) enum ConvexExecutableAction {
    Action(ConvexAction),
    Scheduled(ConvexScheduledCommand),
    Call(ConvexFunctionCallCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexFunctionCallCommand {
    #[serde(rename = "call_query")]
    Query {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "call_mutation")]
    Mutation {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "call_action")]
    Action {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::adapters::convex) enum ConvexScheduledCommand {
    #[serde(rename = "schedule_run_after")]
    RunAfter {
        delay_ms: u64,
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "schedule_run_at")]
    RunAt {
        timestamp_ms: u64,
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "schedule_cancel")]
    Cancel { job_id: String },
}
