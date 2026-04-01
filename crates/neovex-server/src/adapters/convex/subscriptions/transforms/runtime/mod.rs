use std::sync::{Arc, RwLock};

use neovex_core::TenantId;
use neovex_runtime::HostCallCancellation;
use serde_json::Value;

use super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::convex::execution::ConvexSubscriptionEvent;

mod builtins;
mod reeval;
mod selection;

pub(in crate::adapters::convex::subscriptions) async fn apply_subscription_transform(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    runtime_cancellation: &HostCallCancellation,
    event: ConvexSubscriptionEvent<'_>,
    data: Vec<Value>,
) -> Result<Option<Value>, String> {
    let transform = selection::resolve_subscription_transform(transforms, &event);
    let context = reeval::RuntimeTransformContext::new(
        service,
        registry,
        tenant_id,
        transforms,
        runtime_cancellation,
        event,
    );

    match transform {
        ConvexSubscriptionTransform::Identity
        | ConvexSubscriptionTransform::Get { .. }
        | ConvexSubscriptionTransform::First
        | ConvexSubscriptionTransform::Unique => builtins::apply_builtin_transform(transform, data),
        ConvexSubscriptionTransform::RuntimeNamedQuery {
            name,
            args,
            auth,
            read_set,
        } => {
            reeval::apply_runtime_named_query_transform(
                context,
                reeval::RuntimeNamedQueryTransform {
                    name,
                    args,
                    auth,
                    read_set,
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
            read_set,
        } => {
            reeval::apply_runtime_named_paginated_query_transform(
                context,
                reeval::RuntimeNamedPaginatedQueryTransform {
                    name,
                    args,
                    page_size,
                    cursor,
                    auth,
                    read_set,
                },
            )
            .await
        }
    }
}
