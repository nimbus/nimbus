use tonic::{Request, Response, Status, Streaming};

use super::generated::google::firestore::v1::{WriteRequest, WriteResponse};
use super::{FirestoreGrpcService, request_bearer, resolve_bearer_auth};

pub(super) async fn handle_write(
    service: &FirestoreGrpcService,
    request: Request<Streaming<WriteRequest>>,
) -> Result<Response<tonic::codegen::BoxStream<WriteResponse>>, Status> {
    let bearer = request_bearer(&request)?;
    let (state, auth) = resolve_bearer_auth(service, bearer).await?;
    Ok(Response::new(
        nimbus_firebase::grpc::write_stream::write_response_stream(
            state.service.clone(),
            service.write_streams.clone(),
            request.into_inner(),
            auth.principal,
        ),
    ))
}
