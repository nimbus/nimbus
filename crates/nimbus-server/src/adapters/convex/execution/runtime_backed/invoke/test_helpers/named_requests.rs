use std::sync::Arc;

use nimbus_core::{Error, TenantId};
use nimbus_runtime::{InvocationKind, InvocationRequest};
use serde_json::Value;

use crate::adapters::convex::{
    ConvexNamedPaginatedQueryRequest, ConvexNamedRequest, ConvexRegistry,
};

use super::super::super::sync_ops::{
    execute_named_action_request_direct, execute_named_mutation_request_direct,
    execute_named_paginated_query_request_direct, execute_named_query_request_direct,
};
use super::runtime_invocation::invoke_named_convex_function;

pub(in crate::adapters::convex) fn execute_named_query_request(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Query,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        );
    }

    execute_named_query_request_direct(service, registry, tenant_id, &request.name, &request.args)
}

pub(in crate::adapters::convex) fn execute_named_paginated_query_request(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedPaginatedQueryRequest,
) -> Result<nimbus_core::Page, Error> {
    if registry.runtime_bundle().is_some() {
        let value = invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::PaginatedQuery,
                function_name: request.name,
                args: request.args,
                page_size: Some(request.page_size),
                cursor: request.cursor,
                auth: None,
                services: Default::default(),
            },
        )?;
        return serde_json::from_value(value)
            .map_err(|error| Error::Serialization(error.to_string()));
    }

    execute_named_paginated_query_request_direct(
        service,
        registry,
        tenant_id,
        &request.name,
        &request.args,
        request.page_size,
        request.cursor,
    )
}

pub(in crate::adapters::convex) fn execute_named_mutation_request(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        );
    }

    execute_named_mutation_request_direct(
        service,
        registry,
        tenant_id,
        &request.name,
        &request.args,
    )
}

pub(in crate::adapters::convex) fn execute_named_action_request(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Action,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        );
    }

    execute_named_action_request_direct(service, registry, tenant_id, &request.name, &request.args)
}
