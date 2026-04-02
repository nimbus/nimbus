use super::invoke::invoke_named_convex_function_with_trace_async_cancellable;
use super::*;

#[allow(clippy::too_many_arguments)]
pub(in crate::adapters::convex) async fn bootstrap_runtime_named_subscription_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    auth: Option<InvocationAuth>,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<ConvexRuntimeSubscriptionSetup, Error> {
    let kind = if page_size.is_some() {
        InvocationKind::PaginatedQuery
    } else {
        InvocationKind::Query
    };
    let (value, read_set) = invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        InvocationRequest {
            kind: kind.clone(),
            function_name: name.to_string(),
            args: args.clone(),
            page_size,
            cursor: cursor.clone(),
            auth: auth.clone(),
        },
        cancellation,
        server_request_id,
    )
    .await?;
    let base_queries = synthesize_runtime_subscription_base_queries(&read_set)?;
    match kind {
        InvocationKind::Query => {
            let last_value = std::sync::Arc::new(value.clone());
            Ok(ConvexRuntimeSubscriptionSetup {
                initial_value: value,
                base_queries,
                transform: ConvexSubscriptionTransform::RuntimeNamedQuery {
                    name: name.to_string(),
                    args: args.clone(),
                    auth,
                    read_set: Some(read_set),
                    last_value: Some(last_value),
                },
            })
        }
        InvocationKind::PaginatedQuery => {
            let page: neovex_core::Page = serde_json::from_value(value)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            let initial_value = Value::Array(page.data);
            let last_value = std::sync::Arc::new(initial_value.clone());
            Ok(ConvexRuntimeSubscriptionSetup {
                initial_value,
                base_queries,
                transform: ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                    name: name.to_string(),
                    args: args.clone(),
                    page_size: page_size
                        .expect("paginated runtime bootstrap should carry page size"),
                    cursor,
                    auth,
                    read_set: Some(read_set),
                    last_value: Some(last_value),
                },
            })
        }
        InvocationKind::Mutation | InvocationKind::Action => Err(Error::InvalidInput(
            "runtime subscription bootstrap only supports queries".to_string(),
        )),
    }
}
