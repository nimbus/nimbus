use super::*;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexQueryRequest {
    Named(ConvexNamedRequest),
    Raw { query: Query },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexPaginatedQueryRequest {
    Named(ConvexNamedPaginatedQueryRequest),
    Raw { query: PaginatedQuery },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexMutationRequest {
    Named(ConvexNamedRequest),
    Raw { mutation: Mutation },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexActionRequest {
    Named(ConvexNamedRequest),
    Raw { action: ConvexAction },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexScheduleAfterRequest {
    Named(ConvexNamedScheduleAfterRequest),
    Raw {
        mutation: Mutation,
        run_after_ms: u64,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexScheduleAtRequest {
    Named(ConvexNamedScheduleAtRequest),
    Raw { mutation: Mutation, run_at_ms: u64 },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedPaginatedQueryRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedScheduleAfterRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub run_after_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedScheduleAtRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub run_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ConvexAction {
    Query { query: Query },
    PaginatedQuery { query: PaginatedQuery },
    Mutation { mutation: Mutation },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum ConvexExecutableQuery {
    Query(Query),
    Read(ConvexReadCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ConvexReadCommand {
    Get { table: TableName, id: DocumentId },
    First { query: Query },
    Unique { query: Query },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum ConvexExecutableMutation {
    Mutation(Mutation),
    Query(ConvexExecutableQuery),
    Scheduled(ConvexScheduledCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum ConvexExecutableAction {
    Action(ConvexAction),
    Scheduled(ConvexScheduledCommand),
    Call(ConvexFunctionCallCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ConvexFunctionCallCommand {
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
pub(super) enum ConvexScheduledCommand {
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
