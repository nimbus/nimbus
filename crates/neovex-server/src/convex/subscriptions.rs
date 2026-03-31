use super::dispatch::{
    bootstrap_runtime_named_subscription_async,
    invoke_named_convex_function_with_trace_async_cancellable,
};
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

fn subscription_plan_for_named_query(
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

async fn apply_subscription_transform(
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

fn set_pending_transform(
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

fn activate_transform(
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

fn clear_pending_transform(transforms: &RwLock<ConvexSubscriptionTransforms>, request_id: &str) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_request
        .remove(request_id);
}

fn remove_subscription_transform(
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_id: u64,
) {
    transforms
        .write()
        .expect("convex subscription transform lock should not be poisoned")
        .by_id
        .remove(&subscription_id);
}

fn update_runtime_transform_read_set(
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

fn spawn_runtime_subscription_bridge(
    convex_subscription_id: u64,
    mut receiver: mpsc::UnboundedReceiver<SubscriptionUpdate>,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
) {
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                SubscriptionUpdate::Result { commit: None, .. } => {}
                SubscriptionUpdate::Result {
                    commit,
                    deleted_documents,
                    data,
                    ..
                } => {
                    let _ = sender.send(SubscriptionUpdate::Result {
                        subscription_id: convex_subscription_id,
                        request_id: None,
                        commit,
                        deleted_documents,
                        data,
                    });
                }
                SubscriptionUpdate::Error { message, .. } => {
                    let _ = sender.send(SubscriptionUpdate::Error {
                        subscription_id: convex_subscription_id,
                        request_id: None,
                        message,
                    });
                }
            }
        }
    });
}

async fn subscribe_runtime_base_queries(
    service: Arc<neovex_engine::Service>,
    tenant_id: TenantId,
    base_queries: Vec<Query>,
    transform: ConvexSubscriptionTransform,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
) -> Result<ConvexRuntimeSubscriptionHandle, AppError> {
    let mut underlying = Vec::with_capacity(base_queries.len());

    for (index, query) in base_queries.into_iter().enumerate() {
        let (bridge_tx, bridge_rx) = mpsc::unbounded_channel();
        let request_id = format!("convex-runtime-internal-{index}");
        let subscribe_service = service.clone();
        let subscribe_tenant_id = tenant_id.clone();
        match run_blocking(move || {
            subscribe_service.subscribe(&subscribe_tenant_id, query, request_id, bridge_tx)
        })
        .await
        {
            Ok(subscription_id) => underlying.push((subscription_id, bridge_rx)),
            Err(error) => {
                for (subscription_id, _) in underlying {
                    let cleanup_service = service.clone();
                    let cleanup_tenant_id = tenant_id.clone();
                    let _ = run_blocking(move || {
                        cleanup_service.unsubscribe(&cleanup_tenant_id, subscription_id)
                    })
                    .await;
                }
                return Err(error);
            }
        }
    }

    let convex_subscription_id = underlying
        .first()
        .map(|(subscription_id, _)| *subscription_id)
        .expect("runtime base query bootstrap should produce at least one subscription");
    update_runtime_transform_read_set(transforms, convex_subscription_id, transform);

    let mut underlying_subscription_ids = Vec::with_capacity(underlying.len());
    for (subscription_id, receiver) in underlying {
        underlying_subscription_ids.push(subscription_id);
        spawn_runtime_subscription_bridge(convex_subscription_id, receiver, sender.clone());
    }

    Ok(ConvexRuntimeSubscriptionHandle {
        convex_subscription_id,
        underlying_subscription_ids,
    })
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

pub(super) async fn handle_convex_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: TenantId,
    initial_auth: Option<InvocationAuth>,
) {
    let (mut socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (subscription_tx, mut subscription_rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let transforms = Arc::new(RwLock::new(ConvexSubscriptionTransforms::default()));
    let runtime_cancellation = HostCallCancellation::default();
    let convex_registry = state
        .convex_registry
        .clone()
        .expect("convex websocket route requires Convex support state");

    let forward_tx = outbound_tx.clone();
    let transform_state = transforms.clone();
    let forward_service = state.service.clone();
    let forward_registry = convex_registry.clone();
    let forward_tenant_id = tenant_id.clone();
    let forward_runtime_cancellation = runtime_cancellation.clone();
    let forward_task = tokio::spawn(async move {
        while let Some(event) = subscription_rx.recv().await {
            let message = match event {
                SubscriptionUpdate::Result {
                    subscription_id,
                    request_id,
                    commit,
                    deleted_documents,
                    data,
                } => {
                    let request_id_for_transform = request_id.clone();
                    match apply_subscription_transform(
                        &forward_service,
                        &forward_registry,
                        &forward_tenant_id,
                        &transform_state,
                        &forward_runtime_cancellation,
                        ConvexSubscriptionEvent {
                            subscription_id,
                            request_id: request_id_for_transform.as_deref(),
                            commit: commit.as_ref(),
                            deleted_documents: &deleted_documents,
                        },
                        data,
                    )
                    .await
                    {
                        Ok(Some(data)) => ServerMessage::SubscriptionResult {
                            subscription_id,
                            request_id,
                            data,
                        },
                        Ok(None) => continue,
                        Err(message) => ServerMessage::Error {
                            request_id,
                            message,
                        },
                    }
                }
                SubscriptionUpdate::Error {
                    request_id,
                    message,
                    ..
                } => ServerMessage::Error {
                    request_id,
                    message,
                },
            };
            let _ = forward_tx.send(message);
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let mut active_subscriptions: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut current_auth = initial_auth;
    while let Some(message_result) = socket_rx.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Text(text) => match serde_json::from_str::<ConvexClientMessage>(&text) {
                Ok(ConvexClientMessage::Authenticate { token }) => {
                    match convex_registry.verify_socket_token(&token).await {
                        Ok(auth) => {
                            current_auth = Some(auth);
                            crate::state::record_authenticated_usage(&state, current_auth.as_ref())
                                .await;
                            let _ = outbound_tx.send(ServerMessage::Authenticated {
                                is_authenticated: true,
                            });
                        }
                        Err(error) => {
                            let _ = outbound_tx.send(ServerMessage::AuthError {
                                message: error.to_string(),
                            });
                        }
                    }
                }
                Ok(ConvexClientMessage::ClearAuth) => {
                    current_auth = None;
                    let _ = outbound_tx.send(ServerMessage::Authenticated {
                        is_authenticated: false,
                    });
                }
                Ok(ConvexClientMessage::Subscribe { request_id, query }) => {
                    set_pending_transform(
                        &transforms,
                        request_id.clone(),
                        ConvexSubscriptionTransform::Identity,
                    );
                    let request_id_for_worker = request_id.clone();
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    let sender = subscription_tx.clone();
                    match run_blocking(move || {
                        service.subscribe(&tenant_id, query, request_id_for_worker, sender)
                    })
                    .await
                    {
                        Ok(subscription_id) => {
                            active_subscriptions.insert(subscription_id, vec![subscription_id]);
                            activate_transform(
                                &transforms,
                                subscription_id,
                                &request_id,
                                ConvexSubscriptionTransform::Identity,
                            );
                        }
                        Err(error) => {
                            clear_pending_transform(&transforms, &request_id);
                            let _ = outbound_tx.send(ServerMessage::Error {
                                request_id: Some(request_id),
                                message: error.to_string(),
                            });
                        }
                    }
                }
                Ok(ConvexClientMessage::SubscribeNamed {
                    request_id,
                    name,
                    args,
                    page_size,
                    cursor,
                }) => {
                    if convex_registry
                        .runtime_subscription_kind(&name, ConvexFunctionVisibility::Public)
                        .is_some()
                    {
                        let setup = {
                            let service = state.service.clone();
                            let registry = convex_registry.clone();
                            let tenant_id_for_worker = tenant_id.clone();
                            let name_for_worker = name.clone();
                            let args_for_worker = args.clone();
                            let cursor_for_worker = cursor.clone();
                            let runtime_cancellation = runtime_cancellation.clone();
                            match bootstrap_runtime_named_subscription_async(
                                &service,
                                &registry,
                                &tenant_id_for_worker,
                                &name_for_worker,
                                &args_for_worker,
                                page_size,
                                cursor_for_worker,
                                current_auth.clone(),
                                runtime_cancellation,
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(error) => {
                                    let _ = outbound_tx.send(ServerMessage::Error {
                                        request_id: Some(request_id),
                                        message: error.to_string(),
                                    });
                                    continue;
                                }
                            }
                        };

                        let handle = match subscribe_runtime_base_queries(
                            state.service.clone(),
                            tenant_id.clone(),
                            setup.base_queries,
                            setup.transform,
                            &transforms,
                            subscription_tx.clone(),
                        )
                        .await
                        {
                            Ok(handle) => handle,
                            Err(error) => {
                                let _ = outbound_tx.send(ServerMessage::Error {
                                    request_id: Some(request_id),
                                    message: error.to_string(),
                                });
                                continue;
                            }
                        };

                        active_subscriptions.insert(
                            handle.convex_subscription_id,
                            handle.underlying_subscription_ids,
                        );
                        let _ = outbound_tx.send(ServerMessage::SubscriptionResult {
                            subscription_id: handle.convex_subscription_id,
                            request_id: Some(request_id),
                            data: setup.initial_value,
                        });
                        continue;
                    }

                    let (base_query, transform) = {
                        let query = match convex_registry.resolve_subscription_query(&name, &args) {
                            Ok(query) => query,
                            Err(error) => {
                                let _ = outbound_tx.send(ServerMessage::Error {
                                    request_id: Some(request_id),
                                    message: error.to_string(),
                                });
                                continue;
                            }
                        };

                        subscription_plan_for_named_query(
                            &convex_registry,
                            &name,
                            &args,
                            page_size,
                            cursor,
                            query,
                        )
                    };
                    set_pending_transform(&transforms, request_id.clone(), transform.clone());
                    let request_id_for_worker = request_id.clone();
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    let sender = subscription_tx.clone();
                    match run_blocking(move || {
                        service.subscribe(&tenant_id, base_query, request_id_for_worker, sender)
                    })
                    .await
                    {
                        Ok(subscription_id) => {
                            active_subscriptions.insert(subscription_id, vec![subscription_id]);
                            activate_transform(
                                &transforms,
                                subscription_id,
                                &request_id,
                                transform,
                            );
                        }
                        Err(error) => {
                            clear_pending_transform(&transforms, &request_id);
                            let _ = outbound_tx.send(ServerMessage::Error {
                                request_id: Some(request_id),
                                message: error.to_string(),
                            });
                        }
                    }
                }
                Ok(ConvexClientMessage::Unsubscribe { subscription_id }) => {
                    remove_subscription_transform(&transforms, subscription_id);
                    if let Some(underlying_ids) = active_subscriptions.remove(&subscription_id) {
                        for underlying_subscription_id in underlying_ids {
                            let service = state.service.clone();
                            let tenant_id = tenant_id.clone();
                            if let Err(error) = run_blocking(move || {
                                service.unsubscribe(&tenant_id, underlying_subscription_id)
                            })
                            .await
                            {
                                let _ = outbound_tx.send(ServerMessage::Error {
                                    request_id: None,
                                    message: error.to_string(),
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    let _ = outbound_tx.send(ServerMessage::Error {
                        request_id: None,
                        message: format!("invalid websocket message: {error}"),
                    });
                }
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {}
        }
    }

    for (convex_subscription_id, underlying_ids) in active_subscriptions {
        remove_subscription_transform(&transforms, convex_subscription_id);
        for underlying_subscription_id in underlying_ids {
            let service = state.service.clone();
            let tenant_id = tenant_id.clone();
            let _ =
                run_blocking(move || service.unsubscribe(&tenant_id, underlying_subscription_id))
                    .await;
        }
    }
    runtime_cancellation.cancel();
    drop(subscription_tx);
    drop(outbound_tx);
    let _ = forward_task.await;
    let _ = send_task.await;
}
