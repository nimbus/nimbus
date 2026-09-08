use nimbus_core::TenantId;
use nimbus_system::{LOG_PAGE_LIMIT, LogQuery, query_log_lines_async};
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct LogSearchParams {
    /// The tenant scope. Absent reads every tenant.
    tenant: Option<String>,
    /// The run whose lines are wanted.
    run: Option<String>,
    level: Option<String>,
    source: Option<String>,
    category: Option<String>,
    /// Free text: a case-insensitive substring of the line's message.
    q: Option<String>,
    /// The page size, clamped by the query to `1..=LOG_PAGE_LIMIT`.
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LogSearchResponse {
    lines: Vec<serde_json::Value>,
    matched: usize,
    scanned: usize,
    exhaustive: bool,
    limit: usize,
}

/// Search the log lines the server recorded. Backs the console's log search
/// and per-run log view: `GET /api/console/logs?tenant=&run=&level=&source=&category=&q=&limit=`.
///
/// The reactive log stream reads the same `events` table through the
/// `_nimbus` bundle. This route is the request/response read: a bounded
/// window of the newest lines that match the index facets, filtered
/// by text, with the match count and whether the window was exhaustive.
pub(crate) async fn search_logs(
    State(state): State<Arc<AppState>>,
    QueryParams(params): QueryParams<LogSearchParams>,
) -> Result<Json<LogSearchResponse>, AppError> {
    let tenant_id = params
        .tenant
        .as_deref()
        .map(str::trim)
        .filter(|tenant| !tenant.is_empty())
        .map(TenantId::new)
        .transpose()?;
    let page = query_log_lines_async(
        &state.engine,
        LogQuery {
            tenant_id: tenant_id.as_ref(),
            run_id: present(params.run.as_deref()),
            level: present(params.level.as_deref()),
            source: present(params.source.as_deref()),
            category: present(params.category.as_deref()),
            text: present(params.q.as_deref()),
            limit: params.limit.unwrap_or(LOG_PAGE_LIMIT),
        },
    )
    .await?;
    Ok(Json(LogSearchResponse {
        lines: page.lines,
        matched: page.matched,
        scanned: page.scanned,
        exhaustive: page.exhaustive,
        limit: page.limit,
    }))
}

fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
