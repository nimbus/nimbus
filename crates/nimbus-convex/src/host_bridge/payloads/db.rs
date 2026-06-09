use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeDbGetPayload {
    pub table: TableName,
    pub id: DocumentId,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeDbInsertPayload {
    pub table: TableName,
    pub fields: serde_json::Map<String, Value>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeDbPatchPayload {
    pub table: TableName,
    pub id: DocumentId,
    pub patch: serde_json::Map<String, Value>,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvexRuntimeDbDeletePayload {
    pub table: TableName,
    pub id: DocumentId,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}
