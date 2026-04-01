use super::types::{ConvexSubscriptionTransform, ConvexSubscriptionTransforms};
use super::*;

fn subscription_plan_for_query(
    query: ConvexExecutableQuery,
) -> (Query, ConvexSubscriptionTransform) {
    match query {
        ConvexExecutableQuery::Query(query) => (query, ConvexSubscriptionTransform::Identity),
        ConvexExecutableQuery::Read(ConvexReadCommand::Get { table, id }) => (
            Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            ConvexSubscriptionTransform::Get { document_id: id },
        ),
        ConvexExecutableQuery::Read(ConvexReadCommand::First { query }) => {
            (query, ConvexSubscriptionTransform::First)
        }
        ConvexExecutableQuery::Read(ConvexReadCommand::Unique { query }) => {
            (query, ConvexSubscriptionTransform::Unique)
        }
    }
}

pub(super) fn subscription_plan_for_named_query(
    registry: &ConvexRegistry,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    query: ConvexExecutableQuery,
) -> (Query, ConvexSubscriptionTransform) {
    let (base_query, transform) = subscription_plan_for_query(query);
    let Some(definition) = registry.functions.get(name) else {
        return (base_query, transform);
    };
    if registry.runtime_bundle().is_none() {
        return (base_query, transform);
    }

    match definition.kind {
        ConvexFunctionKind::Query => (
            base_query,
            ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: name.to_string(),
                args: args.clone(),
                auth: None,
                read_set: None,
            },
        ),
        ConvexFunctionKind::PaginatedQuery => {
            if let Some(page_size) = page_size {
                (
                    base_query,
                    ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                        name: name.to_string(),
                        args: args.clone(),
                        page_size,
                        cursor,
                        auth: None,
                        read_set: None,
                    },
                )
            } else {
                (base_query, transform)
            }
        }
        _ => (base_query, transform),
    }
}

pub(super) async fn apply_subscription_transform(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    runtime_cancellation: &HostCallCancellation,
    event: ConvexSubscriptionEvent<'_>,
    data: Vec<Value>,
) -> Result<Option<Value>, String> {
    let transform = {
        let mut transforms = transforms
            .write()
            .expect("convex subscription transform lock should not be poisoned");
        if let Some(transform) = transforms.by_id.get(&event.subscription_id).cloned() {
            transform
        } else if let Some(request_id) = event.request_id {
            if let Some(transform) = transforms.by_request.remove(request_id) {
                transforms
                    .by_id
                    .insert(event.subscription_id, transform.clone());
                transform
            } else {
                ConvexSubscriptionTransform::Identity
            }
        } else {
            ConvexSubscriptionTransform::Identity
        }
    };

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
        ConvexSubscriptionTransform::RuntimeNamedQuery {
            name,
            args,
            auth,
            read_set,
        } => {
            if runtime_cancellation.is_cancelled() {
                return Ok(None);
            }
            if let Some(commit) = event.commit
                && let Some(read_set) = read_set.as_ref()
                && !commit_intersects_runtime_read_set(
                    service,
                    tenant_id,
                    commit,
                    read_set,
                    event.deleted_documents,
                )
            {
                return Ok(None);
            }

            let result = match invoke_named_convex_function_with_trace_async_cancellable(
                service,
                registry,
                tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: name.clone(),
                    args: args.clone(),
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                runtime_cancellation.clone(),
                Some(super::next_runtime_subscription_server_request_id(
                    "convex-ws-subscription-reeval",
                )),
            )
            .await
            {
                Ok(result) => result,
                Err(_error) if runtime_cancellation.is_cancelled() => return Ok(None),
                Err(error) => return Err(error.to_string()),
            };
            let (value, new_read_set) = result;
            update_runtime_transform_read_set(
                transforms,
                event.subscription_id,
                ConvexSubscriptionTransform::RuntimeNamedQuery {
                    name,
                    args,
                    auth,
                    read_set: Some(new_read_set),
                },
            );
            Ok(Some(value))
        }
        ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
            name,
            args,
            page_size,
            cursor,
            auth,
            read_set,
        } => {
            if runtime_cancellation.is_cancelled() {
                return Ok(None);
            }
            if let Some(commit) = event.commit
                && let Some(read_set) = read_set.as_ref()
                && !commit_intersects_runtime_read_set(
                    service,
                    tenant_id,
                    commit,
                    read_set,
                    event.deleted_documents,
                )
            {
                return Ok(None);
            }

            let result = match invoke_named_convex_function_with_trace_async_cancellable(
                service,
                registry,
                tenant_id,
                InvocationRequest {
                    kind: InvocationKind::PaginatedQuery,
                    function_name: name.clone(),
                    args: args.clone(),
                    page_size: Some(page_size),
                    cursor: cursor.clone(),
                    auth: auth.clone(),
                },
                runtime_cancellation.clone(),
                Some(super::next_runtime_subscription_server_request_id(
                    "convex-ws-subscription-reeval",
                )),
            )
            .await
            .and_then(|(value, read_set)| {
                let page: neovex_core::Page = serde_json::from_value(value)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                Ok((Value::Array(page.data), read_set))
            }) {
                Ok(result) => result,
                Err(_error) if runtime_cancellation.is_cancelled() => return Ok(None),
                Err(error) => return Err(error.to_string()),
            };
            let (value, new_read_set) = result;
            update_runtime_transform_read_set(
                transforms,
                event.subscription_id,
                ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                    name,
                    args,
                    page_size,
                    cursor,
                    auth,
                    read_set: Some(new_read_set),
                },
            );
            Ok(Some(value))
        }
    }
}

pub(super) fn set_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: String,
    transform: ConvexSubscriptionTransform,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_request
        .insert(request_id, transform);
}

pub(super) fn activate_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    request_id: &str,
    transform: ConvexSubscriptionTransform,
) {
    let mut transforms = transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned");
    transforms.by_request.remove(request_id);
    transforms.by_id.insert(subscription_id, transform);
}

pub(super) fn clear_pending_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    request_id: &str,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_request
        .remove(request_id);
}

pub(super) fn remove_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_id
        .remove(&subscription_id);
}

pub(super) fn update_runtime_transform_read_set(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
    transform: ConvexSubscriptionTransform,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_id
        .insert(subscription_id, transform);
}

fn compare_index_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

pub(super) fn is_scalar_filter_value(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

pub(super) fn should_replace_lower_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };
    match compare_index_values(candidate, current) {
        Some(std::cmp::Ordering::Greater) => true,
        Some(std::cmp::Ordering::Equal) => candidate_inclusive,
        Some(std::cmp::Ordering::Less) => false,
        None => true,
    }
}

pub(super) fn should_replace_upper_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };
    match compare_index_values(candidate, current) {
        Some(std::cmp::Ordering::Less) => true,
        Some(std::cmp::Ordering::Equal) => candidate_inclusive,
        Some(std::cmp::Ordering::Greater) => false,
        None => true,
    }
}
