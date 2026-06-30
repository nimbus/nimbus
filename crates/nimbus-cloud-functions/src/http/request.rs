use std::collections::HashMap;

use http::{HeaderMap, Method, header};
use nimbus_core::InvocationAuth;
use nimbus_core::{Error, Result};
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

pub fn build_http_request_args(
    method: &Method,
    headers: &HeaderMap,
    original_query: Option<&str>,
    request_path: &str,
    query: HashMap<String, String>,
    body: &[u8],
) -> Result<Value> {
    let normalized_headers = normalized_headers(headers);
    let raw_body = if body.is_empty() {
        String::new()
    } else {
        std::str::from_utf8(body)
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
        "original_url": request_url(headers, original_query, request_path),
        "query": query,
        "headers": normalized_headers,
        "body": body,
        "raw_body": raw_body,
    }))
}

pub fn build_callable_request_args(
    headers: &HeaderMap,
    original_query: Option<&str>,
    request_path: &str,
    query: HashMap<String, String>,
    body: &[u8],
    auth: Option<&InvocationAuth>,
) -> Result<Value> {
    let normalized_headers = normalized_headers(headers);
    let raw_body = if body.is_empty() {
        return Err(Error::InvalidInput(
            "cloud functions callable handlers require a JSON request body".to_string(),
        ));
    } else {
        std::str::from_utf8(body)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "cloud functions callable handlers only cover UTF-8 request bodies in the first slice: {error}"
                ))
            })?
            .to_string()
    };
    if !header_value_contains(headers, header::CONTENT_TYPE, "json") {
        return Err(Error::InvalidInput(
            "cloud functions callable handlers require content-type application/json".to_string(),
        ));
    }
    let body: Value = serde_json::from_str(&raw_body).map_err(|error| {
        Error::InvalidInput(format!(
            "cloud functions callable handler could not parse JSON request body: {error}"
        ))
    })?;
    let data = body
        .as_object()
        .and_then(|body| body.get("data"))
        .cloned()
        .ok_or_else(|| {
            Error::InvalidInput(
                "cloud functions callable handlers require a top-level JSON `data` field"
                    .to_string(),
            )
        })?;

    Ok(serde_json::json!({
        "method": "POST",
        "path": request_path,
        "original_url": request_url(headers, original_query, request_path),
        "query": query,
        "headers": normalized_headers,
        "body": body,
        "raw_body": raw_body,
        "callable": {
            "data": data,
            "auth": callable_auth_payload(auth)?,
            "instance_id_token": header_string(headers, "firebase-instance-id-token"),
            "accepts_streaming": false,
        },
    }))
}

fn callable_auth_payload(auth: Option<&InvocationAuth>) -> Result<Option<Value>> {
    let Some(auth) = auth else {
        return Ok(None);
    };
    let uid = auth
        .verified_identity
        .as_ref()
        .map(|identity| identity.subject.clone())
        .or_else(|| {
            auth.identity
                .as_ref()
                .map(|identity| identity.subject.clone())
        });
    let token = if let Some(verified_identity) = auth.verified_identity.as_ref() {
        serialize_object(verified_identity)?
    } else if let Some(identity) = auth.identity.as_ref() {
        serialize_object(identity)?
    } else {
        Map::new()
    };
    Ok(Some(serde_json::json!({
        "uid": uid,
        "token": token,
    })))
}

fn serialize_object<T>(value: &T) -> Result<Map<String, Value>>
where
    T: Serialize,
{
    match serde_json::to_value(value).map_err(|error| Error::Serialization(error.to_string()))? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

pub fn request_url(
    headers: &HeaderMap,
    original_query: Option<&str>,
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
    let query_suffix = original_query
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    format!("{scheme}://{host}{request_path}{query_suffix}")
}

pub fn normalized_headers(headers: &HeaderMap) -> HashMap<String, String> {
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

pub fn header_value_contains(headers: &HeaderMap, name: header::HeaderName, needle: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
