use std::sync::{Arc, RwLock};

use neovex_core::{Error, Page, TenantId};
use neovex_runtime::{HostCallCancellation, InvocationAuth, InvocationKind, InvocationRequest};
use serde_json::Value;

use super::super::super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};
use super::super::state::update_runtime_transform_read_set;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::convex::execution::{
    ConvexSubscriptionEvent, invoke_named_convex_function_with_trace_async_cancellable,
};
use crate::adapters::convex::subscriptions::next_runtime_subscription_server_request_id;
use crate::runtime::read_tracking::{RuntimeReadSet, commit_intersects_runtime_read_set};

pub(super) struct RuntimeTransformContext<'a> {
    service: &'a Arc<neovex_engine::Service>,
    registry: &'a Arc<ConvexRegistry>,
    tenant_id: &'a TenantId,
    transforms: &'a RwLock<ConvexSubscriptionTransforms>,
    runtime_cancellation: &'a HostCallCancellation,
    event: ConvexSubscriptionEvent<'a>,
}

impl<'a> RuntimeTransformContext<'a> {
    pub(super) fn new(
        service: &'a Arc<neovex_engine::Service>,
        registry: &'a Arc<ConvexRegistry>,
        tenant_id: &'a TenantId,
        transforms: &'a RwLock<ConvexSubscriptionTransforms>,
        runtime_cancellation: &'a HostCallCancellation,
        event: ConvexSubscriptionEvent<'a>,
    ) -> Self {
        Self {
            service,
            registry,
            tenant_id,
            transforms,
            runtime_cancellation,
            event,
        }
    }
}

pub(super) struct RuntimeNamedQueryTransform {
    pub(super) name: String,
    pub(super) args: Value,
    pub(super) auth: Option<InvocationAuth>,
    pub(super) read_set: Option<RuntimeReadSet>,
}

pub(super) struct RuntimeNamedPaginatedQueryTransform {
    pub(super) name: String,
    pub(super) args: Value,
    pub(super) page_size: usize,
    pub(super) cursor: Option<String>,
    pub(super) auth: Option<InvocationAuth>,
    pub(super) read_set: Option<RuntimeReadSet>,
}

pub(in crate::adapters::convex::subscriptions) async fn apply_runtime_named_query_transform(
    context: RuntimeTransformContext<'_>,
    transform: RuntimeNamedQueryTransform,
) -> Result<Option<Value>, String> {
    if should_skip_runtime_transform(&context, transform.read_set.as_ref()) {
        return Ok(None);
    }

    let result = match invoke_named_convex_function_with_trace_async_cancellable(
        context.service,
        context.registry,
        context.tenant_id,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: transform.name.clone(),
            args: transform.args.clone(),
            page_size: None,
            cursor: None,
            auth: transform.auth.clone(),
        },
        context.runtime_cancellation.clone(),
        Some(next_runtime_subscription_server_request_id(
            "convex-ws-subscription-reeval",
        )),
    )
    .await
    {
        Ok(result) => result,
        Err(_error) if context.runtime_cancellation.is_cancelled() => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let (value, new_read_set) = result;
    update_runtime_transform_read_set(
        context.transforms,
        context.event.subscription_id,
        ConvexSubscriptionTransform::RuntimeNamedQuery {
            name: transform.name,
            args: transform.args,
            auth: transform.auth,
            read_set: Some(new_read_set),
        },
    );
    Ok(Some(value))
}

pub(in crate::adapters::convex::subscriptions) async fn apply_runtime_named_paginated_query_transform(
    context: RuntimeTransformContext<'_>,
    transform: RuntimeNamedPaginatedQueryTransform,
) -> Result<Option<Value>, String> {
    if should_skip_runtime_transform(&context, transform.read_set.as_ref()) {
        return Ok(None);
    }

    let result = match invoke_named_convex_function_with_trace_async_cancellable(
        context.service,
        context.registry,
        context.tenant_id,
        InvocationRequest {
            kind: InvocationKind::PaginatedQuery,
            function_name: transform.name.clone(),
            args: transform.args.clone(),
            page_size: Some(transform.page_size),
            cursor: transform.cursor.clone(),
            auth: transform.auth.clone(),
        },
        context.runtime_cancellation.clone(),
        Some(next_runtime_subscription_server_request_id(
            "convex-ws-subscription-reeval",
        )),
    )
    .await
    .and_then(|(value, read_set)| {
        let page: Page = serde_json::from_value(value)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok((Value::Array(page.data), read_set))
    }) {
        Ok(result) => result,
        Err(_error) if context.runtime_cancellation.is_cancelled() => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let (value, new_read_set) = result;
    update_runtime_transform_read_set(
        context.transforms,
        context.event.subscription_id,
        ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
            name: transform.name,
            args: transform.args,
            page_size: transform.page_size,
            cursor: transform.cursor,
            auth: transform.auth,
            read_set: Some(new_read_set),
        },
    );
    Ok(Some(value))
}

fn should_skip_runtime_transform(
    context: &RuntimeTransformContext<'_>,
    read_set: Option<&RuntimeReadSet>,
) -> bool {
    if context.runtime_cancellation.is_cancelled() {
        return true;
    }

    if let Some(commit) = context.event.commit
        && let Some(read_set) = read_set
        && !commit_intersects_runtime_read_set(
            context.service,
            context.tenant_id,
            commit,
            read_set,
            context.event.deleted_documents,
        )
    {
        return true;
    }

    false
}
