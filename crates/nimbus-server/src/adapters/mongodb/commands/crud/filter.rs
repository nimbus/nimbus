use std::sync::Arc;

use nimbus_core::{
    Document, DocumentId, Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query,
    TableName, TenantId,
};
use nimbus_engine::Service;

use super::super::super::error::{BAD_VALUE, MongoError};

pub(super) fn has_operator_keys(doc: &bson::Document) -> bool {
    doc.keys().any(|k| k.starts_with('$'))
}

pub(super) fn bson_to_filter_value(value: &bson::Bson) -> serde_json::Value {
    match value {
        bson::Bson::Null => serde_json::Value::Null,
        bson::Bson::Boolean(b) => serde_json::Value::Bool(*b),
        bson::Bson::Int32(n) => serde_json::Value::Number((*n).into()),
        bson::Bson::Int64(n) => serde_json::Value::Number((*n).into()),
        bson::Bson::Double(f) => serde_json::json!(*f),
        bson::Bson::String(s) => serde_json::Value::String(s.clone()),
        bson::Bson::ObjectId(oid) => serde_json::Value::String(oid.to_hex()),
        bson::Bson::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(bson_to_filter_value).collect())
        }
        bson::Bson::Document(doc) => {
            let map: serde_json::Map<String, serde_json::Value> = doc
                .iter()
                .map(|(k, v)| (k.to_string(), bson_to_filter_value(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

pub(super) fn translate_filter(filter_doc: &bson::Document) -> Result<Vec<Filter>, MongoError> {
    translate_filter_impl(filter_doc, false)
}

pub(super) fn translate_filter_excluding_id(
    filter_doc: &bson::Document,
) -> Result<Vec<Filter>, MongoError> {
    translate_filter_impl(filter_doc, true)
}

fn translate_filter_impl(
    filter_doc: &bson::Document,
    exclude_id: bool,
) -> Result<Vec<Filter>, MongoError> {
    let mut filters = Vec::new();
    for (field, value) in filter_doc.iter() {
        if exclude_id && field == "_id" {
            continue;
        }
        if field.starts_with('$') {
            return Err(MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: format!("top-level operator {field} not supported in find filter"),
            });
        }
        match value {
            bson::Bson::Document(ops) if has_operator_keys(ops) => {
                for (op_key, op_val) in ops.iter() {
                    let op = match op_key.as_str() {
                        "$eq" => FilterOp::Eq,
                        "$ne" => FilterOp::Neq,
                        "$gt" => FilterOp::Gt,
                        "$gte" => FilterOp::Gte,
                        "$lt" => FilterOp::Lt,
                        "$lte" => FilterOp::Lte,
                        other => {
                            return Err(MongoError::Command {
                                code: BAD_VALUE.code,
                                code_name: BAD_VALUE.code_name.into(),
                                message: format!(
                                    "unsupported filter operator: {other} on field {field}"
                                ),
                            });
                        }
                    };
                    filters.push(Filter {
                        field: field.to_string(),
                        op,
                        value: bson_to_filter_value(op_val),
                    });
                }
            }
            _ => {
                filters.push(Filter {
                    field: field.to_string(),
                    op: FilterOp::Eq,
                    value: bson_to_filter_value(value),
                });
            }
        }
    }
    Ok(filters)
}

pub(super) fn translate_sort(sort_doc: Option<&bson::Document>) -> Vec<OrderBy> {
    let Some(doc) = sort_doc else {
        return vec![];
    };
    doc.iter()
        .map(|(field, direction_val)| {
            let direction = match direction_val {
                bson::Bson::Int32(n) if *n < 0 => OrderDirection::Desc,
                bson::Bson::Int64(n) if *n < 0 => OrderDirection::Desc,
                bson::Bson::Double(f) if *f < 0.0 => OrderDirection::Desc,
                _ => OrderDirection::Asc,
            };
            OrderBy {
                field: field.to_string(),
                direction,
            }
        })
        .collect()
}

pub(super) fn matches_simple_filters(doc: &Document, filters: &[Filter]) -> bool {
    for filter in filters {
        let Some(field_val) = doc.get_field(&filter.field) else {
            return false;
        };
        let matched = match filter.op {
            FilterOp::Eq => field_val == &filter.value,
            FilterOp::Neq => field_val != &filter.value,
            _ => true,
        };
        if !matched {
            return false;
        }
    }
    true
}

pub(super) fn query_documents(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    filter_doc: &bson::Document,
    orders: Vec<OrderBy>,
    limit: Option<usize>,
) -> Result<Vec<Document>, MongoError> {
    let principal = PrincipalContext::system();

    if let Some(id_val) = filter_doc.get("_id") {
        if !matches!(id_val, bson::Bson::Document(d) if has_operator_keys(d)) {
            let id_str = bson_id_to_string(id_val);
            if let Ok(doc_id) = DocumentId::from_key(&id_str) {
                match service.get_document_with_principal(tenant_id, table, doc_id, &principal) {
                    Ok(doc) => {
                        let non_id_filters = translate_filter_excluding_id(filter_doc)?;
                        if matches_simple_filters(&doc, &non_id_filters) {
                            return Ok(vec![doc]);
                        }
                        return Ok(vec![]);
                    }
                    Err(nimbus_core::Error::DocumentNotFound(_))
                    | Err(nimbus_core::Error::TenantNotFound(_)) => return Ok(vec![]),
                    Err(e) => return Err(MongoError::from(e)),
                }
            }
        }
    }

    let primary_order = orders.first().cloned();
    let filters = translate_filter(filter_doc)?;
    let query = Query {
        table: table.clone(),
        filters,
        order: primary_order,
        limit: if orders.len() > 1 { None } else { limit },
    };
    let mut docs = service
        .query_documents_with_principal(tenant_id, &query, &principal)
        .map_err(MongoError::from)?;

    if orders.len() > 1 {
        apply_compound_sort(&mut docs, &orders);
        if let Some(lim) = limit {
            docs.truncate(lim);
        }
    }

    Ok(docs)
}

fn apply_compound_sort(docs: &mut [Document], orders: &[OrderBy]) {
    docs.sort_by(|a, b| {
        for order in orders {
            let a_val = a.get_field(&order.field);
            let b_val = b.get_field(&order.field);
            let cmp = compare_json_values(a_val, b_val);
            let cmp = match order.direction {
                OrderDirection::Asc => cmp,
                OrderDirection::Desc => cmp.reverse(),
            };
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn compare_json_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    fn rank(v: Option<&serde_json::Value>) -> u8 {
        match v {
            None | Some(serde_json::Value::Null) => 0,
            Some(serde_json::Value::Number(_)) => 1,
            Some(serde_json::Value::String(_)) => 2,
            Some(serde_json::Value::Bool(_)) => 3,
            _ => 4,
        }
    }

    let ra = rank(a);
    let rb = rank(b);
    if ra != rb {
        return ra.cmp(&rb);
    }

    match (a, b) {
        (Some(serde_json::Value::Number(na)), Some(serde_json::Value::Number(nb))) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Some(serde_json::Value::String(sa)), Some(serde_json::Value::String(sb))) => sa.cmp(sb),
        (Some(serde_json::Value::Bool(ba)), Some(serde_json::Value::Bool(bb))) => ba.cmp(bb),
        _ => std::cmp::Ordering::Equal,
    }
}

pub(super) fn bson_id_to_string(value: &bson::Bson) -> String {
    match value {
        bson::Bson::String(s) => s.clone(),
        bson::Bson::ObjectId(oid) => oid.to_hex(),
        bson::Bson::Int32(n) => n.to_string(),
        bson::Bson::Int64(n) => n.to_string(),
        _ => format!("{value}"),
    }
}

pub(super) fn resolve_field_path(doc: &bson::Document, path: &str) -> Option<bson::Bson> {
    let mut parts = path.splitn(2, '.');
    let first = parts.next()?;
    let value = doc.get(first)?;
    match parts.next() {
        None => Some(value.clone()),
        Some(rest) => match value {
            bson::Bson::Document(inner) => resolve_field_path(inner, rest),
            _ => None,
        },
    }
}
