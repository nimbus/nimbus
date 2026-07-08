use std::collections::HashSet;

use nimbus_core::AtomicWrite;
use serde::Deserialize;
use serde_json::{Value, json};

use super::commit_request;
use super::request_error::{FirestoreRequestError, FirestoreRpc};
use super::resource_names::FirestoreDatabaseName;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedBatchWriteRequest {
    pub database: FirestoreDatabaseName,
    pub writes: Vec<AtomicWrite>,
}

pub fn parse_batch_write_request_with_resolver(
    request: &Value,
    resolve_write_key: impl FnMut(
        &nimbus_core::DocumentPath,
    ) -> Result<nimbus_core::WriteKey, FirestoreRequestError>,
) -> Result<ParsedBatchWriteRequest, FirestoreRequestError> {
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

pub fn reject_duplicate_write_targets(writes: &[AtomicWrite]) -> Result<(), FirestoreRequestError> {
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
}

fn invalid_request(message: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::invalid_request(FirestoreRpc::BatchWrite, message)
}

#[allow(dead_code)]
fn unsupported(message: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::unsupported(FirestoreRpc::BatchWrite, message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_batch_write_request_with_resolver;
    use crate::{resolve_write_key, resource_names};

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
            "labels": "ignored request metadata"
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
            error.kind,
            crate::request_error::FirestoreRequestErrorKind::InvalidRequest(_)
        ));
    }
}
