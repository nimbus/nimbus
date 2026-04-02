use super::queries::execute_query_result_async;
use super::scheduling::execute_schedule_command_async;
use super::*;

pub(in crate::adapters::convex) async fn dispatch_mutation_async_with_auth(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    mutation: Mutation,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    let principal = normalize_principal_context(auth);
    match (mutation, cancellation) {
        (Mutation::Insert { table, fields }, Some(cancellation)) => {
            let check_cancellation = cancellation.clone();
            let id = service
                .insert_document_async_cancellable_with_principal(
                    tenant_id.clone(),
                    table,
                    fields,
                    principal,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await?;
            Ok(Value::String(id.to_string()))
        }
        (Mutation::Insert { table, fields }, None) => {
            let id = service
                .insert_document_async_with_principal(tenant_id.clone(), table, fields, principal)
                .await?;
            Ok(Value::String(id.to_string()))
        }
        (Mutation::Update { table, id, patch }, Some(cancellation)) => {
            let check_cancellation = cancellation.clone();
            let id = service
                .update_document_async_cancellable_with_principal(
                    tenant_id.clone(),
                    table,
                    id,
                    patch,
                    principal,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await?;
            Ok(Value::String(id.to_string()))
        }
        (Mutation::Update { table, id, patch }, None) => {
            let id = service
                .update_document_async_with_principal(
                    tenant_id.clone(),
                    table,
                    id,
                    patch,
                    principal,
                )
                .await?;
            Ok(Value::String(id.to_string()))
        }
        (Mutation::Delete { table, id }, Some(cancellation)) => {
            let check_cancellation = cancellation.clone();
            service
                .delete_document_async_cancellable_with_principal(
                    tenant_id.clone(),
                    table,
                    id,
                    principal,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await?;
            Ok(Value::Null)
        }
        (Mutation::Delete { table, id }, None) => {
            service
                .delete_document_async_with_principal(tenant_id.clone(), table, id, principal)
                .await?;
            Ok(Value::Null)
        }
    }
}

pub(in crate::adapters::convex) async fn dispatch_convex_mutation_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    mutation: ConvexExecutableMutation,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match mutation {
        ConvexExecutableMutation::Mutation(mutation) => {
            if let Some(cancellation) = cancellation.as_ref() {
                check_host_cancellation(cancellation)?;
            }
            dispatch_mutation_async_with_auth(service, tenant_id, mutation, auth, cancellation)
                .await
        }
        ConvexExecutableMutation::Query(query) => {
            execute_query_result_async(service, tenant_id, query, auth, cancellation).await
        }
        ConvexExecutableMutation::Scheduled(command) => {
            execute_schedule_command_async(service, registry, tenant_id, command, cancellation)
                .await
        }
    }
}
