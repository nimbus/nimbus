use std::cmp::Ordering;
use std::collections::HashSet;

use neovex_core::{
    CompositeOperator, Document, DocumentPath, Error, FieldFilterOperator, OrderDirection, Query,
    ResourcePathBinding, Result, TableName, UnaryFilterOperator,
};
use serde_json::Value;

use super::{
    CollectionGroupTableTarget, PreparedCursor, PreparedField, PreparedFilter, PreparedOrder,
    PreparedStructuredQuery, ProjectionMode, StructuredDocumentRow,
};

fn compare_structured_order_values(
    left: Option<&Value>,
    right: Option<&Value>,
) -> Result<Ordering> {
    match (left, right) {
        (Some(left), Some(right)) => match (left, right) {
            (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
            (Value::Number(left), Value::Number(right)) => {
                let left = left.as_f64().ok_or_else(|| {
                    Error::InvalidInput("unsupported numeric comparison".to_string())
                })?;
                let right = right.as_f64().ok_or_else(|| {
                    Error::InvalidInput("unsupported numeric comparison".to_string())
                })?;
                left.partial_cmp(&right).ok_or_else(|| {
                    Error::InvalidInput("invalid numeric ordering comparison".to_string())
                })
            }
            _ => Err(Error::InvalidInput(
                "comparisons only support string and number fields in phase 1".to_string(),
            )),
        },
        (Some(_), None) => Ok(Ordering::Less),
        (None, Some(_)) => Ok(Ordering::Greater),
        (None, None) => Ok(Ordering::Equal),
    }
}

fn validate_structured_order_domains<'a>(
    documents: impl Iterator<Item = &'a Document>,
    order_by: &[PreparedOrder],
) -> Result<()> {
    if order_by.is_empty() {
        return Ok(());
    }

    let mut observed_kinds = vec![None; order_by.len()];
    for document in documents {
        for (index, order) in order_by.iter().enumerate() {
            let Some(field) = order.field.user_field() else {
                observed_kinds[index] = Some(StructuredOrderValueKind::String);
                continue;
            };
            let Some(value) = document.get_field(field) else {
                continue;
            };
            let kind = match value {
                Value::String(_) => StructuredOrderValueKind::String,
                Value::Number(number) if number.as_f64().is_some() => {
                    StructuredOrderValueKind::Number
                }
                _ => {
                    return Err(Error::InvalidInput(
                        "ordering only supports string and number fields in phase 1".to_string(),
                    ));
                }
            };

            if let Some(previous) = observed_kinds[index] {
                if previous != kind {
                    return Err(Error::InvalidInput(
                        "ordering cannot mix string and number values in the same field"
                            .to_string(),
                    ));
                }
            } else {
                observed_kinds[index] = Some(kind);
            }
        }
    }

    Ok(())
}

fn sort_documents_for_structured_query(
    documents: &mut [StructuredDocumentRow],
    order_by: &[PreparedOrder],
) -> Result<()> {
    if order_by.is_empty() {
        documents.sort_by(|left, right| left.document_name.cmp(&right.document_name));
        return Ok(());
    }

    validate_structured_order_domains(documents.iter().map(|row| &row.document), order_by)?;
    documents.sort_by(|left, right| {
        for order in order_by {
            let ordering = match &order.field {
                PreparedField::UserField(field) => compare_structured_order_values(
                    left.document.get_field(field),
                    right.document.get_field(field),
                ),
                PreparedField::DocumentName => Ok(left.document_name.cmp(&right.document_name)),
            }
            .expect("ordering inputs should be validated before sorting");
            let ordering = match order.direction {
                OrderDirection::Asc => ordering,
                OrderDirection::Desc => ordering.reverse(),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }

        Ordering::Equal
    });
    Ok(())
}

fn compare_document_to_structured_cursor(
    document: &StructuredDocumentRow,
    order_by: &[PreparedOrder],
    cursor: &PreparedCursor,
) -> Result<Ordering> {
    for (index, boundary_value) in cursor.values.iter().enumerate() {
        let order = &order_by[index];
        let ordering = match &order.field {
            PreparedField::UserField(field) => compare_structured_order_values(
                document.document.get_field(field),
                Some(boundary_value),
            ),
            PreparedField::DocumentName => match boundary_value {
                Value::String(boundary_value) => {
                    Ok(document.document_name.as_str().cmp(boundary_value))
                }
                _ => Err(Error::InvalidInput(
                    "document ID cursor values must be strings".to_string(),
                )),
            },
        }?;
        let ordering = match order.direction {
            OrderDirection::Asc => ordering,
            OrderDirection::Desc => ordering.reverse(),
        };
        if ordering != Ordering::Equal {
            return Ok(ordering);
        }
    }

    Ok(Ordering::Equal)
}

fn document_matches_start_cursor(
    document: &StructuredDocumentRow,
    order_by: &[PreparedOrder],
    cursor: &PreparedCursor,
) -> Result<bool> {
    let ordering = compare_document_to_structured_cursor(document, order_by, cursor)?;
    Ok(match ordering {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => cursor.before,
    })
}

fn document_matches_end_cursor(
    document: &StructuredDocumentRow,
    order_by: &[PreparedOrder],
    cursor: &PreparedCursor,
) -> Result<bool> {
    let ordering = compare_document_to_structured_cursor(document, order_by, cursor)?;
    Ok(match ordering {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => !cursor.before,
    })
}

fn value_is_nan(value: &Value) -> bool {
    value.as_f64().is_some_and(|value| value.is_nan())
}

fn value_in_membership_array(value: &Value, candidates: &Value) -> Result<bool> {
    let candidates = candidates.as_array().ok_or_else(|| {
        Error::InvalidInput("set-membership filters require array comparison values".to_string())
    })?;
    Ok(candidates.iter().any(|candidate| candidate == value))
}

fn array_contains_value(field_value: &Value, target: &Value) -> bool {
    matches!(field_value, Value::Array(values) if values.iter().any(|value| value == target))
}

fn array_contains_any_value(field_value: &Value, targets: &Value) -> Result<bool> {
    let targets = targets.as_array().ok_or_else(|| {
        Error::InvalidInput(
            "ARRAY_CONTAINS_ANY filters require array comparison values".to_string(),
        )
    })?;
    Ok(match field_value {
        Value::Array(values) => values
            .iter()
            .any(|value| targets.iter().any(|candidate| candidate == value)),
        _ => false,
    })
}

fn matches_prepared_field_filter(
    document: &StructuredDocumentRow,
    field: &PreparedField,
    op: FieldFilterOperator,
    value: &Value,
) -> Result<bool> {
    match field {
        PreparedField::DocumentName => {
            let document_id = Value::String(document.document_name.clone());
            match op {
                FieldFilterOperator::LessThan => {
                    compare_structured_order_values(Some(&document_id), Some(value))
                        .map(|ordering| ordering == Ordering::Less)
                }
                FieldFilterOperator::LessThanOrEqual => {
                    compare_structured_order_values(Some(&document_id), Some(value))
                        .map(|ordering| matches!(ordering, Ordering::Less | Ordering::Equal))
                }
                FieldFilterOperator::GreaterThan => {
                    compare_structured_order_values(Some(&document_id), Some(value))
                        .map(|ordering| ordering == Ordering::Greater)
                }
                FieldFilterOperator::GreaterThanOrEqual => {
                    compare_structured_order_values(Some(&document_id), Some(value))
                        .map(|ordering| matches!(ordering, Ordering::Greater | Ordering::Equal))
                }
                FieldFilterOperator::Equal => Ok(document_id == *value),
                FieldFilterOperator::NotEqual => Ok(document_id != *value),
                FieldFilterOperator::In => value_in_membership_array(&document_id, value),
                FieldFilterOperator::NotIn => {
                    value_in_membership_array(&document_id, value).map(|contains| !contains)
                }
                FieldFilterOperator::ArrayContains | FieldFilterOperator::ArrayContainsAny => {
                    Err(Error::InvalidInput(
                        "document ID filters do not support array membership operators".to_string(),
                    ))
                }
            }
        }
        PreparedField::UserField(field) => {
            let Some(field_value) = document.document.get_field(field) else {
                return Ok(false);
            };
            match op {
                FieldFilterOperator::LessThan => {
                    compare_structured_order_values(Some(field_value), Some(value))
                        .map(|ordering| ordering == Ordering::Less)
                }
                FieldFilterOperator::LessThanOrEqual => {
                    compare_structured_order_values(Some(field_value), Some(value))
                        .map(|ordering| matches!(ordering, Ordering::Less | Ordering::Equal))
                }
                FieldFilterOperator::GreaterThan => {
                    compare_structured_order_values(Some(field_value), Some(value))
                        .map(|ordering| ordering == Ordering::Greater)
                }
                FieldFilterOperator::GreaterThanOrEqual => {
                    compare_structured_order_values(Some(field_value), Some(value))
                        .map(|ordering| matches!(ordering, Ordering::Greater | Ordering::Equal))
                }
                FieldFilterOperator::Equal => Ok(field_value == value),
                FieldFilterOperator::NotEqual => Ok(field_value != value),
                FieldFilterOperator::ArrayContains => Ok(array_contains_value(field_value, value)),
                FieldFilterOperator::In => value_in_membership_array(field_value, value),
                FieldFilterOperator::ArrayContainsAny => {
                    array_contains_any_value(field_value, value)
                }
                FieldFilterOperator::NotIn => {
                    value_in_membership_array(field_value, value).map(|contains| !contains)
                }
            }
        }
    }
}

fn matches_prepared_unary_filter(
    document: &StructuredDocumentRow,
    field: &PreparedField,
    op: UnaryFilterOperator,
) -> Result<bool> {
    let PreparedField::UserField(field) = field else {
        return Err(Error::InvalidInput(
            "unary filters do not support the `__name__` document ID sentinel".to_string(),
        ));
    };
    let Some(field_value) = document.document.get_field(field) else {
        return Ok(false);
    };
    Ok(match op {
        UnaryFilterOperator::IsNan => value_is_nan(field_value),
        UnaryFilterOperator::IsNull => field_value.is_null(),
        UnaryFilterOperator::IsNotNan => !value_is_nan(field_value),
        UnaryFilterOperator::IsNotNull => !field_value.is_null(),
    })
}

fn matches_prepared_filter(
    document: &StructuredDocumentRow,
    filter: &PreparedFilter,
) -> Result<bool> {
    match filter {
        PreparedFilter::Composite {
            op: CompositeOperator::And,
            filters,
        } => filters.iter().try_fold(true, |matched, filter| {
            if !matched {
                return Ok(false);
            }
            matches_prepared_filter(document, filter)
        }),
        PreparedFilter::Composite {
            op: CompositeOperator::Or,
            filters,
        } => filters.iter().try_fold(false, |matched, filter| {
            if matched {
                return Ok(true);
            }
            matches_prepared_filter(document, filter)
        }),
        PreparedFilter::Field { field, op, value } => {
            matches_prepared_field_filter(document, field, *op, value)
        }
        PreparedFilter::Unary { field, op } => matches_prepared_unary_filter(document, field, *op),
    }
}

fn apply_projection(
    row: StructuredDocumentRow,
    projection: &ProjectionMode,
) -> StructuredDocumentRow {
    match projection {
        ProjectionMode::AllFields => row,
        ProjectionMode::SelectedFields(fields) => {
            let mut projected_fields = serde_json::Map::with_capacity(fields.len());
            for field in fields {
                if let Some(value) = row.document.fields.get(field).cloned() {
                    projected_fields.insert(field.clone(), value);
                }
            }
            let projected_typed_fields = row
                .document
                .typed_fields
                .into_iter()
                .filter(|(field, _)| fields.contains(field))
                .collect();
            StructuredDocumentRow {
                document: Document {
                    id: row.document.id,
                    table: row.document.table,
                    creation_time: row.document.creation_time,
                    update_time: row.document.update_time,
                    fields: projected_fields,
                    typed_fields: projected_typed_fields,
                },
                document_name: row.document_name,
                document_path: row.document_path,
            }
        }
    }
}

pub(crate) fn finalize_structured_rows(
    documents: Vec<StructuredDocumentRow>,
    prepared: &PreparedStructuredQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<StructuredDocumentRow>> {
    let mut filtered = Vec::with_capacity(documents.len());
    for document in documents {
        check_cancel()?;
        if let Some(filter) = &prepared.filter
            && !matches_prepared_filter(&document, filter)?
        {
            continue;
        }
        filtered.push(document);
    }

    check_cancel()?;
    sort_documents_for_structured_query(&mut filtered, &prepared.order_by)?;

    let mut bounded = Vec::with_capacity(filtered.len());
    for document in filtered {
        check_cancel()?;
        if let Some(start_at) = &prepared.start_at
            && !document_matches_start_cursor(&document, &prepared.order_by, start_at)?
        {
            continue;
        }
        if let Some(end_at) = &prepared.end_at
            && !document_matches_end_cursor(&document, &prepared.order_by, end_at)?
        {
            continue;
        }
        bounded.push(document);
    }

    let iter = bounded.into_iter().skip(prepared.offset);
    let limited: Vec<_> = match prepared.limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    };

    check_cancel()?;
    Ok(limited
        .into_iter()
        .map(|document| apply_projection(document, &prepared.projection))
        .collect())
}

pub(crate) fn finalize_structured_documents(
    documents: Vec<Document>,
    prepared: &PreparedStructuredQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let rows = documents
        .into_iter()
        .map(|document| StructuredDocumentRow {
            document_name: document.id.to_string(),
            document,
            document_path: None,
        })
        .collect::<Vec<_>>();
    finalize_structured_rows(rows, prepared, check_cancel)
        .map(|rows| rows.into_iter().map(|row| row.document).collect::<Vec<_>>())
}

pub(crate) fn structured_base_query(
    table: &TableName,
    prepared: &PreparedStructuredQuery,
) -> Query {
    Query {
        table: table.clone(),
        filters: prepared.pushdown_filters.clone(),
        order: None,
        limit: None,
    }
}

fn document_path_is_within_collection_group_scope(
    document_path: &DocumentPath,
    ancestor: Option<&DocumentPath>,
) -> bool {
    let Some(ancestor) = ancestor else {
        return true;
    };
    let ancestor_segments = ancestor.segments();
    let document_segments = document_path.segments();
    document_segments.len() > ancestor_segments.len()
        && document_segments.starts_with(ancestor_segments.as_slice())
}

pub(crate) fn collection_group_table_targets(
    bindings: Vec<ResourcePathBinding>,
    ancestor: Option<&DocumentPath>,
) -> Vec<CollectionGroupTableTarget> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for binding in bindings {
        if !document_path_is_within_collection_group_scope(&binding.document_path, ancestor) {
            continue;
        }
        let target = CollectionGroupTableTarget {
            table: binding.locator.table.clone(),
            collection_path: binding.document_path.collection_path().clone(),
        };
        if seen.insert(target.clone()) {
            targets.push(target);
        }
    }
    targets
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuredOrderValueKind {
    String,
    Number,
}
