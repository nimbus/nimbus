//! Error fingerprints and error groups.
//!
//! A fingerprint names one failure identity: the function path, the error
//! class, and the normalized message. Two runs that throw the same error
//! share a fingerprint, so the console can fold them into one group with a
//! count, a first-seen time, and a last-seen time.

use std::collections::HashMap;
use std::sync::Arc;

use nimbus_core::{Error, Filter, FilterOp, OrderBy, OrderDirection, Query, Result, TenantId};
use nimbus_engine::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::identity::system_tenant_id;
use crate::schema::SystemTable;

/// The most groups one answer carries. A larger `limit` is clamped to it.
pub const ERROR_GROUP_LIMIT: usize = 200;

/// How many of the newest failed runs a group query scans. The tenant and
/// status filters are index reads; the grouping runs over this window, so
/// a count is exact only when the window held every failed run.
pub const ERROR_SCAN_WINDOW: usize = 2_000;

/// The stable class name of an error: the commit vocabulary when the error
/// is a commit outcome, else the variant name. The class is one of the three
/// fingerprint inputs, so it must not carry per-occurrence detail.
pub fn error_class(error: &Error) -> &'static str {
    match error {
        Error::Cancelled => "cancelled",
        Error::RuntimeTimeout { .. } => "runtime_timeout",
        Error::RuntimePromiseStalled => "runtime_promise_stalled",
        Error::FunctionThrown { .. } => "function_thrown",
        Error::TenantNotFound(_) => "tenant_not_found",
        Error::DocumentNotFound(_) => "document_not_found",
        Error::ScheduledJobNotFound(_) => "scheduled_job_not_found",
        Error::NotFound(_) => "not_found",
        Error::AlreadyExists(_) => "already_exists",
        Error::ResourceExhausted(_) => "resource_exhausted",
        Error::PermissionDenied(_) => "permission_denied",
        Error::Conflict { .. } => "conflict",
        Error::Overloaded { .. } => "overloaded",
        Error::CommitterFull { .. } => "committer_full",
        Error::CommitterFenced { .. } => "committer_fenced",
        Error::RejectedBeforeExecution { .. } => "rejected_before_execution",
        Error::RateLimited { .. } => "rate_limited",
        Error::OutOfRetention { .. } => "out_of_retention",
        Error::CapExceeded { .. } => "cap_exceeded",
        Error::PreconditionFailed(_) => "precondition_failed",
        Error::InvalidInput(_) => "invalid_input",
        Error::MissingIndex { .. } => "missing_index",
        Error::SchemaValidation(_) => "schema_validation",
        Error::SchemaNotFound(_) => "schema_not_found",
        Error::Storage { .. } => "storage",
        Error::HistoricalRead { .. } => "historical_read",
        Error::Serialization(_) => "serialization",
        Error::Transport(_) => "transport",
        Error::Internal(_) => "internal",
    }
}

/// The message with its per-occurrence detail removed: the first line only,
/// runs of digits folded to `#`, whitespace collapsed. The ` (at module:line)`
/// suffix the runtime remap appends is kept verbatim, because the source
/// line is part of the failure's identity, not noise.
pub fn normalize_error_message(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or_default().trim();
    let (body, location) = match first_line.rfind(" (at ") {
        Some(at) if first_line.ends_with(')') => first_line.split_at(at),
        _ => (first_line, ""),
    };
    let mut normalized = String::with_capacity(body.len());
    let mut last_was_digit = false;
    let mut last_was_space = false;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            if !last_was_digit {
                normalized.push('#');
            }
            last_was_digit = true;
            last_was_space = false;
        } else if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
            }
            last_was_space = true;
            last_was_digit = false;
        } else {
            normalized.push(ch);
            last_was_digit = false;
            last_was_space = false;
        }
    }
    let mut normalized: String = normalized.trim().chars().take(240).collect();
    normalized.push_str(location);
    normalized
}

/// The fingerprint: sixteen hex characters of SHA-256 over the function
/// path, the error class, and the normalized message.
pub fn error_fingerprint(function_path: &str, class: &str, message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(function_path.as_bytes());
    hasher.update([0]);
    hasher.update(class.as_bytes());
    hasher.update([0]);
    hasher.update(normalize_error_message(message).as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One error group read.
#[derive(Debug, Clone, Copy)]
pub struct ErrorGroupQuery<'a> {
    /// The tenant the runs belong to, or `None` for every tenant.
    pub tenant_id: Option<&'a TenantId>,
    /// The most groups to answer with, clamped to `1..=ERROR_GROUP_LIMIT`.
    pub limit: usize,
}

/// One failure identity and the runs that share it.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorGroup {
    pub fingerprint: String,
    pub tenant_id: String,
    pub function_path: String,
    pub kind: String,
    pub class: String,
    /// The message of the newest run in the group.
    pub message: String,
    /// The lifted `module:line` of the newest run, when it had one.
    pub location: Option<String>,
    pub count: usize,
    pub first_seen: u64,
    pub last_seen: u64,
    /// The newest run in the group, for the drill-in.
    pub latest_run_id: String,
}

impl ErrorGroup {
    pub fn to_json(&self) -> Value {
        json!({
            "fingerprint": self.fingerprint,
            "tenantId": self.tenant_id,
            "functionPath": self.function_path,
            "kind": self.kind,
            "class": self.class,
            "message": self.message,
            "location": self.location,
            "count": self.count,
            "firstSeen": self.first_seen,
            "lastSeen": self.last_seen,
            "latestRunId": self.latest_run_id,
        })
    }
}

/// The answer to an [`ErrorGroupQuery`].
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorGroupPage {
    /// The groups, newest last-seen first, at most `limit` of them.
    pub groups: Vec<ErrorGroup>,
    /// How many failed runs the window held.
    pub scanned: usize,
    /// `true` when the window held every failed run the filters admit, so
    /// every count is the whole count.
    pub exhaustive: bool,
    pub limit: usize,
}

pub async fn query_error_groups_async(
    engine: &Arc<Engine>,
    query: ErrorGroupQuery<'_>,
) -> Result<ErrorGroupPage> {
    let limit = query.limit.clamp(1, ERROR_GROUP_LIMIT);
    let mut filters = vec![Filter {
        field: "status".to_owned(),
        op: FilterOp::Eq,
        value: json!("error"),
    }];
    if let Some(tenant_id) = query.tenant_id {
        filters.push(Filter {
            field: "tenantId".to_owned(),
            op: FilterOp::Eq,
            value: json!(tenant_id.as_str()),
        });
    }
    let documents = engine
        .query_documents_async(
            system_tenant_id()?,
            Query {
                table: SystemTable::Runs.table_name()?,
                filters,
                order: Some(OrderBy {
                    field: "startedAt".to_owned(),
                    direction: OrderDirection::Desc,
                }),
                limit: Some(ERROR_SCAN_WINDOW),
            },
        )
        .await?;
    let scanned = documents.len();
    let exhaustive = scanned < ERROR_SCAN_WINDOW;

    // Newest first, so the first run seen for a fingerprint is the group's
    // sample and its `last_seen`; every later run only moves `first_seen`
    // back and the count up.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, ErrorGroup> = HashMap::new();
    for document in &documents {
        let fields = &document.fields;
        let Some(fingerprint) = fields.get("fingerprint").and_then(Value::as_str) else {
            continue;
        };
        let started_at = fields
            .get("startedAt")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        match groups.get_mut(fingerprint) {
            Some(group) => {
                group.count += 1;
                group.first_seen = group.first_seen.min(started_at);
                group.last_seen = group.last_seen.max(started_at);
            }
            None => {
                let text = |key: &str| {
                    fields
                        .get(key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                let error = fields.get("error");
                let error_text = |key: &str| {
                    error
                        .and_then(|error| error.get(key))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                order.push(fingerprint.to_owned());
                groups.insert(
                    fingerprint.to_owned(),
                    ErrorGroup {
                        fingerprint: fingerprint.to_owned(),
                        tenant_id: text("tenantId"),
                        function_path: text("functionPath"),
                        kind: text("kind"),
                        class: error_text("class").unwrap_or_default(),
                        message: error_text("message").unwrap_or_default(),
                        location: error_text("location"),
                        count: 1,
                        first_seen: started_at,
                        last_seen: started_at,
                        latest_run_id: document.id.to_string(),
                    },
                );
            }
        }
    }
    let groups = order
        .into_iter()
        .take(limit)
        .filter_map(|fingerprint| groups.remove(&fingerprint))
        .collect();
    Ok(ErrorGroupPage {
        groups,
        scanned,
        exhaustive,
        limit,
    })
}

#[cfg(test)]
mod tests {
    use nimbus_testing::EngineFixture;

    use super::*;
    use crate::records::{RunError, RunRecord, record_run_async};

    #[test]
    fn fingerprint_is_stable_across_two_identical_throws() {
        let first = nimbus_core::Error::function_thrown(
            "messages:send",
            "Message text must not be empty (at messages:12)",
            Some("Error: Message text must not be empty\n    at handler (<anonymous>:5:11)".into()),
        );
        let second = nimbus_core::Error::function_thrown(
            "messages:send",
            "Message text must not be empty (at messages:12)",
            Some("Error: Message text must not be empty\n    at handler (<anonymous>:9:3)".into()),
        );
        let a = error_fingerprint(
            "messages:send",
            error_class(&first),
            "Message text must not be empty (at messages:12)",
        );
        let b = error_fingerprint(
            "messages:send",
            error_class(&second),
            "Message text must not be empty (at messages:12)",
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_separates_function_class_and_message() {
        let base = error_fingerprint("messages:send", "function_thrown", "boom (at messages:12)");
        assert_ne!(
            base,
            error_fingerprint("messages:list", "function_thrown", "boom (at messages:12)")
        );
        assert_ne!(
            base,
            error_fingerprint("messages:send", "invalid_input", "boom (at messages:12)")
        );
        assert_ne!(
            base,
            error_fingerprint("messages:send", "function_thrown", "boom (at messages:13)")
        );
    }

    #[test]
    fn normalization_folds_occurrence_detail_and_keeps_the_location() {
        assert_eq!(
            normalize_error_message(
                "order 1234 not found for user 9  (at orders:7)\n    at handler"
            ),
            "order # not found for user # (at orders:7)"
        );
        assert_eq!(normalize_error_message("  spaced   out  "), "spaced out");
        assert_eq!(
            error_fingerprint("orders:get", "not_found", "order 1 not found"),
            error_fingerprint("orders:get", "not_found", "order 22 not found\n    at x")
        );
    }

    #[test]
    fn error_class_uses_the_variant_name() {
        assert_eq!(
            error_class(&nimbus_core::Error::function_thrown("f", "m", None)),
            "function_thrown"
        );
        assert_eq!(
            error_class(&nimbus_core::Error::InvalidInput("x".into())),
            "invalid_input"
        );
    }

    async fn write_run(
        engine: &Arc<Engine>,
        tenant: &TenantId,
        function_path: &str,
        started_at: u64,
        error: Option<&nimbus_core::Error>,
    ) {
        let display = error.map(ToString::to_string);
        let error = error
            .zip(display.as_deref())
            .map(|(error, display)| RunError::from_core_error(error, display));
        record_run_async(
            engine,
            RunRecord {
                tenant_id: tenant,
                function_path,
                kind: "mutation",
                started_at,
                duration_ms: 3.0,
                status: if error.is_some() { "error" } else { "ok" },
                error,
                spans: Vec::new(),
            },
        )
        .await
        .expect("run should record");
    }

    #[tokio::test]
    async fn error_groups_fold_runs_by_fingerprint_and_scope_to_a_tenant() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let acme = TenantId::new("acme").expect("tenant id should parse");
        let beta = TenantId::new("beta").expect("tenant id should parse");
        let empty = nimbus_core::Error::function_thrown(
            "messages:send",
            "Message text must not be empty (at messages:12)",
            None,
        );
        let missing = nimbus_core::Error::function_thrown(
            "orders:get",
            "order 41 not found (at orders:7)",
            None,
        );
        let missing_again = nimbus_core::Error::function_thrown(
            "orders:get",
            "order 42 not found (at orders:7)",
            None,
        );

        write_run(&engine, &acme, "messages:send", 1_000, Some(&empty)).await;
        write_run(&engine, &acme, "messages:send", 3_000, Some(&empty)).await;
        write_run(&engine, &acme, "orders:get", 2_000, Some(&missing)).await;
        write_run(&engine, &acme, "orders:get", 2_500, Some(&missing_again)).await;
        write_run(&engine, &acme, "messages:list", 4_000, None).await;
        write_run(&engine, &beta, "messages:send", 5_000, Some(&empty)).await;

        let page = query_error_groups_async(
            &engine,
            ErrorGroupQuery {
                tenant_id: Some(&acme),
                limit: 50,
            },
        )
        .await
        .expect("group query should answer");
        assert_eq!(page.scanned, 4);
        assert!(page.exhaustive);
        assert_eq!(page.groups.len(), 2);

        let send = &page.groups[0];
        assert_eq!(send.function_path, "messages:send");
        assert_eq!(send.class, "function_thrown");
        assert_eq!(send.count, 2);
        assert_eq!(send.first_seen, 1_000);
        assert_eq!(send.last_seen, 3_000);
        assert_eq!(send.location.as_deref(), Some("messages:12"));
        assert_eq!(send.tenant_id, "acme");

        let get = &page.groups[1];
        assert_eq!(get.function_path, "orders:get");
        assert_eq!(get.count, 2, "digits fold, so the two ids share one group");
        assert_eq!(get.message, "order 42 not found (at orders:7)");
        assert_eq!(get.first_seen, 2_000);
        assert_eq!(get.last_seen, 2_500);

        let all = query_error_groups_async(
            &engine,
            ErrorGroupQuery {
                tenant_id: None,
                limit: 1,
            },
        )
        .await
        .expect("group query should answer");
        assert_eq!(all.scanned, 5);
        assert_eq!(all.groups.len(), 1, "the limit clamps the answer");
        assert_eq!(all.groups[0].tenant_id, "beta");
        assert_eq!(all.groups[0].last_seen, 5_000);
    }
}
