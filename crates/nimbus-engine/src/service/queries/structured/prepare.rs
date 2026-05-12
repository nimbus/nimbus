use nimbus_core::{
    CollectionSelector, CompositeOperator, DocumentId, DocumentPath, Error, FieldFilterOperator,
    Filter, FilterOp, OrderDirection, Projection, QueryDirection, QueryFilter, Result,
    StructuredCursor, StructuredOrder, StructuredQuery, TableName, TableSchema,
    UnaryFilterOperator,
};
use serde_json::Value;

use super::{
    DocumentNameMode, PreparedCursor, PreparedField, PreparedFilter, PreparedFilterStatistics,
    PreparedOrder, PreparedStructuredQuery, ProjectionMode, push_unique,
    unsupported_structured_query_feature,
};

fn top_level_field_path(field_path: &str, context: &str) -> Result<PreparedField> {
    if field_path == "__name__" {
        return Ok(PreparedField::DocumentName);
    }
    if field_path.contains('.') {
        return Err(unsupported_structured_query_feature(context));
    }
    Ok(PreparedField::UserField(field_path.to_string()))
}

fn lower_structured_source(table: &TableName, from: &[CollectionSelector]) -> Result<()> {
    match from {
        [] => Ok(()),
        [selector] if selector.is_collection_group() => Err(unsupported_structured_query_feature(
            "collection group sources",
        )),
        [selector] if selector.collection_id.as_str() != table.as_str() => Err(
            unsupported_structured_query_feature("raw collection-to-table source mapping"),
        ),
        [_selector] => Ok(()),
        _ => Err(unsupported_structured_query_feature(
            "multiple query sources",
        )),
    }
}

fn normalize_relative_document_path_value(value: &Value) -> Result<Value> {
    let value = value
        .as_str()
        .ok_or_else(|| Error::InvalidInput("document name values must be strings".to_string()))?;
    let relative_path = value
        .split_once("/documents/")
        .map(|(_, path)| path)
        .unwrap_or(value);
    let document_path = DocumentPath::from_segments(relative_path.split('/'))?;
    Ok(Value::String(document_path.to_string()))
}

fn normalize_document_name_value(value: &Value, mode: DocumentNameMode) -> Result<Value> {
    match mode {
        DocumentNameMode::LeafId => {
            let value = value.as_str().ok_or_else(|| {
                Error::InvalidInput("document ID values must be strings".to_string())
            })?;
            let leaf_id = value
                .rsplit('/')
                .next()
                .filter(|segment| !segment.is_empty())
                .ok_or_else(|| {
                    Error::InvalidInput("document ID values cannot be empty".to_string())
                })?;
            let document_id = DocumentId::from_key(leaf_id.to_string())?;
            Ok(Value::String(document_id.to_string()))
        }
        DocumentNameMode::RelativePath => normalize_relative_document_path_value(value),
    }
}

fn normalize_membership_array(
    value: &Value,
    filter_name: &str,
    max_items: Option<usize>,
    normalize_items: impl Fn(&Value) -> Result<Value>,
) -> Result<Value> {
    let values = value.as_array().ok_or_else(|| {
        Error::InvalidInput(format!(
            "{filter_name} filters require an array comparison value"
        ))
    })?;
    if values.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{filter_name} filters require a non-empty array comparison value"
        )));
    }
    if let Some(max_items) = max_items
        && values.len() > max_items
    {
        return Err(Error::InvalidInput(format!(
            "{filter_name} filters support at most {max_items} comparison values"
        )));
    }
    Ok(Value::Array(
        values
            .iter()
            .map(normalize_items)
            .collect::<Result<Vec<_>>>()?,
    ))
}

fn normalize_filter_value(
    field: &PreparedField,
    op: FieldFilterOperator,
    value: &Value,
    document_name_mode: DocumentNameMode,
) -> Result<Value> {
    match field {
        PreparedField::DocumentName => match op {
            FieldFilterOperator::ArrayContains | FieldFilterOperator::ArrayContainsAny => {
                Err(Error::InvalidInput(
                    "document ID filters do not support array membership operators".to_string(),
                ))
            }
            FieldFilterOperator::In => normalize_membership_array(value, "IN", None, |value| {
                normalize_document_name_value(value, document_name_mode)
            }),
            FieldFilterOperator::NotIn => {
                normalize_membership_array(value, "NOT_IN", Some(10), |value| {
                    normalize_document_name_value(value, document_name_mode)
                })
            }
            FieldFilterOperator::LessThan
            | FieldFilterOperator::LessThanOrEqual
            | FieldFilterOperator::GreaterThan
            | FieldFilterOperator::GreaterThanOrEqual
            | FieldFilterOperator::Equal
            | FieldFilterOperator::NotEqual => {
                normalize_document_name_value(value, document_name_mode)
            }
        },
        PreparedField::UserField(_) => match op {
            FieldFilterOperator::In => {
                normalize_membership_array(value, "IN", None, |value| Ok(value.clone()))
            }
            FieldFilterOperator::ArrayContainsAny => {
                normalize_membership_array(value, "ARRAY_CONTAINS_ANY", None, |value| {
                    Ok(value.clone())
                })
            }
            FieldFilterOperator::NotIn => {
                normalize_membership_array(value, "NOT_IN", Some(10), |value| Ok(value.clone()))
            }
            FieldFilterOperator::LessThan
            | FieldFilterOperator::LessThanOrEqual
            | FieldFilterOperator::GreaterThan
            | FieldFilterOperator::GreaterThanOrEqual
            | FieldFilterOperator::Equal
            | FieldFilterOperator::NotEqual
            | FieldFilterOperator::ArrayContains => Ok(value.clone()),
        },
    }
}

fn record_negative_filter(stats: &mut PreparedFilterStatistics) {
    stats.negative_filter_count += 1;
}

fn record_inequality_field(stats: &mut PreparedFilterStatistics, field: &PreparedField) {
    push_unique(
        &mut stats.inequality_fields,
        field.display_name().to_string(),
    );
}

fn prepare_structured_filter(
    filter: &QueryFilter,
    stats: &mut PreparedFilterStatistics,
    document_name_mode: DocumentNameMode,
) -> Result<PreparedFilter> {
    match filter {
        QueryFilter::CompositeFilter(filter) => {
            if filter.filters.is_empty() {
                return Err(Error::InvalidInput(
                    "composite filters must include at least one child filter".to_string(),
                ));
            }
            if filter.op == CompositeOperator::Or {
                stats.has_or = true;
            }
            Ok(PreparedFilter::Composite {
                op: filter.op,
                filters: filter
                    .filters
                    .iter()
                    .map(|filter| prepare_structured_filter(filter, stats, document_name_mode))
                    .collect::<Result<Vec<_>>>()?,
            })
        }
        QueryFilter::FieldFilter(filter) => {
            let field =
                top_level_field_path(filter.field.as_str(), "nested field paths in filters")?;
            if let Some(field_name) = field.user_field() {
                push_unique(&mut stats.referenced_fields, field_name.to_string());
            }
            match filter.op {
                FieldFilterOperator::LessThan
                | FieldFilterOperator::LessThanOrEqual
                | FieldFilterOperator::GreaterThan
                | FieldFilterOperator::GreaterThanOrEqual => {
                    record_inequality_field(stats, &field);
                }
                FieldFilterOperator::NotEqual => {
                    record_inequality_field(stats, &field);
                    record_negative_filter(stats);
                }
                FieldFilterOperator::ArrayContainsAny => {
                    stats.array_contains_any_count += 1;
                }
                FieldFilterOperator::In => {
                    stats.has_in = true;
                }
                FieldFilterOperator::NotIn => {
                    stats.has_not_in = true;
                    record_inequality_field(stats, &field);
                    record_negative_filter(stats);
                }
                FieldFilterOperator::Equal | FieldFilterOperator::ArrayContains => {}
            }
            Ok(PreparedFilter::Field {
                field: field.clone(),
                op: filter.op,
                value: normalize_filter_value(
                    &field,
                    filter.op,
                    &filter.value,
                    document_name_mode,
                )?,
            })
        }
        QueryFilter::UnaryFilter(filter) => {
            let field =
                top_level_field_path(filter.field.as_str(), "nested field paths in unary filters")?;
            if matches!(field, PreparedField::DocumentName) {
                return Err(Error::InvalidInput(
                    "unary filters do not support the `__name__` document ID sentinel".to_string(),
                ));
            }
            if let Some(field_name) = field.user_field() {
                push_unique(&mut stats.referenced_fields, field_name.to_string());
            }
            if matches!(
                filter.op,
                UnaryFilterOperator::IsNotNan | UnaryFilterOperator::IsNotNull
            ) {
                record_inequality_field(stats, &field);
                record_negative_filter(stats);
            }
            Ok(PreparedFilter::Unary {
                field,
                op: filter.op,
            })
        }
    }
}

fn validate_filter_combinations(stats: &PreparedFilterStatistics) -> Result<()> {
    if stats.array_contains_any_count > 1 {
        return Err(Error::InvalidInput(
            "structured query cannot use more than one ARRAY_CONTAINS_ANY filter".to_string(),
        ));
    }
    if stats.negative_filter_count > 1 {
        return Err(Error::InvalidInput(
            "structured query cannot combine multiple NOT_EQUAL, NOT_IN, IS_NOT_NULL, or IS_NOT_NAN filters".to_string(),
        ));
    }
    if stats.has_not_in && stats.has_or {
        return Err(Error::InvalidInput(
            "structured query NOT_IN filters cannot be combined with OR filters".to_string(),
        ));
    }
    if stats.has_not_in && stats.has_in {
        return Err(Error::InvalidInput(
            "structured query cannot combine NOT_IN and IN filters".to_string(),
        ));
    }
    if stats.has_not_in && stats.array_contains_any_count > 0 {
        return Err(Error::InvalidInput(
            "structured query cannot combine NOT_IN and ARRAY_CONTAINS_ANY filters".to_string(),
        ));
    }
    Ok(())
}

fn first_inequality_field(stats: &PreparedFilterStatistics) -> Result<Option<PreparedField>> {
    let mut user_fields = stats
        .inequality_fields
        .iter()
        .filter(|field| field.as_str() != "__name__")
        .cloned()
        .collect::<Vec<_>>();
    user_fields.sort();
    user_fields.dedup();
    if user_fields.len() > 1 {
        return Err(Error::InvalidInput(
            "structured query support for multiple distinct inequality fields is deferred"
                .to_string(),
        ));
    }
    if let Some(field) = user_fields.into_iter().next() {
        return Ok(Some(PreparedField::UserField(field)));
    }
    if stats
        .inequality_fields
        .iter()
        .any(|field| field == "__name__")
    {
        return Ok(Some(PreparedField::DocumentName));
    }
    Ok(None)
}

fn lower_structured_order(
    order_by: &[StructuredOrder],
    stats: &PreparedFilterStatistics,
) -> Result<Vec<PreparedOrder>> {
    let mut lowered: Vec<PreparedOrder> = Vec::with_capacity(order_by.len());
    for order in order_by {
        let field = top_level_field_path(
            order.field.as_str(),
            "nested field paths in order_by clauses",
        )?;
        if lowered.iter().any(|existing| existing.field == field) {
            return Err(Error::InvalidInput(format!(
                "structured query cannot order by `{}` more than once",
                field.display_name()
            )));
        }

        let direction = match order.direction {
            QueryDirection::Ascending => OrderDirection::Asc,
            QueryDirection::Descending => OrderDirection::Desc,
        };

        lowered.push(PreparedOrder { field, direction });
    }

    let appended_direction = lowered
        .last()
        .map(|order| order.direction)
        .unwrap_or(OrderDirection::Asc);
    if let Some(required_first) = first_inequality_field(stats)? {
        if let Some(first_explicit) = lowered.first()
            && first_explicit.field != required_first
        {
            return Err(Error::InvalidInput(format!(
                "structured query inequality filters require the first order_by field to be `{}`",
                required_first.display_name()
            )));
        }
        if lowered.iter().all(|order| order.field != required_first) {
            lowered.push(PreparedOrder {
                field: required_first,
                direction: appended_direction,
            });
        }
    }
    if lowered
        .iter()
        .all(|order| !matches!(order.field, PreparedField::DocumentName))
    {
        lowered.push(PreparedOrder {
            field: PreparedField::DocumentName,
            direction: appended_direction,
        });
    }

    Ok(lowered)
}

fn lower_projection(select: Option<&Projection>) -> Result<ProjectionMode> {
    let Some(select) = select else {
        return Ok(ProjectionMode::AllFields);
    };
    if select.fields.is_empty() {
        return Ok(ProjectionMode::AllFields);
    }

    let mut fields = Vec::new();
    for field in &select.fields {
        let field_path = top_level_field_path(field.as_str(), "nested projection field paths")?;
        let PreparedField::UserField(field_path) = field_path else {
            continue;
        };
        if !fields.iter().any(|existing| existing == &field_path) {
            fields.push(field_path);
        }
    }

    Ok(ProjectionMode::SelectedFields(fields))
}

fn normalize_cursor_value(
    field: &PreparedField,
    value: &Value,
    document_name_mode: DocumentNameMode,
) -> Result<Value> {
    match field {
        PreparedField::UserField(_) => Ok(value.clone()),
        PreparedField::DocumentName => normalize_document_name_value(value, document_name_mode),
    }
}

fn prepare_structured_cursor(
    cursor: &StructuredCursor,
    order_by: &[PreparedOrder],
    document_name_mode: DocumentNameMode,
) -> Result<PreparedCursor> {
    if cursor.values.len() > order_by.len() {
        return Err(Error::InvalidInput(
            "structured cursor cannot include more values than order_by fields".to_string(),
        ));
    }
    Ok(PreparedCursor {
        values: cursor
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                normalize_cursor_value(&order_by[index].field, value, document_name_mode)
            })
            .collect::<Result<Vec<_>>>()?,
        before: cursor.before,
    })
}

fn collect_pushdown_filters(filter: &PreparedFilter, pushdown_filters: &mut Vec<Filter>) {
    match filter {
        PreparedFilter::Composite {
            op: CompositeOperator::And,
            filters,
        } => {
            for filter in filters {
                collect_pushdown_filters(filter, pushdown_filters);
            }
        }
        PreparedFilter::Field {
            field: PreparedField::UserField(field),
            op,
            value,
        } => {
            let op = match op {
                FieldFilterOperator::LessThan => Some(FilterOp::Lt),
                FieldFilterOperator::LessThanOrEqual => Some(FilterOp::Lte),
                FieldFilterOperator::GreaterThan => Some(FilterOp::Gt),
                FieldFilterOperator::GreaterThanOrEqual => Some(FilterOp::Gte),
                FieldFilterOperator::Equal => Some(FilterOp::Eq),
                FieldFilterOperator::NotEqual => Some(FilterOp::Neq),
                FieldFilterOperator::ArrayContains
                | FieldFilterOperator::In
                | FieldFilterOperator::ArrayContainsAny
                | FieldFilterOperator::NotIn => None,
            };
            if let Some(op) = op {
                pushdown_filters.push(Filter {
                    field: field.clone(),
                    op,
                    value: value.clone(),
                });
            }
        }
        PreparedFilter::Composite {
            op: CompositeOperator::Or,
            ..
        }
        | PreparedFilter::Field { .. }
        | PreparedFilter::Unary { .. } => {}
    }
}

fn prepare_structured_query_with_document_name_mode(
    table: Option<&TableName>,
    query: &StructuredQuery,
    document_name_mode: DocumentNameMode,
) -> Result<PreparedStructuredQuery> {
    if let Some(table) = table {
        lower_structured_source(table, &query.from)?;
    } else if !query.from.is_empty() {
        return Err(unsupported_structured_query_feature(
            "raw collection-group source mapping",
        ));
    }

    if query.find_nearest.is_some() {
        return Err(unsupported_structured_query_feature("find_nearest"));
    }

    let mut filter_stats = PreparedFilterStatistics::default();
    let filter = query
        .where_filter
        .as_ref()
        .map(|filter| prepare_structured_filter(filter, &mut filter_stats, document_name_mode))
        .transpose()?;
    validate_filter_combinations(&filter_stats)?;
    let order_by = lower_structured_order(&query.order_by, &filter_stats)?;
    let start_at = query
        .start_at
        .as_ref()
        .map(|cursor| prepare_structured_cursor(cursor, &order_by, document_name_mode))
        .transpose()?;
    let end_at = query
        .end_at
        .as_ref()
        .map(|cursor| prepare_structured_cursor(cursor, &order_by, document_name_mode))
        .transpose()?;
    let projection = lower_projection(query.select.as_ref())?;
    let mut filters = Vec::new();
    if let Some(filter) = &filter {
        collect_pushdown_filters(filter, &mut filters);
    }
    let mut required_index_fields = filter_stats.referenced_fields;
    for order in &order_by {
        if let Some(field) = order.field.user_field() {
            push_unique(&mut required_index_fields, field.to_string());
        }
    }

    Ok(PreparedStructuredQuery {
        // The legacy engine query still owns filter pushdown and authorization.
        // Richer StructuredQuery ordering, cursor, offset, and projection
        // semantics are applied after the candidate rows are loaded.
        pushdown_filters: filters,
        filter,
        order_by,
        projection,
        start_at,
        end_at,
        offset: query.offset.unwrap_or(0) as usize,
        limit: query.limit.map(|limit| limit as usize),
        required_index_fields,
    })
}

pub(crate) fn prepare_structured_query(
    table: &TableName,
    query: &StructuredQuery,
) -> Result<PreparedStructuredQuery> {
    prepare_structured_query_with_document_name_mode(Some(table), query, DocumentNameMode::LeafId)
}

pub(crate) fn prepare_collection_group_structured_query(
    query: &StructuredQuery,
) -> Result<PreparedStructuredQuery> {
    prepare_structured_query_with_document_name_mode(None, query, DocumentNameMode::RelativePath)
}

pub(super) fn required_structured_query_index_fields(
    prepared: &PreparedStructuredQuery,
) -> Option<Vec<String>> {
    (prepared.required_index_fields.len() > 1).then(|| prepared.required_index_fields.clone())
}

fn table_schema_supports_structured_query_index(
    table_schema: &TableSchema,
    required_fields: &[String],
) -> bool {
    table_schema
        .indexes
        .iter()
        .any(|index| index.fields.as_slice().starts_with(required_fields))
}

pub(crate) fn ensure_structured_query_index(
    table_schema: Option<&TableSchema>,
    prepared: &PreparedStructuredQuery,
) -> Result<()> {
    let Some(required_fields) = required_structured_query_index_fields(prepared) else {
        return Ok(());
    };
    if table_schema.is_some_and(|table_schema| {
        table_schema_supports_structured_query_index(table_schema, &required_fields)
    }) {
        return Ok(());
    }

    Err(Error::InvalidInput(format!(
        "structured query requires an index covering fields: {}",
        required_fields.join(", ")
    )))
}
