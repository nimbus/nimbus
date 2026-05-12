use super::helpers::placeholder_name;
use super::*;

pub(in crate::adapters::convex) fn resolve_template(
    template: &Value,
    args: &Value,
) -> Result<Value, Error> {
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

fn args_object(args: &Value) -> Result<Map<String, Value>, Error> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(object) => Ok(object.clone()),
        _ => Err(Error::InvalidInput(
            "convex function args must be a JSON object".to_string(),
        )),
    }
}
