use super::*;

pub(super) fn build_http_response_parts(parts: ConvexHttpResponseParts) -> Result<Response, Error> {
    build_http_response(parts.kind, parts.body, parts.status, parts.headers)
}

fn build_http_response(
    kind: ConvexHttpResponseKind,
    body: Value,
    status: Option<Value>,
    headers: Option<Value>,
) -> Result<Response, Error> {
    let status = parse_http_status(status)?;
    let mut builder = Response::builder().status(status);
    let header_map = parse_http_headers(headers)?;
    let has_content_type = header_map
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));

    for (name, value) in header_map {
        builder = builder.header(name, value);
    }

    if kind == ConvexHttpResponseKind::Json && !has_content_type {
        builder = builder.header("content-type", "application/json");
    }

    let body = match kind {
        ConvexHttpResponseKind::Json => {
            serde_json::to_vec(&body).map_err(|error| Error::Serialization(error.to_string()))?
        }
        ConvexHttpResponseKind::Text => render_http_text_body(body)?.into_bytes(),
    };

    builder
        .body(axum::body::Body::from(body))
        .map_err(|error| Error::Internal(error.to_string()))
}

fn parse_http_status(status: Option<Value>) -> Result<StatusCode, Error> {
    let Some(status) = status else {
        return Ok(StatusCode::OK);
    };
    let code = status.as_u64().ok_or_else(|| {
        Error::InvalidInput("convex http response status must be a number".to_string())
    })?;
    StatusCode::from_u16(code as u16).map_err(|error| {
        Error::InvalidInput(format!("invalid convex http response status: {error}"))
    })
}

fn parse_http_headers(headers: Option<Value>) -> Result<Vec<(String, String)>, Error> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    let Value::Object(object) = headers else {
        return Err(Error::InvalidInput(
            "convex http response headers must resolve to a JSON object".to_string(),
        ));
    };
    object
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::Null => None,
            Value::String(value) => Some(Ok((name, value))),
            Value::Number(value) => Some(Ok((name, value.to_string()))),
            Value::Bool(value) => Some(Ok((name, value.to_string()))),
            _ => Some(Err(Error::InvalidInput(format!(
                "convex http response header {name} must resolve to a string-coercible value"
            )))),
        })
        .collect()
}

fn render_http_text_body(body: Value) -> Result<String, Error> {
    match body {
        Value::Null => Ok(String::new()),
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidInput(
            "convex http text responses must resolve to a string-coercible value".to_string(),
        )),
    }
}
