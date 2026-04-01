use super::*;

pub(super) async fn query_documents_async_with_optional_cancellation(
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

pub(super) async fn paginate_documents_async_with_optional_cancellation(
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

pub(in crate::adapters::convex) async fn execute_query_result_async(
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
