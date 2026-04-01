#[cfg(test)]
use super::queries::execute_query_result;
use super::queries::execute_query_result_cancellable_with_auth;
#[cfg(test)]
use super::scheduling::execute_schedule_command;
use super::scheduling::execute_schedule_command_cancellable;
use super::*;

#[cfg(test)]
pub(in crate::adapters::convex) fn dispatch_mutation(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    mutation: Mutation,
) -> Result<Value, Error> {
    dispatch_mutation_with_auth(service, tenant_id, mutation, None)
}

pub(in crate::adapters::convex) fn dispatch_mutation_with_auth(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    mutation: Mutation,
    auth: Option<&InvocationAuth>,
) -> Result<Value, Error> {
    let principal = normalize_principal_context(auth);
    match mutation {
        Mutation::Insert { table, fields } => {
            let id =
                service.insert_document_with_principal(tenant_id, table, fields, &principal)?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Update { table, id, patch } => {
            let id =
                service.update_document_with_principal(tenant_id, table, id, patch, &principal)?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Delete { table, id } => {
            service.delete_document_with_principal(tenant_id, table, id, &principal)?;
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_named_mutation_request_direct(
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
pub(in crate::adapters::convex) fn dispatch_convex_mutation(
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

pub(in crate::adapters::convex) fn dispatch_convex_mutation_cancellable_with_auth(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    mutation: ConvexExecutableMutation,
    auth: Option<&InvocationAuth>,
    cancellation: &HostCallCancellation,
) -> Result<Value, Error> {
    match mutation {
        ConvexExecutableMutation::Mutation(mutation) => {
            check_host_cancellation(cancellation)?;
            dispatch_mutation_with_auth(service, tenant_id, mutation, auth)
        }
        ConvexExecutableMutation::Query(query) => {
            let mut check_cancel = || check_host_cancellation(cancellation);
            execute_query_result_cancellable_with_auth(
                service,
                tenant_id,
                query,
                auth,
                &mut check_cancel,
            )
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
