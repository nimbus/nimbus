use std::path::Path;
use std::sync::Arc;

use ::http::Method;
use nimbus_core::InvocationAuth;
use nimbus_runtime::{InvocationKind, NimbusRuntimeError, RuntimeBundle, RuntimeLimits};

use super::auth::{ConvexAuthVerifier, read_auth_config};
use super::templates::{method_name, resolve_template};
use super::*;
use crate::manifest::*;

mod deploy_summary;
mod http_routes;
mod loading;
mod resolution;
mod schema;

pub use deploy_summary::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistryDeploySummary,
};
pub use http_routes::validate_runtime_http_route;
