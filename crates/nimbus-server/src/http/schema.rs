use serde::{Deserialize, Serialize};

use super::*;

/// The number of documents one scan page carries while the apply route
/// checks a table. The scan walks every page, so the size bounds memory,
/// not coverage.
const APPLY_SCAN_PAGE_SIZE: usize = 500;

/// The number of violations the apply response lists. `violation_count`
/// still reports the full total so the operator knows the size of the
/// repair.
const APPLY_VIOLATION_LIMIT: usize = 50;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ApplyTableSchemaParams {
    /// Scan and report without storing the schema.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SchemaViolation {
    id: String,
    message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ApplyTableSchemaResponse {
    applied: bool,
    dry_run: bool,
    scanned: usize,
    violation_count: usize,
    violations: Vec<SchemaViolation>,
}

/// Applies a table schema only when every existing document satisfies it.
///
/// `PUT` stores a schema without reading the table, so a document that
/// predates the schema can violate it silently and every later write to
/// that document fails. This route scans the whole table first. When any
/// document violates the schema the response reports `applied: false` and
/// the violations, and the stored schema does not change. The response is
/// `200` in both outcomes so the client can read the report.
pub(crate) async fn apply_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
    QueryParams(params): QueryParams<ApplyTableSchemaParams>,
    Json(table_schema): Json<TableSchema>,
) -> Result<Json<ApplyTableSchemaResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schema.apply")?;
    let path_table = TableName::new(table)?;
    if table_schema.table != path_table {
        return Err(AppError::from(Error::InvalidInput(
            "schema table must match the path table".to_string(),
        )));
    }
    table_schema.validate_indexes()?;

    let guard = RequestCancellationGuard::new();
    let cancellation = guard.token();
    let mut scanned = 0usize;
    let mut violation_count = 0usize;
    let mut violations = Vec::new();
    let mut after = None;
    loop {
        let cancellation_check = cancellation.clone();
        let page = state
            .engine
            .paginate_documents_async_cancellable(
                tenant.tenant_id().clone(),
                PaginatedQuery {
                    query: Query {
                        table: path_table.clone(),
                        filters: Vec::new(),
                        order: None,
                        limit: None,
                    },
                    page_size: APPLY_SCAN_PAGE_SIZE,
                    after,
                },
                cancellation.cancelled(),
                move || {
                    if cancellation_check.is_cancelled() {
                        Err(Error::Cancelled)
                    } else {
                        Ok(())
                    }
                },
            )
            .await?;
        for document in &page.data {
            scanned += 1;
            let Some(fields) = document.as_object() else {
                continue;
            };
            if let Err(error) = table_schema.validate(fields) {
                violation_count += 1;
                if violations.len() < APPLY_VIOLATION_LIMIT {
                    violations.push(SchemaViolation {
                        id: fields
                            .get("_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        message: match error {
                            Error::SchemaValidation(message) => message,
                            other => other.to_string(),
                        },
                    });
                }
            }
        }
        if !page.has_more {
            break;
        }
        after = page.next_cursor;
    }

    let applied = violation_count == 0 && !params.dry_run;
    if applied {
        state
            .engine
            .set_table_schema_async(tenant.tenant_id().clone(), table_schema)
            .await?;
    }
    Ok(Json(ApplyTableSchemaResponse {
        applied,
        dry_run: params.dry_run,
        scanned,
        violation_count,
        violations,
    }))
}

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
