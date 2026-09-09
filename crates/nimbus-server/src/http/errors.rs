use nimbus_core::TenantId;
use nimbus_system::{ERROR_GROUP_LIMIT, ErrorGroupQuery, query_error_groups_async};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct ErrorGroupParams {
    /// The tenant scope. Absent reads every tenant.
    tenant: Option<String>,
    /// The number of groups, clamped by the query to `1..=ERROR_GROUP_LIMIT`.
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorGroupResponse {
    groups: Vec<serde_json::Value>,
    scanned: usize,
    exhaustive: bool,
    limit: usize,
}

/// The failed runs the server recorded, folded by error fingerprint. Backs
/// the console's Errors tab: `GET /api/console/errors?tenant=&limit=`.
///
/// A group carries its count, first and last seen, the newest run's id, and
/// the sample message. `exhaustive` is false when the scan window ended
/// before the oldest failed run, so older groups may exist.
pub(crate) async fn error_groups(
    State(state): State<Arc<AppState>>,
    QueryParams(params): QueryParams<ErrorGroupParams>,
) -> Result<Json<ErrorGroupResponse>, AppError> {
    let tenant_id = params
        .tenant
        .as_deref()
        .map(str::trim)
        .filter(|tenant| !tenant.is_empty())
        .map(TenantId::new)
        .transpose()?;
    let page = query_error_groups_async(
        &state.engine,
        ErrorGroupQuery {
            tenant_id: tenant_id.as_ref(),
            limit: params.limit.unwrap_or(ERROR_GROUP_LIMIT),
        },
    )
    .await?;
    Ok(Json(ErrorGroupResponse {
        groups: page.groups.iter().map(|group| group.to_json()).collect(),
        scanned: page.scanned,
        exhaustive: page.exhaustive,
        limit: page.limit,
    }))
}
