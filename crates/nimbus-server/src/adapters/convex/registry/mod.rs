use std::path::Path;
use std::sync::Arc;

use axum::http::Method;
use nimbus_runtime::{
    InvocationAuth, InvocationKind, NimbusRuntimeError, RuntimeBundle, RuntimeExecutor,
    RuntimeLimits, RuntimePolicy,
};

use super::auth::{ConvexAuthVerifier, read_auth_config};
use super::templates::{method_name, resolve_template};
use super::*;

mod deploy_summary;
mod http_routes;
mod loading;
mod resolution;
mod schema;

pub(crate) use deploy_summary::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistryDeploySummary,
};
pub(in crate::adapters::convex) use http_routes::validate_runtime_http_route;
