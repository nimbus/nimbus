use super::*;

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

pub(in crate::convex) async fn dispatch_mutation_async(
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

pub(in crate::convex) async fn execute_query_result_async(
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

pub(in crate::convex) async fn dispatch_convex_mutation_async(
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

pub(in crate::convex) async fn execute_convex_action_async(
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

pub(in crate::convex) async fn execute_schedule_command_async(
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
