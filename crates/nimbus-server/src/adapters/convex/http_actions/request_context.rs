use super::*;

pub(super) fn build_http_request_context(
    method: &Method,
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
    query: HashMap<String, String>,
    body: Bytes,
) -> ConvexHttpRequestContext {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let query_suffix = original_uri
        .0
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!("{scheme}://{host}{request_path}{query_suffix}");
    let text = String::from_utf8_lossy(&body).into_owned();
    let normalized_headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect();

    ConvexHttpRequestContext {
        method: method.as_str().to_string(),
        url,
        pathname: request_path.to_string(),
        query,
        headers: normalized_headers,
        body_bytes: body.to_vec(),
        body_text: text,
    }
}
