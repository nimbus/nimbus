use neovex_core::{
    AtomicWriteBatchOutcome, AtomicWriteResult, Document, DocumentPath, Error, Result,
    StructuredAggregationResult, Timestamp,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tonic::Code;

use super::errors::firestore_google_rpc_status_json;
use super::resource_names;
use super::serializer;
use super::{BatchWriteOutcome, RunQueryDocument};

pub(crate) fn run_query_response_entries(
    database: &resource_names::FirestoreDatabaseName,
    documents: Vec<RunQueryDocument>,
    read_time: Timestamp,
    skipped_results: usize,
) -> Result<Vec<Value>> {
    let read_time = format_timestamp(read_time)?;
    if documents.is_empty() {
        let mut response = serde_json::Map::new();
        response.insert("readTime".to_string(), Value::String(read_time));
        if skipped_results > 0 {
            response.insert("skippedResults".to_string(), json!(skipped_results));
        }
        return Ok(vec![Value::Object(response)]);
    }

    let mut responses = Vec::with_capacity(documents.len());
    for (index, entry) in documents.into_iter().enumerate() {
        let document_name = firestore_document_name(database, &entry.document_path);
        let mut response = serde_json::Map::new();
        response.insert(
            "document".to_string(),
            firestore_document_json(&document_name, &entry.document, None)?,
        );
        response.insert("readTime".to_string(), Value::String(read_time.clone()));
        if index == 0 && skipped_results > 0 {
            response.insert("skippedResults".to_string(), json!(skipped_results));
        }
        responses.push(Value::Object(response));
    }
    Ok(responses)
}

pub(crate) fn run_aggregation_query_response_entries(
    result: &StructuredAggregationResult,
    read_time: Timestamp,
) -> Result<Vec<Value>> {
    let read_time = format_timestamp(read_time)?;
    Ok(vec![json!({
        "result": aggregation_result_json(result)?,
        "readTime": read_time,
    })])
}

fn aggregation_result_json(result: &StructuredAggregationResult) -> Result<Value> {
    let mut aggregate_fields = serde_json::Map::new();
    for (alias, value) in &result.aggregate_fields {
        aggregate_fields.insert(
            alias.clone(),
            serializer::encode_proto_json_value(value).map_err(|error| {
                Error::Serialization(format!(
                    "failed to encode Firestore aggregation field `{alias}`: {error}"
                ))
            })?,
        );
    }
    Ok(json!({ "aggregateFields": aggregate_fields }))
}

pub(crate) fn batch_get_entry_json(
    document_name: &str,
    document: Option<Document>,
    mask: Option<&[String]>,
    read_time: &str,
) -> Result<Value> {
    match document {
        Some(document) => Ok(json!({
            "found": firestore_document_json(document_name, &document, mask)?,
            "readTime": read_time,
        })),
        None => Ok(json!({
            "missing": document_name,
            "readTime": read_time,
        })),
    }
}

pub(crate) fn firestore_parent_name(
    database: &resource_names::FirestoreDatabaseName,
    parent_document_path: Option<&DocumentPath>,
) -> String {
    match parent_document_path {
        Some(parent_document_path) => format!(
            "projects/{}/databases/(default)/documents/{}",
            database.project_id, parent_document_path
        ),
        None => format!(
            "projects/{}/databases/(default)/documents",
            database.project_id
        ),
    }
}

pub(crate) fn firestore_document_name(
    database: &resource_names::FirestoreDatabaseName,
    document_path: &DocumentPath,
) -> String {
    format!(
        "projects/{}/databases/(default)/documents/{}",
        database.project_id, document_path
    )
}

fn firestore_document_json(
    document_name: &str,
    document: &Document,
    mask: Option<&[String]>,
) -> Result<Value> {
    let mut object = serde_json::Map::new();
    let create_time = format_timestamp(document.creation_time)?;
    let update_time = format_timestamp(document.update_time)?;
    object.insert("name".to_string(), Value::String(document_name.to_string()));
    object.insert("createTime".to_string(), Value::String(create_time));
    object.insert("updateTime".to_string(), Value::String(update_time));
    object.insert(
        "fields".to_string(),
        Value::Object(firestore_document_fields(document, mask)?),
    );
    Ok(Value::Object(object))
}

fn firestore_document_fields(
    document: &Document,
    mask: Option<&[String]>,
) -> Result<serde_json::Map<String, Value>> {
    let mut fields = serde_json::Map::new();
    match mask {
        Some(mask) => {
            for field_name in mask {
                if let Some(value) = document.fields.get(field_name) {
                    fields.insert(
                        field_name.clone(),
                        encode_firestore_field_value(document, field_name, value)?,
                    );
                }
            }
        }
        None => {
            for (field_name, value) in &document.fields {
                fields.insert(
                    field_name.clone(),
                    encode_firestore_field_value(document, field_name, value)?,
                );
            }
        }
    }
    Ok(fields)
}

fn encode_firestore_field_value(
    document: &Document,
    field_name: &str,
    value: &Value,
) -> Result<Value> {
    serializer::encode_proto_json_document_value(document, field_name, value).map_err(|error| {
        Error::Serialization(format!(
            "failed to encode Firestore document field `{field_name}`: {error}"
        ))
    })
}

pub(crate) fn serialize_json_lines(entries: &[Value]) -> Result<String> {
    entries
        .iter()
        .map(|entry| {
            serde_json::to_string(entry).map_err(|error| {
                Error::Serialization(format!("failed to encode JSON response: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|lines| lines.join("\n"))
}

pub(crate) fn commit_response_json(outcome: &AtomicWriteBatchOutcome) -> Result<Value> {
    let write_results = outcome
        .write_results
        .iter()
        .map(write_result_json)
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "writeResults": write_results,
        "commitTime": format_timestamp(outcome.commit_time)?,
    }))
}

pub(crate) fn batch_write_response_json(outcome: &BatchWriteOutcome) -> Result<Value> {
    Ok(json!({
        "writeResults": outcome
            .entries
            .iter()
            .map(|entry| match entry.write_result.as_ref() {
                Some(write_result) => write_result_json(write_result),
                None => Ok(json!({})),
            })
            .collect::<Result<Vec<_>>>()?,
        "status": outcome
            .entries
            .iter()
            .map(|entry| match entry.error.as_ref() {
                Some(error) => Ok(firestore_google_rpc_status_json(error)),
                None => Ok(json!({ "code": Code::Ok as i32 })),
            })
            .collect::<Result<Vec<_>>>()?,
    }))
}

pub(crate) fn write_result_json(result: &AtomicWriteResult) -> Result<Value> {
    let mut object = serde_json::Map::new();
    if let Some(update_time) = result.update_time {
        object.insert(
            "updateTime".to_string(),
            Value::String(format_timestamp(update_time)?),
        );
    }
    if !result.transform_results.is_empty() {
        let encoded = result
            .transform_results
            .iter()
            .map(serializer::encode_proto_json_stored_value)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| Error::Serialization(error.to_string()))?;
        object.insert("transformResults".to_string(), Value::Array(encoded));
    }
    Ok(Value::Object(object))
}

pub(crate) fn format_timestamp(timestamp: Timestamp) -> Result<String> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp.0) * 1_000_000)
        .map_err(|error| {
            Error::Serialization(format!("invalid timestamp {}: {error}", timestamp.0))
        })?
        .format(&Rfc3339)
        .map_err(|error| Error::Serialization(format!("failed to format timestamp: {error}")))
}
