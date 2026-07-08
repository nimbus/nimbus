use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_engine::Engine;
use serde_json::json;

use crate::identity::{is_system_tenant_id, system_tenant_id};
use crate::schema::SystemTable;

use super::{ensure_system_tenant_async, object_fields};

pub struct RunRecord<'a> {
    pub tenant_id: &'a TenantId,
    pub function_path: &'a str,
    pub kind: &'a str,
    pub started_at: u64,
    pub duration_ms: f64,
    pub status: &'a str,
    pub error: Option<&'a str>,
}

pub async fn record_run_async(engine: &Arc<Engine>, record: RunRecord<'_>) -> Result<()> {
    if is_system_tenant_id(record.tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    let mut fields = object_fields(json!({
        "functionPath": record.function_path,
        "kind": record.kind,
        "durationMs": record.duration_ms,
        "status": record.status,
        "startedAt": record.started_at,
    }));
    if let Some(error) = record.error {
        let mut error_value = json!({ "message": error });
        if let (Some(location), Some(map)) =
            (extract_error_location(error), error_value.as_object_mut())
        {
            map.insert("location".to_owned(), json!(location));
        }
        fields.insert("error".to_owned(), error_value);
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
