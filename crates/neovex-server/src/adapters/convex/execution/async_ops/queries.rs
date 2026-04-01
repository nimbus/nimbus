use super::*;

pub(super) async fn query_documents_async_with_optional_cancellation(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: Query,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<Vec<neovex_core::Document>, Error> {
    let principal = normalize_principal_context(auth);
    match cancellation {
        Some(cancellation) => {
            let check_cancellation = cancellation.clone();
            service
                .query_documents_async_cancellable_with_principal(
                    tenant_id.clone(),
                    query,
                    principal,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        None => {
            service
                .query_documents_async_with_principal(tenant_id.clone(), query, principal)
                .await
        }
    }
}

pub(super) async fn paginate_documents_async_with_optional_cancellation(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: PaginatedQuery,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<neovex_core::Page, Error> {
    let principal = normalize_principal_context(auth);
    match cancellation {
        Some(cancellation) => {
            let check_cancellation = cancellation.clone();
            service
                .paginate_documents_async_cancellable_with_principal(
                    tenant_id.clone(),
                    query,
                    principal,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&check_cancellation),
                )
                .await
        }
        None => {
            service
                .paginate_documents_async_with_principal(tenant_id.clone(), query, principal)
                .await
        }
    }
}

pub(in crate::adapters::convex) async fn execute_query_result_async(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<Value, Error> {
    match query {
        ConvexExecutableQuery::Query(query) => {
            let documents = query_documents_async_with_optional_cancellation(
                service,
                tenant_id,
                query,
                auth,
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
            let principal = normalize_principal_context(auth);
            match service
                .get_document_async_with_principal(tenant_id.clone(), table, id, principal)
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
                auth,
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
                auth,
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
