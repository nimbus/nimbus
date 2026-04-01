use super::*;

pub(in crate::adapters::convex::templates) fn placeholder_name(
    object: &Map<String, Value>,
) -> Option<&str> {
    if object.len() == 1 {
        object.get("$arg").and_then(Value::as_str)
    } else {
        None
    }
}

pub(in crate::adapters::convex::templates) fn resolve_nested_value(
    value: &Value,
    path: &str,
) -> Result<Value, Error> {
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

pub(in crate::adapters::convex) fn empty_args() -> Value {
    json!({})
}

pub(in crate::adapters::convex) fn normalize_http_request_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub(in crate::adapters::convex) fn method_name(method: &Method) -> &str {
    method.as_str()
}

pub(in crate::adapters::convex) fn parse_job_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!("invalid document id: {error}")))
    })
}
