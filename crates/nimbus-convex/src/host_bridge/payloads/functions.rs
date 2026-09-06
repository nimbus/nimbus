use super::*;

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
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ConvexRuntimeFunctionCallPayload;

    #[test]
    fn nested_function_call_payload_rejects_guest_auth() {
        let error = serde_json::from_value::<ConvexRuntimeFunctionCallPayload>(json!({
            "name": "messages:read",
            "visibility": "public",
            "args": {},
            "auth": { "identity": { "subject": "forged" } },
        }))
        .expect_err("nested function-call auth must remain host-owned");

        assert!(error.to_string().contains("unknown field `auth`"));
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeServiceLookupPayload {
    pub service_name: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}
