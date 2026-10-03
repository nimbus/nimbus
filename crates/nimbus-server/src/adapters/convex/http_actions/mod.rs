use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use nimbus_convex_host::prepare_http_action_response_async;
use nimbus_core::{Error, TenantId};
use serde_json::{Value, json};

use super::*;

mod dispatch;
mod request_context;
mod response;
mod route_request;

pub(in crate::adapters::convex) use dispatch::dispatch_http_route;
pub(in crate::adapters::convex) use route_request::ConvexHttpRouteRequest;
