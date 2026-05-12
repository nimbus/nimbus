use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::adapters::convex) struct ConvexHttpRequestContext {
    pub(in crate::adapters::convex) method: String,
    pub(in crate::adapters::convex) url: String,
    pub(in crate::adapters::convex) pathname: String,
    pub(in crate::adapters::convex) query: HashMap<String, String>,
    pub(in crate::adapters::convex) headers: HashMap<String, String>,
    pub(in crate::adapters::convex) body_bytes: Vec<u8>,
    pub(in crate::adapters::convex) body_text: String,
}

pub(in crate::adapters::convex) struct ConvexHttpRouteRequest {
    pub(in crate::adapters::convex) request_path: String,
    pub(in crate::adapters::convex) method: Method,
    pub(in crate::adapters::convex) headers: HeaderMap,
    pub(in crate::adapters::convex) original_uri: OriginalUri,
    pub(in crate::adapters::convex) query: HashMap<String, String>,
    pub(in crate::adapters::convex) body: Bytes,
}

pub(in crate::adapters::convex) struct ConvexSubscriptionEvent<'a> {
    pub(in crate::adapters::convex) subscription_id: u64,
    pub(in crate::adapters::convex) request_id: Option<&'a str>,
    pub(in crate::adapters::convex) commit: Option<&'a CommitEntry>,
    pub(in crate::adapters::convex) deleted_documents: &'a [nimbus_core::Document],
}
