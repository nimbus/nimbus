use std::collections::HashMap;
use std::sync::Arc;

use neovex_core::{
    Document, Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query, TableName,
    TenantId,
};
use neovex_engine::Service;

use super::super::bson_bridge;
use super::super::connection::ConnectionState;
use super::super::error::{BAD_VALUE, MongoError};
use super::crud::apply_projection;
use super::tenant::{DEFAULT_TENANT, ensure_tenant, resolve_tenant};

pub fn aggregate(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("aggregate").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in aggregate command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let pipeline = body
        .get_array("pipeline")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing pipeline array in aggregate command".into(),
        })?;

    let batch_size = match body
        .get_document("cursor")
        .ok()
        .and_then(|c| c.get("batchSize"))
    {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        _ => None,
    };

    ensure_tenant(service, &tenant_id)?;

    if is_change_stream_pipeline(pipeline) {
        let resume_token = get_change_stream_options(pipeline)
            .and_then(super::change_stream::extract_resume_option);
        return open_change_stream_aggregate(conn, service, collection, db_name, resume_token);
    }

    let stages = parse_pipeline(pipeline)?;

    let documents = load_initial_documents(service, &tenant_id, &table, &stages)?;

    let mut bson_docs: Vec<bson::Document> = documents
        .into_iter()
        .map(|doc| bson_bridge::document_to_bson_doc(&doc))
        .collect();

    for stage in &stages {
        bson_docs = execute_stage(stage, bson_docs)?;
    }

    let effective_batch_size = batch_size.unwrap_or(101);
    let ns = format!("{db_name}.{collection}");

    let (cursor_id, first_batch) =
        conn.cursor_store
            .create(ns.clone(), bson_docs, effective_batch_size);

    Ok(bson::doc! {
        "cursor": {
            "firstBatch": first_batch,
            "id": cursor_id,
            "ns": &ns,
        },
        "ok": 1.0,
    })
}

#[derive(Debug)]
enum Stage {
    Match(bson::Document),
    Sort(bson::Document),
    Limit(usize),
    Skip(usize),
    Project(bson::Document),
    AddFields(bson::Document),
    Count(String),
    Group {
        id_expr: bson::Bson,
        accumulators: Vec<(String, String, bson::Bson)>,
    },
    Unwind {
        path: String,
        preserve_null: bool,
        include_index: Option<String>,
    },
}

fn parse_pipeline(pipeline: &[bson::Bson]) -> Result<Vec<Stage>, MongoError> {
    let mut stages = Vec::new();
    for stage_bson in pipeline {
        let stage_doc = stage_bson
            .as_document()
            .ok_or_else(|| MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: "pipeline stage must be a document".into(),
            })?;

        let (name, value) = stage_doc.iter().next().ok_or_else(|| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "pipeline stage must have exactly one field".into(),
        })?;

        let stage = match name.as_str() {
            "$match" => {
                let doc = value.as_document().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$match requires a document".into(),
                })?;
                Stage::Match(doc.clone())
            }
            "$sort" => {
                let doc = value.as_document().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$sort requires a document".into(),
                })?;
                Stage::Sort(doc.clone())
            }
            "$limit" => {
                let n = bson_to_usize(value).ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$limit requires a positive integer".into(),
                })?;
                Stage::Limit(n)
            }
            "$skip" => {
                let n = bson_to_usize(value).ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$skip requires a non-negative integer".into(),
                })?;
                Stage::Skip(n)
            }
            "$project" => {
                let doc = value.as_document().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$project requires a document".into(),
                })?;
                Stage::Project(doc.clone())
            }
            "$addFields" => {
                let doc = value.as_document().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$addFields requires a document".into(),
                })?;
                Stage::AddFields(doc.clone())
            }
            "$count" => {
                let field = match value {
                    bson::Bson::String(s) => s.clone(),
                    _ => {
                        return Err(MongoError::Command {
                            code: BAD_VALUE.code,
                            code_name: BAD_VALUE.code_name.into(),
                            message: "$count requires a string field name".into(),
                        });
                    }
                };
                Stage::Count(field)
            }
            "$group" => {
                let doc = value.as_document().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$group requires a document".into(),
                })?;
                let id_expr = doc.get("_id").cloned().ok_or_else(|| MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: "$group requires an _id expression".into(),
                })?;
                let mut accumulators = Vec::new();
                for (field, acc_val) in doc.iter() {
                    if field == "_id" {
                        continue;
                    }
                    let acc_doc = acc_val.as_document().ok_or_else(|| MongoError::Command {
                        code: BAD_VALUE.code,
                        code_name: BAD_VALUE.code_name.into(),
                        message: format!("$group accumulator for '{field}' must be a document"),
                    })?;
                    let (op, operand) =
                        acc_doc.iter().next().ok_or_else(|| MongoError::Command {
                            code: BAD_VALUE.code,
                            code_name: BAD_VALUE.code_name.into(),
                            message: format!("$group accumulator for '{field}' is empty"),
                        })?;
                    accumulators.push((field.clone(), op.clone(), operand.clone()));
                }
                Stage::Group {
                    id_expr,
                    accumulators,
                }
            }
            "$unwind" => match value {
                bson::Bson::String(path) => Stage::Unwind {
                    path: path.trim_start_matches('$').to_string(),
                    preserve_null: false,
                    include_index: None,
                },
                bson::Bson::Document(doc) => {
                    let path = doc
                        .get_str("path")
                        .map_err(|_| MongoError::Command {
                            code: BAD_VALUE.code,
                            code_name: BAD_VALUE.code_name.into(),
                            message: "$unwind requires a path field".into(),
                        })?
                        .trim_start_matches('$')
                        .to_string();
                    let preserve_null = doc.get_bool("preserveNullAndEmptyArrays").unwrap_or(false);
                    let include_index =
                        doc.get_str("includeArrayIndex").ok().map(|s| s.to_string());
                    Stage::Unwind {
                        path,
                        preserve_null,
                        include_index,
                    }
                }
                _ => {
                    return Err(MongoError::Command {
                        code: BAD_VALUE.code,
                        code_name: BAD_VALUE.code_name.into(),
                        message: "$unwind requires a string path or document".into(),
                    });
                }
            },
            other => {
                return Err(MongoError::Command {
                    code: 40324,
                    code_name: "Location40324".into(),
                    message: format!("Unrecognized pipeline stage name: '{other}'"),
                });
            }
        };
        stages.push(stage);
    }
    Ok(stages)
}

fn load_initial_documents(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    _stages: &[Stage],
) -> Result<Vec<Document>, MongoError> {
    let principal = PrincipalContext::system();
    let query = Query {
        table: table.clone(),
        filters: vec![],
        order: None,
        limit: None,
    };
    service
        .query_documents_with_principal(tenant_id, &query, &principal)
        .map_err(MongoError::from)
}

fn execute_stage(
    stage: &Stage,
    docs: Vec<bson::Document>,
) -> Result<Vec<bson::Document>, MongoError> {
    match stage {
        Stage::Match(filter) => execute_match(docs, filter),
        Stage::Sort(sort_doc) => Ok(execute_sort(docs, sort_doc)),
        Stage::Limit(n) => Ok(docs.into_iter().take(*n).collect()),
        Stage::Skip(n) => Ok(docs.into_iter().skip(*n).collect()),
        Stage::Project(proj) => Ok(execute_project(docs, proj)),
        Stage::AddFields(fields) => Ok(execute_add_fields(docs, fields)),
        Stage::Count(field) => Ok(vec![bson::doc! { field: docs.len() as i64 }]),
        Stage::Group {
            id_expr,
            accumulators,
        } => execute_group(docs, id_expr, accumulators),
        Stage::Unwind {
            path,
            preserve_null,
            include_index,
        } => Ok(execute_unwind(docs, path, *preserve_null, include_index)),
    }
}

fn execute_match(
    docs: Vec<bson::Document>,
    filter: &bson::Document,
) -> Result<Vec<bson::Document>, MongoError> {
    if filter.is_empty() {
        return Ok(docs);
    }
    let mut result = Vec::new();
    for doc in docs {
        if bson_doc_matches_filter(&doc, filter) {
            result.push(doc);
        }
    }
    Ok(result)
}

fn bson_doc_matches_filter(doc: &bson::Document, filter: &bson::Document) -> bool {
    for (field, value) in filter.iter() {
        if field.starts_with('$') {
            continue;
        }
        let doc_val = doc.get(field);
        match value {
            bson::Bson::Document(ops) if ops.keys().any(|k| k.starts_with('$')) => {
                for (op, op_val) in ops.iter() {
                    let matches = match op.as_str() {
                        "$eq" => doc_val.is_some_and(|v| v == op_val),
                        "$ne" => doc_val.is_none_or(|v| v != op_val),
                        "$gt" => doc_val
                            .is_some_and(|v| bson_cmp(v, op_val) == std::cmp::Ordering::Greater),
                        "$gte" => {
                            doc_val.is_some_and(|v| bson_cmp(v, op_val) != std::cmp::Ordering::Less)
                        }
                        "$lt" => {
                            doc_val.is_some_and(|v| bson_cmp(v, op_val) == std::cmp::Ordering::Less)
                        }
                        "$lte" => doc_val
                            .is_some_and(|v| bson_cmp(v, op_val) != std::cmp::Ordering::Greater),
                        _ => true,
                    };
                    if !matches {
                        return false;
                    }
                }
            }
            _ => {
                if doc_val != Some(value) {
                    return false;
                }
            }
        }
    }
    true
}

fn bson_cmp(a: &bson::Bson, b: &bson::Bson) -> std::cmp::Ordering {
    let a_f = bson_as_f64(a);
    let b_f = bson_as_f64(b);
    match (a_f, b_f) {
        (Some(a_val), Some(b_val)) => a_val
            .partial_cmp(&b_val)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
    }
}

fn bson_as_f64(value: &bson::Bson) -> Option<f64> {
    match value {
        bson::Bson::Int32(n) => Some(*n as f64),
        bson::Bson::Int64(n) => Some(*n as f64),
        bson::Bson::Double(f) => Some(*f),
        _ => None,
    }
}

fn execute_sort(mut docs: Vec<bson::Document>, sort_doc: &bson::Document) -> Vec<bson::Document> {
    if let Some((field, direction_val)) = sort_doc.iter().next() {
        let descending = matches!(
            direction_val,
            bson::Bson::Int32(n) if *n < 0
        ) || matches!(
            direction_val,
            bson::Bson::Int64(n) if *n < 0
        ) || matches!(
            direction_val,
            bson::Bson::Double(f) if *f < 0.0
        );
        let field = field.clone();
        docs.sort_by(|a, b| {
            let a_val = a.get(&field);
            let b_val = b.get(&field);
            let cmp = match (a_val.and_then(bson_as_f64), b_val.and_then(bson_as_f64)) {
                (Some(a_f), Some(b_f)) => {
                    a_f.partial_cmp(&b_f).unwrap_or(std::cmp::Ordering::Equal)
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => match (a_val, b_val) {
                    (Some(bson::Bson::String(a_s)), Some(bson::Bson::String(b_s))) => a_s.cmp(b_s),
                    _ => std::cmp::Ordering::Equal,
                },
            };
            if descending { cmp.reverse() } else { cmp }
        });
    }
    docs
}

fn execute_project(docs: Vec<bson::Document>, projection: &bson::Document) -> Vec<bson::Document> {
    docs.into_iter()
        .map(|mut doc| {
            apply_projection(&mut doc, projection);
            doc
        })
        .collect()
}

fn execute_add_fields(docs: Vec<bson::Document>, fields: &bson::Document) -> Vec<bson::Document> {
    docs.into_iter()
        .map(|mut doc| {
            for (key, val) in fields.iter() {
                let resolved = resolve_bson_expr(val, &doc);
                doc.insert(key, resolved);
            }
            doc
        })
        .collect()
}

fn resolve_bson_expr(expr: &bson::Bson, doc: &bson::Document) -> bson::Bson {
    match expr {
        bson::Bson::String(s) if s.starts_with('$') => {
            let field = &s[1..];
            doc.get(field).cloned().unwrap_or(bson::Bson::Null)
        }
        other => other.clone(),
    }
}

fn execute_group(
    docs: Vec<bson::Document>,
    id_expr: &bson::Bson,
    accumulators: &[(String, String, bson::Bson)],
) -> Result<Vec<bson::Document>, MongoError> {
    let mut groups: HashMap<String, (bson::Bson, Vec<bson::Document>)> = HashMap::new();
    let mut order = Vec::new();

    for doc in &docs {
        let group_key = evaluate_group_id(id_expr, doc);
        let key_str = format!("{group_key:?}");
        let entry = groups
            .entry(key_str.clone())
            .or_insert_with(|| (group_key, Vec::new()));
        if !order.contains(&key_str) {
            order.push(key_str);
        }
        entry.1.push(doc.clone());
    }

    let mut result = Vec::new();
    for key_str in &order {
        let (group_key, group_docs) = groups.remove(key_str).unwrap();
        let mut out_doc = bson::doc! { "_id": group_key };

        for (field, op, operand) in accumulators {
            let value = compute_accumulator(op, operand, &group_docs)?;
            out_doc.insert(field, value);
        }

        result.push(out_doc);
    }

    Ok(result)
}

fn evaluate_group_id(expr: &bson::Bson, doc: &bson::Document) -> bson::Bson {
    match expr {
        bson::Bson::String(s) if s.starts_with('$') => {
            doc.get(&s[1..]).cloned().unwrap_or(bson::Bson::Null)
        }
        bson::Bson::Null => bson::Bson::Null,
        other => other.clone(),
    }
}

fn compute_accumulator(
    op: &str,
    operand: &bson::Bson,
    docs: &[bson::Document],
) -> Result<bson::Bson, MongoError> {
    match op {
        "$sum" => {
            let mut total = 0.0_f64;
            for doc in docs {
                let val = resolve_accumulator_value(operand, doc);
                if let Some(n) = bson_as_f64(&val) {
                    total += n;
                }
            }
            if total == total.floor() && total.abs() < i64::MAX as f64 {
                Ok(bson::Bson::Int64(total as i64))
            } else {
                Ok(bson::Bson::Double(total))
            }
        }
        "$avg" => {
            let mut total = 0.0_f64;
            let mut count = 0_usize;
            for doc in docs {
                let val = resolve_accumulator_value(operand, doc);
                if let Some(n) = bson_as_f64(&val) {
                    total += n;
                    count += 1;
                }
            }
            if count == 0 {
                Ok(bson::Bson::Null)
            } else {
                Ok(bson::Bson::Double(total / count as f64))
            }
        }
        "$min" => {
            let mut min: Option<f64> = None;
            for doc in docs {
                let val = resolve_accumulator_value(operand, doc);
                if let Some(n) = bson_as_f64(&val) {
                    min = Some(min.map_or(n, |m: f64| m.min(n)));
                }
            }
            Ok(min.map_or(bson::Bson::Null, bson::Bson::Double))
        }
        "$max" => {
            let mut max: Option<f64> = None;
            for doc in docs {
                let val = resolve_accumulator_value(operand, doc);
                if let Some(n) = bson_as_f64(&val) {
                    max = Some(max.map_or(n, |m: f64| m.max(n)));
                }
            }
            Ok(max.map_or(bson::Bson::Null, bson::Bson::Double))
        }
        "$first" => {
            let val = docs
                .first()
                .map(|d| resolve_accumulator_value(operand, d))
                .unwrap_or(bson::Bson::Null);
            Ok(val)
        }
        "$last" => {
            let val = docs
                .last()
                .map(|d| resolve_accumulator_value(operand, d))
                .unwrap_or(bson::Bson::Null);
            Ok(val)
        }
        "$push" => {
            let arr: Vec<bson::Bson> = docs
                .iter()
                .map(|d| resolve_accumulator_value(operand, d))
                .collect();
            Ok(bson::Bson::Array(arr))
        }
        "$addToSet" => {
            let mut seen = Vec::<bson::Bson>::new();
            for doc in docs {
                let val = resolve_accumulator_value(operand, doc);
                if !seen.contains(&val) {
                    seen.push(val);
                }
            }
            Ok(bson::Bson::Array(seen))
        }
        other => Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: format!("unsupported $group accumulator: {other}"),
        }),
    }
}

fn resolve_accumulator_value(operand: &bson::Bson, doc: &bson::Document) -> bson::Bson {
    match operand {
        bson::Bson::String(s) if s.starts_with('$') => {
            doc.get(&s[1..]).cloned().unwrap_or(bson::Bson::Null)
        }
        bson::Bson::Int32(n) => bson::Bson::Int32(*n),
        bson::Bson::Int64(n) => bson::Bson::Int64(*n),
        bson::Bson::Double(f) => bson::Bson::Double(*f),
        other => other.clone(),
    }
}

fn execute_unwind(
    docs: Vec<bson::Document>,
    path: &str,
    preserve_null: bool,
    include_index: &Option<String>,
) -> Vec<bson::Document> {
    let mut result = Vec::new();
    for doc in docs {
        match doc.get(path) {
            Some(bson::Bson::Array(arr)) if !arr.is_empty() => {
                for (i, elem) in arr.iter().enumerate() {
                    let mut new_doc = doc.clone();
                    new_doc.insert(path, elem.clone());
                    if let Some(idx_field) = include_index {
                        new_doc.insert(idx_field, i as i64);
                    }
                    result.push(new_doc);
                }
            }
            Some(bson::Bson::Array(_)) | None => {
                if preserve_null {
                    let mut new_doc = doc.clone();
                    if doc.get(path).is_none() {
                        // field missing — keep doc as is
                    } else {
                        new_doc.remove(path);
                    }
                    if let Some(idx_field) = include_index {
                        new_doc.insert(idx_field, bson::Bson::Null);
                    }
                    result.push(new_doc);
                }
            }
            Some(_) => {
                let mut new_doc = doc;
                if let Some(idx_field) = include_index {
                    new_doc.insert(idx_field, bson::Bson::Null);
                }
                result.push(new_doc);
            }
        }
    }
    result
}

fn translate_filter(filter_doc: &bson::Document) -> Result<Vec<Filter>, MongoError> {
    let mut filters = Vec::new();
    for (field, value) in filter_doc.iter() {
        if field.starts_with('$') {
            return Err(MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: format!("top-level operator {field} not supported"),
            });
        }
        match value {
            bson::Bson::Document(ops) if ops.keys().any(|k| k.starts_with('$')) => {
                for (op_key, op_val) in ops.iter() {
                    let op = match op_key.as_str() {
                        "$eq" => FilterOp::Eq,
                        "$ne" => FilterOp::Neq,
                        "$gt" => FilterOp::Gt,
                        "$gte" => FilterOp::Gte,
                        "$lt" => FilterOp::Lt,
                        "$lte" => FilterOp::Lte,
                        _ => {
                            return Err(MongoError::Command {
                                code: BAD_VALUE.code,
                                code_name: BAD_VALUE.code_name.into(),
                                message: format!("unsupported filter operator: {op_key}"),
                            });
                        }
                    };
                    filters.push(Filter {
                        field: field.to_string(),
                        op,
                        value: bson_to_json(op_val),
                    });
                }
            }
            _ => {
                filters.push(Filter {
                    field: field.to_string(),
                    op: FilterOp::Eq,
                    value: bson_to_json(value),
                });
            }
        }
    }
    Ok(filters)
}

fn translate_sort(sort_doc: &bson::Document) -> Option<OrderBy> {
    let (field, direction_val) = sort_doc.iter().next()?;
    let direction = match direction_val {
        bson::Bson::Int32(n) if *n < 0 => OrderDirection::Desc,
        bson::Bson::Int64(n) if *n < 0 => OrderDirection::Desc,
        bson::Bson::Double(f) if *f < 0.0 => OrderDirection::Desc,
        _ => OrderDirection::Asc,
    };
    Some(OrderBy {
        field: field.to_string(),
        direction,
    })
}

fn bson_to_json(value: &bson::Bson) -> serde_json::Value {
    match value {
        bson::Bson::Null => serde_json::Value::Null,
        bson::Bson::Boolean(b) => serde_json::Value::Bool(*b),
        bson::Bson::Int32(n) => serde_json::Value::Number((*n).into()),
        bson::Bson::Int64(n) => serde_json::Value::Number((*n).into()),
        bson::Bson::Double(f) => serde_json::json!(*f),
        bson::Bson::String(s) => serde_json::Value::String(s.clone()),
        _ => serde_json::Value::Null,
    }
}

fn bson_to_usize(value: &bson::Bson) -> Option<usize> {
    match value {
        bson::Bson::Int32(n) if *n >= 0 => Some(*n as usize),
        bson::Bson::Int64(n) if *n >= 0 => Some(*n as usize),
        bson::Bson::Double(f) if *f >= 0.0 => Some(*f as usize),
        _ => None,
    }
}

fn get_change_stream_options(pipeline: &[bson::Bson]) -> Option<&bson::Document> {
    let first = pipeline.first()?.as_document()?;
    let cs_value = first.get("$changeStream")?;
    cs_value.as_document()
}

fn is_change_stream_pipeline(pipeline: &[bson::Bson]) -> bool {
    pipeline
        .first()
        .and_then(|s| s.as_document())
        .is_some_and(|doc| doc.contains_key("$changeStream"))
}

fn open_change_stream_aggregate(
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    collection: &str,
    db_name: &str,
    resume_token: Option<super::change_stream::ResumeToken>,
) -> Result<bson::Document, MongoError> {
    let ns = format!("{}.{}", db_name, collection);
    let (cursor_id, cursor) =
        super::change_stream::open_change_stream(collection, db_name, service, resume_token)?;
    conn.change_stream_store.insert(cursor_id, cursor);

    Ok(bson::doc! {
        "cursor": {
            "firstBatch": Vec::<bson::Bson>::new(),
            "id": cursor_id,
            "ns": &ns,
        },
        "ok": 1.0,
    })
}

#[cfg(test)]
mod tests;
