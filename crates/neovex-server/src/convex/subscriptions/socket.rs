use super::runtime::subscribe_runtime_base_queries;
use super::transforms::{
    activate_transform, apply_subscription_transform, clear_pending_transform,
    remove_subscription_transform, set_pending_transform, subscription_plan_for_named_query,
};
use super::*;

async fn unsubscribe_active_subscriptions(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    active_subscriptions: HashMap<u64, Vec<u64>>,
    outbound_tx: &mpsc::UnboundedSender<ServerMessage>,
    emit_errors: bool,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
) {
    for (convex_subscription_id, underlying_ids) in active_subscriptions {
        remove_subscription_transform(transforms, convex_subscription_id);
        for underlying_subscription_id in underlying_ids {
            let result = service
                .unsubscribe_async(tenant_id.clone(), underlying_subscription_id)
                .await;
            if emit_errors && let Err(error) = result {
                let _ = outbound_tx.send(ServerMessage::Error {
                    request_id: None,
                    message: error.to_string(),
                });
            }
        }
    }
}

fn spawn_subscription_forwarder(
    subscription_rx: mpsc::UnboundedReceiver<SubscriptionUpdate>,
    outbound_tx: mpsc::UnboundedSender<ServerMessage>,
    transforms: Arc<RwLock<ConvexSubscriptionTransforms>>,
    service: Arc<neovex_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    runtime_cancellation: HostCallCancellation,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut subscription_rx = subscription_rx;
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
                        &service,
                        &registry,
                        &tenant_id,
                        &transforms,
                        &runtime_cancellation,
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
            let _ = outbound_tx.send(message);
        }
    })
}

fn spawn_socket_sender(
    mut socket_tx: futures::stream::SplitSink<WebSocket, Message>,
    mut outbound_rx: mpsc::UnboundedReceiver<ServerMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    })
}

pub(super) async fn handle_convex_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: TenantId,
    initial_auth: Option<InvocationAuth>,
) {
    let (socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (subscription_tx, subscription_rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let transforms = Arc::new(RwLock::new(ConvexSubscriptionTransforms::default()));
    let runtime_cancellation = HostCallCancellation::default();
    let convex_registry = state
        .convex_registry
        .clone()
        .expect("convex websocket route requires Convex support state");

    let forward_task = spawn_subscription_forwarder(
        subscription_rx,
        outbound_tx.clone(),
        transforms.clone(),
        state.service.clone(),
        convex_registry.clone(),
        tenant_id.clone(),
        runtime_cancellation.clone(),
    );
    let send_task = spawn_socket_sender(socket_tx, outbound_rx);

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
                    match service
                        .subscribe_async(tenant_id, query, request_id_for_worker, sender)
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
                                Some(super::next_runtime_subscription_server_request_id(
                                    "convex-ws-subscription-bootstrap",
                                )),
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
                    match service
                        .subscribe_async(tenant_id, base_query, request_id_for_worker, sender)
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
                    if let Some(underlying_ids) = active_subscriptions.remove(&subscription_id) {
                        unsubscribe_active_subscriptions(
                            &state.service,
                            &tenant_id,
                            HashMap::from([(subscription_id, underlying_ids)]),
                            &outbound_tx,
                            true,
                            &transforms,
                        )
                        .await;
                    } else {
                        remove_subscription_transform(&transforms, subscription_id);
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

    unsubscribe_active_subscriptions(
        &state.service,
        &tenant_id,
        active_subscriptions,
        &outbound_tx,
        false,
        &transforms,
    )
    .await;
    runtime_cancellation.cancel_due_to_disconnect();
    drop(subscription_tx);
    drop(outbound_tx);
    let _ = forward_task.await;
    let _ = send_task.await;
}
