use serde_json::Value;

use nimbus_convex::subscriptions::{
    ConvexSubscriptionTransform, apply_builtin_transform, resolve_subscription_transform,
};

mod reeval;

pub(in crate::adapters::convex::subscriptions) use reeval::RuntimeTransformContext;

pub(in crate::adapters::convex::subscriptions) async fn apply_subscription_transform(
    context: RuntimeTransformContext<'_>,
    data: Vec<Value>,
) -> Result<Option<Value>, String> {
    let transform = resolve_subscription_transform(context.transforms, &context.event);

    match transform {
        ConvexSubscriptionTransform::Identity
        | ConvexSubscriptionTransform::Get { .. }
        | ConvexSubscriptionTransform::First
        | ConvexSubscriptionTransform::Unique => apply_builtin_transform(transform, data),
        ConvexSubscriptionTransform::RuntimeNamedQuery {
            name,
            args,
            auth,
            services,
            read_set,
            last_value,
        } => {
            reeval::apply_runtime_named_query_transform(
                context,
                reeval::RuntimeNamedQueryTransform {
                    name,
                    args,
                    auth,
                    services,
                    read_set,
                    last_value,
                },
            )
            .await
        }
        ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
            name,
            args,
            page_size,
            cursor,
            auth,
            services,
            read_set,
            last_value,
        } => {
            reeval::apply_runtime_named_paginated_query_transform(
                context,
                reeval::RuntimeNamedPaginatedQueryTransform {
                    name,
                    args,
                    page_size,
                    cursor,
                    auth,
                    services,
                    read_set,
                    last_value,
                },
            )
            .await
        }
    }
}
