use serde_json::Value;

use super::super::super::types::ConvexSubscriptionTransform;

pub(in crate::adapters::convex::subscriptions) fn apply_builtin_transform(
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
            unreachable!(
                "runtime transforms should not be routed through builtin transform handling"
            )
        }
    }
}
