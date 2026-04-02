use neovex_core::{DocumentId, Error, Query};
use neovex_engine::encode_cursor;
use serde_json::Value;

pub(in crate::adapters::convex) fn synthesize_runtime_paginate_cursor(
    query: &Query,
    page_size: usize,
    page: &mut neovex_core::Page,
) -> Result<(), Error> {
    if page.next_cursor.is_some() || page.data.is_empty() || page.data.len() != page_size {
        return Ok(());
    }

    let Some((sort_value, document_id)) = page
        .data
        .last()
        .and_then(|value| extract_runtime_paginate_boundary(query, value))
    else {
        return Ok(());
    };

    page.next_cursor = Some(encode_cursor(sort_value.as_ref(), &document_id, query)?);
    Ok(())
}

fn extract_runtime_paginate_boundary(
    query: &Query,
    value: &Value,
) -> Option<(Option<Value>, DocumentId)> {
    let Value::Object(object) = value else {
        return None;
    };
    let document_id = object
        .get("_id")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())?;
    let sort_value = query
        .order
        .as_ref()
        .and_then(|order| object.get(&order.field).cloned());
    Some((sort_value, document_id))
}
