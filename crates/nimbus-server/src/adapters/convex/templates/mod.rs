use axum::http::Method;
use nimbus_core::{DocumentId, Error};
use serde_json::{Map, Value, json};

use super::execution::ConvexHttpRequestContext;
use crate::state::AppError;

mod function;
mod helpers;
mod http;

pub(super) use function::resolve_template;
pub(super) use helpers::{empty_args, method_name, normalize_http_request_path, parse_job_id};
pub(super) use http::resolve_http_template;
