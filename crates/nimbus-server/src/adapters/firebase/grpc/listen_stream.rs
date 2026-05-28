use futures::Stream;
use nimbus_core::PrincipalContext;
use tonic::{Request, Response, Status, Streaming};

use super::generated::google::firestore::v1::{ListenRequest, ListenResponse};
use super::{FirestoreGrpcService, request_bearer, resolve_bearer_auth};

pub(super) fn listen_response_stream<S>(
    service: &FirestoreGrpcService,
    requests: S,
    principal: PrincipalContext,
) -> Result<tonic::codegen::BoxStream<ListenResponse>, Status>
where
    S: Stream<Item = Result<ListenRequest, Status>> + Send + 'static,
{
    nimbus_firebase::grpc::listen_stream::listen_response_stream(
        service.app_state()?.service.clone(),
        service.listen_targets.clone(),
        requests,
        principal,
    )
}

pub(super) async fn handle_listen(
    service: &FirestoreGrpcService,
    request: Request<Streaming<ListenRequest>>,
) -> Result<Response<tonic::codegen::BoxStream<ListenResponse>>, Status> {
    let bearer = request_bearer(&request)?;
    let (_state, auth) = resolve_bearer_auth(service, bearer).await?;
    Ok(Response::new(listen_response_stream(
        service,
        request.into_inner(),
        auth.principal,
    )?))
}
