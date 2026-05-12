use nimbus_core::{DocumentId, Error, Query};
use nimbus_engine::encode_cursor;
use serde_json::Value;

pub(in crate::adapters::convex) fn synthesize_runtime_paginate_cursor(
    query: &Query,
    page_size: usize,
    page: &mut nimbus_core::Page,
) -> Result<(), Error> {
    // Convex runtime paginate() responses expose JSON documents with `_id`
    // fields, so this adapter synthesizes the opaque cursor from that payload
    // shape. Firestore and other transports should keep using the shared query
    // cursor metadata instead of depending on Convex's JSON envelope.
    if page.next_cursor.is_some() || page.data.is_empty() || page.data.len() != page_size {
        return Ok(());
    }

    let Some((sort_values, document_id)) = page
        .data
        .last()
        .and_then(|value| extract_runtime_paginate_boundary(query, value))
    else {
        return Ok(());
    };

    page.next_cursor = Some(encode_cursor(&sort_values, &document_id, query)?);
    Ok(())
}

fn extract_runtime_paginate_boundary(
    query: &Query,
    value: &Value,
) -> Option<(Vec<Option<Value>>, DocumentId)> {
    let Value::Object(object) = value else {
        return None;
    };
    let document_id = object
        .get("_id")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())?;
    let sort_values = match query.order.as_ref() {
        Some(order) => vec![object.get(&order.field).cloned()],
        None => Vec::new(),
    };
    Some((sort_values, document_id))
}
