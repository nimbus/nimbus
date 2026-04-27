use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neovex_core::{
    AtomicWrite, AtomicWriteBatch, FieldTransform, FieldTransformOperation, Timestamp, WriteKey,
    WritePrecondition, WriteSetMode,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::resource_names::{self, FirestoreDatabaseName, FirestoreResourceNameError};
use super::serializer::{self, FirestoreProtoJsonError};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedCommitRequest {
    pub database: FirestoreDatabaseName,
    pub batch: AtomicWriteBatch,
    pub transaction: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreCommitRequestError {
    #[error("invalid Firestore Commit request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore Commit feature: {0}")]
    Unsupported(String),
    #[error(transparent)]
    InvalidResource(#[from] FirestoreResourceNameError),
    #[error(transparent)]
    InvalidValue(#[from] FirestoreProtoJsonError),
}

pub(crate) fn parse_commit_request_with_resolver(
    request: &Value,
    mut resolve_write_key: impl FnMut(
        &neovex_core::DocumentPath,
    ) -> Result<WriteKey, FirestoreCommitRequestError>,
) -> Result<ParsedCommitRequest, FirestoreCommitRequestError> {
    let request: CommitRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let database = resource_names::parse_database_name(&request.database)?;
    let transaction = request
        .transaction
        .as_deref()
        .map(parse_transaction)
        .transpose()?;

    let writes = request
        .writes
        .into_iter()
        .map(|write| lower_commit_write(write, &database, &mut resolve_write_key))
        .collect::<Result<Vec<_>, _>>()?;
    let batch = AtomicWriteBatch::new(writes)
        .map_err(|error| invalid_request(format!("invalid atomic write batch: {error}")))?;

    Ok(ParsedCommitRequest {
        database,
        batch,
        transaction,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitRequestJson {
    database: String,
    #[serde(default)]
    writes: Vec<CommitWriteJson>,
    transaction: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitWriteJson {
    update: Option<FirestoreDocumentJson>,
    delete: Option<String>,
    verify: Option<String>,
    transform: Option<FirestoreDocumentTransformJson>,
    update_mask: Option<FirestoreDocumentMaskJson>,
    #[serde(default)]
    update_transforms: Vec<FirestoreFieldTransformJson>,
    current_document: Option<FirestorePreconditionJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreDocumentJson {
    name: String,
    #[serde(default)]
    fields: Map<String, Value>,
    create_time: Option<String>,
    update_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreDocumentMaskJson {
    #[serde(default)]
    field_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestorePreconditionJson {
    exists: Option<bool>,
    update_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreDocumentTransformJson {
    document: String,
    #[serde(default)]
    field_transforms: Vec<FirestoreFieldTransformJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreFieldTransformJson {
    field_path: String,
    set_to_server_value: Option<String>,
    increment: Option<Value>,
    maximum: Option<Value>,
    minimum: Option<Value>,
    append_missing_elements: Option<FirestoreArrayValueJson>,
    remove_all_from_array: Option<FirestoreArrayValueJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreArrayValueJson {
    #[serde(default)]
    values: Vec<Value>,
}

fn lower_commit_write(
    write: CommitWriteJson,
    database: &FirestoreDatabaseName,
    resolve_write_key: &mut impl FnMut(
        &neovex_core::DocumentPath,
    ) -> Result<WriteKey, FirestoreCommitRequestError>,
) -> Result<AtomicWrite, FirestoreCommitRequestError> {
    let operation_count = u8::from(write.update.is_some())
        + u8::from(write.delete.is_some())
        + u8::from(write.verify.is_some())
        + u8::from(write.transform.is_some());
    if operation_count != 1 {
        return Err(invalid_request(
            "each write must set exactly one of `update`, `delete`, `verify`, or `transform`",
        ));
    }

    if let Some(update) = write.update {
        let parsed_document = resource_names::parse_document_name(&update.name)?;
        ensure_database_match(database, &parsed_document.database, "update document")?;
        reject_document_metadata(&update)?;

        let key = resolve_write_key(&parsed_document.document_path)?;
        let document = lower_document_fields(update.fields)?;
        let precondition = lower_precondition(write.current_document)?;
        let transforms = lower_field_transforms(write.update_transforms)?;

        return Ok(match write.update_mask {
            Some(mask) => AtomicWrite::Patch {
                key,
                field_patch: document,
                mask: mask.field_paths,
                precondition,
                transforms,
            },
            None => AtomicWrite::Set {
                key,
                document,
                mode: WriteSetMode::Overwrite,
                precondition,
                transforms,
            },
        });
    }

    if !write.update_transforms.is_empty() {
        return Err(invalid_request(
            "`updateTransforms` can only be set when `update` is present",
        ));
    }
    if write.update_mask.is_some() {
        return Err(invalid_request(
            "`updateMask` can only be set when `update` is present",
        ));
    }

    if let Some(delete_name) = write.delete {
        let parsed_document = resource_names::parse_document_name(&delete_name)?;
        ensure_database_match(database, &parsed_document.database, "delete document")?;
        let key = resolve_write_key(&parsed_document.document_path)?;
        let precondition = lower_precondition(write.current_document)?;
        let missing_ok = precondition.is_empty();
        return Ok(AtomicWrite::Delete {
            key,
            precondition,
            missing_ok,
        });
    }

    if let Some(verify_name) = write.verify {
        let parsed_document = resource_names::parse_document_name(&verify_name)?;
        ensure_database_match(database, &parsed_document.database, "verify document")?;
        let key = resolve_write_key(&parsed_document.document_path)?;
        let precondition = lower_precondition(write.current_document)?;
        return Ok(AtomicWrite::Verify { key, precondition });
    }

    let transform = write
        .transform
        .ok_or_else(|| invalid_request("missing write operation"))?;
    let parsed_document = resource_names::parse_document_name(&transform.document)?;
    ensure_database_match(database, &parsed_document.database, "transform document")?;
    let key = resolve_write_key(&parsed_document.document_path)?;
    let precondition = lower_precondition(write.current_document)?;
    let transforms = lower_field_transforms(transform.field_transforms)?;
    if transforms.is_empty() {
        return Err(invalid_request(
            "`transform.fieldTransforms` must contain at least one transform",
        ));
    }
    Ok(AtomicWrite::Transform {
        key,
        transforms,
        precondition,
    })
}

fn lower_document_fields(
    fields: Map<String, Value>,
) -> Result<Map<String, Value>, FirestoreCommitRequestError> {
    fields
        .into_iter()
        .map(|(field, value)| {
            serializer::decode_proto_json_value(&value).map(|value| (field, value))
        })
        .collect::<Result<Map<_, _>, _>>()
        .map_err(Into::into)
}

fn lower_precondition(
    precondition: Option<FirestorePreconditionJson>,
) -> Result<WritePrecondition, FirestoreCommitRequestError> {
    let Some(precondition) = precondition else {
        return Ok(WritePrecondition::default());
    };
    let update_time = precondition
        .update_time
        .as_deref()
        .map(parse_timestamp)
        .transpose()?;
    let precondition = WritePrecondition {
        exists: precondition.exists,
        update_time,
    };
    precondition.validate().map_err(|error| {
        invalid_request(format!("invalid currentDocument precondition: {error}"))
    })?;
    Ok(precondition)
}

fn lower_field_transforms(
    transforms: Vec<FirestoreFieldTransformJson>,
) -> Result<Vec<FieldTransform>, FirestoreCommitRequestError> {
    transforms
        .into_iter()
        .map(lower_field_transform)
        .collect::<Result<Vec<_>, _>>()
}

fn lower_field_transform(
    transform: FirestoreFieldTransformJson,
) -> Result<FieldTransform, FirestoreCommitRequestError> {
    if transform.field_path.is_empty() {
        return Err(invalid_request(
            "field transform `fieldPath` cannot be empty",
        ));
    }

    let transform_count = u8::from(transform.set_to_server_value.is_some())
        + u8::from(transform.increment.is_some())
        + u8::from(transform.maximum.is_some())
        + u8::from(transform.minimum.is_some())
        + u8::from(transform.append_missing_elements.is_some())
        + u8::from(transform.remove_all_from_array.is_some());
    if transform_count != 1 {
        return Err(invalid_request(
            "each field transform must set exactly one transform type",
        ));
    }

    let operation = if let Some(server_value) = transform.set_to_server_value {
        if server_value == "REQUEST_TIME" {
            FieldTransformOperation::ServerTimestamp
        } else {
            return Err(unsupported(format!(
                "unsupported setToServerValue `{server_value}`"
            )));
        }
    } else if let Some(operand) = transform.increment {
        FieldTransformOperation::Increment {
            operand: serializer::decode_proto_json_numeric_value(&operand)?,
        }
    } else if let Some(operand) = transform.maximum {
        FieldTransformOperation::Maximum {
            operand: serializer::decode_proto_json_numeric_value(&operand)?,
        }
    } else if let Some(operand) = transform.minimum {
        FieldTransformOperation::Minimum {
            operand: serializer::decode_proto_json_numeric_value(&operand)?,
        }
    } else if let Some(values) = transform.append_missing_elements {
        FieldTransformOperation::AppendMissingElements {
            values: lower_array_transform_values(values.values)?,
        }
    } else {
        let values = transform
            .remove_all_from_array
            .ok_or_else(|| invalid_request("missing field transform operation"))?;
        FieldTransformOperation::RemoveAllFromArray {
            values: lower_array_transform_values(values.values)?,
        }
    };

    Ok(FieldTransform {
        field: transform.field_path,
        transform: operation,
    })
}

fn lower_array_transform_values(
    values: Vec<Value>,
) -> Result<Vec<Value>, FirestoreCommitRequestError> {
    values
        .into_iter()
        .map(|value| serializer::decode_proto_json_value(&value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn parse_transaction(value: &str) -> Result<Vec<u8>, FirestoreCommitRequestError> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|error| invalid_request(format!("invalid base64 transaction bytes: {error}")))
}

fn parse_timestamp(value: &str) -> Result<Timestamp, FirestoreCommitRequestError> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339).map_err(|error| {
        invalid_request(format!("invalid RFC3339 timestamp `{value}`: {error}"))
    })?;
    let millis = parsed.unix_timestamp_nanos() / 1_000_000;
    let millis = u64::try_from(millis).map_err(|_| {
        invalid_request(format!(
            "timestamp before unix epoch is unsupported: {value}"
        ))
    })?;
    Ok(Timestamp(millis))
}

fn reject_document_metadata(
    document: &FirestoreDocumentJson,
) -> Result<(), FirestoreCommitRequestError> {
    if document.create_time.is_some() || document.update_time.is_some() {
        return Err(invalid_request(
            "input document metadata fields `createTime` and `updateTime` are not supported",
        ));
    }
    Ok(())
}

fn ensure_database_match(
    expected: &FirestoreDatabaseName,
    actual: &FirestoreDatabaseName,
    kind: &'static str,
) -> Result<(), FirestoreCommitRequestError> {
    if expected.project_id == actual.project_id {
        Ok(())
    } else {
        Err(invalid_request(format!(
            "{kind} must be a child of database `projects/{}/databases/(default)`",
            expected.project_id
        )))
    }
}

fn invalid_request(message: impl Into<String>) -> FirestoreCommitRequestError {
    FirestoreCommitRequestError::InvalidRequest(message.into())
}

fn unsupported(message: impl Into<String>) -> FirestoreCommitRequestError {
    FirestoreCommitRequestError::Unsupported(message.into())
}

#[cfg(test)]
mod tests {
    use neovex_core::{
        DocumentId, DocumentLocator, FieldTransformOperation, NumericValue, ResourcePathBinding,
        SpecialDouble, TableName,
    };
    use serde_json::json;

    use super::*;

    fn resolve_preview_key(
        document_path: &neovex_core::DocumentPath,
    ) -> Result<WriteKey, FirestoreCommitRequestError> {
        Ok(WriteKey::from(ResourcePathBinding::new(
            DocumentLocator::new(
                TableName::new("firebase_preview").expect("table should parse"),
                DocumentId::from_key(format!(
                    "preview-{}",
                    document_path.to_string().replace('/', "_")
                ))
                .expect("id should parse"),
            ),
            document_path.clone(),
        )))
    }

    #[test]
    fn parses_update_patch_delete_verify_and_transaction_bytes() {
        let request = json!({
            "database": "projects/demo/databases/(default)",
            "transaction": "AQID",
            "writes": [
                {
                    "update": {
                        "name": "projects/demo/databases/(default)/documents/cities/SF",
                        "fields": {
                            "name": { "stringValue": "San Francisco" }
                        }
                    }
                },
                {
                    "update": {
                        "name": "projects/demo/databases/(default)/documents/cities/SF",
                        "fields": {
                            "country": { "stringValue": "USA" }
                        }
                    },
                    "updateMask": {
                        "fieldPaths": ["country", "population"]
                    },
                    "currentDocument": {
                        "exists": true
                    }
                },
                {
                    "delete": "projects/demo/databases/(default)/documents/cities/LA"
                },
                {
                    "verify": "projects/demo/databases/(default)/documents/cities/NYC",
                    "currentDocument": {
                        "updateTime": "2024-01-02T03:04:05Z"
                    }
                }
            ]
        });

        let parsed = parse_commit_request_with_resolver(&request, resolve_preview_key)
            .expect("commit request should parse");

        assert_eq!(parsed.database.project_id, "demo");
        assert_eq!(parsed.transaction, Some(vec![1, 2, 3]));
        assert_eq!(parsed.batch.writes.len(), 4);
        assert!(matches!(
            &parsed.batch.writes[0],
            AtomicWrite::Set {
                mode: WriteSetMode::Overwrite,
                ..
            }
        ));
        assert!(matches!(
            &parsed.batch.writes[1],
            AtomicWrite::Patch { mask, .. } if mask == &vec!["country".to_string(), "population".to_string()]
        ));
        assert!(matches!(
            &parsed.batch.writes[2],
            AtomicWrite::Delete {
                missing_ok: true,
                ..
            }
        ));
        assert!(matches!(
            &parsed.batch.writes[3],
            AtomicWrite::Verify {
                precondition: WritePrecondition {
                    update_time: Some(_),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn parses_transform_operations_into_shared_field_transforms() {
        let request = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [
                {
                    "transform": {
                        "document": "projects/demo/databases/(default)/documents/counters/visits",
                        "fieldTransforms": [
                            {
                                "fieldPath": "updatedAt",
                                "setToServerValue": "REQUEST_TIME"
                            },
                            {
                                "fieldPath": "count",
                                "increment": { "integerValue": "1" }
                            },
                            {
                                "fieldPath": "tags",
                                "appendMissingElements": {
                                    "values": [
                                        { "stringValue": "new" }
                                    ]
                                }
                            }
                        ]
                    }
                }
            ]
        });

        let parsed = parse_commit_request_with_resolver(&request, resolve_preview_key)
            .expect("transform request should parse");

        assert!(matches!(
            &parsed.batch.writes[0],
            AtomicWrite::Transform { transforms, .. } if transforms.len() == 3
        ));
    }

    #[test]
    fn rejects_invalid_operation_shapes_and_database_mismatches() {
        let invalid_oneof = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [
                {
                    "delete": "projects/demo/databases/(default)/documents/cities/SF",
                    "verify": "projects/demo/databases/(default)/documents/cities/SF"
                }
            ]
        });
        assert!(parse_commit_request_with_resolver(&invalid_oneof, resolve_preview_key).is_err());

        let wrong_database = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [
                {
                    "delete": "projects/other/databases/(default)/documents/cities/SF"
                }
            ]
        });
        assert!(parse_commit_request_with_resolver(&wrong_database, resolve_preview_key).is_err());
    }

    #[test]
    fn parses_special_double_transform_values_into_shared_numeric_operands() {
        let request = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [
                {
                    "transform": {
                        "document": "projects/demo/databases/(default)/documents/counters/visits",
                        "fieldTransforms": [
                            {
                                "fieldPath": "score",
                                "maximum": { "doubleValue": "NaN" }
                            }
                        ]
                    }
                }
            ]
        });

        let parsed = parse_commit_request_with_resolver(&request, resolve_preview_key)
            .expect("special double transform operand should parse");
        assert!(matches!(
            &parsed.batch.writes[0],
            AtomicWrite::Transform { transforms, .. }
                if matches!(
                    transforms.as_slice(),
                    [neovex_core::FieldTransform {
                        field,
                        transform: FieldTransformOperation::Maximum {
                            operand: NumericValue::SpecialDouble {
                                value: SpecialDouble::Nan,
                            },
                        },
                    }] if field == "score"
                )
        ));
    }
}
