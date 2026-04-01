use super::*;

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_named_action_request_direct(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
) -> Result<Value, Error> {
    let action = registry.resolve_action(name, args)?;
    execute_convex_action(service, registry, tenant_id, action)
}

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_convex_action(
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
            function_calls::execute_function_call(service, registry, tenant_id, command)
        }
    }
}

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_convex_action_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    execute_convex_action_cancellable_with_auth(
        service,
        registry,
        tenant_id,
        action,
        None,
        cancellation,
    )
}

pub(in crate::adapters::convex) fn execute_convex_action_cancellable_with_auth(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
    auth: Option<&InvocationAuth>,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    match action {
        ConvexExecutableAction::Action(ConvexAction::Query { query }) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable_with_auth(
                service,
                tenant_id,
                ConvexExecutableQuery::Query(query),
                auth,
                &mut check_cancel,
            )
        }
        ConvexExecutableAction::Action(ConvexAction::PaginatedQuery { query }) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            serde_json::to_value(service.paginate_documents_with_principal_cancellable(
                tenant_id,
                &query,
                &normalize_principal_context(auth),
                &mut check_cancel,
            )?)
            .map_err(|error| Error::Serialization(error.to_string()))
        }
        ConvexExecutableAction::Action(ConvexAction::Mutation { mutation }) => {
            check_host_cancellation(cancellation)?;
            dispatch_mutation_with_auth(service, tenant_id, mutation, auth)
        }
        ConvexExecutableAction::Scheduled(command) => execute_schedule_command_cancellable(
            service,
            registry,
            tenant_id,
            command,
            cancellation,
        ),
        ConvexExecutableAction::Call(command) => function_calls::execute_function_call_cancellable(
            service,
            registry,
            tenant_id,
            command,
            auth,
            cancellation,
        ),
    }
}
