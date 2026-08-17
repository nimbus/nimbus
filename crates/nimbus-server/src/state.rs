use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use nimbus_core::Error;

use crate::config::transport::TransportConfig;
use crate::error_envelope::StructuredHttpError;
use crate::local_server::LocalServerPolicyError;
use crate::system::VersionCheck;
use crate::workload_composition::ServerWorkloadProfile;

pub(crate) use nimbus_compute::config::control_plane::ControlPlaneConfig;
pub(crate) use nimbus_compute::config::deployment::DeploymentConfig;
pub(crate) use nimbus_compute::config::node_services::NodeServicesConfig;
pub(crate) use nimbus_compute::config::runtime::RuntimeGovernorConfig;
pub(crate) use nimbus_compute::state::{
    ComputeError, ComputeState, ComputeStateConfig, DeploymentState, RequestCancellationGuard,
    record_authenticated_usage,
};

pub(crate) struct AppStateConfig {
    pub(crate) workload: ServerWorkloadProfile,
    pub(crate) deployment: DeploymentConfig,
    pub(crate) control_plane: ControlPlaneConfig,
    pub(crate) node_services: NodeServicesConfig,
    pub(crate) transport: TransportConfig,
    pub(crate) runtime: RuntimeGovernorConfig,
}

/// Shared application state.
///
/// `AppState` is a thin transport-owning wrapper around `ComputeState`
/// (the axum-free compute plane, `nimbus-compute`). It `Deref`s to
/// `ComputeState` rather than exposing a `compute()` accessor: handler code
/// across this crate already reads compute fields directly (`state.engine`)
/// and calls compute methods (`state.tenant_isolation_mode()`); `Deref`
/// lets every one of those call sites keep compiling unchanged, since
/// Rust's deref coercion applies to both field access and method
/// resolution and chains through `Arc<AppState> -> AppState ->
/// ComputeState` automatically.
pub(crate) struct AppState {
    compute: ComputeState,
    transport: TransportConfig,
}

impl std::ops::Deref for AppState {
    type Target = ComputeState;

    fn deref(&self) -> &ComputeState {
        &self.compute
    }
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            workload,
            deployment,
            control_plane,
            node_services,
            transport,
            runtime,
        } = config;
        workload.authenticate_node_services(&node_services);
        let (engine, workload_composition) = workload.into_compute();
        let compute = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition,
            deployment,
            control_plane,
            node_services,
            runtime,
        });
        Self { compute, transport }
    }

    pub(crate) fn listen_addr(&self) -> Option<std::net::SocketAddr> {
        self.transport.listen_addr()
    }

    pub(crate) fn version_check(&self) -> Arc<VersionCheck> {
        self.transport.version_check()
    }

    pub(crate) fn request_server_shutdown(&self) -> std::result::Result<(), AppError> {
        let sender = self.transport.server_shutdown().ok_or_else(|| {
            AppError::from(Error::Internal(
                "server shutdown is unavailable for this router".to_owned(),
            ))
        })?;
        sender.send_replace(true);
        Ok(())
    }
}

/// HTTP-facing application error wrapper.
#[derive(Debug)]
pub(crate) enum AppError {
    Core(Error),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Structured(Box<StructuredHttpError>),
}

impl From<Error> for AppError {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

impl From<LocalServerPolicyError> for AppError {
    fn from(value: LocalServerPolicyError) -> Self {
        if value.is_forbidden() {
            Self::Forbidden(value.into_message())
        } else {
            Self::Unauthorized(value.into_message())
        }
    }
}

/// Bridges the axum-free `ComputeError` (raised by `nimbus-compute`) into
/// `AppError` (this crate's `IntoResponse` type). Each variant maps onto the
/// `AppError` constructor with matching semantics, so the rendered
/// status/body is identical to constructing the `AppError` variant
/// directly.
impl From<ComputeError> for AppError {
    fn from(value: ComputeError) -> Self {
        match value {
            ComputeError::Core(error) => Self::Core(error),
            ComputeError::Unauthorized(message) => Self::Unauthorized(message),
            ComputeError::Forbidden(message) => Self::Forbidden(message),
            ComputeError::NotFound(message) => Self::NotFound(message),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        StructuredHttpError::from_app_error(self).into_response()
    }
}

impl AppError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structured(error) => write!(f, "{error}"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Unauthorized(message) => write!(f, "{message}"),
            Self::Forbidden(message) => write!(f, "{message}"),
            Self::NotFound(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    #[test]
    fn unavailable_storage_error_maps_to_service_unavailable() {
        let response = AppError::from(Error::storage(
            nimbus_core::StorageErrorKind::Unavailable,
            "postgres pool unavailable",
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
