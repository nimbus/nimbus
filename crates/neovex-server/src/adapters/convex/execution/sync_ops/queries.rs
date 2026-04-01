use super::*;

#[cfg(test)]
pub(in crate::adapters::convex) fn execute_named_query_request_direct(
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
pub(in crate::adapters::convex) fn execute_named_paginated_query_request_direct(
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
pub(in crate::adapters::convex) fn execute_query_result(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    query: ConvexExecutableQuery,
) -> Result<Value, Error> {
    execute_query_result_cancellable(service, tenant_id, query, &mut || Ok(()))
}

pub(in crate::adapters::convex) fn execute_query_result_cancellable(
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
