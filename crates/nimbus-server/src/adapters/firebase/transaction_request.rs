use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use nimbus_core::TransactionSessionMode;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::resource_names::{
    FirestoreDatabaseName, FirestoreResourceNameError, parse_database_name,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedBeginTransactionRequest {
    pub database: FirestoreDatabaseName,
    pub mode: TransactionSessionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedRollbackRequest {
    pub database: FirestoreDatabaseName,
    pub transaction: Vec<u8>,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreTransactionRequestError {
    #[error("invalid Firestore transaction request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore transaction request feature: {0}")]
    Unsupported(String),
    #[error(transparent)]
    InvalidResource(#[from] FirestoreResourceNameError),
}

pub(crate) fn parse_begin_transaction_request(
    request: &Value,
    route_database: &FirestoreDatabaseName,
) -> Result<ParsedBeginTransactionRequest, FirestoreTransactionRequestError> {
    let request: BeginTransactionRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let database = resolve_database(request.database.as_deref(), route_database)?;
    let mode = lower_transaction_mode(request.options)?;

    Ok(ParsedBeginTransactionRequest { database, mode })
}

pub(crate) fn parse_rollback_request(
    request: &Value,
    route_database: &FirestoreDatabaseName,
) -> Result<ParsedRollbackRequest, FirestoreTransactionRequestError> {
    let request: RollbackRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let database = resolve_database(request.database.as_deref(), route_database)?;
    let transaction = BASE64_STANDARD
        .decode(request.transaction)
        .map_err(|error| invalid_request(format!("invalid base64 transaction bytes: {error}")))?;

    Ok(ParsedRollbackRequest {
        database,
        transaction,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeginTransactionRequestJson {
    database: Option<String>,
    options: Option<TransactionOptionsJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RollbackRequestJson {
    database: Option<String>,
    transaction: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionOptionsJson {
    read_only: Option<ReadOnlyTransactionOptionsJson>,
    read_write: Option<ReadWriteTransactionOptionsJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadOnlyTransactionOptionsJson {
    read_time: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadWriteTransactionOptionsJson {
    retry_transaction: Option<String>,
}

fn resolve_database(
    request_database: Option<&str>,
    route_database: &FirestoreDatabaseName,
) -> Result<FirestoreDatabaseName, FirestoreTransactionRequestError> {
    let Some(request_database) = request_database else {
        return Ok(route_database.clone());
    };
    let database = parse_database_name(request_database)?;
    if &database == route_database {
        return Ok(database);
    }

    Err(invalid_request(format!(
        "route database `projects/{}/databases/(default)` does not match request body database `projects/{}/databases/(default)`",
        route_database.project_id, database.project_id
    )))
}

fn lower_transaction_mode(
    options: Option<TransactionOptionsJson>,
) -> Result<TransactionSessionMode, FirestoreTransactionRequestError> {
    let Some(options) = options else {
        return Ok(TransactionSessionMode::ReadWrite);
    };
    match (options.read_write, options.read_only) {
        (Some(_), Some(_)) => Err(invalid_request(
            "`options` must set at most one of `readWrite` or `readOnly`",
        )),
        (Some(read_write), None) => {
            if read_write
                .retry_transaction
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            {
                return Err(unsupported("`options.readWrite.retryTransaction`"));
            }
            Ok(TransactionSessionMode::ReadWrite)
        }
        (None, Some(read_only)) => {
            if read_only.read_time.is_some() {
                return Err(unsupported("`options.readOnly.readTime`"));
            }
            Ok(TransactionSessionMode::ReadOnly)
        }
        (None, None) => Ok(TransactionSessionMode::ReadWrite),
    }
}

fn invalid_request(reason: impl Into<String>) -> FirestoreTransactionRequestError {
    FirestoreTransactionRequestError::InvalidRequest(reason.into())
}

fn unsupported(feature: impl Into<String>) -> FirestoreTransactionRequestError {
    FirestoreTransactionRequestError::Unsupported(feature.into())
}

#[cfg(test)]
mod tests {
    use nimbus_core::TransactionSessionMode;
    use serde_json::json;

    use super::{parse_begin_transaction_request, parse_rollback_request};
    use crate::adapters::firebase::resource_names;

    #[test]
    fn parses_begin_transaction_modes_and_optional_database() {
        let route_database =
            resource_names::parse_database_name("projects/demo/databases/(default)")
                .expect("database should parse");
        let read_write = json!({});
        let read_only = json!({
            "database": "projects/demo/databases/(default)",
            "options": {
                "readOnly": {}
            }
        });

        let default_mode =
            parse_begin_transaction_request(&read_write, &route_database).expect("default mode");
        let explicit_read_only =
            parse_begin_transaction_request(&read_only, &route_database).expect("read only mode");

        assert_eq!(default_mode.mode, TransactionSessionMode::ReadWrite);
        assert_eq!(explicit_read_only.mode, TransactionSessionMode::ReadOnly);
        assert_eq!(explicit_read_only.database, route_database);
    }

    #[test]
    fn rejects_unsupported_begin_transaction_features_and_bad_rollback_tokens() {
        let route_database =
            resource_names::parse_database_name("projects/demo/databases/(default)")
                .expect("database should parse");
        let unsupported = json!({
            "options": {
                "readWrite": {
                    "retryTransaction": "AQID"
                }
            }
        });
        let mismatch = json!({
            "database": "projects/other/databases/(default)",
            "transaction": "AQID"
        });
        let bad_transaction = json!({
            "transaction": "!not-base64!"
        });

        let unsupported_error = parse_begin_transaction_request(&unsupported, &route_database)
            .expect_err("retryTransaction should be rejected");
        let mismatch_error =
            parse_rollback_request(&mismatch, &route_database).expect_err("database mismatch");
        let bad_transaction_error = parse_rollback_request(&bad_transaction, &route_database)
            .expect_err("bad rollback token");

        assert!(unsupported_error.to_string().contains("retryTransaction"));
        assert!(mismatch_error.to_string().contains("route database"));
        assert!(bad_transaction_error.to_string().contains("base64"));
    }
}
