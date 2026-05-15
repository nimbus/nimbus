use super::*;

/// Stores or updates a table schema.
pub(crate) async fn set_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
    Json(table_schema): Json<TableSchema>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let path_table = TableName::new(table)?;
    if table_schema.table != path_table {
        return Err(AppError::from(Error::InvalidInput(
            "schema table must match the path table".to_string(),
        )));
    }

    state
        .service
        .set_table_schema_async(tenant_id, table_schema)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns the full tenant schema.
pub(crate) async fn get_schema(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<Schema>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    let schema = service.get_schema_async(tenant_id).await?;
    Ok(Json(schema))
}

/// Returns a single table schema.
pub(crate) async fn get_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<Json<TableSchema>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let table_schema = service.get_table_schema_async(tenant_id, table).await?;
    Ok(Json(table_schema))
}

/// Deletes a single table schema.
pub(crate) async fn delete_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let table = TableName::new(table)?;
    state
        .service
        .delete_table_schema_async(tenant_id, table)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
