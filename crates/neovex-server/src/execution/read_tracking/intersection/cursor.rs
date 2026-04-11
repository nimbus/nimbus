use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use neovex_core::{Cursor, DocumentId, OrderBy};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct RuntimeCursorBoundaryPayload {
    sort_values: Vec<Option<Value>>,
    doc_id: String,
}

pub(in crate::execution::read_tracking) fn decode_runtime_cursor_boundary(
    cursor: &Cursor,
) -> Option<(Vec<Option<Value>>, DocumentId)> {
    let bytes = URL_SAFE_NO_PAD.decode(&cursor.0).ok()?;
    let payload: RuntimeCursorBoundaryPayload = serde_json::from_slice(&bytes).ok()?;
    let document_id = payload.doc_id.parse().ok()?;
    Some((payload.sort_values, document_id))
}

pub(in crate::execution::read_tracking) fn extract_runtime_cursor_boundary(
    order: Option<&OrderBy>,
    value: &Value,
) -> Option<(Vec<Option<Value>>, DocumentId)> {
    let Value::Object(object) = value else {
        return None;
    };
    let document_id = object
        .get("_id")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())?;
    let sort_values = match order {
        Some(order) => vec![object.get(&order.field).cloned()],
        None => Vec::new(),
    };
    Some((sort_values, document_id))
}
