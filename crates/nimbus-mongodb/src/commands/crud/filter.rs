use std::{cmp::Ordering, sync::Arc};

use nimbus_core::{
    Document, DocumentId, Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query,
    TableName, TenantId, TransactionSessionToken,
};
use nimbus_engine::Engine;

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

pub(in crate::commands) fn translate_filter(
    filter_doc: &bson::Document,
) -> Result<Vec<Filter>, MongoError> {
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
            FilterOp::Gt => compare_json_values(Some(field_val), Some(&filter.value)).is_gt(),
            FilterOp::Gte => !compare_json_values(Some(field_val), Some(&filter.value)).is_lt(),
            FilterOp::Lt => compare_json_values(Some(field_val), Some(&filter.value)).is_lt(),
            FilterOp::Lte => !compare_json_values(Some(field_val), Some(&filter.value)).is_gt(),
        };
        if !matched {
            return false;
        }
    }
    true
}

pub(super) struct QueryDocumentsRequest<'a> {
    pub tenant_id: &'a TenantId,
    pub table: &'a TableName,
    pub filter_doc: &'a bson::Document,
    pub orders: Vec<OrderBy>,
    pub limit: Option<usize>,
    pub transaction_token: Option<&'a TransactionSessionToken>,
    pub principal: &'a PrincipalContext,
}

pub(super) fn query_documents(
    engine: &Arc<Engine>,
    request: QueryDocumentsRequest<'_>,
) -> Result<Vec<Document>, MongoError> {
    let QueryDocumentsRequest {
        tenant_id,
        table,
        filter_doc,
        orders,
        limit,
        transaction_token,
        principal,
    } = request;

    if let Some(id_val) = filter_doc.get("_id")
        && !matches!(id_val, bson::Bson::Document(d) if has_operator_keys(d))
    {
        let id_str = bson_id_to_string(id_val);
        if let Ok(doc_id) = DocumentId::from_key(&id_str) {
            let result = match transaction_token {
                Some(transaction_token) => engine.get_document_in_transaction(
                    tenant_id,
                    transaction_token,
                    principal,
                    table,
                    doc_id,
                ),
                None => engine
                    .get_document_with_principal(tenant_id, table, doc_id, principal)
                    .map(Some),
            };
            match result {
                Ok(doc) => {
                    let non_id_filters = translate_filter_excluding_id(filter_doc)?;
                    if let Some(doc) = doc
                        && matches_simple_filters(&doc, &non_id_filters)
                    {
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

    let primary_order = orders.first().cloned();
    let filters = translate_filter(filter_doc)?;
    let query = Query {
        table: table.clone(),
        filters,
        order: primary_order,
        limit: if orders.len() > 1 { None } else { limit },
    };
    let mut docs = match transaction_token {
        Some(transaction_token) => {
            engine.query_documents_in_transaction(tenant_id, transaction_token, principal, &query)
        }
        None => engine.query_documents_with_principal(tenant_id, &query, principal),
    }
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
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        Ordering::Equal
    });
}

fn compare_json_values(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> Ordering {
    let ra = json_value_rank(a);
    let rb = json_value_rank(b);
    if ra != rb {
        return ra.cmp(&rb);
    }

    match (a, b) {
        (None, None)
        | (None, Some(serde_json::Value::Null))
        | (Some(serde_json::Value::Null), None)
        | (Some(serde_json::Value::Null), Some(serde_json::Value::Null)) => Ordering::Equal,
        (Some(serde_json::Value::Number(na)), Some(serde_json::Value::Number(nb))) => {
            compare_json_numbers(na, nb)
        }
        (Some(serde_json::Value::String(sa)), Some(serde_json::Value::String(sb))) => sa.cmp(sb),
        (Some(serde_json::Value::Object(oa)), Some(serde_json::Value::Object(ob))) => {
            compare_json_objects(oa, ob)
        }
        (Some(serde_json::Value::Array(aa)), Some(serde_json::Value::Array(ab))) => {
            compare_json_arrays(aa, ab)
        }
        (Some(serde_json::Value::Bool(ba)), Some(serde_json::Value::Bool(bb))) => ba.cmp(bb),
        _ => Ordering::Equal,
    }
}

fn json_value_rank(v: Option<&serde_json::Value>) -> u8 {
    match v {
        None | Some(serde_json::Value::Null) => 0,
        Some(serde_json::Value::Number(_)) => 1,
        Some(serde_json::Value::String(_)) => 2,
        Some(serde_json::Value::Object(_)) => 3,
        Some(serde_json::Value::Array(_)) => 4,
        Some(serde_json::Value::Bool(_)) => 5,
    }
}

fn compare_json_numbers(a: &serde_json::Number, b: &serde_json::Number) -> Ordering {
    match (a.as_i64(), b.as_i64()) {
        (Some(ai), Some(bi)) => return ai.cmp(&bi),
        (Some(ai), None) if ai < 0 => return Ordering::Less,
        (None, Some(bi)) if bi < 0 => return Ordering::Greater,
        _ => {}
    }

    match (a.as_u64(), b.as_u64()) {
        (Some(au), Some(bu)) => return au.cmp(&bu),
        (Some(au), None) => {
            if let Some(bi) = b.as_i64() {
                return au.cmp(&(bi as u64));
            }
        }
        (None, Some(bu)) => {
            if let Some(ai) = a.as_i64() {
                return (ai as u64).cmp(&bu);
            }
        }
        _ => {}
    }

    match (a.as_f64(), b.as_f64()) {
        (Some(af), Some(bf)) => af
            .partial_cmp(&bf)
            .unwrap_or_else(|| a.to_string().cmp(&b.to_string())),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

fn compare_json_arrays(a: &[serde_json::Value], b: &[serde_json::Value]) -> Ordering {
    for (av, bv) in a.iter().zip(b.iter()) {
        let cmp = compare_json_values(Some(av), Some(bv));
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    a.len().cmp(&b.len())
}

fn compare_json_objects(
    a: &serde_json::Map<String, serde_json::Value>,
    b: &serde_json::Map<String, serde_json::Value>,
) -> Ordering {
    let mut a_entries = a.iter().collect::<Vec<_>>();
    let mut b_entries = b.iter().collect::<Vec<_>>();
    a_entries.sort_by_key(|(key, _)| *key);
    b_entries.sort_by_key(|(key, _)| *key);

    for ((ak, av), (bk, bv)) in a_entries.iter().zip(b_entries.iter()) {
        let key_cmp = ak.cmp(bk);
        if key_cmp != Ordering::Equal {
            return key_cmp;
        }
        let value_cmp = compare_json_values(Some(av), Some(bv));
        if value_cmp != Ordering::Equal {
            return value_cmp;
        }
    }
    a_entries.len().cmp(&b_entries.len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compare_json_values_orders_json_type_subset() {
        let number = json!(1);
        let string = json!("a");
        let object = json!({ "a": 1 });
        let array = json!([1]);
        let boolean = json!(false);

        assert_eq!(
            compare_json_values(None, Some(&serde_json::Value::Null)),
            Ordering::Equal
        );
        assert!(compare_json_values(Some(&serde_json::Value::Null), Some(&number)).is_lt());
        assert!(compare_json_values(Some(&number), Some(&string)).is_lt());
        assert!(compare_json_values(Some(&string), Some(&object)).is_lt());
        assert!(compare_json_values(Some(&object), Some(&array)).is_lt());
        assert!(compare_json_values(Some(&array), Some(&boolean)).is_lt());
    }

    #[test]
    fn compare_json_values_orders_arrays_lexicographically() {
        let one = json!([1]);
        let one_two = json!([1, 2]);
        let two = json!([2]);
        let another_two = json!([2]);

        assert!(compare_json_values(Some(&one), Some(&one_two)).is_lt());
        assert!(compare_json_values(Some(&one_two), Some(&two)).is_lt());
        assert_eq!(
            compare_json_values(Some(&two), Some(&another_two)),
            Ordering::Equal
        );
    }

    #[test]
    fn compare_json_values_orders_objects_by_key_then_value() {
        let a_one = json!({ "a": 1 });
        let a_one_b_zero = json!({ "a": 1, "b": 0 });
        let a_two = json!({ "a": 2 });
        let b_zero = json!({ "b": 0 });

        assert!(compare_json_values(Some(&a_one), Some(&a_one_b_zero)).is_lt());
        assert!(compare_json_values(Some(&a_one), Some(&a_two)).is_lt());
        assert!(compare_json_values(Some(&a_one), Some(&b_zero)).is_lt());
    }
}
