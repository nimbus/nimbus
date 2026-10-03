use super::*;

pub(in crate::adapters::convex) struct ConvexHttpRouteRequest {
    pub(in crate::adapters::convex) request_path: String,
    pub(in crate::adapters::convex) method: Method,
    pub(in crate::adapters::convex) headers: HeaderMap,
    pub(in crate::adapters::convex) original_uri: OriginalUri,
    pub(in crate::adapters::convex) query: HashMap<String, String>,
    pub(in crate::adapters::convex) body: Bytes,
}
