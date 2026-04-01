use axum::http::Method;
use neovex_core::{DocumentId, Error};
use serde_json::{Map, Value, json};

use super::dispatch::ConvexHttpRequestContext;
use crate::state::AppError;

pub(super) fn resolve_template(template: &Value, args: &Value) -> Result<Value, Error> {
    let args = args_object(args)?;
    match template {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(template.clone()),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_template(item, &Value::Object(args.clone())))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(object) => {
            if let Some(argument_name) = placeholder_name(object) {
                return args.get(argument_name).cloned().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex function argument missing: {argument_name}"
                    ))
                });
            }

            let mut resolved = Map::new();
            for (key, nested) in object {
                resolved.insert(
                    key.clone(),
                    resolve_template(nested, &Value::Object(args.clone()))?,
                );
            }
            Ok(Value::Object(resolved))
        }
    }
}

pub(super) fn resolve_http_template(
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

fn resolve_nested_value(value: &Value, path: &str) -> Result<Value, Error> {
    if path.is_empty() {
        return Ok(value.clone());
    }

    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(object) => {
                current = object.get(segment).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder path not found: {path}"
                    ))
                })?;
            }
            Value::Array(items) => {
                let index = segment.parse::<usize>().map_err(|_| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder path segment is not a valid index: {segment}"
                    ))
                })?;
                current = items.get(index).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder index out of bounds: {path}"
                    ))
                })?;
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "convex httpAction placeholder path not found: {path}"
                )));
            }
        }
    }
    Ok(current.clone())
}

fn placeholder_name(object: &Map<String, Value>) -> Option<&str> {
    if object.len() == 1 {
        object.get("$arg").and_then(Value::as_str)
    } else {
        None
    }
}

fn args_object(args: &Value) -> Result<Map<String, Value>, Error> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(object) => Ok(object.clone()),
        _ => Err(Error::InvalidInput(
            "convex function args must be a JSON object".to_string(),
        )),
    }
}

pub(super) fn empty_args() -> Value {
    json!({})
}

pub(super) fn normalize_http_request_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub(super) fn method_name(method: &Method) -> &str {
    method.as_str()
}

pub(super) fn parse_job_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!("invalid document id: {error}")))
    })
}
