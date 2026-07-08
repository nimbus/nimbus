use std::collections::HashSet;

use nimbus_core::DocumentPath;
use serde::Deserialize;
use serde_json::Value;

use super::request_error::{FirestoreRequestError, FirestoreRpc};
use super::resource_names::{self, FirestoreDatabaseName};
use super::response::firestore_document_name;
use super::transaction_token;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedBatchGetRequest {
    pub documents: Vec<ParsedBatchGetDocument>,
    pub mask: Option<Vec<String>>,
    pub transaction: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBatchGetDocument {
    pub document_path: DocumentPath,
    pub document_name: String,
}

pub fn parse_batch_get_request(
    request: &Value,
    database: &FirestoreDatabaseName,
) -> Result<ParsedBatchGetRequest, FirestoreRequestError> {
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
        .map(transaction_token::decode)
        .transpose()
        .map_err(|error| invalid_request(error.to_string()))?;
    let mask = request.mask.map(lower_document_mask).transpose()?;

    let mut seen_documents = HashSet::new();
    let mut documents = Vec::new();
    for document_name in request.documents {
        let parsed_document =
            resource_names::parse_document_name(&document_name).map_err(|error| {
                FirestoreRequestError::invalid_resource(FirestoreRpc::BatchGetDocuments, error)
            })?;
        resource_names::ensure_database_match(database, &parsed_document.database).map_err(
            |error| {
                invalid_request(format!(
                    "requested document belongs to database `{}`, but request database is `{}`",
                    error.actual(),
                    error.expected()
                ))
            },
        )?;
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
) -> Result<Vec<String>, FirestoreRequestError> {
    lower_document_mask_paths(mask.field_paths)
}

pub fn lower_document_mask_paths<I>(field_paths: I) -> Result<Vec<String>, FirestoreRequestError>
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

fn invalid_request(reason: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::invalid_request(FirestoreRpc::BatchGetDocuments, reason)
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::unsupported(FirestoreRpc::BatchGetDocuments, feature)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_batch_get_request;
    use crate::resource_names;

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
        assert_eq!(
            parsed.documents[0].document_name,
            "projects/demo/databases/(default)/documents/cities/SF"
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

    #[test]
    fn rejects_documents_from_a_different_database() {
        let database = resource_names::parse_database_name("projects/demo/databases/(default)")
            .expect("database should parse");
        let request = json!({
            "documents": [
                "projects/other/databases/(default)/documents/cities/SF"
            ]
        });

        let error = parse_batch_get_request(&request, &database)
            .expect_err("database mismatch should fail");

        let error = error.to_string();
        assert!(error.contains("projects/other/databases/(default)"));
        assert!(error.contains("projects/demo/databases/(default)"));
    }
}
