use std::cmp::Ordering;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use neovex_core::{Cursor, Document, DocumentId, Error, OrderBy, OrderDirection, Query, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ordering::compare_order_field;

#[derive(Debug, Serialize, Deserialize)]
struct CursorPayload {
    query_signature: String,
    sort_values: Vec<Option<Value>>,
    doc_id: String,
}

/// Encodes a pagination cursor.
pub fn encode_cursor(
    sort_values: &[Option<Value>],
    doc_id: &DocumentId,
    query: &Query,
) -> Result<Cursor> {
    let payload = CursorPayload {
        query_signature: query_signature(query)?,
        sort_values: sort_values.to_vec(),
        doc_id: doc_id.to_string(),
    };
    let json = serde_json::to_vec(&payload)
        .map_err(|error| Error::Internal(format!("cursor should serialize: {error}")))?;
    Ok(Cursor(URL_SAFE_NO_PAD.encode(&json)))
}

/// Decodes a pagination cursor.
pub fn decode_cursor(cursor: &Cursor, query: &Query) -> Result<(Vec<Option<Value>>, DocumentId)> {
    let bytes = URL_SAFE_NO_PAD
        .decode(&cursor.0)
        .map_err(|_| Error::InvalidInput("invalid cursor".to_string()))?;
    let payload: CursorPayload = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("invalid cursor".to_string()))?;
    if payload.query_signature != query_signature(query)? {
        return Err(Error::InvalidInput("invalid cursor".to_string()));
    }
    if payload.sort_values.len() != expected_cursor_sort_value_count(query) {
        return Err(Error::InvalidInput("invalid cursor".to_string()));
    }
    let doc_id = payload
        .doc_id
        .parse::<DocumentId>()
        .map_err(|_| Error::InvalidInput("invalid cursor document id".to_string()))?;
    Ok((payload.sort_values, doc_id))
}

pub(super) fn compare_document_to_cursor(
    document: &Document,
    order: Option<&OrderBy>,
    cursor_sort_values: &[Option<Value>],
    cursor_doc_id: &DocumentId,
) -> Result<Ordering> {
    let ordering = match order {
        Some(order) => {
            let boundary_value = cursor_sort_values.first().and_then(Option::as_ref);
            let ordering = compare_order_field(document.get_field(&order.field), boundary_value)?;
            match order.direction {
                OrderDirection::Asc => ordering,
                OrderDirection::Desc => ordering.reverse(),
            }
        }
        None if !cursor_sort_values.is_empty() => {
            return Err(Error::InvalidInput("invalid cursor".to_string()));
        }
        None => Ordering::Equal,
    };

    Ok(ordering.then_with(|| document.id.cmp(cursor_doc_id)))
}

pub(super) fn cursor_sort_values_for_document(
    order: Option<&OrderBy>,
    document: &Document,
) -> Vec<Option<Value>> {
    match order {
        Some(order) => vec![document.get_field(&order.field).cloned()],
        None => Vec::new(),
    }
}

fn query_signature(query: &Query) -> Result<String> {
    let mut normalized = query.clone();
    normalized.limit = None;
    let bytes = serde_json::to_vec(&normalized).map_err(|error| {
        Error::Internal(format!("query signature serialization failed: {error}"))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn expected_cursor_sort_value_count(query: &Query) -> usize {
    usize::from(query.order.is_some())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use neovex_core::{OrderBy, TableName};

    fn tasks_query_with_rank_order() -> Query {
        Query {
            table: TableName::new("tasks").expect("table should be valid"),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        }
    }

    #[test]
    fn cursor_roundtrips_tuple_capable_sort_values() {
        let query = tasks_query_with_rank_order();
        let document_id = DocumentId::new();
        let cursor =
            encode_cursor(&[Some(json!(7))], &document_id, &query).expect("cursor should encode");

        let (sort_values, decoded_document_id) =
            decode_cursor(&cursor, &query).expect("cursor should decode");

        assert_eq!(sort_values, vec![Some(json!(7))]);
        assert_eq!(decoded_document_id, document_id);
    }

    #[test]
    fn cursor_rejects_wrong_tuple_width_for_query_shape() {
        let query = tasks_query_with_rank_order();
        let document_id = DocumentId::new();
        let cursor = Cursor(
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&CursorPayload {
                    query_signature: query_signature(&query)
                        .expect("query signature should serialize"),
                    sort_values: Vec::new(),
                    doc_id: document_id.to_string(),
                })
                .expect("cursor payload should serialize"),
            ),
        );

        let error = decode_cursor(&cursor, &query).expect_err("cursor should be rejected");
        assert!(matches!(error, Error::InvalidInput(_)));
    }
}
