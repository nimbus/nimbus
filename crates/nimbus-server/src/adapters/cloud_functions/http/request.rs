use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, Method, header};
use nimbus_core::{Error, Result};
use serde_json::Value;

pub(super) fn build_http_request_args(
    method: &Method,
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
    query: HashMap<String, String>,
    body: Bytes,
) -> Result<Value> {
    let normalized_headers = normalized_headers(headers);
    let raw_body = if body.is_empty() {
        String::new()
    } else {
        std::str::from_utf8(&body)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "cloud functions http handlers only cover UTF-8 request bodies in the first slice: {error}"
                ))
            })?
            .to_string()
    };
    let body = if raw_body.is_empty() {
        Value::Null
    } else if header_value_contains(headers, header::CONTENT_TYPE, "json") {
        serde_json::from_str(&raw_body).map_err(|error| {
            Error::InvalidInput(format!(
                "cloud functions http handler could not parse JSON request body: {error}"
            ))
        })?
    } else {
        Value::String(raw_body.clone())
    };

    Ok(serde_json::json!({
        "method": method.as_str(),
        "path": request_path,
        "original_url": request_url(headers, original_uri, request_path),
        "query": query,
        "headers": normalized_headers,
        "body": body,
        "raw_body": raw_body,
    }))
}

pub(super) fn request_url(
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let query_suffix = original_uri
        .0
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    format!("{scheme}://{host}{request_path}{query_suffix}")
}

pub(super) fn normalized_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

pub(super) fn header_value_contains(
    headers: &HeaderMap,
    name: header::HeaderName,
    needle: &str,
) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
}
