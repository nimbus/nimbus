use super::*;

/// Inserts a document into a tenant table.
pub(crate) async fn insert_document(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<InsertDocumentRequest>,
) -> Result<(StatusCode, Json<DocumentResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(request.table)?;
    let service = state.service.clone();
    let document_id = service
        .insert_document_async(tenant_id, table, request.fields)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(DocumentResponse {
            id: document_id.to_string(),
        }),
    ))
}

/// Updates a document within a tenant table.
pub(crate) async fn update_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
    Json(request): Json<UpdateDocumentRequest>,
) -> Result<Json<DocumentResponse>, AppError> {
    let document_id = parse_document_id(&document_id)?;
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let document_id = service
        .update_document_async(tenant_id, table, document_id, request.patch)
        .await?;

    Ok(Json(DocumentResponse {
        id: document_id.to_string(),
    }))
}

/// Deletes a document within a tenant table.
pub(crate) async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let document_id = parse_document_id(&document_id)?;
    let service = state.service.clone();
    service
        .delete_document_async(tenant_id, table, document_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists documents in a tenant table.
pub(crate) async fn list_documents(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<Json<DataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let documents = service.list_documents_async(tenant_id, table).await?;
    Ok(Json(DataResponse {
        data: documents
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
    }))
}

/// Fetches a single document in a tenant table.
pub(crate) async fn get_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
) -> Result<Json<DocumentDataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let document_id = parse_document_id(&document_id)?;
    let service = state.service.clone();
    let document = service
        .get_document_async(tenant_id, table, document_id)
        .await?;
    Ok(Json(DocumentDataResponse {
        document: document.to_json(),
    }))
}
