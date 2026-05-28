use axum::body::Body;
use axum::response::Response;
use nimbus_cloud_functions::CloudFunctionsHttpResponseParts;
use nimbus_core::Error;

use crate::state::AppError;

pub(super) fn build_http_response(
    parts: CloudFunctionsHttpResponseParts,
) -> std::result::Result<Response, AppError> {
    let mut builder = Response::builder().status(parts.status);
    for (name, value) in parts.headers {
        builder = builder.header(name, value);
    }
    builder.body(Body::from(parts.body)).map_err(|error| {
        AppError::from(Error::Internal(format!(
            "cloud functions http response could not build: {error}"
        )))
    })
}
