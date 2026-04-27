use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neovex_core::DocumentPath;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::resource_names::{self, FirestoreDatabaseName, FirestoreResourceNameError};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedBatchGetRequest {
    pub documents: Vec<ParsedBatchGetDocument>,
    pub mask: Option<Vec<String>>,
    pub transaction: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedBatchGetDocument {
    pub document_path: DocumentPath,
    pub document_name: String,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreBatchGetRequestError {
    #[error("invalid Firestore BatchGetDocuments request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore BatchGetDocuments feature: {0}")]
    Unsupported(String),
    #[error(transparent)]
    InvalidResource(#[from] FirestoreResourceNameError),
}

pub(crate) fn parse_batch_get_request(
    request: &Value,
    database: &FirestoreDatabaseName,
) -> Result<ParsedBatchGetRequest, FirestoreBatchGetRequestError> {
    let request: BatchGetDocumentsRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    if request.documents.is_empty() {
        return Err(invalid_request(
            "`documents` must contain at least one document resource name",
        ));
    }

    let consistency_selector_count = usize::from(request.transaction.is_some())
        + usize::from(request.new_transaction.is_some())
        + usize::from(request.read_time.is_some());
    if consistency_selector_count > 1 {
        return Err(invalid_request(
            "BatchGetDocuments request must set at most one of `transaction`, `newTransaction`, or `readTime`",
        ));
    }
    if request.new_transaction.is_some() {
        return Err(unsupported_request("`newTransaction`"));
    }
    if request.read_time.is_some() {
        return Err(unsupported_request("`readTime`"));
    }

    let transaction = request
        .transaction
        .as_deref()
        .map(parse_transaction)
        .transpose()?;
    let mask = request.mask.map(lower_document_mask).transpose()?;

    let mut seen_documents = HashSet::new();
    let mut documents = Vec::new();
    for document_name in request.documents {
        let parsed_document = resource_names::parse_document_name(&document_name)?;
        ensure_database_match(database, &parsed_document.database, "requested document")?;
        let canonical_name = firestore_document_name(database, &parsed_document.document_path);
        if seen_documents.insert(canonical_name.clone()) {
            documents.push(ParsedBatchGetDocument {
                document_path: parsed_document.document_path,
                document_name: canonical_name,
            });
        }
    }

    Ok(ParsedBatchGetRequest {
        documents,
        mask,
        transaction,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchGetDocumentsRequestJson {
    #[serde(default)]
    documents: Vec<String>,
    mask: Option<FirestoreDocumentMaskJson>,
    transaction: Option<String>,
    #[serde(default)]
    new_transaction: Option<Value>,
    read_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreDocumentMaskJson {
    #[serde(default)]
    field_paths: Vec<String>,
}

fn lower_document_mask(
    mask: FirestoreDocumentMaskJson,
) -> Result<Vec<String>, FirestoreBatchGetRequestError> {
    lower_document_mask_paths(mask.field_paths)
}

pub(crate) fn lower_document_mask_paths<I>(
    field_paths: I,
) -> Result<Vec<String>, FirestoreBatchGetRequestError>
where
    I: IntoIterator<Item = String>,
{
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for field_path in field_paths {
        if field_path == "__name__" {
            continue;
        }
        if field_path.contains('.') || field_path.contains('`') {
            return Err(unsupported_request(
                "nested or quoted field paths in `mask.fieldPaths`",
            ));
        }
        if seen.insert(field_path.clone()) {
            deduped.push(field_path);
        }
    }
    Ok(deduped)
}

fn parse_transaction(value: &str) -> Result<Vec<u8>, FirestoreBatchGetRequestError> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|error| invalid_request(format!("invalid base64 transaction bytes: {error}")))
}

fn ensure_database_match(
    expected: &FirestoreDatabaseName,
    actual: &FirestoreDatabaseName,
    context: &str,
) -> Result<(), FirestoreBatchGetRequestError> {
    if expected.project_id == actual.project_id {
        return Ok(());
    }
    Err(invalid_request(format!(
        "{context} belongs to project `{}`, but request database project is `{}`",
        actual.project_id, expected.project_id
    )))
}

fn firestore_document_name(
    database: &FirestoreDatabaseName,
    document_path: &DocumentPath,
) -> String {
    format!(
        "projects/{}/databases/(default)/documents/{}",
        database.project_id, document_path
    )
}

fn invalid_request(reason: impl Into<String>) -> FirestoreBatchGetRequestError {
    FirestoreBatchGetRequestError::InvalidRequest(reason.into())
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreBatchGetRequestError {
    FirestoreBatchGetRequestError::Unsupported(feature.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_batch_get_request;
    use crate::adapters::firebase::resource_names;

    #[test]
    fn parses_documents_mask_and_transaction_and_elides_duplicates() {
        let database = resource_names::parse_database_name("projects/demo/databases/(default)")
            .expect("database should parse");
        let request = json!({
            "documents": [
                "projects/demo/databases/(default)/documents/cities/SF",
                "projects/demo/databases/(default)/documents/cities/SF",
                "projects/demo/databases/(default)/documents/cities/LA"
            ],
            "mask": {
                "fieldPaths": ["name", "name", "__name__"]
            },
            "transaction": "AQID"
        });

        let parsed = parse_batch_get_request(&request, &database).expect("request should parse");

        assert_eq!(parsed.documents.len(), 2);
        assert_eq!(
            parsed.documents[0].document_path.to_string(),
            "cities/SF".to_string()
        );
        assert_eq!(parsed.mask, Some(vec!["name".to_string()]));
        assert_eq!(parsed.transaction, Some(vec![1, 2, 3]));
    }

    #[test]
    fn rejects_unsupported_consistency_selectors_and_bad_mask_paths() {
        let database = resource_names::parse_database_name("projects/demo/databases/(default)")
            .expect("database should parse");
        let unsupported = json!({
            "documents": [
                "projects/demo/databases/(default)/documents/cities/SF"
            ],
            "newTransaction": {}
        });
        let nested_mask = json!({
            "documents": [
                "projects/demo/databases/(default)/documents/cities/SF"
            ],
            "mask": {
                "fieldPaths": ["address.city"]
            }
        });

        let unsupported_error = parse_batch_get_request(&unsupported, &database)
            .expect_err("newTransaction should be rejected");
        let mask_error = parse_batch_get_request(&nested_mask, &database)
            .expect_err("nested mask paths should be rejected");

        assert!(unsupported_error.to_string().contains("newTransaction"));
        assert!(mask_error.to_string().contains("mask.fieldPaths"));
    }
}
