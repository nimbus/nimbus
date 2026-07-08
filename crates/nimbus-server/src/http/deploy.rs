use std::sync::Arc;

use super::*;
use crate::local_server::authorize_deploy_admin_bearer;

use nimbus_compute::deploy::{DeployRequest, DeployResponse};

pub(crate) async fn deploy_app(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<DeployRequest>,
) -> Result<Json<DeployResponse>, AppError> {
    authorize_deploy_admin_bearer(state.deploy_admin_token(), &headers)?;
    let response = nimbus_compute::deploy::deploy_app(&state, request).await?;
    Ok(Json(response))
}
