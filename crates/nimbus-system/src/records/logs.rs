//! The log line query: the console's read over the `events` system table,
//! scoped to a tenant and narrowed by run, level, source, category, and
//! free text.
//!
//! The reactive stream in the console reads the same table through the
//! `_nimbus` bundle. This query is the request/response half: a text search
//! scans a bounded window of the newest lines and answers with the matches,
//! the match count, and whether the window covered every candidate line.

use std::sync::Arc;

use nimbus_core::{Filter, FilterOp, OrderBy, OrderDirection, Query, Result, TenantId};
use nimbus_engine::Engine;
use serde_json::{Value, json};

use crate::identity::system_tenant_id;
use crate::schema::SystemTable;

/// The most lines one page carries. A larger `limit` is clamped to it.
pub const LOG_PAGE_LIMIT: usize = 200;

/// How many of the newest candidate lines a query scans. The tenant, run,
/// and level filters are index reads; the text filter runs over this window,
/// so a search that finds nothing in the window says so instead of walking
/// the whole table.
pub const LOG_SCAN_WINDOW: usize = 2_000;

/// One log line read.
#[derive(Debug, Clone, Copy)]
pub struct LogQuery<'a> {
    /// The tenant the lines belong to, or `None` for every tenant.
    pub tenant_id: Option<&'a TenantId>,
    /// The run whose lines are wanted: the line's `correlationId`.
    pub run_id: Option<&'a str>,
    pub level: Option<&'a str>,
    pub source: Option<&'a str>,
    pub category: Option<&'a str>,
    /// Case-insensitive substring of the line's `message`.
    pub text: Option<&'a str>,
    /// The page size, clamped to `1..=LOG_PAGE_LIMIT`.
    pub limit: usize,
}

/// The answer to a [`LogQuery`].
#[derive(Debug, Clone, PartialEq)]
pub struct LogPage {
    /// The newest matches first, at most `limit` of them.
    pub lines: Vec<Value>,
    /// How many lines in the scanned window matched every filter.
    pub matched: usize,
    /// How many lines the window held.
    pub scanned: usize,
    /// `true` when the window held every line the index filters admit, so
    /// `matched` is the whole count.
    pub exhaustive: bool,
    /// The effective page size after clamping.
    pub limit: usize,
}

pub async fn query_log_lines_async(engine: &Arc<Engine>, query: LogQuery<'_>) -> Result<LogPage> {
    let limit = query.limit.clamp(1, LOG_PAGE_LIMIT);
    let mut filters = Vec::new();
    if let Some(tenant_id) = query.tenant_id {
        filters.push(eq("tenantId", json!(tenant_id.as_str())));
    }
    if let Some(run_id) = query.run_id {
        filters.push(eq("correlationId", json!(run_id)));
    }
    if let Some(level) = query.level {
        filters.push(eq("level", json!(level)));
    }
    if let Some(source) = query.source {
        filters.push(eq("source", json!(source)));
    }
    if let Some(category) = query.category {
        filters.push(eq("category", json!(category)));
    }
    let documents = engine
        .query_documents_async(
            system_tenant_id()?,
            Query {
                table: SystemTable::Events.table_name()?,
                filters,
                order: Some(OrderBy {
                    field: "createdAt".to_owned(),
                    direction: OrderDirection::Desc,
                }),
                limit: Some(LOG_SCAN_WINDOW),
            },
        )
        .await?;
    let scanned = documents.len();
    let exhaustive = scanned < LOG_SCAN_WINDOW;
    let needle = query
        .text
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_lowercase);
    let matches: Vec<Value> = documents
        .iter()
        .filter(|document| match &needle {
            Some(needle) => document
                .fields
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.to_lowercase().contains(needle.as_str())),
            None => true,
        })
        .map(|document| document.to_json())
        .collect();
    let matched = matches.len();
    Ok(LogPage {
        lines: matches.into_iter().take(limit).collect(),
        matched,
        scanned,
        exhaustive,
        limit,
    })
}

fn eq(field: &str, value: Value) -> Filter {
    Filter {
        field: field.to_owned(),
        op: FilterOp::Eq,
        value,
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::TenantId;
    use nimbus_engine::Engine;
    use nimbus_testing::EngineFixture;
    use serde_json::{Value, json};

    use super::*;
    use crate::records::{RunRecord, SystemEvent, record_run_async, record_system_event_async};

    async fn write_line(
        engine: &Arc<Engine>,
        tenant: Option<&TenantId>,
        level: &str,
        message: &str,
        run_id: Option<&str>,
    ) {
        record_system_event_async(
            engine,
            SystemEvent {
                tenant_id: tenant,
                source: "runtime",
                level,
                category: "function",
                message,
                data: json!({}),
                correlation_id: run_id,
            },
        )
        .await
        .expect("event should record");
    }

    fn messages(page: &LogPage) -> Vec<String> {
        let mut messages: Vec<String> = page
            .lines
            .iter()
            .filter_map(|line| line.get("message").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        messages.sort();
        messages
    }

    async fn read(engine: &Arc<Engine>, query: LogQuery<'_>) -> LogPage {
        query_log_lines_async(engine, query)
            .await
            .expect("log query should answer")
    }

    const ALL: LogQuery<'static> = LogQuery {
        tenant_id: None,
        run_id: None,
        level: None,
        source: None,
        category: None,
        text: None,
        limit: 50,
    };

    #[tokio::test]
    async fn log_query_scopes_lines_to_a_tenant_and_searches_text() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let acme = TenantId::new("acme").expect("tenant id should parse");
        let beta = TenantId::new("beta").expect("tenant id should parse");

        write_line(
            &engine,
            Some(&acme),
            "info",
            "payment accepted for order 7",
            Some("run-a"),
        )
        .await;
        write_line(
            &engine,
            Some(&acme),
            "error",
            "cache miss on user 3",
            Some("run-a"),
        )
        .await;
        write_line(
            &engine,
            Some(&beta),
            "info",
            "Payment refused: card expired",
            None,
        )
        .await;
        write_line(&engine, None, "info", "server shutdown requested", None).await;

        // The tenant scope is the tenant's own lines: not the other tenant's
        // and not the server's.
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&acme),
                ..ALL
            },
        )
        .await;
        assert_eq!(
            messages(&page),
            vec!["cache miss on user 3", "payment accepted for order 7"]
        );
        assert_eq!((page.matched, page.scanned, page.exhaustive), (2, 2, true));
        assert!(
            page.lines
                .iter()
                .all(|line| line["tenantId"] == json!("acme"))
        );

        // Text is a case-insensitive substring of the message, across every
        // tenant when no scope is set.
        let page = read(
            &engine,
            LogQuery {
                text: Some("payment"),
                ..ALL
            },
        )
        .await;
        assert_eq!(
            messages(&page),
            vec![
                "Payment refused: card expired",
                "payment accepted for order 7"
            ]
        );
        assert_eq!(page.matched, 2);

        // Text and tenant narrow the same window.
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&beta),
                text: Some("PAYMENT"),
                ..ALL
            },
        )
        .await;
        assert_eq!(messages(&page), vec!["Payment refused: card expired"]);
        assert_eq!(page.matched, 1);

        // A run id is the line's correlation id, inside the tenant scope.
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&acme),
                run_id: Some("run-a"),
                ..ALL
            },
        )
        .await;
        assert_eq!(page.matched, 2);
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&beta),
                run_id: Some("run-a"),
                ..ALL
            },
        )
        .await;
        assert_eq!((page.matched, page.lines.len()), (0, 0));

        // Level is an exact match.
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&acme),
                level: Some("error"),
                ..ALL
            },
        )
        .await;
        assert_eq!(messages(&page), vec!["cache miss on user 3"]);

        // The page is bounded; the count is not.
        let page = read(
            &engine,
            LogQuery {
                tenant_id: Some(&acme),
                limit: 1,
                ..ALL
            },
        )
        .await;
        assert_eq!((page.lines.len(), page.matched, page.limit), (1, 2, 1));
        let page = read(
            &engine,
            LogQuery {
                limit: 10_000,
                ..ALL
            },
        )
        .await;
        assert_eq!(page.limit, LOG_PAGE_LIMIT);
    }

    #[tokio::test]
    async fn run_rows_record_their_tenant() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let acme = TenantId::new("acme").expect("tenant id should parse");
        let beta = TenantId::new("beta").expect("tenant id should parse");
        for (tenant, function_path) in [(&acme, "messages:send"), (&beta, "orders:place")] {
            record_run_async(
                &engine,
                RunRecord {
                    tenant_id: tenant,
                    function_path,
                    kind: "mutation",
                    started_at: 1_700_000_000_000,
                    duration_ms: 2.5,
                    status: "ok",
                    error: None,
                },
            )
            .await
            .expect("run should record");
        }

        let runs = engine
            .query_documents_async(
                system_tenant_id().expect("system tenant id"),
                Query {
                    table: SystemTable::Runs.table_name().expect("runs table"),
                    filters: vec![eq("tenantId", json!("acme"))],
                    order: None,
                    limit: None,
                },
            )
            .await
            .expect("runs should query");
        let paths: Vec<&str> = runs
            .iter()
            .filter_map(|run| run.fields.get("functionPath").and_then(Value::as_str))
            .collect();
        assert_eq!(paths, vec!["messages:send"]);
    }
}
