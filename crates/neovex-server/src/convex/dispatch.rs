#![cfg_attr(test, allow(dead_code))]

use super::*;

#[cfg(test)]
pub(super) fn execute_named_query_request(
    service: &Arc<neovex_engine::Service>,
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
            },
        );
    }

    execute_named_query_request_direct(service, registry, tenant_id, &request.name, &request.args)
}

#[cfg(test)]
pub(super) fn execute_named_paginated_query_request(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedPaginatedQueryRequest,
) -> Result<neovex_core::Page, Error> {
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

#[cfg(test)]
pub(super) fn execute_named_mutation_request(
    service: &Arc<neovex_engine::Service>,
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

#[cfg(test)]
pub(super) fn execute_named_action_request(
    service: &Arc<neovex_engine::Service>,
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
            },
        );
    }

    execute_named_action_request_direct(service, registry, tenant_id, &request.name, &request.args)
}

#[cfg(test)]
fn invoke_named_convex_function(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace(service, registry, tenant_id, request)
        .map(|(value, _)| value)
}

pub(super) async fn invoke_named_convex_function_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        cancellation,
    )
    .await
    .map(|(value, _)| value)
}

#[cfg(test)]
fn invoke_named_convex_function_with_trace(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
    )
}

#[cfg(test)]
fn invoke_named_convex_function_with_trace_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    let bundle = registry
        .runtime_bundle()
        .cloned()
        .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
    let bridge = Arc::new(ConvexRuntimeBridge::new(
        service.clone(),
        registry.clone(),
        tenant_id.clone(),
    ));
    let runtime = NeovexRuntime::with_policy(bridge.clone(), registry.runtime_policy());
    let response = registry
        .runtime_executor()
        .invoke_blocking_with_cancellation(
            runtime,
            bundle,
            request.clone(),
            RuntimeInvocationContext::top_level(&request),
            Some(cancellation),
        )
        .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, bridge.snapshot_read_set()))
}

#[cfg(test)]
async fn invoke_named_convex_function_with_trace_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
    )
    .await
}

pub(super) async fn invoke_named_convex_function_with_trace_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    let bundle = registry
        .runtime_bundle()
        .cloned()
        .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
    let bridge = Arc::new(ConvexRuntimeBridge::new(
        service.clone(),
        registry.clone(),
        tenant_id.clone(),
    ));
    let runtime = NeovexRuntime::with_policy(bridge.clone(), registry.runtime_policy());
    let response = registry
        .runtime_executor()
        .invoke_on_worker(
            runtime,
            bundle,
            request.clone(),
            RuntimeInvocationContext::top_level(&request),
            Some(cancellation),
        )
        .await
        .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, bridge.snapshot_read_set()))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn bootstrap_runtime_named_subscription_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    auth: Option<InvocationAuth>,
    cancellation: HostCallCancellation,
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
    )
    .await?;
    let base_queries = synthesize_runtime_subscription_base_queries(&read_set)?;
    match kind {
        InvocationKind::Query => Ok(ConvexRuntimeSubscriptionSetup {
            initial_value: value,
            base_queries,
            transform: ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: name.to_string(),
                args: args.clone(),
                auth,
                read_set: Some(read_set),
            },
        }),
        InvocationKind::PaginatedQuery => {
            let page: neovex_core::Page = serde_json::from_value(value)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            Ok(ConvexRuntimeSubscriptionSetup {
                initial_value: Value::Array(page.data),
                base_queries,
                transform: ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                    name: name.to_string(),
                    args: args.clone(),
                    page_size: page_size
                        .expect("paginated runtime bootstrap should carry page size"),
                    cursor,
                    auth,
                    read_set: Some(read_set),
                },
            })
        }
        InvocationKind::Mutation | InvocationKind::Action => Err(Error::InvalidInput(
            "runtime subscription bootstrap only supports queries".to_string(),
        )),
    }
}

#[cfg(test)]
fn execute_named_query_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let query = registry.resolve_query(name, args)?;
    execute_query_result(service, tenant_id, query)
}

#[cfg(test)]
fn execute_named_paginated_query_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    page_size: usize,
    cursor: Option<String>,
) -> Result<neovex_core::Page, Error> {
    let query = registry.resolve_paginated_query(name, args, page_size, cursor)?;
    service.paginate_documents(tenant_id, &query)
}

#[cfg(test)]
pub(super) fn execute_named_mutation_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let mutation = registry.resolve_mutation(name, args)?;
    dispatch_convex_mutation(service, registry, tenant_id, mutation)
}

pub(super) fn execute_named_mutation_request_direct_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    let mutation = registry.resolve_mutation(name, args)?;
    dispatch_convex_mutation_cancellable(service, registry, tenant_id, mutation, cancellation)
}

#[cfg(test)]
pub(super) fn execute_named_action_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let action = registry.resolve_action(name, args)?;
    execute_convex_action(service, registry, tenant_id, action)
}

pub(super) fn execute_named_action_request_direct_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    let action = registry.resolve_action(name, args)?;
    execute_convex_action_cancellable(service, registry, tenant_id, action, cancellation)
}

pub(super) fn runtime_error_to_core(error: NeovexRuntimeError) -> Error {
    match error {
        NeovexRuntimeError::Cancelled | NeovexRuntimeError::ExecutionTimeout(_) => Error::Cancelled,
        other => Error::Internal(format!("convex runtime error: {other}")),
    }
}

pub(super) fn check_host_cancellation(cancellation: &HostCallCancellation) -> Result<(), Error> {
    if cancellation.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) fn ensure_runtime_host_not_cancelled(
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NeovexRuntimeError> {
    check_host_cancellation(cancellation).map_err(|_| NeovexRuntimeError::Cancelled)
}

pub(super) fn encode_runtime_core_result(
    result: Result<Value, Error>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    match result {
        Ok(value) => serde_json::to_value(ConvexRuntimeResponseEnvelope::ok(value))
            .map_err(NeovexRuntimeError::from),
        Err(Error::Cancelled) => Err(NeovexRuntimeError::Cancelled),
        Err(error) => serde_json::to_value(ConvexRuntimeResponseEnvelope::from_core_error(error))
            .map_err(NeovexRuntimeError::from),
    }
}

pub(super) fn dispatch_mutation(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    mutation: Mutation,
) -> Result<Value, Error> {
    match mutation {
        Mutation::Insert { table, fields } => {
            let id = service.insert_document(tenant_id, table, fields)?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Update { table, id, patch } => {
            let id = service.update_document(tenant_id, table, id, patch)?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Delete { table, id } => {
            service.delete_document(tenant_id, table, id)?;
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
pub(super) fn dispatch_convex_mutation(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    mutation: ConvexExecutableMutation,
) -> Result<Value, Error> {
    match mutation {
        ConvexExecutableMutation::Mutation(mutation) => {
            dispatch_mutation(service, tenant_id, mutation)
        }
        ConvexExecutableMutation::Query(query) => execute_query_result(service, tenant_id, query),
        ConvexExecutableMutation::Scheduled(command) => {
            execute_schedule_command(service, registry, tenant_id, command)
        }
    }
}

pub(super) fn dispatch_convex_mutation_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    mutation: ConvexExecutableMutation,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    match mutation {
        ConvexExecutableMutation::Mutation(mutation) => {
            check_host_cancellation(cancellation)?;
            dispatch_mutation(service, tenant_id, mutation)
        }
        ConvexExecutableMutation::Query(query) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(service, tenant_id, query, &mut check_cancel)
        }
        ConvexExecutableMutation::Scheduled(command) => execute_schedule_command_cancellable(
            service,
            registry,
            tenant_id,
            command,
            cancellation,
        ),
    }
}

#[cfg(test)]
pub(super) fn execute_query_result(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
) -> Result<Value, Error> {
    execute_query_result_cancellable(service, tenant_id, query, &mut || Ok(()))
}

pub(super) fn execute_query_result_cancellable(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
    check_cancel: &mut dyn FnMut() -> std::result::Result<(), Error>,
) -> Result<Value, Error> {
    match query {
        ConvexExecutableQuery::Query(query) => {
            let documents = service.query_documents_cancellable(tenant_id, &query, check_cancel)?;
            Ok(Value::Array(
                documents
                    .into_iter()
                    .map(|document| document.to_json())
                    .collect(),
            ))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => {
            match service.get_document(tenant_id, &table, id) {
                Ok(document) => Ok(document.to_json()),
                Err(Error::DocumentNotFound(_)) => Ok(Value::Null),
                Err(error) => Err(error),
            }
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            let mut documents =
                service.query_documents_cancellable(tenant_id, &query, check_cancel)?;
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.to_json())
                .unwrap_or(Value::Null))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            let mut documents =
                service.query_documents_cancellable(tenant_id, &query, check_cancel)?;
            if documents.len() > 1 {
                return Err(Error::InvalidInput(
                    "convex unique query matched multiple documents".to_string(),
                ));
            }
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.to_json())
                .unwrap_or(Value::Null))
        }
    }
}

#[cfg(test)]
pub(super) fn execute_convex_action(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
) -> Result<Value, Error> {
    match action {
        ConvexExecutableAction::Action(ConvexAction::Query { query }) => {
            execute_query_result(service, tenant_id, ConvexExecutableQuery::Query(query))
        }
        ConvexExecutableAction::Action(ConvexAction::PaginatedQuery { query }) => {
            serde_json::to_value(service.paginate_documents(tenant_id, &query)?)
                .map_err(|error| Error::Serialization(error.to_string()))
        }
        ConvexExecutableAction::Action(ConvexAction::Mutation { mutation }) => {
            dispatch_mutation(service, tenant_id, mutation)
        }
        ConvexExecutableAction::Scheduled(command) => {
            execute_schedule_command(service, registry, tenant_id, command)
        }
        ConvexExecutableAction::Call(command) => {
            execute_function_call(service, registry, tenant_id, command)
        }
    }
}

pub(super) fn execute_convex_action_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    match action {
        ConvexExecutableAction::Action(ConvexAction::Query { query }) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(
                service,
                tenant_id,
                ConvexExecutableQuery::Query(query),
                &mut check_cancel,
            )
        }
        ConvexExecutableAction::Action(ConvexAction::PaginatedQuery { query }) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            serde_json::to_value(service.paginate_documents_cancellable(
                tenant_id,
                &query,
                &mut check_cancel,
            )?)
            .map_err(|error| Error::Serialization(error.to_string()))
        }
        ConvexExecutableAction::Action(ConvexAction::Mutation { mutation }) => {
            check_host_cancellation(cancellation)?;
            dispatch_mutation(service, tenant_id, mutation)
        }
        ConvexExecutableAction::Scheduled(command) => execute_schedule_command_cancellable(
            service,
            registry,
            tenant_id,
            command,
            cancellation,
        ),
        ConvexExecutableAction::Call(command) => {
            execute_function_call_cancellable(service, registry, tenant_id, command, cancellation)
        }
    }
}

#[cfg(test)]
fn execute_function_call(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexFunctionCallCommand,
) -> Result<Value, Error> {
    match command {
        ConvexFunctionCallCommand::Query {
            name,
            visibility,
            args,
        } => {
            let query = registry.resolve_query_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            execute_query_result(service, tenant_id, query)
        }
        ConvexFunctionCallCommand::Mutation {
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            dispatch_convex_mutation(service, registry, tenant_id, mutation)
        }
        ConvexFunctionCallCommand::Action {
            name,
            visibility,
            args,
        } => {
            let action = registry.resolve_action_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            execute_convex_action(service, registry, tenant_id, action)
        }
    }
}

fn execute_function_call_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexFunctionCallCommand,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    match command {
        ConvexFunctionCallCommand::Query {
            name,
            visibility,
            args,
        } => {
            let query = registry.resolve_query_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable(service, tenant_id, query, &mut check_cancel)
        }
        ConvexFunctionCallCommand::Mutation {
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            dispatch_convex_mutation_cancellable(
                service,
                registry,
                tenant_id,
                mutation,
                cancellation,
            )
        }
        ConvexFunctionCallCommand::Action {
            name,
            visibility,
            args,
        } => {
            let action = registry.resolve_action_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            execute_convex_action_cancellable(service, registry, tenant_id, action, cancellation)
        }
    }
}

pub(super) fn execute_schedule_command(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
) -> Result<Value, Error> {
    match command {
        ConvexScheduledCommand::RunAfter {
            delay_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let job_id = service.schedule_mutation(
                tenant_id,
                ScheduleRequest {
                    run_after_ms: delay_ms,
                    mutation,
                },
            )?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::RunAt {
            timestamp_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let delay_ms = timestamp_ms.saturating_sub(Timestamp::now().0);
            let job_id = service.schedule_mutation(
                tenant_id,
                ScheduleRequest {
                    run_after_ms: delay_ms,
                    mutation,
                },
            )?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::Cancel { job_id } => {
            let job_id = job_id
                .parse()
                .map_err(|error| Error::InvalidInput(format!("invalid document id: {error}")))?;
            service.cancel_scheduled_job(tenant_id, &job_id)?;
            Ok(Value::Null)
        }
    }
}

fn execute_schedule_command_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    check_host_cancellation(cancellation)?;
    execute_schedule_command(service, registry, tenant_id, command)
}

async fn query_documents_async_with_optional_cancellation(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: Query,
    cancellation: Option<HostCallCancellation>,
) -> Result<Vec<neovex_core::Document>, Error> {
    match cancellation {
        Some(cancellation) => {
            let check_cancellation = cancellation.clone();
            service
                .query_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        None => {
            service
                .query_documents_async(tenant_id.clone(), query)
                .await
        }
    }
}

async fn paginate_documents_async_with_optional_cancellation(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: PaginatedQuery,
    cancellation: Option<HostCallCancellation>,
) -> Result<neovex_core::Page, Error> {
    match cancellation {
        Some(cancellation) => {
            let check_cancellation = cancellation.clone();
            service
                .paginate_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        None => {
            service
                .paginate_documents_async(tenant_id.clone(), query)
                .await
        }
    }
}

pub(super) async fn dispatch_mutation_async(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    mutation: Mutation,
) -> Result<Value, Error> {
    match mutation {
        Mutation::Insert { table, fields } => {
            let id = service
                .insert_document_async(tenant_id.clone(), table, fields)
                .await?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Update { table, id, patch } => {
            let id = service
                .update_document_async(tenant_id.clone(), table, id, patch)
                .await?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Delete { table, id } => {
            service
                .delete_document_async(tenant_id.clone(), table, id)
                .await?;
            Ok(Value::Null)
        }
    }
}

pub(super) async fn execute_query_result_async(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match query {
        ConvexExecutableQuery::Query(query) => {
            let documents = query_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                cancellation,
            )
            .await?;
            Ok(Value::Array(
                documents
                    .into_iter()
                    .map(|document| document.to_json())
                    .collect(),
            ))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => {
            match service
                .get_document_async(tenant_id.clone(), table, id)
                .await
            {
                Ok(document) => Ok(document.to_json()),
                Err(Error::DocumentNotFound(_)) => Ok(Value::Null),
                Err(error) => Err(error),
            }
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            let mut documents = query_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                cancellation,
            )
            .await?;
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.to_json())
                .unwrap_or(Value::Null))
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            let mut documents = query_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                cancellation,
            )
            .await?;
            if documents.len() > 1 {
                return Err(Error::InvalidInput(
                    "convex unique query matched multiple documents".to_string(),
                ));
            }
            Ok(documents
                .drain(..)
                .next()
                .map(|document| document.to_json())
                .unwrap_or(Value::Null))
        }
    }
}

pub(super) async fn dispatch_convex_mutation_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    mutation: ConvexExecutableMutation,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match mutation {
        ConvexExecutableMutation::Mutation(mutation) => {
            if let Some(cancellation) = cancellation.as_ref() {
                check_host_cancellation(cancellation)?;
            }
            dispatch_mutation_async(service, tenant_id, mutation).await
        }
        ConvexExecutableMutation::Query(query) => {
            execute_query_result_async(service, tenant_id, query, cancellation).await
        }
        ConvexExecutableMutation::Scheduled(command) => {
            execute_schedule_command_async(service, registry, tenant_id, command, cancellation)
                .await
        }
    }
}

pub(super) async fn execute_convex_action_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match action {
        ConvexExecutableAction::Action(ConvexAction::Query { query }) => {
            execute_query_result_async(
                service,
                tenant_id,
                ConvexExecutableQuery::Query(query),
                cancellation,
            )
            .await
        }
        ConvexExecutableAction::Action(ConvexAction::PaginatedQuery { query }) => {
            let page = paginate_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                cancellation,
            )
            .await?;
            serde_json::to_value(page).map_err(|error| Error::Serialization(error.to_string()))
        }
        ConvexExecutableAction::Action(ConvexAction::Mutation { mutation }) => {
            if let Some(cancellation) = cancellation.as_ref() {
                check_host_cancellation(cancellation)?;
            }
            dispatch_mutation_async(service, tenant_id, mutation).await
        }
        ConvexExecutableAction::Scheduled(command) => {
            execute_schedule_command_async(service, registry, tenant_id, command, cancellation)
                .await
        }
        ConvexExecutableAction::Call(command) => {
            Box::pin(execute_function_call_async(
                service,
                registry,
                tenant_id,
                command,
                cancellation,
            ))
            .await
        }
    }
}

async fn execute_function_call_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    command: ConvexFunctionCallCommand,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match command {
        ConvexFunctionCallCommand::Query {
            name,
            visibility,
            args,
        } => {
            let query = registry.resolve_query_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            execute_query_result_async(service, tenant_id, query, cancellation).await
        }
        ConvexFunctionCallCommand::Mutation {
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            dispatch_convex_mutation_async(service, registry, tenant_id, mutation, cancellation)
                .await
        }
        ConvexFunctionCallCommand::Action {
            name,
            visibility,
            args,
        } => {
            let action = registry.resolve_action_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            Box::pin(execute_convex_action_async(
                service,
                registry,
                tenant_id,
                action,
                cancellation,
            ))
            .await
        }
    }
}

pub(super) async fn execute_schedule_command_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    command: ConvexScheduledCommand,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    if let Some(cancellation) = cancellation.as_ref() {
        check_host_cancellation(cancellation)?;
    }

    match command {
        ConvexScheduledCommand::RunAfter {
            delay_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let job_id = service
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: delay_ms,
                        mutation,
                    },
                )
                .await?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::RunAt {
            timestamp_ms,
            name,
            visibility,
            args,
        } => {
            let mutation = registry.resolve_scheduled_mutation_for_visibility(
                &name,
                &args,
                visibility.unwrap_or(ConvexFunctionVisibility::Public),
            )?;
            let delay_ms = timestamp_ms.saturating_sub(Timestamp::now().0);
            let job_id = service
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: delay_ms,
                        mutation,
                    },
                )
                .await?;
            Ok(Value::String(job_id.to_string()))
        }
        ConvexScheduledCommand::Cancel { job_id } => {
            let job_id = job_id
                .parse()
                .map_err(|error| Error::InvalidInput(format!("invalid document id: {error}")))?;
            service
                .cancel_scheduled_job_async(tenant_id.clone(), job_id)
                .await?;
            Ok(Value::Null)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpRequestContext {
    pub(super) method: String,
    pub(super) url: String,
    pub(super) pathname: String,
    pub(super) query: HashMap<String, String>,
    pub(super) headers: HashMap<String, String>,
    pub(super) body_bytes: Vec<u8>,
    pub(super) body_text: String,
}

pub(super) struct ConvexHttpRouteRequest {
    pub(super) request_path: String,
    pub(super) method: Method,
    pub(super) headers: HeaderMap,
    pub(super) original_uri: OriginalUri,
    pub(super) query: HashMap<String, String>,
    pub(super) body: Bytes,
}

pub(super) struct ConvexSubscriptionEvent<'a> {
    pub(super) subscription_id: u64,
    pub(super) request_id: Option<&'a str>,
    pub(super) commit: Option<&'a CommitEntry>,
    pub(super) deleted_documents: &'a [neovex_core::Document],
}
