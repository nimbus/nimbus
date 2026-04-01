use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeDbGetPayload {
    pub(in crate::adapters::convex) table: TableName,
    pub(in crate::adapters::convex) id: DocumentId,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeDbInsertPayload {
    pub(in crate::adapters::convex) table: TableName,
    pub(in crate::adapters::convex) fields: serde_json::Map<String, Value>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeDbPatchPayload {
    pub(in crate::adapters::convex) table: TableName,
    pub(in crate::adapters::convex) id: DocumentId,
    pub(in crate::adapters::convex) patch: serde_json::Map<String, Value>,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex) struct ConvexRuntimeDbDeletePayload {
    pub(in crate::adapters::convex) table: TableName,
    pub(in crate::adapters::convex) id: DocumentId,
    #[serde(default)]
    pub(in crate::adapters::convex) session_id: Option<String>,
}
