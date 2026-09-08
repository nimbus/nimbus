use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_engine::Engine;
use serde_json::{Value, json};

use crate::identity::{is_system_tenant_id, system_tenant_id};
use crate::schema::SystemTable;

use super::errors::{error_class, error_fingerprint};
use super::trace::RunSpan;
use super::{ensure_system_tenant_async, object_fields};

pub struct RunRecord<'a> {
    pub tenant_id: &'a TenantId,
    pub function_path: &'a str,
    pub kind: &'a str,
    pub started_at: u64,
    pub duration_ms: f64,
    pub status: &'a str,
    pub error: Option<RunError<'a>>,
    /// The run's spans, the function's own span first. Empty when the
    /// caller recorded none.
    pub spans: Vec<RunSpan>,
}

/// The failure a run row stores: the message the reader sees, the stable
/// class the fingerprint folds on, and the stack when the function's own
/// code threw.
pub struct RunError<'a> {
    pub message: &'a str,
    pub class: &'static str,
    pub stack: Option<&'a str>,
}

impl<'a> RunError<'a> {
    /// A thrown function error keeps its own message (with the remapped
    /// `(at module:line)` suffix) and stack; every other error stores its
    /// display text.
    pub fn from_core_error(error: &'a nimbus_core::Error, display: &'a str) -> Self {
        let class = error_class(error);
        match error {
            nimbus_core::Error::FunctionThrown { message, stack, .. } => Self {
                message,
                class,
                stack: stack.as_deref(),
            },
            _ => Self {
                message: display,
                class,
                stack: None,
            },
        }
    }
}

pub async fn record_run_async(engine: &Arc<Engine>, record: RunRecord<'_>) -> Result<()> {
    if is_system_tenant_id(record.tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    let mut fields = object_fields(json!({
        "tenantId": record.tenant_id.as_str(),
        "functionPath": record.function_path,
        "kind": record.kind,
        "durationMs": record.duration_ms,
        "status": record.status,
        "startedAt": record.started_at,
    }));
    if let Some(error) = record.error {
        let mut error_value = json!({ "message": error.message, "class": error.class });
        if let Some(map) = error_value.as_object_mut() {
            if let Some(location) = extract_error_location(error.message) {
                map.insert("location".to_owned(), json!(location));
            }
            if let Some(stack) = error.stack {
                map.insert("stack".to_owned(), json!(stack));
            }
        }
        fields.insert("error".to_owned(), error_value);
        fields.insert(
            "fingerprint".to_owned(),
            json!(error_fingerprint(
                record.function_path,
                error.class,
                error.message
            )),
        );
    }
    if !record.spans.is_empty() {
        fields.insert(
            "spans".to_owned(),
            Value::Array(record.spans.iter().map(RunSpan::to_json).collect()),
        );
    }
    engine
        .insert_document_async(system_tenant_id()?, SystemTable::Runs.table_name()?, fields)
        .await?;
    Ok(())
}

/// Lift a `module:line` source location out of a remapped runtime-handler error.
///
/// The runtime remap (codegen `emit/runtime_remap.mjs`) appends ` (at module:line)`
/// to the thrown message so a failed run names the developer's own source line.
/// Storing that location as a structured field lets the console link the failure
/// straight to its source line instead of forcing the reader to parse the
/// message string. Returns `None` for messages without a well-formed location.
fn extract_error_location(message: &str) -> Option<&str> {
    let after = message.find("(at ")? + "(at ".len();
    let rest = &message[after..];
    let close = rest.find(')')?;
    let location = &rest[..close];
    let (module, line) = location.rsplit_once(':')?;
    if module.is_empty() || line.is_empty() || !line.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(location)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_error_keeps_the_thrown_message_and_stack() {
        let thrown = nimbus_core::Error::function_thrown(
            "messages:send",
            "Message text must not be empty (at messages:12)",
            Some("Error: Message text must not be empty\n    at handler".to_string()),
        );
        let display = thrown.to_string();
        let error = RunError::from_core_error(&thrown, &display);
        assert_eq!(
            error.message,
            "Message text must not be empty (at messages:12)"
        );
        assert_eq!(extract_error_location(error.message), Some("messages:12"));
        assert_eq!(error.class, "function_thrown");
        assert!(
            error
                .stack
                .is_some_and(|stack| stack.contains("at handler"))
        );

        let internal = nimbus_core::Error::Internal("boom".to_string());
        let display = internal.to_string();
        let error = RunError::from_core_error(&internal, &display);
        assert_eq!(error.message, "internal error: boom");
        assert_eq!(error.stack, None);
        assert_eq!(error.class, "internal");
    }

    #[test]
    fn extract_error_location_lifts_remapped_source_location() {
        // The runtime remap appends ` (at module:line)` to the thrown message.
        let message = "runtime JavaScript error: Error: message body must not be empty (at messages:24)\n    at eval";
        assert_eq!(extract_error_location(message), Some("messages:24"));

        // Nested module paths (admin/users) and the first `(at ...)` win.
        assert_eq!(
            extract_error_location("Error: nope (at admin/users:7)"),
            Some("admin/users:7"),
        );
    }

    #[test]
    fn extract_error_location_returns_none_without_a_wellformed_location() {
        assert_eq!(extract_error_location("plain error, no location"), None);
        // Missing line number / malformed are rejected, not stored as garbage.
        assert_eq!(extract_error_location("boom (at messages)"), None);
        assert_eq!(extract_error_location("boom (at messages:abc)"), None);
        assert_eq!(extract_error_location("boom (at :24)"), None);
    }
}
