use super::*;

#[cfg(test)]
pub(in crate::convex) fn execute_named_query_request_direct(
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
pub(in crate::convex) fn execute_named_paginated_query_request_direct(
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
pub(in crate::convex) fn execute_named_mutation_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let mutation = registry.resolve_mutation(name, args)?;
    dispatch_convex_mutation(service, registry, tenant_id, mutation)
}

#[cfg(test)]
pub(in crate::convex) fn execute_named_action_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let action = registry.resolve_action(name, args)?;
    execute_convex_action(service, registry, tenant_id, action)
}

pub(in crate::convex) fn runtime_error_to_core(error: NeovexRuntimeError) -> Error {
    match error {
        NeovexRuntimeError::Cancelled | NeovexRuntimeError::ExecutionTimeout(_) => Error::Cancelled,
        other => Error::Internal(format!("convex runtime error: {other}")),
    }
}

pub(in crate::convex) fn check_host_cancellation(
    cancellation: &HostCallCancellation,
) -> Result<(), Error> {
    if cancellation.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

pub(in crate::convex) fn ensure_runtime_host_not_cancelled(
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NeovexRuntimeError> {
    check_host_cancellation(cancellation).map_err(|_| NeovexRuntimeError::Cancelled)
}

pub(in crate::convex) fn encode_runtime_core_result(
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

pub(in crate::convex) fn dispatch_mutation(
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
pub(in crate::convex) fn dispatch_convex_mutation(
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

pub(in crate::convex) fn dispatch_convex_mutation_cancellable(
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
pub(in crate::convex) fn execute_query_result(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
) -> Result<Value, Error> {
    execute_query_result_cancellable(service, tenant_id, query, &mut || Ok(()))
}

pub(in crate::convex) fn execute_query_result_cancellable(
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
pub(in crate::convex) fn execute_convex_action(
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

pub(in crate::convex) fn execute_convex_action_cancellable(
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

pub(in crate::convex) fn execute_schedule_command(
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
