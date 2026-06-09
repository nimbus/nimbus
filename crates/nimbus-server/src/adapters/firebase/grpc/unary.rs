use tonic::{Request, Response, Status};

use super::generated::google::firestore::v1::{
    self as proto, BatchGetDocumentsRequest, BatchGetDocumentsResponse, BatchWriteRequest,
    BatchWriteResponse, BeginTransactionRequest, BeginTransactionResponse, CommitRequest,
    CommitResponse, CreateDocumentRequest, DeleteDocumentRequest, GetDocumentRequest,
    ListCollectionIdsRequest, ListCollectionIdsResponse, ListDocumentsRequest,
    ListDocumentsResponse, RollbackRequest, RunAggregationQueryRequest,
    RunAggregationQueryResponse, RunQueryRequest, RunQueryResponse, UpdateDocumentRequest,
};
use super::{FirestoreGrpcService, request_bearer, resolve_bearer_auth};

pub(super) async fn handle_commit(
    service: &FirestoreGrpcService,
    request: Request<CommitRequest>,
) -> Result<Response<CommitResponse>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_commit(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_get_document(
    service: &FirestoreGrpcService,
    request: Request<GetDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_get_document(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_batch_get_documents(
    service: &FirestoreGrpcService,
    request: Request<BatchGetDocumentsRequest>,
) -> Result<Response<tonic::codegen::BoxStream<BatchGetDocumentsResponse>>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_batch_get_documents(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_batch_write(
    service: &FirestoreGrpcService,
    request: Request<BatchWriteRequest>,
) -> Result<Response<BatchWriteResponse>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_batch_write(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_list_documents(
    service: &FirestoreGrpcService,
    request: Request<ListDocumentsRequest>,
) -> Result<Response<ListDocumentsResponse>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_list_documents(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_list_collection_ids(
    service: &FirestoreGrpcService,
    request: Request<ListCollectionIdsRequest>,
) -> Result<Response<ListCollectionIdsResponse>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_list_collection_ids(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_create_document(
    service: &FirestoreGrpcService,
    request: Request<CreateDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_create_document(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_begin_transaction(
    service: &FirestoreGrpcService,
    request: Request<BeginTransactionRequest>,
) -> Result<Response<BeginTransactionResponse>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_begin_transaction(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_rollback(
    service: &FirestoreGrpcService,
    request: Request<RollbackRequest>,
) -> Result<Response<()>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_rollback(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_update_document(
    service: &FirestoreGrpcService,
    request: Request<UpdateDocumentRequest>,
) -> Result<Response<proto::Document>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_update_document(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_delete_document(
    service: &FirestoreGrpcService,
    request: Request<DeleteDocumentRequest>,
) -> Result<Response<()>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_delete_document(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_run_query(
    service: &FirestoreGrpcService,
    request: Request<RunQueryRequest>,
) -> Result<Response<tonic::codegen::BoxStream<RunQueryResponse>>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_run_query(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}

pub(super) async fn handle_run_aggregation_query(
    service: &FirestoreGrpcService,
    request: Request<RunAggregationQueryRequest>,
) -> Result<Response<tonic::codegen::BoxStream<RunAggregationQueryResponse>>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    nimbus_firebase::grpc::unary::handle_run_aggregation_query(
        &state.engine,
        &auth.principal,
        request.into_inner(),
    )
    .await
}
