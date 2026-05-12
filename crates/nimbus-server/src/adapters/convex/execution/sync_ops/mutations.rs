#[cfg(test)]
use super::queries::execute_query_result;
use super::queries::execute_query_result_cancellable_with_auth;
#[cfg(test)]
use super::scheduling::execute_schedule_command;
use super::scheduling::execute_schedule_command_cancellable;
use super::*;
use crate::application_auth::normalize_principal_context;

#[cfg(test)]
pub(in crate::adapters::convex) fn dispatch_mutation(
    service: &nimbus_engine::Service,
    tenant_id: &TenantId,
    mutation: Mutation,
) -> Result<Value, Error> {
    dispatch_mutation_with_auth(service, tenant_id, mutation, None)
}

pub(in crate::adapters::convex) fn dispatch_mutation_with_auth(
    service: &nimbus_engine::Service,
    tenant_id: &TenantId,
    mutation: Mutation,
    auth: Option<&InvocationAuth>,
) -> Result<Value, Error> {
    let principal = normalize_principal_context(auth);
    match mutation {
        Mutation::Insert { table, id, fields } => {
            let id = service.insert_document_with(
                tenant_id,
                table,
                id,
                fields,
                nimbus_engine::MutationActor::with_principal(&principal),
            )?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Update { table, id, patch } => {
            let id = service.update_document_with(
                tenant_id,
                table,
                id,
                patch,
                nimbus_engine::MutationActor::with_principal(&principal),
            )?;
            Ok(Value::String(id.to_string()))
        }
        Mutation::Delete { table, id } => {
            service.delete_document_with(
                tenant_id,
                table,
                id,
                nimbus_engine::MutationActor::with_principal(&principal),
            )?;
            Ok(Value::Null)
        }
    }
}

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_named_mutation_request_direct(
    service: &nimbus_engine::Service,
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
    service: &nimbus_engine::Service,
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
    service: &nimbus_engine::Service,
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
