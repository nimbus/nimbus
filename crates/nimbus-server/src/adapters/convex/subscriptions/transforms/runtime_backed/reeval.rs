use std::sync::{Arc, RwLock};

use nimbus_core::{Error, Page};
use nimbus_runtime::{
    HostCallCancellation, InvocationAuth, InvocationKind, InvocationRequest, InvocationServices,
};
use serde_json::Value;

use crate::adapters::convex::ConvexRegistry;
use crate::adapters::convex::execution::{
    RuntimeInvocationContext, invoke_named_convex_function_with_trace_async_cancellable,
};
use crate::adapters::convex::subscriptions::next_runtime_subscription_server_request_id;
use nimbus_bridge::read_tracking::{RuntimeReadSet, commit_intersects_runtime_read_set};
use nimbus_convex::subscriptions::{
    ConvexSubscriptionEvent, ConvexSubscriptionTransform, ConvexSubscriptionTransforms,
    update_runtime_transform_read_set,
};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{TenantIsolationContext, TenantIsolationMode};

pub(in crate::adapters::convex::subscriptions) struct RuntimeTransformContext<'a> {
    pub(in crate::adapters::convex::subscriptions) service: &'a Arc<nimbus_engine::Service>,
    pub(in crate::adapters::convex::subscriptions) registry: &'a Arc<ConvexRegistry>,
    pub(in crate::adapters::convex::subscriptions) runtime_service_registry:
        &'a Arc<dyn RuntimeServiceRegistry>,
    pub(in crate::adapters::convex::subscriptions) tenant_context: &'a TenantIsolationContext,
    pub(in crate::adapters::convex::subscriptions) transforms:
        &'a RwLock<ConvexSubscriptionTransforms>,
    pub(in crate::adapters::convex::subscriptions) runtime_cancellation: &'a HostCallCancellation,
    pub(in crate::adapters::convex::subscriptions) tenant_isolation_mode: TenantIsolationMode,
    pub(in crate::adapters::convex::subscriptions) event: ConvexSubscriptionEvent<'a>,
}

impl<'a> RuntimeTransformContext<'a> {
    fn runtime_invocation_context(&self) -> RuntimeInvocationContext<'_> {
        RuntimeInvocationContext::new(
            self.service,
            self.registry,
            self.runtime_service_registry,
            self.tenant_context.reauthorize_application(
                nimbus_core::PrincipalContext::anonymous(),
                "convex_subscription_runtime",
            ),
            self.tenant_isolation_mode,
        )
    }
}

pub(super) struct RuntimeNamedQueryTransform {
    pub(super) name: String,
    pub(super) args: Value,
    pub(super) auth: Option<InvocationAuth>,
    pub(super) services: InvocationServices,
    pub(super) read_set: Option<RuntimeReadSet>,
    pub(super) last_value: Option<Arc<Value>>,
}

pub(super) struct RuntimeNamedPaginatedQueryTransform {
    pub(super) name: String,
    pub(super) args: Value,
    pub(super) page_size: usize,
    pub(super) cursor: Option<String>,
    pub(super) auth: Option<InvocationAuth>,
    pub(super) services: InvocationServices,
    pub(super) read_set: Option<RuntimeReadSet>,
    pub(super) last_value: Option<Arc<Value>>,
}

pub(in crate::adapters::convex::subscriptions) async fn apply_runtime_named_query_transform(
    context: RuntimeTransformContext<'_>,
    transform: RuntimeNamedQueryTransform,
) -> Result<Option<Value>, String> {
    if should_skip_runtime_transform(&context, transform.read_set.as_ref()) {
        return Ok(None);
    }

    let result = match invoke_named_convex_function_with_trace_async_cancellable(
        &context.runtime_invocation_context(),
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: transform.name.clone(),
            args: transform.args.clone(),
            page_size: None,
            cursor: None,
            auth: transform.auth.clone(),
            services: transform.services.clone(),
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
    let suppress_duplicate = transform
        .last_value
        .as_ref()
        .is_some_and(|last_value| last_value.as_ref() == &value);
    let value = Arc::new(value);
    update_runtime_transform_read_set(
        context.transforms,
        context.event.subscription_id,
        ConvexSubscriptionTransform::RuntimeNamedQuery {
            name: transform.name,
            args: transform.args,
            auth: transform.auth,
            services: transform.services,
            read_set: Some(new_read_set),
            last_value: Some(value.clone()),
        },
    );
    Ok((!suppress_duplicate).then(|| value.as_ref().clone()))
}

pub(in crate::adapters::convex::subscriptions) async fn apply_runtime_named_paginated_query_transform(
    context: RuntimeTransformContext<'_>,
    transform: RuntimeNamedPaginatedQueryTransform,
) -> Result<Option<Value>, String> {
    if should_skip_runtime_transform(&context, transform.read_set.as_ref()) {
        return Ok(None);
    }

    let result = match invoke_named_convex_function_with_trace_async_cancellable(
        &context.runtime_invocation_context(),
        InvocationRequest {
            kind: InvocationKind::PaginatedQuery,
            function_name: transform.name.clone(),
            args: transform.args.clone(),
            page_size: Some(transform.page_size),
            cursor: transform.cursor.clone(),
            auth: transform.auth.clone(),
            services: transform.services.clone(),
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
    let suppress_duplicate = transform
        .last_value
        .as_ref()
        .is_some_and(|last_value| last_value.as_ref() == &value);
    let value = Arc::new(value);
    update_runtime_transform_read_set(
        context.transforms,
        context.event.subscription_id,
        ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
            name: transform.name,
            args: transform.args,
            page_size: transform.page_size,
            cursor: transform.cursor,
            auth: transform.auth,
            services: transform.services,
            read_set: Some(new_read_set),
            last_value: Some(value.clone()),
        },
    );
    Ok((!suppress_duplicate).then(|| value.as_ref().clone()))
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
            context.tenant_context.tenant_id(),
            commit,
            read_set,
            context.event.deleted_documents,
        )
    {
        return true;
    }

    false
}
