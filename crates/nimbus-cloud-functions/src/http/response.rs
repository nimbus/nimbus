use std::collections::HashMap;

use http::{StatusCode, header};
use nimbus_core::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFunctionsHttpResponseParts {
    pub status: StatusCode,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn build_http_response_parts(value: Value) -> Result<CloudFunctionsHttpResponseParts> {
    let envelope: CloudFunctionsHttpResponseEnvelope =
        serde_json::from_value(value).map_err(|error| {
            Error::InvalidInput(format!(
                "cloud functions http handler must return a response envelope: {error}"
            ))
        })?;
    let status = envelope
        .status
        .map(StatusCode::from_u16)
        .transpose()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "cloud functions http handler returned an invalid status code: {error}"
            ))
        })?
        .unwrap_or(StatusCode::OK);
    let mut has_content_type = false;
    let mut headers = parse_headers(envelope.headers)?;

    for (name, _) in &headers {
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
    }

    let body_kind = envelope.body_kind.unwrap_or(match &envelope.body {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            CloudFunctionsHttpBodyKind::Text
        }
        _ => CloudFunctionsHttpBodyKind::Json,
    });
    if matches!(body_kind, CloudFunctionsHttpBodyKind::Json) && !has_content_type {
        headers.push((
            header::CONTENT_TYPE.as_str().to_string(),
            "application/json".to_string(),
        ));
    }
    let body = match body_kind {
        CloudFunctionsHttpBodyKind::Json => serde_json::to_vec(&envelope.body)
            .map_err(|error| Error::Serialization(error.to_string()))?,
        CloudFunctionsHttpBodyKind::Text => render_text_body(envelope.body)?,
    };
    Ok(CloudFunctionsHttpResponseParts {
        status,
        headers,
        body,
    })
}

fn parse_headers(headers: Option<HashMap<String, Value>>) -> Result<Vec<(String, String)>> {
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
            _ => Some(Err(Error::InvalidInput(format!(
                "cloud functions http header `{name}` must resolve to a string-coercible value"
            )))),
        })
        .collect()
}

fn render_text_body(body: Value) -> Result<Vec<u8>> {
    match body {
        Value::Null => Ok(Vec::new()),
        Value::String(value) => Ok(value.into_bytes()),
        Value::Bool(value) => Ok(value.to_string().into_bytes()),
        Value::Number(value) => Ok(value.to_string().into_bytes()),
        _ => Err(Error::InvalidInput(
            "cloud functions http text responses must resolve to a string-coercible value"
                .to_string(),
        )),
    }
}
