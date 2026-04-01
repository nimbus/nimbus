use super::*;

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route_root(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        ConvexHttpRouteRequest {
            request_path: "/".to_string(),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, path)): AxumPath<(String, String)>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        ConvexHttpRouteRequest {
            request_path: normalize_http_request_path(&path),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}
