use serde_json::Value;

use super::super::super::types::ConvexSubscriptionTransform;

pub fn apply_builtin_transform(
    transform: ConvexSubscriptionTransform,
    data: Vec<Value>,
) -> Result<Option<Value>, String> {
    match transform {
        ConvexSubscriptionTransform::Identity => Ok(Some(Value::Array(data))),
        ConvexSubscriptionTransform::Get { document_id } => {
            let expected_id = document_id.to_string();
            Ok(Some(
                data.into_iter()
                    .find(|document| {
                        document
                            .get("_id")
                            .and_then(Value::as_str)
                            .is_some_and(|value| value == expected_id)
                    })
                    .unwrap_or(Value::Null),
            ))
        }
        ConvexSubscriptionTransform::First => {
            Ok(Some(data.into_iter().next().unwrap_or(Value::Null)))
        }
        ConvexSubscriptionTransform::Unique => {
            if data.len() > 1 {
                Err("convex unique subscription matched multiple documents".to_string())
            } else {
                Ok(Some(data.into_iter().next().unwrap_or(Value::Null)))
            }
        }
        ConvexSubscriptionTransform::RuntimeNamedQuery { .. }
        | ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery { .. } => {
            Err("runtime transforms must be resolved before builtin handling".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use nimbus_runtime::InvocationServices;
    use serde_json::json;

    use super::*;

    #[test]
    fn runtime_transforms_return_error_from_builtin_handler() {
        let named_query_error = apply_builtin_transform(
            ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: "messages:list".to_string(),
                args: json!({}),
                auth: None,
                services: InvocationServices::default(),
                read_set: None,
                last_value: None,
            },
            Vec::new(),
        )
        .expect_err("runtime named query should not be handled as a builtin transform");
        assert_eq!(
            named_query_error,
            "runtime transforms must be resolved before builtin handling"
        );

        let paginated_query_error = apply_builtin_transform(
            ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                name: "messages:list".to_string(),
                args: json!({}),
                page_size: 25,
                cursor: None,
                auth: None,
                services: InvocationServices::default(),
                read_set: None,
                last_value: None,
            },
            Vec::new(),
        )
        .expect_err("runtime named paginated query should not be handled as a builtin transform");
        assert_eq!(
            paginated_query_error,
            "runtime transforms must be resolved before builtin handling"
        );
    }
}
