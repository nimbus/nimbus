use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::convex) struct ConvexHttpRequestContext {
    pub(in crate::convex) method: String,
    pub(in crate::convex) url: String,
    pub(in crate::convex) pathname: String,
    pub(in crate::convex) query: HashMap<String, String>,
    pub(in crate::convex) headers: HashMap<String, String>,
    pub(in crate::convex) body_bytes: Vec<u8>,
    pub(in crate::convex) body_text: String,
}

pub(in crate::convex) struct ConvexHttpRouteRequest {
    pub(in crate::convex) request_path: String,
    pub(in crate::convex) method: Method,
    pub(in crate::convex) headers: HeaderMap,
    pub(in crate::convex) original_uri: OriginalUri,
    pub(in crate::convex) query: HashMap<String, String>,
    pub(in crate::convex) body: Bytes,
}

pub(in crate::convex) struct ConvexSubscriptionEvent<'a> {
    pub(in crate::convex) subscription_id: u64,
    pub(in crate::convex) request_id: Option<&'a str>,
    pub(in crate::convex) commit: Option<&'a CommitEntry>,
    pub(in crate::convex) deleted_documents: &'a [neovex_core::Document],
}
