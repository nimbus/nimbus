use super::mutations::{dispatch_convex_mutation_async, dispatch_mutation_async_with_auth};
use super::queries::{
    execute_query_result_async, paginate_documents_async_with_optional_cancellation,
};
use super::scheduling::execute_schedule_command_async;
use super::*;

pub(in crate::adapters::convex) async fn execute_convex_action_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    action: ConvexExecutableAction,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match action {
        ConvexExecutableAction::Action(ConvexAction::Query { query }) => {
            execute_query_result_async(
                service,
                tenant_id,
                ConvexExecutableQuery::Query(query),
                auth,
                cancellation,
            )
            .await
        }
        ConvexExecutableAction::Action(ConvexAction::PaginatedQuery { query }) => {
            let page = paginate_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                auth,
                cancellation,
            )
            .await?;
            serde_json::to_value(page).map_err(|error| Error::Serialization(error.to_string()))
        }
        ConvexExecutableAction::Action(ConvexAction::Mutation { mutation }) => {
            if let Some(cancellation) = cancellation.as_ref() {
                check_host_cancellation(cancellation)?;
            }
            dispatch_mutation_async_with_auth(service, tenant_id, mutation, auth, cancellation)
                .await
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
                auth,
                cancellation,
            ))
            .await
        }
    }
}

async fn execute_function_call_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    command: ConvexFunctionCallCommand,
    auth: Option<&InvocationAuth>,
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
            execute_query_result_async(service, tenant_id, query, auth, cancellation).await
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
            dispatch_convex_mutation_async(
                service,
                registry,
                tenant_id,
                mutation,
                auth,
                cancellation,
            )
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
                auth,
                cancellation,
            ))
            .await
        }
    }
}
