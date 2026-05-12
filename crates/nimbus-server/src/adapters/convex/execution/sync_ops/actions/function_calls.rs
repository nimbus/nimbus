use super::*;

#[cfg(test)]
pub(super) fn execute_function_call(
    service: &nimbus_engine::Service,
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
            super::top_level::execute_convex_action(service, registry, tenant_id, action)
        }
    }
}

pub(super) fn execute_function_call_cancellable(
    service: &nimbus_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    command: ConvexFunctionCallCommand,
    auth: Option<&InvocationAuth>,
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
            execute_query_result_cancellable_with_auth(
                service,
                tenant_id,
                query,
                auth,
                &mut check_cancel,
            )
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
            dispatch_convex_mutation_cancellable_with_auth(
                service,
                registry,
                tenant_id,
                mutation,
                auth,
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
            super::top_level::execute_convex_action_cancellable_with_auth(
                service,
                registry,
                tenant_id,
                action,
                auth,
                cancellation,
            )
        }
    }
}
