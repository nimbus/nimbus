pub(crate) mod batch_get_request;
pub(crate) mod batch_write_request;
pub(crate) mod commit_request;
pub(crate) mod errors;
pub(crate) mod grpc;
pub(crate) mod list_collection_ids_request;
pub(crate) mod operations;
pub(crate) mod resource_names;
pub(crate) mod response;
pub(crate) mod run_aggregation_query_request;
pub(crate) mod run_query_request;
pub(crate) mod serializer;
pub(crate) mod transaction_request;

pub(crate) use self::errors::{
    batch_get_request_error_to_core, batch_write_request_error_to_core,
    commit_request_error_to_core, firebase_error_response, firebase_error_to_app,
    firestore_grpc_code, list_collection_ids_request_error_to_core, resource_name_error_to_core,
    run_aggregation_query_request_error_to_core, run_query_request_error_to_core,
    transaction_request_error_to_core,
};
pub(crate) use self::operations::{
    batch_get_documents_for_database, batch_write_for_database,
    begin_transaction_session_for_database, commit_batch_for_database, get_document_for_database,
    list_collection_ids_for_database, resolve_write_key, rollback_transaction_session_for_database,
    run_aggregation_query_for_database, run_query_documents_for_database, tenant_id_for_database,
};
pub(crate) use self::response::{
    batch_get_entry_json, batch_write_response_json, commit_response_json, firestore_document_name,
    format_timestamp, run_aggregation_query_response_entries, run_query_response_entries,
    serialize_json_lines,
};
#[cfg(test)]
pub(crate) use crate::provider_family::firestore::locator_for_document_path;
pub(crate) use crate::provider_family::firestore::storage_table_for_collection_path;

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neovex_core::{
    AtomicWriteResult, Document, DocumentPath, Error, Result, StructuredAggregationResult,
    Timestamp,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::application_auth::resolve_application_auth_from_headers;
use crate::state::record_authenticated_usage;
use crate::state::{AppError, AppState};

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct FirebaseConfig {
    allow_emulator_mock_user_token_auth: bool,
}

impl FirebaseConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_emulator_mock_user_token_auth(mut self) -> Self {
        self.allow_emulator_mock_user_token_auth = true;
        self
    }

    pub(crate) fn allows_emulator_mock_user_token_auth(&self) -> bool {
        self.allow_emulator_mock_user_token_auth
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct FirestoreRouteParams {
    project_id: String,
    database_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FirestoreDocumentRunQueryRouteParams {
    project_id: String,
    database_id: String,
    document_request: String,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchGetDocumentsOutcome {
    pub entries: Vec<BatchGetDocumentEntry>,
    pub read_time: Timestamp,
}

pub(crate) struct BatchWriteOutcome {
    pub entries: Vec<BatchWriteEntryOutcome>,
}

pub(crate) struct BatchWriteEntryOutcome {
    pub write_result: Option<AtomicWriteResult>,
    pub error: Option<Error>,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchGetDocumentEntry {
    pub document_name: String,
    pub document: Option<Document>,
}

#[derive(Debug, Clone)]
pub(crate) struct RunQueryOutcome {
    pub documents: Vec<RunQueryDocument>,
    pub read_time: Timestamp,
    pub skipped_results: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RunQueryDocument {
    pub document_path: DocumentPath,
    pub document: Document,
}

#[derive(Debug, Clone)]
pub(crate) struct RunAggregationQueryOutcome {
    pub result: StructuredAggregationResult,
    pub read_time: Timestamp,
}

pub(crate) async fn commit(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, &headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let request_json = parse_json_body(&body).map_err(firebase_error_to_app)?;
    let route_database =
        resource_names::decode_rest_database(&params.project_id, &params.database_id)
            .map_err(resource_name_error_to_core)
            .map_err(firebase_error_to_app)?;
    let parsed_commit =
        commit_request::parse_commit_request_with_resolver(&request_json, resolve_write_key)
            .map_err(commit_request_error_to_core)
            .map_err(firebase_error_to_app)?;
    if parsed_commit.database != route_database {
        return Ok(firebase_error_response(Error::InvalidInput(format!(
            "route database `projects/{}/databases/{}` does not match request body database `projects/{}/databases/(default)`",
            params.project_id, params.database_id, parsed_commit.database.project_id
        ))));
    }

    let outcome = commit_batch_for_database(
        &state,
        &parsed_commit.database,
        &auth.principal,
        parsed_commit.batch,
        parsed_commit.transaction.as_deref(),
    );

    match outcome {
        Ok(outcome) => Ok((
            StatusCode::OK,
            Json(commit_response_json(&outcome).map_err(AppError::from)?),
        )),
        Err(error) => Ok(firebase_error_response(error)),
    }
}

pub(crate) async fn batch_write(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, &headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error)),
    };
    let route_database =
        match resource_names::decode_rest_database(&params.project_id, &params.database_id) {
            Ok(database) => database,
            Err(error) => return Ok(firebase_error_response(resource_name_error_to_core(error))),
        };
    let parsed_request = match batch_write_request::parse_batch_write_request_with_resolver(
        &request_json,
        resolve_write_key,
    ) {
        Ok(request) => request,
        Err(error) => {
            return Ok(firebase_error_response(batch_write_request_error_to_core(
                error,
            )));
        }
    };
    if parsed_request.database != route_database {
        return Ok(firebase_error_response(Error::InvalidInput(format!(
            "route database `projects/{}/databases/{}` does not match request body database `projects/{}/databases/(default)`",
            params.project_id, params.database_id, parsed_request.database.project_id
        ))));
    }

    match batch_write_for_database(
        &state,
        &parsed_request.database,
        &auth.principal,
        parsed_request.writes,
    ) {
        Ok(outcome) => Ok((
            StatusCode::OK,
            Json(batch_write_response_json(&outcome).map_err(AppError::from)?),
        )),
        Err(error) => Ok(firebase_error_response(error)),
    }
}

pub(crate) async fn batch_get_documents(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, &headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let route_database =
        match resource_names::decode_rest_database(&params.project_id, &params.database_id) {
            Ok(database) => database,
            Err(error) => {
                return Ok(
                    firebase_error_response(resource_name_error_to_core(error)).into_response()
                );
            }
        };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let parsed_request =
        match batch_get_request::parse_batch_get_request(&request_json, &route_database) {
            Ok(request) => request,
            Err(error) => {
                return Ok(
                    firebase_error_response(batch_get_request_error_to_core(error)).into_response(),
                );
            }
        };
    let outcome = match batch_get_documents_for_database(
        &state,
        &route_database,
        &auth.principal,
        &parsed_request,
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let read_time = match format_timestamp(outcome.read_time) {
        Ok(read_time) => read_time,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let response_entries = match outcome
        .entries
        .into_iter()
        .map(|entry| {
            batch_get_entry_json(
                &entry.document_name,
                entry.document,
                parsed_request.mask.as_deref(),
                &read_time,
            )
        })
        .collect::<Result<Vec<_>>>()
    {
        Ok(entries) => entries,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let body = match serialize_json_lines(&response_entries) {
        Ok(body) => body,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

pub(crate) async fn begin_transaction(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, &headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let route_database =
        match resource_names::decode_rest_database(&params.project_id, &params.database_id) {
            Ok(database) => database,
            Err(error) => return Ok(firebase_error_response(resource_name_error_to_core(error))),
        };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error)),
    };
    let parsed_request = match transaction_request::parse_begin_transaction_request(
        &request_json,
        &route_database,
    ) {
        Ok(request) => request,
        Err(error) => {
            return Ok(firebase_error_response(transaction_request_error_to_core(
                error,
            )));
        }
    };
    let session = match begin_transaction_session_for_database(
        &state,
        &parsed_request.database,
        &auth.principal,
        parsed_request.mode,
    ) {
        Ok(session) => session,
        Err(error) => return Ok(firebase_error_response(error)),
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "transaction": BASE64_STANDARD.encode(session.token.as_str().as_bytes()),
        })),
    ))
}

pub(crate) async fn rollback(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, &headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let route_database =
        match resource_names::decode_rest_database(&params.project_id, &params.database_id) {
            Ok(database) => database,
            Err(error) => return Ok(firebase_error_response(resource_name_error_to_core(error))),
        };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error)),
    };
    let parsed_request =
        match transaction_request::parse_rollback_request(&request_json, &route_database) {
            Ok(request) => request,
            Err(error) => {
                return Ok(firebase_error_response(transaction_request_error_to_core(
                    error,
                )));
            }
        };
    match rollback_transaction_session_for_database(
        &state,
        &parsed_request.database,
        &auth.principal,
        &parsed_request.transaction,
    ) {
        Ok(()) => Ok((StatusCode::OK, Json(json!({})))),
        Err(error) => Ok(firebase_error_response(error)),
    }
}

pub(crate) async fn list_collection_ids(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    list_collection_ids_for_parent_document(
        &params.project_id,
        &params.database_id,
        None,
        state,
        body,
    )
    .await
}

pub(crate) async fn run_query(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    run_query_for_parent_document(
        &params.project_id,
        &params.database_id,
        None,
        state,
        &headers,
        body,
    )
    .await
}

pub(crate) async fn run_aggregation_query(
    Path(params): Path<FirestoreRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    run_aggregation_query_for_parent_document(
        &params.project_id,
        &params.database_id,
        None,
        state,
        &headers,
        body,
    )
    .await
}

pub(crate) async fn run_document_action_under_parent_document(
    Path(params): Path<FirestoreDocumentRunQueryRouteParams>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    if let Some(parent_document_path) = params.document_request.strip_suffix(":runQuery") {
        return run_query_for_parent_document(
            &params.project_id,
            &params.database_id,
            Some(parent_document_path),
            state,
            &headers,
            body,
        )
        .await;
    }
    if let Some(parent_document_path) = params.document_request.strip_suffix(":runAggregationQuery")
    {
        return run_aggregation_query_for_parent_document(
            &params.project_id,
            &params.database_id,
            Some(parent_document_path),
            state,
            &headers,
            body,
        )
        .await;
    }
    if let Some(parent_document_path) = params.document_request.strip_suffix(":listCollectionIds") {
        return list_collection_ids_for_parent_document(
            &params.project_id,
            &params.database_id,
            Some(parent_document_path),
            state,
            body,
        )
        .await
        .map(IntoResponse::into_response);
    }
    Err(AppError::not_found("firebase route not found"))
}

async fn list_collection_ids_for_parent_document(
    project_id: &str,
    database_id: &str,
    parent_document_path: Option<&str>,
    state: Arc<AppState>,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<Value>), AppError> {
    ensure_firebase_enabled(&state)?;
    let route_database = match resource_names::decode_rest_database(project_id, database_id) {
        Ok(database) => database,
        Err(error) => return Ok(firebase_error_response(resource_name_error_to_core(error))),
    };
    let parent_document_path = match parent_document_path {
        Some(parent_document_path) => {
            match resource_names::decode_rest_document_path(parent_document_path) {
                Ok(document_path) => Some(document_path),
                Err(error) => {
                    return Ok(firebase_error_response(resource_name_error_to_core(error)));
                }
            }
        }
        None => None,
    };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error)),
    };
    let parsed_request =
        match list_collection_ids_request::parse_list_collection_ids_request(&request_json) {
            Ok(request) => request,
            Err(error) => {
                return Ok(firebase_error_response(
                    list_collection_ids_request_error_to_core(error),
                ));
            }
        };
    let page = match list_collection_ids_for_database(
        &state,
        &route_database,
        parent_document_path.as_ref(),
        &parsed_request,
    ) {
        Ok(page) => page,
        Err(error) => return Ok(firebase_error_response(error)),
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "collectionIds": page.collection_ids,
            "nextPageToken": page.next_page_token,
        })),
    ))
}

async fn run_query_for_parent_document(
    project_id: &str,
    database_id: &str,
    parent_document_path: Option<&str>,
    state: Arc<AppState>,
    headers: &HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let route_database = match resource_names::decode_rest_database(project_id, database_id) {
        Ok(database) => database,
        Err(error) => {
            return Ok(firebase_error_response(resource_name_error_to_core(error)).into_response());
        }
    };
    let parent_document_path = match parent_document_path {
        Some(parent_document_path) => {
            match resource_names::decode_rest_document_path(parent_document_path) {
                Ok(document_path) => Some(document_path),
                Err(error) => {
                    return Ok(
                        firebase_error_response(resource_name_error_to_core(error)).into_response()
                    );
                }
            }
        }
        None => None,
    };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let parsed_request = match run_query_request::parse_run_query_request(&request_json) {
        Ok(request) => request,
        Err(error) => {
            return Ok(
                firebase_error_response(run_query_request_error_to_core(error)).into_response(),
            );
        }
    };
    let outcome = match run_query_documents_for_database(
        &state,
        &route_database,
        &auth.principal,
        parent_document_path.as_ref(),
        parsed_request.structured_query,
        parsed_request.transaction.as_deref(),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let response_entries = match run_query_response_entries(
        &route_database,
        outcome.documents,
        outcome.read_time,
        outcome.skipped_results,
    ) {
        Ok(entries) => entries,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let body = match serialize_json_lines(&response_entries) {
        Ok(body) => body,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

async fn run_aggregation_query_for_parent_document(
    project_id: &str,
    database_id: &str,
    parent_document_path: Option<&str>,
    state: Arc<AppState>,
    headers: &HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    ensure_firebase_enabled(&state)?;
    let auth = resolve_application_auth_from_headers(&state, headers).await?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let route_database = match resource_names::decode_rest_database(project_id, database_id) {
        Ok(database) => database,
        Err(error) => {
            return Ok(firebase_error_response(resource_name_error_to_core(error)).into_response());
        }
    };
    let parent_document_path = match parent_document_path {
        Some(parent_document_path) => {
            match resource_names::decode_rest_document_path(parent_document_path) {
                Ok(document_path) => Some(document_path),
                Err(error) => {
                    return Ok(
                        firebase_error_response(resource_name_error_to_core(error)).into_response()
                    );
                }
            }
        }
        None => None,
    };
    let request_json = match parse_json_body(&body) {
        Ok(json) => json,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let parsed_request =
        match run_aggregation_query_request::parse_run_aggregation_query_request(&request_json) {
            Ok(request) => request,
            Err(error) => {
                return Ok(
                    firebase_error_response(run_aggregation_query_request_error_to_core(error))
                        .into_response(),
                );
            }
        };
    let outcome = match run_aggregation_query_for_database(
        &state,
        &route_database,
        &auth.principal,
        parent_document_path.as_ref(),
        parsed_request.aggregation_query,
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };
    let response_entries =
        match run_aggregation_query_response_entries(&outcome.result, outcome.read_time) {
            Ok(entries) => entries,
            Err(error) => return Ok(firebase_error_response(error).into_response()),
        };
    let body = match serialize_json_lines(&response_entries) {
        Ok(body) => body,
        Err(error) => return Ok(firebase_error_response(error).into_response()),
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response())
}

fn ensure_firebase_enabled(state: &Arc<AppState>) -> std::result::Result<(), AppError> {
    state
        .firebase_config
        .current()
        .map(|_| ())
        .ok_or_else(|| AppError::not_found("firebase adapter is disabled"))
}

fn parse_json_body(body: &Bytes) -> Result<Value> {
    serde_json::from_slice(body)
        .map_err(|error| Error::InvalidInput(format!("invalid Firebase JSON body: {error}")))
}
