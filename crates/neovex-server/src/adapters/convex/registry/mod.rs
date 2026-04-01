use std::path::Path;
use std::sync::Arc;

use axum::http::{HeaderMap, Method};
use neovex_runtime::{
    InvocationAuth, InvocationKind, NeovexRuntimeError, RuntimeBundle, RuntimeExecutor,
    RuntimeLimits, RuntimePolicy,
};

use super::auth::{ConvexAuthVerifier, read_auth_config};
use super::templates::{method_name, resolve_template};
use super::*;

mod http_routes;
mod loading;
mod resolution;

pub(in crate::adapters::convex) use http_routes::validate_runtime_http_route;
