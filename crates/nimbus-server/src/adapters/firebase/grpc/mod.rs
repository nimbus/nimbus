#![allow(dead_code, clippy::result_large_err)]

mod listen_stream;
mod listen_websocket;
mod unary;
mod write_stream;

pub(crate) use nimbus_firebase::grpc::generated;

use std::sync::Arc;

use nimbus_auth::ResolvedApplicationAuth;
use tonic::{Request, Response, Status};

use crate::application_auth::{
    extract_bearer_token_from_metadata, grpc_status_from_app_error,
    resolve_application_auth_from_bearer,
};
use crate::state::AppState;
use crate::state::record_authenticated_usage;

use generated::google::firestore::v1::BatchGetDocumentsRequest;
use generated::google::firestore::v1::BatchGetDocumentsResponse;
use generated::google::firestore::v1::BatchWriteRequest;
use generated::google::firestore::v1::BatchWriteResponse;
use generated::google::firestore::v1::BeginTransactionRequest;
use generated::google::firestore::v1::BeginTransactionResponse;
use generated::google::firestore::v1::CommitRequest;
use generated::google::firestore::v1::CommitResponse;
use generated::google::firestore::v1::CreateDocumentRequest;
use generated::google::firestore::v1::DeleteDocumentRequest;
use generated::google::firestore::v1::GetDocumentRequest;
use generated::google::firestore::v1::ListCollectionIdsRequest;
use generated::google::firestore::v1::ListCollectionIdsResponse;
use generated::google::firestore::v1::ListDocumentsRequest;
use generated::google::firestore::v1::ListDocumentsResponse;
use generated::google::firestore::v1::ListenRequest;
use generated::google::firestore::v1::ListenResponse;
use generated::google::firestore::v1::RollbackRequest;
use generated::google::firestore::v1::RunAggregationQueryRequest;
use generated::google::firestore::v1::RunAggregationQueryResponse;
use generated::google::firestore::v1::RunQueryRequest;
use generated::google::firestore::v1::RunQueryResponse;
use generated::google::firestore::v1::UpdateDocumentRequest;
use generated::google::firestore::v1::WriteRequest;
use generated::google::firestore::v1::WriteResponse;
use generated::google::firestore::v1::firestore_server::{Firestore, FirestoreServer};

pub(crate) use listen_websocket::listen_websocket;

#[derive(Clone)]
pub(crate) struct FirestoreGrpcService {
    state: Option<Arc<AppState>>,
    listen_targets: Arc<nimbus_firebase::grpc::RetainedListenRegistry>,
    write_streams: Arc<nimbus_firebase::grpc::WriteStreamRegistry>,
}

impl std::fmt::Debug for FirestoreGrpcService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirestoreGrpcService")
            .field("has_state", &self.state.is_some())
            .finish()
    }
}

impl Default for FirestoreGrpcService {
    fn default() -> Self {
        Self::new()
    }
}

impl FirestoreGrpcService {
    pub(crate) fn new() -> Self {
        Self {
            state: None,
            listen_targets: Arc::new(nimbus_firebase::grpc::RetainedListenRegistry::new()),
            write_streams: Arc::new(nimbus_firebase::grpc::WriteStreamRegistry::new()),
        }
    }

    pub(crate) fn from_state(state: Arc<AppState>) -> Self {
        Self {
            state: Some(state),
            listen_targets: Arc::new(nimbus_firebase::grpc::RetainedListenRegistry::new()),
            write_streams: Arc::new(nimbus_firebase::grpc::WriteStreamRegistry::new()),
        }
    }

    pub(crate) fn into_server(self) -> FirestoreServer<Self> {
        FirestoreServer::new(self)
    }

    fn app_state(&self) -> Result<Arc<AppState>, Status> {
        self.state.clone().ok_or_else(|| {
            Status::internal("Firestore gRPC server constructed without application state")
        })
    }
}

fn request_bearer<T>(request: &Request<T>) -> Result<Option<String>, Status> {
    extract_bearer_token_from_metadata(request.metadata()).map_err(grpc_status_from_app_error)
}

async fn resolve_bearer_auth(
    service: &FirestoreGrpcService,
    bearer: Option<String>,
) -> Result<(Arc<AppState>, ResolvedApplicationAuth), Status> {
    let state = service.app_state()?;
    let auth = resolve_application_auth_from_bearer(&state, bearer.as_deref())
        .await
        .map_err(grpc_status_from_app_error)?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    Ok((state, auth))
}

#[tonic::async_trait]
impl Firestore for FirestoreGrpcService {
    async fn batch_get_documents(
        &self,
        request: Request<BatchGetDocumentsRequest>,
    ) -> std::result::Result<Response<tonic::codegen::BoxStream<BatchGetDocumentsResponse>>, Status>
    {
        unary::handle_batch_get_documents(self, request).await
    }

    async fn batch_write(
        &self,
        request: Request<BatchWriteRequest>,
    ) -> std::result::Result<Response<BatchWriteResponse>, Status> {
        unary::handle_batch_write(self, request).await
    }

    async fn begin_transaction(
        &self,
        request: Request<BeginTransactionRequest>,
    ) -> std::result::Result<Response<BeginTransactionResponse>, Status> {
        unary::handle_begin_transaction(self, request).await
    }

    async fn commit(
        &self,
        request: Request<CommitRequest>,
    ) -> std::result::Result<Response<CommitResponse>, Status> {
        unary::handle_commit(self, request).await
    }

    async fn create_document(
        &self,
        request: Request<CreateDocumentRequest>,
    ) -> std::result::Result<Response<generated::google::firestore::v1::Document>, Status> {
        unary::handle_create_document(self, request).await
    }

    async fn delete_document(
        &self,
        request: Request<DeleteDocumentRequest>,
    ) -> std::result::Result<Response<()>, Status> {
        unary::handle_delete_document(self, request).await
    }

    async fn get_document(
        &self,
        request: Request<GetDocumentRequest>,
    ) -> std::result::Result<Response<generated::google::firestore::v1::Document>, Status> {
        unary::handle_get_document(self, request).await
    }

    async fn list_documents(
        &self,
        request: Request<ListDocumentsRequest>,
    ) -> std::result::Result<Response<ListDocumentsResponse>, Status> {
        unary::handle_list_documents(self, request).await
    }

    async fn list_collection_ids(
        &self,
        request: Request<ListCollectionIdsRequest>,
    ) -> std::result::Result<Response<ListCollectionIdsResponse>, Status> {
        unary::handle_list_collection_ids(self, request).await
    }

    async fn write(
        &self,
        request: Request<tonic::Streaming<WriteRequest>>,
    ) -> std::result::Result<Response<tonic::codegen::BoxStream<WriteResponse>>, Status> {
        write_stream::handle_write(self, request).await
    }

    async fn listen(
        &self,
        request: Request<tonic::Streaming<ListenRequest>>,
    ) -> std::result::Result<Response<tonic::codegen::BoxStream<ListenResponse>>, Status> {
        listen_stream::handle_listen(self, request).await
    }

    async fn rollback(
        &self,
        request: Request<RollbackRequest>,
    ) -> std::result::Result<Response<()>, Status> {
        unary::handle_rollback(self, request).await
    }

    async fn run_query(
        &self,
        request: Request<RunQueryRequest>,
    ) -> std::result::Result<Response<tonic::codegen::BoxStream<RunQueryResponse>>, Status> {
        unary::handle_run_query(self, request).await
    }

    async fn run_aggregation_query(
        &self,
        request: Request<RunAggregationQueryRequest>,
    ) -> std::result::Result<Response<tonic::codegen::BoxStream<RunAggregationQueryResponse>>, Status>
    {
        unary::handle_run_aggregation_query(self, request).await
    }

    async fn update_document(
        &self,
        request: Request<UpdateDocumentRequest>,
    ) -> std::result::Result<Response<generated::google::firestore::v1::Document>, Status> {
        unary::handle_update_document(self, request).await
    }
}

pub(crate) fn firestore_grpc_server() -> FirestoreServer<FirestoreGrpcService> {
    FirestoreGrpcService::new().into_server()
}

pub(crate) fn firestore_grpc_server_with_state(
    state: Arc<AppState>,
) -> FirestoreServer<FirestoreGrpcService> {
    FirestoreGrpcService::from_state(state).into_server()
}

#[cfg(test)]
mod tests {
    use tonic::server::NamedService;
    use tonic::{Code, Request};

    use super::generated::google::firestore::v1::firestore_server::{Firestore, FirestoreServer};
    use super::generated::google::firestore::v1::{CommitRequest, RunQueryRequest};
    use super::{FirestoreGrpcService, firestore_grpc_server};

    #[test]
    fn firestore_grpc_server_uses_canonical_service_name() {
        let _server = firestore_grpc_server();
        assert_eq!(
            FirestoreServer::<FirestoreGrpcService>::NAME,
            "google.firestore.v1.Firestore"
        );
    }

    #[tokio::test]
    async fn firestore_grpc_unary_stub_reports_missing_application_state() {
        let error = match FirestoreGrpcService::new()
            .commit(Request::new(CommitRequest::default()))
            .await
        {
            Ok(_) => panic!("default Firestore commit stub should require application state"),
            Err(error) => error,
        };
        assert_eq!(error.code(), Code::Internal);
        assert_eq!(
            error.message(),
            "Firestore gRPC server constructed without application state"
        );
    }

    #[tokio::test]
    async fn firestore_grpc_server_streaming_stub_reports_missing_application_state() {
        let error = match FirestoreGrpcService::new()
            .run_query(Request::new(RunQueryRequest::default()))
            .await
        {
            Ok(_) => panic!("default Firestore run_query stub should require application state"),
            Err(error) => error,
        };
        assert_eq!(error.code(), Code::Internal);
        assert_eq!(
            error.message(),
            "Firestore gRPC server constructed without application state"
        );
    }
}
