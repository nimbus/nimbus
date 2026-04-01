use super::*;

pub(super) async fn registry_and_auth(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    expectation: &'static str,
) -> Result<(Arc<ConvexRegistry>, Option<InvocationAuth>), AppError> {
    let registry = state.convex_registry.clone().expect(expectation);
    let auth = registry.verify_authorization_header(headers).await?;
    record_authenticated_usage(state, auth.as_ref()).await;
    Ok((registry, auth))
}
