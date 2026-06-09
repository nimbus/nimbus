use super::*;

/// Stores or updates a table schema.
pub(crate) async fn set_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
    Json(table_schema): Json<TableSchema>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schema.set")?;
    let path_table = TableName::new(table)?;
    if table_schema.table != path_table {
        return Err(AppError::from(Error::InvalidInput(
            "schema table must match the path table".to_string(),
        )));
    }

    state
        .engine
        .set_table_schema_async(tenant.tenant_id().clone(), table_schema)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns the full tenant schema.
pub(crate) async fn get_schema(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<Schema>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schema.get")?;
    let service = state.engine.clone();
    let schema = service.get_schema_async(tenant.tenant_id().clone()).await?;
    Ok(Json(schema))
}

/// Returns a single table schema.
pub(crate) async fn get_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<Json<TableSchema>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schema.get_table")?;
    let table = TableName::new(table)?;
    let service = state.engine.clone();
    let table_schema = service
        .get_table_schema_async(tenant.tenant_id().clone(), table)
        .await?;
    Ok(Json(table_schema))
}

/// Deletes a single table schema.
pub(crate) async fn delete_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schema.delete")?;
    let table = TableName::new(table)?;
    state
        .engine
        .delete_table_schema_async(tenant.tenant_id().clone(), table)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
