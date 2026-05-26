use std::collections::HashMap;

use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::Response;
use nimbus_core::Error;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppError;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CloudFunctionsHttpBodyKind {
    Json,
    Text,
}

#[derive(Debug, Deserialize)]
struct CloudFunctionsHttpResponseEnvelope {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    headers: Option<HashMap<String, Value>>,
    #[serde(default)]
    body_kind: Option<CloudFunctionsHttpBodyKind>,
    #[serde(default)]
    body: Value,
}

pub(super) fn build_http_response(value: Value) -> std::result::Result<Response, AppError> {
    let envelope: CloudFunctionsHttpResponseEnvelope =
        serde_json::from_value(value).map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "cloud functions http handler must return a response envelope: {error}"
            )))
        })?;
    let status = envelope
        .status
        .map(StatusCode::from_u16)
        .transpose()
        .map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "cloud functions http handler returned an invalid status code: {error}"
            )))
        })?
        .unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    let mut has_content_type = false;

    for (name, value) in parse_headers(envelope.headers)? {
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        builder = builder.header(name, value);
    }

    let body_kind = envelope.body_kind.unwrap_or(match &envelope.body {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            CloudFunctionsHttpBodyKind::Text
        }
        _ => CloudFunctionsHttpBodyKind::Json,
    });
    if matches!(body_kind, CloudFunctionsHttpBodyKind::Json) && !has_content_type {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let body = match body_kind {
        CloudFunctionsHttpBodyKind::Json => serde_json::to_vec(&envelope.body)
            .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?,
        CloudFunctionsHttpBodyKind::Text => render_text_body(envelope.body)?,
    };
    builder.body(Body::from(body)).map_err(|error| {
        AppError::from(Error::Internal(format!(
            "cloud functions http response could not build: {error}"
        )))
    })
}

fn parse_headers(
    headers: Option<HashMap<String, Value>>,
) -> std::result::Result<Vec<(String, String)>, AppError> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    headers
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::Null => None,
            Value::String(value) => Some(Ok((name, value))),
            Value::Number(value) => Some(Ok((name, value.to_string()))),
            Value::Bool(value) => Some(Ok((name, value.to_string()))),
            _ => Some(Err(AppError::from(Error::InvalidInput(format!(
                "cloud functions http header `{name}` must resolve to a string-coercible value"
            ))))),
        })
        .collect()
}

fn render_text_body(body: Value) -> std::result::Result<Vec<u8>, AppError> {
    match body {
        Value::Null => Ok(Vec::new()),
        Value::String(value) => Ok(value.into_bytes()),
        Value::Bool(value) => Ok(value.to_string().into_bytes()),
        Value::Number(value) => Ok(value.to_string().into_bytes()),
        _ => Err(AppError::from(Error::InvalidInput(
            "cloud functions http text responses must resolve to a string-coercible value"
                .to_string(),
        ))),
    }
}
