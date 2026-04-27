use std::collections::HashSet;

use neovex_core::AtomicWrite;
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use super::commit_request::{self, FirestoreCommitRequestError};
use super::resource_names::{FirestoreDatabaseName, FirestoreResourceNameError};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedBatchWriteRequest {
    pub database: FirestoreDatabaseName,
    pub writes: Vec<AtomicWrite>,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreBatchWriteRequestError {
    #[error("invalid Firestore BatchWrite request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore BatchWrite feature: {0}")]
    Unsupported(String),
    #[error(transparent)]
    InvalidResource(#[from] FirestoreResourceNameError),
    #[error(transparent)]
    InvalidValue(#[from] commit_request::FirestoreCommitRequestError),
}

pub(crate) fn parse_batch_write_request_with_resolver(
    request: &Value,
    resolve_write_key: impl FnMut(
        &neovex_core::DocumentPath,
    ) -> Result<neovex_core::WriteKey, FirestoreCommitRequestError>,
) -> Result<ParsedBatchWriteRequest, FirestoreBatchWriteRequestError> {
    let request: BatchWriteRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let parsed_commit = commit_request::parse_commit_request_with_resolver(
        &json!({
            "database": request.database,
            "writes": request.writes,
        }),
        resolve_write_key,
    )?;
    reject_duplicate_write_targets(&parsed_commit.batch.writes)?;

    Ok(ParsedBatchWriteRequest {
        database: parsed_commit.database,
        writes: parsed_commit.batch.writes,
    })
}

pub(crate) fn reject_duplicate_write_targets(
    writes: &[AtomicWrite],
) -> Result<(), FirestoreBatchWriteRequestError> {
    let mut seen = HashSet::new();
    for write in writes {
        let locator = write.key().locator();
        if !seen.insert((
            locator.table.as_str().to_string(),
            locator.id.as_str().to_string(),
        )) {
            return Err(invalid_request(
                "BatchWrite requests cannot write to the same document more than once",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchWriteRequestJson {
    database: String,
    #[serde(default)]
    writes: Vec<Value>,
    #[serde(default)]
    _labels: std::collections::HashMap<String, String>,
}

fn invalid_request(message: impl Into<String>) -> FirestoreBatchWriteRequestError {
    FirestoreBatchWriteRequestError::InvalidRequest(message.into())
}

#[allow(dead_code)]
fn unsupported(message: impl Into<String>) -> FirestoreBatchWriteRequestError {
    FirestoreBatchWriteRequestError::Unsupported(message.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_batch_write_request_with_resolver;
    use crate::adapters::firebase::{resolve_write_key, resource_names};

    #[test]
    fn parses_writes_and_ignores_labels() {
        let request = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [{
                "update": {
                    "name": "projects/demo/databases/(default)/documents/cities/SF",
                    "fields": {
                        "name": { "stringValue": "San Francisco" }
                    }
                }
            }],
            "labels": {
                "sdk": "web"
            }
        });

        let parsed = parse_batch_write_request_with_resolver(&request, resolve_write_key)
            .expect("BatchWrite request should parse");

        assert_eq!(
            parsed.database,
            resource_names::parse_database_name("projects/demo/databases/(default)")
                .expect("database should parse")
        );
        assert_eq!(parsed.writes.len(), 1);
    }

    #[test]
    fn rejects_duplicate_document_targets() {
        let request = json!({
            "database": "projects/demo/databases/(default)",
            "writes": [
                {
                    "update": {
                        "name": "projects/demo/databases/(default)/documents/cities/SF",
                        "fields": {}
                    }
                },
                {
                    "delete": "projects/demo/databases/(default)/documents/cities/SF"
                }
            ]
        });

        let error = parse_batch_write_request_with_resolver(&request, resolve_write_key)
            .expect_err("duplicate document targets should fail");

        assert!(matches!(
            error,
            super::FirestoreBatchWriteRequestError::InvalidRequest(_)
        ));
    }
}
