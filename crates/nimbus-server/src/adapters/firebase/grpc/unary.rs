use std::collections::{HashMap, HashSet};

use futures::stream;
use nimbus_core::{
    AggregationOperator, CollectionName, CollectionSelector, CompositeFilter, CompositeOperator,
    CountAggregation, DistanceMeasure, Document, DocumentId, FieldFilter, FieldFilterOperator,
    FieldReference, FindNearest, Projection, QueryDirection, QueryFilter, StructuredAggregation,
    StructuredAggregationQuery, StructuredCursor, StructuredOrder, StructuredQuery,
    TransactionSessionMode, UnaryFilter, UnaryFilterOperator,
};
use serde_json::Number;
use tonic::{Request, Response, Status};

use super::FirestoreGrpcService;
use super::generated::google::firestore::v1::batch_get_documents_request::ConsistencySelector as BatchGetConsistencySelector;
use super::generated::google::firestore::v1::batch_get_documents_response::Result as BatchGetResult;
use super::generated::google::firestore::v1::run_aggregation_query_request::ConsistencySelector as RunAggregationConsistencySelector;
use super::generated::google::firestore::v1::run_aggregation_query_request::QueryType as RunAggregationQueryType;
use super::generated::google::firestore::v1::run_query_request::ConsistencySelector as RunQueryConsistencySelector;
use super::generated::google::firestore::v1::run_query_request::QueryType as RunQueryType;
use super::generated::google::firestore::v1::structured_aggregation_query::aggregation::Operator as ProtoAggregationOperator;
use super::generated::google::firestore::v1::structured_query::Direction as ProtoQueryDirection;
use super::generated::google::firestore::v1::structured_query::composite_filter::Operator as ProtoCompositeOperator;
use super::generated::google::firestore::v1::structured_query::field_filter::Operator as ProtoFieldFilterOperator;
use super::generated::google::firestore::v1::structured_query::filter::FilterType as ProtoFilterType;
use super::generated::google::firestore::v1::structured_query::find_nearest::DistanceMeasure as ProtoDistanceMeasure;
use super::generated::google::firestore::v1::structured_query::unary_filter::OperandType as ProtoUnaryOperandType;
use super::generated::google::firestore::v1::structured_query::unary_filter::Operator as ProtoUnaryOperator;
use super::generated::google::firestore::v1::transaction_options::Mode as ProtoTransactionMode;
use super::generated::google::firestore::v1::{
    self as proto, BatchGetDocumentsRequest, BatchGetDocumentsResponse, BatchWriteRequest,
    BatchWriteResponse, BeginTransactionRequest, BeginTransactionResponse, CommitRequest,
    CommitResponse, CreateDocumentRequest, DeleteDocumentRequest, GetDocumentRequest,
    ListCollectionIdsRequest, ListDocumentsRequest, RollbackRequest, RunAggregationQueryRequest,
    RunAggregationQueryResponse, RunQueryRequest, RunQueryResponse, UpdateDocumentRequest,
};
use super::generated::google::rpc::Status as RpcStatus;
use super::write_stream::{
    decode_nimbus_value_from_grpc, encode_document_field_to_grpc, encode_nimbus_value_to_grpc,
    firebase_grpc_status, lower_write_batch, prost_timestamp_from_core, proto_write_result,
};
use crate::adapters::firebase::batch_get_request::{
    ParsedBatchGetDocument, ParsedBatchGetRequest, lower_document_mask_paths,
};
use crate::adapters::firebase::batch_write_request;
use crate::adapters::firebase::resource_names::{self, FirestoreDatabaseName, FirestoreParentName};
use crate::adapters::firebase::{
    batch_get_documents_for_database, batch_get_request_error_to_core, batch_write_for_database,
    batch_write_request_error_to_core, begin_transaction_session_for_database,
    commit_batch_for_database, firestore_document_name, firestore_grpc_code,
    get_document_for_database, list_collection_ids_for_database, list_collection_ids_request,
    list_collection_ids_request_error_to_core, resource_name_error_to_core,
    rollback_transaction_session_for_database, run_aggregation_query_for_database,
    run_query_documents_for_database,
};
use crate::application_auth::{
    extract_bearer_token_from_metadata, grpc_status_from_app_error,
    resolve_application_auth_from_bearer,
};
use crate::state::{AppState, record_authenticated_usage};

async fn resolve_request_auth<T>(
    service: &FirestoreGrpcService,
    request: &Request<T>,
) -> Result<
    (
        std::sync::Arc<AppState>,
        crate::application_auth::ResolvedApplicationAuth,
    ),
    Status,
> {
    let state = service.app_state()?;
    let bearer = extract_bearer_token_from_metadata(request.metadata())
        .map_err(grpc_status_from_app_error)?;
    let auth = resolve_application_auth_from_bearer(&state, bearer.as_deref())
        .await
        .map_err(grpc_status_from_app_error)?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    Ok((state, auth))
}

pub(super) async fn handle_commit(
    service: &FirestoreGrpcService,
    request: Request<CommitRequest>,
) -> Result<Response<CommitResponse>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let database = parse_database_name(&request.database)?;
    let batch = lower_write_batch(&request.writes, &database)?;
    let outcome = commit_batch_for_database(
        &state,
        &database,
        &auth.principal,
        batch,
        (!request.transaction.is_empty()).then_some(request.transaction.as_slice()),
    )
    .map_err(firebase_grpc_status)?;

    Ok(Response::new(CommitResponse {
        write_results: outcome
            .write_results
            .iter()
            .map(proto_write_result)
            .collect::<Result<Vec<_>, _>>()?,
        commit_time: Some(prost_timestamp_from_core(outcome.commit_time)?),
    }))
}

pub(super) async fn handle_get_document(
    service: &FirestoreGrpcService,
    request: Request<GetDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let parsed_document = resource_names::parse_document_name(&request.name)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let mask = lower_optional_document_mask(request.mask)?;
    let transaction = match request.consistency_selector {
        None => None,
        Some(proto::get_document_request::ConsistencySelector::Transaction(transaction)) => {
            Some(transaction)
        }
        Some(proto::get_document_request::ConsistencySelector::ReadTime(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore GetDocument feature: `read_time`",
            ));
        }
    };
    let document = get_document_for_database(
        &state,
        &parsed_document.database,
        &auth.principal,
        &parsed_document.document_path,
        transaction.as_deref(),
    )
    .map_err(firebase_grpc_status)?
    .ok_or_else(|| Status::not_found(format!("document `{}` was not found", request.name)))?;

    Ok(Response::new(proto_document(
        &request.name,
        &document,
        mask.as_deref(),
    )?))
}

pub(super) async fn handle_batch_get_documents(
    service: &FirestoreGrpcService,
    request: Request<BatchGetDocumentsRequest>,
) -> Result<Response<tonic::codegen::BoxStream<BatchGetDocumentsResponse>>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let (database, request) = lower_batch_get_request(request.into_inner())?;
    let outcome = batch_get_documents_for_database(&state, &database, &auth.principal, &request)
        .map_err(firebase_grpc_status)?;
    let read_time = Some(prost_timestamp_from_core(outcome.read_time)?);
    let responses = outcome
        .entries
        .into_iter()
        .map(|entry| {
            let result = match entry.document {
                Some(document) => BatchGetResult::Found(proto_document(
                    &entry.document_name,
                    &document,
                    request.mask.as_deref(),
                )?),
                None => BatchGetResult::Missing(entry.document_name),
            };
            Ok(BatchGetDocumentsResponse {
                transaction: Vec::new(),
                read_time,
                result: Some(result),
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;

    let output: tonic::codegen::BoxStream<BatchGetDocumentsResponse> =
        Box::pin(stream::iter(responses.into_iter().map(Ok::<_, Status>)));
    Ok(Response::new(output))
}

pub(super) async fn handle_batch_write(
    service: &FirestoreGrpcService,
    request: Request<BatchWriteRequest>,
) -> Result<Response<BatchWriteResponse>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let database = parse_database_name(&request.database)?;
    let batch = lower_write_batch(&request.writes, &database)?;
    batch_write_request::reject_duplicate_write_targets(&batch.writes)
        .map_err(batch_write_request_error_to_core)
        .map_err(firebase_grpc_status)?;
    let outcome = batch_write_for_database(&state, &database, &auth.principal, batch.writes)
        .map_err(firebase_grpc_status)?;

    Ok(Response::new(BatchWriteResponse {
        write_results: outcome
            .entries
            .iter()
            .map(|entry| match entry.write_result.as_ref() {
                Some(write_result) => proto_write_result(write_result),
                None => Ok(proto::WriteResult::default()),
            })
            .collect::<Result<Vec<_>, _>>()?,
        status: outcome
            .entries
            .into_iter()
            .map(|entry| match entry.error {
                Some(error) => rpc_status_from_error(error),
                None => RpcStatus {
                    code: tonic::Code::Ok as i32,
                    message: String::new(),
                    details: Vec::new(),
                },
            })
            .collect(),
    }))
}

pub(super) async fn handle_list_documents(
    service: &FirestoreGrpcService,
    request: Request<ListDocumentsRequest>,
) -> Result<Response<proto::ListDocumentsResponse>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    if request.collection_id.is_empty() {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListDocuments feature: `collection_id` must be set",
        ));
    }
    if request.page_size > 0 {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListDocuments feature: `page_size`",
        ));
    }
    if !request.page_token.is_empty() {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListDocuments feature: `page_token`",
        ));
    }
    if !request.order_by.is_empty() {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListDocuments feature: `order_by`",
        ));
    }
    if request.show_missing {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListDocuments feature: `show_missing`",
        ));
    }
    match request.consistency_selector {
        None => {}
        Some(proto::list_documents_request::ConsistencySelector::Transaction(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore ListDocuments feature: `transaction`",
            ));
        }
        Some(proto::list_documents_request::ConsistencySelector::ReadTime(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore ListDocuments feature: `read_time`",
            ));
        }
    }

    let parent = resource_names::parse_parent_name(&request.parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let response_mask = lower_optional_document_mask(request.mask)?;
    let outcome = run_query_documents_for_database(
        &state,
        &parent.database,
        &auth.principal,
        parent.parent_document_path.as_ref(),
        StructuredQuery {
            from: vec![CollectionSelector::collection(
                CollectionName::new(request.collection_id).map_err(firebase_grpc_status)?,
            )],
            ..StructuredQuery::default()
        },
        None,
    )
    .map_err(firebase_grpc_status)?;

    let mut documents = outcome
        .documents
        .into_iter()
        .map(|entry| {
            let document_name = firestore_document_name(&parent.database, &entry.document_path);
            Ok::<(String, Document), Status>((document_name, entry.document))
        })
        .collect::<Result<Vec<_>, Status>>()?;
    documents.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

    Ok(Response::new(proto::ListDocumentsResponse {
        documents: documents
            .into_iter()
            .map(|(document_name, document)| {
                proto_document(&document_name, &document, response_mask.as_deref())
            })
            .collect::<Result<Vec<_>, _>>()?,
        next_page_token: String::new(),
    }))
}

pub(super) async fn handle_list_collection_ids(
    service: &FirestoreGrpcService,
    request: Request<ListCollectionIdsRequest>,
) -> Result<Response<proto::ListCollectionIdsResponse>, Status> {
    let (state, _auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    if let Some(proto::list_collection_ids_request::ConsistencySelector::ReadTime(_)) =
        request.consistency_selector.as_ref()
    {
        return Err(Status::invalid_argument(
            "unsupported Firestore ListCollectionIds feature: `read_time`",
        ));
    }

    let parsed_request = list_collection_ids_request::ParsedListCollectionIdsRequest {
        page_size: if request.page_size > 0 {
            Some(
                usize::try_from(request.page_size)
                    .map_err(|_| Status::invalid_argument("`page_size` exceeds supported range"))?,
            )
        } else if request.page_size < 0 {
            return Err(Status::invalid_argument("`page_size` must not be negative"));
        } else {
            None
        },
        page_offset: list_collection_ids_request::parse_list_collection_ids_page_token(
            &request.page_token,
        )
        .map_err(list_collection_ids_request_error_to_core)
        .map_err(firebase_grpc_status)?,
    };

    let parent = resource_names::parse_parent_name(&request.parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let page = list_collection_ids_for_database(
        &state,
        &parent.database,
        parent.parent_document_path.as_ref(),
        &parsed_request,
    )
    .map_err(firebase_grpc_status)?;

    Ok(Response::new(proto::ListCollectionIdsResponse {
        collection_ids: page.collection_ids,
        next_page_token: page.next_page_token,
    }))
}

fn rpc_status_from_error(error: nimbus_core::Error) -> RpcStatus {
    RpcStatus {
        code: firestore_grpc_code(&error) as i32,
        message: error.to_string(),
        details: Vec::new(),
    }
}

pub(super) async fn handle_create_document(
    service: &FirestoreGrpcService,
    request: Request<CreateDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let parent = resource_names::parse_parent_name(&request.parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let collection_target =
        resource_names::parse_collection_target(&request.parent, &request.collection_id)
            .map_err(resource_name_error_to_core)
            .map_err(firebase_grpc_status)?;
    let response_mask = lower_optional_document_mask(request.mask)?;
    let mut document = request.document.ok_or_else(|| {
        Status::invalid_argument("CreateDocument requests must include a document")
    })?;
    if !document.name.is_empty() {
        return Err(Status::invalid_argument(
            "CreateDocument request documents must not set name",
        ));
    }
    let document_id = if request.document_id.is_empty() {
        DocumentId::new()
    } else {
        DocumentId::from_key(request.document_id).map_err(firebase_grpc_status)?
    };
    let document_path =
        nimbus_core::DocumentPath::new(collection_target.collection_path, document_id);
    let document_name = firestore_document_name(&parent.database, &document_path);
    document.name = document_name.clone();

    let batch = lower_write_batch(
        &[proto::Write {
            operation: Some(proto::write::Operation::Update(document)),
            update_mask: None,
            update_transforms: Vec::new(),
            current_document: Some(proto::Precondition {
                condition_type: Some(proto::precondition::ConditionType::Exists(false)),
            }),
        }],
        &parent.database,
    )?;
    commit_batch_for_database(&state, &parent.database, &auth.principal, batch, None)
        .map_err(firebase_grpc_status)?;
    let created = get_document_for_database(
        &state,
        &parent.database,
        &auth.principal,
        &document_path,
        None,
    )
    .map_err(firebase_grpc_status)?
    .ok_or_else(|| Status::internal("created document was not readable after commit"))?;

    Ok(Response::new(proto_document(
        &document_name,
        &created,
        response_mask.as_deref(),
    )?))
}

pub(super) async fn handle_begin_transaction(
    service: &FirestoreGrpcService,
    request: Request<BeginTransactionRequest>,
) -> Result<Response<BeginTransactionResponse>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let database = parse_database_name(&request.database)?;
    let mode = lower_transaction_mode(request.options)?;
    let session = begin_transaction_session_for_database(&state, &database, &auth.principal, mode)
        .map_err(firebase_grpc_status)?;

    Ok(Response::new(BeginTransactionResponse {
        transaction: session.token.as_str().as_bytes().to_vec(),
    }))
}

pub(super) async fn handle_rollback(
    service: &FirestoreGrpcService,
    request: Request<RollbackRequest>,
) -> Result<Response<()>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let database = parse_database_name(&request.database)?;
    rollback_transaction_session_for_database(
        &state,
        &database,
        &auth.principal,
        &request.transaction,
    )
    .map_err(firebase_grpc_status)?;
    Ok(Response::new(()))
}

pub(super) async fn handle_update_document(
    service: &FirestoreGrpcService,
    request: Request<UpdateDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let document = request.document.ok_or_else(|| {
        Status::invalid_argument("UpdateDocument requests must include a document")
    })?;
    let parsed_document = resource_names::parse_document_name(&document.name)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let response_mask = lower_optional_document_mask(request.mask)?;
    let document_name = document.name.clone();
    let document_path = parsed_document.document_path.clone();
    let batch = lower_write_batch(
        &[proto::Write {
            operation: Some(proto::write::Operation::Update(document)),
            update_mask: request.update_mask,
            update_transforms: Vec::new(),
            current_document: request.current_document,
        }],
        &parsed_document.database,
    )?;
    commit_batch_for_database(
        &state,
        &parsed_document.database,
        &auth.principal,
        batch,
        None,
    )
    .map_err(firebase_grpc_status)?;
    let updated = get_document_for_database(
        &state,
        &parsed_document.database,
        &auth.principal,
        &document_path,
        None,
    )
    .map_err(firebase_grpc_status)?
    .ok_or_else(|| Status::internal("updated document was not readable after commit"))?;

    Ok(Response::new(proto_document(
        &document_name,
        &updated,
        response_mask.as_deref(),
    )?))
}

pub(super) async fn handle_delete_document(
    service: &FirestoreGrpcService,
    request: Request<DeleteDocumentRequest>,
) -> Result<Response<()>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = request.into_inner();
    let parsed_document = resource_names::parse_document_name(&request.name)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let batch = lower_write_batch(
        &[proto::Write {
            operation: Some(proto::write::Operation::Delete(request.name)),
            update_mask: None,
            update_transforms: Vec::new(),
            current_document: request.current_document,
        }],
        &parsed_document.database,
    )?;
    commit_batch_for_database(
        &state,
        &parsed_document.database,
        &auth.principal,
        batch,
        None,
    )
    .map_err(firebase_grpc_status)?;
    Ok(Response::new(()))
}

pub(super) async fn handle_run_query(
    service: &FirestoreGrpcService,
    request: Request<RunQueryRequest>,
) -> Result<Response<tonic::codegen::BoxStream<RunQueryResponse>>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = lower_run_query_request(request.into_inner())?;
    let outcome = run_query_documents_for_database(
        &state,
        &request.database,
        &auth.principal,
        request.parent_document_path.as_ref(),
        request.structured_query,
        request.transaction.as_deref(),
    )
    .map_err(firebase_grpc_status)?;
    let read_time = Some(prost_timestamp_from_core(outcome.read_time)?);

    let responses = if outcome.documents.is_empty() {
        vec![RunQueryResponse {
            transaction: Vec::new(),
            document: None,
            read_time,
            skipped_results: outcome.skipped_results as i32,
            explain_metrics: None,
            continuation_selector: None,
        }]
    } else {
        outcome
            .documents
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let document_name =
                    firestore_document_name(&request.database, &entry.document_path);
                Ok(RunQueryResponse {
                    transaction: Vec::new(),
                    document: Some(proto_document(&document_name, &entry.document, None)?),
                    read_time,
                    skipped_results: if index == 0 {
                        outcome.skipped_results as i32
                    } else {
                        0
                    },
                    explain_metrics: None,
                    continuation_selector: None,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?
    };

    let output: tonic::codegen::BoxStream<RunQueryResponse> =
        Box::pin(stream::iter(responses.into_iter().map(Ok::<_, Status>)));
    Ok(Response::new(output))
}

pub(super) async fn handle_run_aggregation_query(
    service: &FirestoreGrpcService,
    request: Request<RunAggregationQueryRequest>,
) -> Result<Response<tonic::codegen::BoxStream<RunAggregationQueryResponse>>, Status> {
    let (state, auth) = resolve_request_auth(service, &request).await?;
    let request = lower_run_aggregation_query_request(request.into_inner())?;
    let outcome = run_aggregation_query_for_database(
        &state,
        &request.database,
        &auth.principal,
        request.parent_document_path.as_ref(),
        request.aggregation_query,
    )
    .map_err(firebase_grpc_status)?;

    let response = RunAggregationQueryResponse {
        result: Some(proto_aggregation_result(&outcome.result)?),
        transaction: Vec::new(),
        read_time: Some(prost_timestamp_from_core(outcome.read_time)?),
        explain_metrics: None,
    };
    let output: tonic::codegen::BoxStream<RunAggregationQueryResponse> =
        Box::pin(stream::iter(vec![Ok::<_, Status>(response)]));
    Ok(Response::new(output))
}

fn parse_database_name(database: &str) -> Result<FirestoreDatabaseName, Status> {
    resource_names::parse_database_name(database)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)
}

fn lower_optional_document_mask(
    mask: Option<proto::DocumentMask>,
) -> Result<Option<Vec<String>>, Status> {
    mask.map(|mask| lower_document_mask_paths(mask.field_paths))
        .transpose()
        .map_err(batch_get_request_error_to_core)
        .map_err(firebase_grpc_status)
}

fn lower_batch_get_request(
    request: BatchGetDocumentsRequest,
) -> Result<(FirestoreDatabaseName, ParsedBatchGetRequest), Status> {
    let database = parse_database_name(&request.database)?;
    if request.documents.is_empty() {
        return Err(Status::invalid_argument(
            "BatchGetDocuments request must include at least one document",
        ));
    }

    let transaction = match request.consistency_selector {
        None => None,
        Some(BatchGetConsistencySelector::Transaction(transaction)) => Some(transaction),
        Some(BatchGetConsistencySelector::NewTransaction(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore BatchGetDocuments feature: `new_transaction`",
            ));
        }
        Some(BatchGetConsistencySelector::ReadTime(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore BatchGetDocuments feature: `read_time`",
            ));
        }
    };
    let mask = request
        .mask
        .map(|mask| lower_document_mask_paths(mask.field_paths))
        .transpose()
        .map_err(batch_get_request_error_to_core)
        .map_err(firebase_grpc_status)?;

    let mut seen_documents = HashSet::new();
    let mut documents = Vec::new();
    for document_name in request.documents {
        let parsed_document = resource_names::parse_document_name(&document_name)
            .map_err(resource_name_error_to_core)
            .map_err(firebase_grpc_status)?;
        ensure_database_match(
            &database,
            &parsed_document.database,
            "requested document belongs to a different database",
        )?;
        let canonical_name = firestore_document_name(&database, &parsed_document.document_path);
        if seen_documents.insert(canonical_name.clone()) {
            documents.push(ParsedBatchGetDocument {
                document_path: parsed_document.document_path,
                document_name: canonical_name,
            });
        }
    }

    Ok((
        database,
        ParsedBatchGetRequest {
            documents,
            mask,
            transaction,
        },
    ))
}

struct LoweredRunQueryRequest {
    database: FirestoreDatabaseName,
    parent_document_path: Option<nimbus_core::DocumentPath>,
    structured_query: StructuredQuery,
    transaction: Option<Vec<u8>>,
}

struct LoweredRunAggregationQueryRequest {
    database: FirestoreDatabaseName,
    parent_document_path: Option<nimbus_core::DocumentPath>,
    aggregation_query: StructuredAggregationQuery,
}

fn lower_run_query_request(request: RunQueryRequest) -> Result<LoweredRunQueryRequest, Status> {
    if request.explain_options.is_some() {
        return Err(Status::invalid_argument(
            "unsupported Firestore RunQuery feature: `explain_options`",
        ));
    }
    let transaction = match request.consistency_selector {
        None => None,
        Some(RunQueryConsistencySelector::Transaction(transaction)) => Some(transaction),
        Some(RunQueryConsistencySelector::NewTransaction(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore RunQuery feature: `new_transaction`",
            ));
        }
        Some(RunQueryConsistencySelector::ReadTime(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore RunQuery feature: `read_time`",
            ));
        }
    };

    let FirestoreParentName {
        database,
        parent_document_path,
    } = resource_names::parse_parent_name(&request.parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let structured_query = match request.query_type {
        Some(RunQueryType::StructuredQuery(query)) => lower_structured_query(query)?,
        None => {
            return Err(Status::invalid_argument(
                "RunQuery requests must include a structured_query",
            ));
        }
    };

    Ok(LoweredRunQueryRequest {
        database,
        parent_document_path,
        structured_query,
        transaction,
    })
}

fn lower_run_aggregation_query_request(
    request: RunAggregationQueryRequest,
) -> Result<LoweredRunAggregationQueryRequest, Status> {
    if request.explain_options.is_some() {
        return Err(Status::invalid_argument(
            "unsupported Firestore RunAggregationQuery feature: `explain_options`",
        ));
    }
    match request.consistency_selector {
        None => {}
        Some(RunAggregationConsistencySelector::Transaction(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore RunAggregationQuery feature: `transaction`",
            ));
        }
        Some(RunAggregationConsistencySelector::NewTransaction(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore RunAggregationQuery feature: `new_transaction`",
            ));
        }
        Some(RunAggregationConsistencySelector::ReadTime(_)) => {
            return Err(Status::invalid_argument(
                "unsupported Firestore RunAggregationQuery feature: `read_time`",
            ));
        }
    }

    let FirestoreParentName {
        database,
        parent_document_path,
    } = resource_names::parse_parent_name(&request.parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    let aggregation_query = match request.query_type {
        Some(RunAggregationQueryType::StructuredAggregationQuery(query)) => {
            lower_structured_aggregation_query(query)?
        }
        None => {
            return Err(Status::invalid_argument(
                "RunAggregationQuery requests must include a structured_aggregation_query",
            ));
        }
    };

    Ok(LoweredRunAggregationQueryRequest {
        database,
        parent_document_path,
        aggregation_query,
    })
}

fn lower_transaction_mode(
    options: Option<proto::TransactionOptions>,
) -> Result<TransactionSessionMode, Status> {
    match options.and_then(|options| options.mode) {
        None => Ok(TransactionSessionMode::ReadWrite),
        Some(ProtoTransactionMode::ReadWrite(read_write)) => {
            if !read_write.retry_transaction.is_empty() {
                return Err(Status::invalid_argument(
                    "unsupported Firestore BeginTransaction feature: `read_write.retry_transaction`",
                ));
            }
            Ok(TransactionSessionMode::ReadWrite)
        }
        Some(ProtoTransactionMode::ReadOnly(read_only)) => {
            if read_only.consistency_selector.is_some() {
                return Err(Status::invalid_argument(
                    "unsupported Firestore BeginTransaction feature: `read_only.read_time`",
                ));
            }
            Ok(TransactionSessionMode::ReadOnly)
        }
    }
}

fn lower_structured_query(query: proto::StructuredQuery) -> Result<StructuredQuery, Status> {
    Ok(StructuredQuery {
        select: query.select.map(lower_projection).transpose()?,
        from: query
            .from
            .into_iter()
            .map(lower_collection_selector)
            .collect::<Result<Vec<_>, _>>()?,
        where_filter: query.r#where.map(lower_query_filter).transpose()?,
        order_by: query
            .order_by
            .into_iter()
            .map(lower_structured_order)
            .collect::<Result<Vec<_>, _>>()?,
        start_at: query.start_at.map(lower_cursor).transpose()?,
        end_at: query.end_at.map(lower_cursor).transpose()?,
        offset: if query.offset == 0 {
            None
        } else {
            Some(u32::try_from(query.offset).map_err(|_| {
                Status::invalid_argument("structured query offset cannot be negative")
            })?)
        },
        limit: match query.limit {
            Some(limit) => Some(u32::try_from(limit).map_err(|_| {
                Status::invalid_argument("structured query limit cannot be negative")
            })?),
            None => None,
        },
        find_nearest: query.find_nearest.map(lower_find_nearest).transpose()?,
    })
}

fn lower_structured_aggregation_query(
    query: proto::StructuredAggregationQuery,
) -> Result<StructuredAggregationQuery, Status> {
    let structured_query = match query.query_type {
        Some(proto::structured_aggregation_query::QueryType::StructuredQuery(query)) => {
            lower_structured_query(query)?
        }
        None => {
            return Err(Status::invalid_argument(
                "structured_aggregation_query must include a structured_query",
            ));
        }
    };
    if query.aggregations.is_empty() {
        return Err(Status::invalid_argument(
            "structured_aggregation_query must include at least one aggregation",
        ));
    }

    let mut seen_aliases = HashSet::new();
    let mut generated_aliases = 0usize;
    let aggregations = query
        .aggregations
        .into_iter()
        .map(|aggregation| {
            lower_structured_aggregation(aggregation, &mut generated_aliases, &mut seen_aliases)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(StructuredAggregationQuery {
        structured_query,
        aggregations,
    })
}

fn lower_structured_aggregation(
    aggregation: proto::structured_aggregation_query::Aggregation,
    generated_aliases: &mut usize,
    seen_aliases: &mut HashSet<String>,
) -> Result<StructuredAggregation, Status> {
    let alias =
        normalize_proto_aggregation_alias(aggregation.alias, generated_aliases, seen_aliases)?;
    let operator = match aggregation.operator {
        Some(ProtoAggregationOperator::Count(count)) => {
            AggregationOperator::Count(CountAggregation {
                up_to: lower_count_up_to(count.up_to)?,
            })
        }
        Some(ProtoAggregationOperator::Sum(sum)) => AggregationOperator::Sum(FieldReference::new(
            sum.field
                .ok_or_else(|| {
                    Status::invalid_argument("sum aggregations must include a field reference")
                })?
                .field_path,
        )),
        Some(ProtoAggregationOperator::Avg(avg)) => AggregationOperator::Avg(FieldReference::new(
            avg.field
                .ok_or_else(|| {
                    Status::invalid_argument("avg aggregations must include a field reference")
                })?
                .field_path,
        )),
        None => {
            return Err(Status::invalid_argument(
                "each aggregation must set exactly one operator",
            ));
        }
    };
    Ok(StructuredAggregation { alias, operator })
}

fn normalize_proto_aggregation_alias(
    alias: String,
    generated_aliases: &mut usize,
    seen_aliases: &mut HashSet<String>,
) -> Result<String, Status> {
    let alias = if alias.is_empty() {
        *generated_aliases += 1;
        format!("field_{generated_aliases}")
    } else {
        alias
    };
    if alias.trim().is_empty() {
        return Err(Status::invalid_argument(
            "aggregation aliases must not be empty",
        ));
    }
    if !seen_aliases.insert(alias.clone()) {
        return Err(Status::invalid_argument(format!(
            "aggregation alias `{alias}` must be unique"
        )));
    }
    Ok(alias)
}

fn lower_count_up_to(up_to: Option<i64>) -> Result<Option<u64>, Status> {
    let Some(up_to) = up_to else {
        return Ok(None);
    };
    if up_to <= 0 {
        return Err(Status::invalid_argument(
            "count aggregation `up_to` must be greater than zero",
        ));
    }
    Ok(Some(u64::try_from(up_to).map_err(|_| {
        Status::invalid_argument("count aggregation `up_to` exceeds Firestore int64 range")
    })?))
}

fn proto_aggregation_result(
    result: &nimbus_core::StructuredAggregationResult,
) -> Result<proto::AggregationResult, Status> {
    Ok(proto::AggregationResult {
        aggregate_fields: result
            .aggregate_fields
            .iter()
            .map(|(alias, value)| {
                encode_nimbus_value_to_grpc(value).map(|value| (alias.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, _>>()?,
    })
}

fn lower_collection_selector(
    selector: proto::structured_query::CollectionSelector,
) -> Result<CollectionSelector, Status> {
    Ok(CollectionSelector {
        collection_id: CollectionName::new(selector.collection_id).map_err(firebase_grpc_status)?,
        all_descendants: selector.all_descendants,
    })
}

fn lower_projection(projection: proto::structured_query::Projection) -> Result<Projection, Status> {
    Ok(Projection {
        fields: projection
            .fields
            .into_iter()
            .map(lower_field_reference)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn lower_query_filter(filter: proto::structured_query::Filter) -> Result<QueryFilter, Status> {
    match filter.filter_type {
        Some(ProtoFilterType::CompositeFilter(filter)) => {
            Ok(QueryFilter::CompositeFilter(CompositeFilter {
                op: match ProtoCompositeOperator::try_from(filter.op) {
                    Ok(ProtoCompositeOperator::And) => CompositeOperator::And,
                    Ok(ProtoCompositeOperator::Or) => CompositeOperator::Or,
                    Ok(ProtoCompositeOperator::Unspecified) | Err(_) => {
                        return Err(Status::invalid_argument(
                            "structured query composite filters must set a supported operator",
                        ));
                    }
                },
                filters: filter
                    .filters
                    .into_iter()
                    .map(lower_query_filter)
                    .collect::<Result<Vec<_>, _>>()?,
            }))
        }
        Some(ProtoFilterType::FieldFilter(filter)) => Ok(QueryFilter::FieldFilter(FieldFilter {
            field: filter
                .field
                .map(lower_field_reference)
                .transpose()?
                .ok_or_else(|| {
                    Status::invalid_argument("structured query field filters must include a field")
                })?,
            op: match ProtoFieldFilterOperator::try_from(filter.op) {
                Ok(ProtoFieldFilterOperator::LessThan) => FieldFilterOperator::LessThan,
                Ok(ProtoFieldFilterOperator::LessThanOrEqual) => {
                    FieldFilterOperator::LessThanOrEqual
                }
                Ok(ProtoFieldFilterOperator::GreaterThan) => FieldFilterOperator::GreaterThan,
                Ok(ProtoFieldFilterOperator::GreaterThanOrEqual) => {
                    FieldFilterOperator::GreaterThanOrEqual
                }
                Ok(ProtoFieldFilterOperator::Equal) => FieldFilterOperator::Equal,
                Ok(ProtoFieldFilterOperator::NotEqual) => FieldFilterOperator::NotEqual,
                Ok(ProtoFieldFilterOperator::ArrayContains) => FieldFilterOperator::ArrayContains,
                Ok(ProtoFieldFilterOperator::In) => FieldFilterOperator::In,
                Ok(ProtoFieldFilterOperator::ArrayContainsAny) => {
                    FieldFilterOperator::ArrayContainsAny
                }
                Ok(ProtoFieldFilterOperator::NotIn) => FieldFilterOperator::NotIn,
                Ok(ProtoFieldFilterOperator::Unspecified) | Err(_) => {
                    return Err(Status::invalid_argument(
                        "structured query field filters must set a supported operator",
                    ));
                }
            },
            value: filter
                .value
                .as_ref()
                .ok_or_else(|| {
                    Status::invalid_argument("structured query field filters must include a value")
                })
                .and_then(decode_query_value_from_grpc)?,
        })),
        Some(ProtoFilterType::UnaryFilter(filter)) => {
            let field = match filter.operand_type {
                Some(ProtoUnaryOperandType::Field(field)) => lower_field_reference(field)?,
                None => {
                    return Err(Status::invalid_argument(
                        "structured query unary filters must include a field operand",
                    ));
                }
            };
            let op = match ProtoUnaryOperator::try_from(filter.op) {
                Ok(ProtoUnaryOperator::IsNan) => UnaryFilterOperator::IsNan,
                Ok(ProtoUnaryOperator::IsNull) => UnaryFilterOperator::IsNull,
                Ok(ProtoUnaryOperator::IsNotNan) => UnaryFilterOperator::IsNotNan,
                Ok(ProtoUnaryOperator::IsNotNull) => UnaryFilterOperator::IsNotNull,
                Ok(ProtoUnaryOperator::Unspecified) | Err(_) => {
                    return Err(Status::invalid_argument(
                        "structured query unary filters must set a supported operator",
                    ));
                }
            };
            Ok(QueryFilter::UnaryFilter(UnaryFilter { op, field }))
        }
        None => Err(Status::invalid_argument(
            "structured query filters must set exactly one filter type",
        )),
    }
}

fn lower_structured_order(
    order: proto::structured_query::Order,
) -> Result<StructuredOrder, Status> {
    Ok(StructuredOrder {
        field: order
            .field
            .map(lower_field_reference)
            .transpose()?
            .ok_or_else(|| {
                Status::invalid_argument("structured query order_by clauses must include a field")
            })?,
        direction: match ProtoQueryDirection::try_from(order.direction) {
            Ok(ProtoQueryDirection::Ascending) | Ok(ProtoQueryDirection::Unspecified) => {
                QueryDirection::Ascending
            }
            Ok(ProtoQueryDirection::Descending) => QueryDirection::Descending,
            Err(_) => {
                return Err(Status::invalid_argument(
                    "structured query order_by clauses must use a supported direction",
                ));
            }
        },
    })
}

fn lower_field_reference(
    field: proto::structured_query::FieldReference,
) -> Result<FieldReference, Status> {
    if field.field_path.is_empty() {
        return Err(Status::invalid_argument(
            "structured query field references cannot be empty",
        ));
    }
    Ok(FieldReference::new(field.field_path))
}

fn decode_query_value_from_grpc(value: &proto::Value) -> Result<serde_json::Value, Status> {
    match value.value_type.as_ref() {
        Some(proto::value::ValueType::ReferenceValue(reference)) => {
            Ok(serde_json::Value::String(reference.clone()))
        }
        Some(proto::value::ValueType::ArrayValue(array)) => Ok(serde_json::Value::Array(
            array
                .values
                .iter()
                .map(decode_query_value_from_grpc)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Some(proto::value::ValueType::MapValue(map)) => Ok(serde_json::Value::Object(
            map.fields
                .iter()
                .map(|(field, value)| {
                    decode_query_value_from_grpc(value).map(|value| (field.clone(), value))
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()?,
        )),
        _ => decode_nimbus_value_from_grpc(value),
    }
}

fn lower_cursor(cursor: proto::Cursor) -> Result<StructuredCursor, Status> {
    Ok(StructuredCursor {
        values: cursor
            .values
            .iter()
            .map(decode_query_value_from_grpc)
            .collect::<Result<Vec<_>, _>>()?,
        before: cursor.before,
    })
}

fn lower_find_nearest(
    find_nearest: proto::structured_query::FindNearest,
) -> Result<FindNearest, Status> {
    let limit = u32::try_from(find_nearest.limit.ok_or_else(|| {
        Status::invalid_argument("structured query find_nearest must include a positive limit")
    })?)
    .map_err(|_| {
        Status::invalid_argument("structured query find_nearest limit cannot be negative")
    })?;
    if limit == 0 {
        return Err(Status::invalid_argument(
            "structured query find_nearest limit must be positive",
        ));
    }

    Ok(FindNearest {
        vector_field: find_nearest
            .vector_field
            .map(lower_field_reference)
            .transpose()?
            .ok_or_else(|| {
                Status::invalid_argument("structured query find_nearest must include vector_field")
            })?,
        query_vector: find_nearest
            .query_vector
            .as_ref()
            .ok_or_else(|| {
                Status::invalid_argument("structured query find_nearest must include query_vector")
            })
            .and_then(decode_nimbus_value_from_grpc)?,
        distance_measure: match ProtoDistanceMeasure::try_from(find_nearest.distance_measure) {
            Ok(ProtoDistanceMeasure::Euclidean) => DistanceMeasure::Euclidean,
            Ok(ProtoDistanceMeasure::Cosine) => DistanceMeasure::Cosine,
            Ok(ProtoDistanceMeasure::DotProduct) => DistanceMeasure::DotProduct,
            Ok(ProtoDistanceMeasure::Unspecified) | Err(_) => {
                return Err(Status::invalid_argument(
                    "structured query find_nearest must include a supported distance_measure",
                ));
            }
        },
        limit,
        distance_result_field: (!find_nearest.distance_result_field.is_empty())
            .then_some(find_nearest.distance_result_field),
        distance_threshold: find_nearest
            .distance_threshold
            .map(|threshold| {
                Number::from_f64(threshold).ok_or_else(|| {
                    Status::invalid_argument(
                        "structured query find_nearest distance_threshold must be finite",
                    )
                })
            })
            .transpose()?,
    })
}

fn proto_document(
    document_name: &str,
    document: &Document,
    mask: Option<&[String]>,
) -> Result<proto::Document, Status> {
    let fields = match mask {
        Some(mask) => mask
            .iter()
            .filter_map(|field| {
                document
                    .fields
                    .get(field)
                    .map(|value| (field.clone(), value))
            })
            .map(|(field, value)| {
                encode_document_field_to_grpc(document, &field, value).map(|value| (field, value))
            })
            .collect::<Result<HashMap<_, _>, _>>()?,
        None => document
            .fields
            .iter()
            .map(|(field, value)| {
                encode_document_field_to_grpc(document, field, value)
                    .map(|value| (field.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, _>>()?,
    };
    let create_time = Some(prost_timestamp_from_core(document.creation_time)?);
    let update_time = Some(prost_timestamp_from_core(document.update_time)?);
    Ok(proto::Document {
        name: document_name.to_string(),
        fields,
        create_time,
        update_time,
    })
}

fn ensure_database_match(
    expected: &FirestoreDatabaseName,
    actual: &FirestoreDatabaseName,
    context: &str,
) -> Result<(), Status> {
    if expected == actual {
        Ok(())
    } else {
        Err(Status::invalid_argument(context.to_string()))
    }
}
