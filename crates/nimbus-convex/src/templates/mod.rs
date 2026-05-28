use ::http::Method;
use nimbus_core::{DocumentId, Error};
use serde_json::{Map, Value, json};

use crate::ConvexHttpRequestContext;

mod function;
mod helpers;
mod http;

pub use function::resolve_template;
pub use helpers::{empty_args, method_name, normalize_http_request_path, parse_job_id};
pub use http::resolve_http_template;
