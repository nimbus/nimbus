use super::helpers::{placeholder_name, resolve_nested_value};
use super::*;

pub(in crate::adapters::convex) fn resolve_http_template(
    template: &Value,
    request: &ConvexHttpRequestContext,
    result: Option<&Value>,
) -> Result<Value, Error> {
    match template {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(template.clone()),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_http_template(item, request, result))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(object) => {
            if let Some(argument_name) = placeholder_name(object) {
                return Err(Error::InvalidInput(format!(
                    "convex httpAction does not support args placeholder: {argument_name}"
                )));
            }
            if let Some(request_descriptor) = object.get("$request") {
                return resolve_http_request_placeholder(request_descriptor, request);
            }
            if let Some(result_descriptor) = object.get("$result") {
                return resolve_http_result_placeholder(result_descriptor, result);
            }

            let mut resolved = Map::new();
            for (key, nested) in object {
                resolved.insert(key.clone(), resolve_http_template(nested, request, result)?);
            }
            Ok(Value::Object(resolved))
        }
    }
}

fn resolve_http_request_placeholder(
    descriptor: &Value,
    request: &ConvexHttpRequestContext,
) -> Result<Value, Error> {
    let Value::Object(object) = descriptor else {
        return Err(Error::InvalidInput(
            "convex httpAction request placeholder must be an object".to_string(),
        ));
    };
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::InvalidInput(
                "convex httpAction request placeholder is missing source".to_string(),
            )
        })?;
    match source {
        "method" => Ok(Value::String(request.method.clone())),
        "url" => Ok(Value::String(request.url.clone())),
        "pathname" => Ok(Value::String(request.pathname.clone())),
        "text" => Ok(Value::String(request.body_text.clone())),
        "header" => {
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "convex httpAction header placeholder is missing name".to_string(),
                    )
                })?
                .to_ascii_lowercase();
            Ok(request
                .headers
                .get(&name)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "query" => {
            let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                Error::InvalidInput(
                    "convex httpAction query placeholder is missing name".to_string(),
                )
            })?;
            Ok(request
                .query
                .get(name)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "json" => {
            let json_body = parse_http_json_body(request)?;
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            resolve_nested_value(&json_body, path)
        }
        _ => Err(Error::InvalidInput(format!(
            "unsupported convex httpAction request source: {source}"
        ))),
    }
}

fn resolve_http_result_placeholder(
    descriptor: &Value,
    result: Option<&Value>,
) -> Result<Value, Error> {
    let Some(result) = result else {
        return Err(Error::InvalidInput(
            "convex httpAction response referenced an operation result, but no operation ran"
                .to_string(),
        ));
    };
    let Value::Object(object) = descriptor else {
        return Err(Error::InvalidInput(
            "convex httpAction result placeholder must be an object".to_string(),
        ));
    };
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    resolve_nested_value(result, path)
}

fn parse_http_json_body(request: &ConvexHttpRequestContext) -> Result<Value, Error> {
    if request.body_bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&request.body_bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "convex httpAction request body must be valid JSON: {error}"
        ))
    })
}
