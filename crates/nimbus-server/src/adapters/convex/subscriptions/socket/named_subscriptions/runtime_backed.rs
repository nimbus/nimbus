use crate::execution::subscriptions::RuntimeSubscriptionHandle;

use super::*;
use crate::application_auth::normalize_principal_context;

struct RuntimeSubscriptionPublishContext<'a> {
    outbound_tx: &'a mpsc::Sender<ServerMessage>,
    subscription_tx: &'a mpsc::Sender<SubscriptionUpdate>,
    transforms: &'a RwLock<ConvexSubscriptionTransforms>,
    active_subscriptions: &'a mut ActiveSubscriptions,
}

pub(super) async fn handle_runtime_named_subscription(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
    request: NamedSubscriptionRequest,
) {
    let NamedSubscriptionRequest {
        request_id,
        name,
        args,
        page_size,
        cursor,
    } = request;

    let setup = {
        let service = ctx.state.service.clone();
        let registry = ctx.convex_registry.clone();
        let tenant_id_for_worker = ctx.tenant_id.clone();
        let name_for_worker = name.clone();
        let args_for_worker = args.clone();
        let cursor_for_worker = cursor.clone();
        let runtime_cancellation = ctx.runtime_cancellation.clone();
        match bootstrap_runtime_named_subscription_async(
            &service,
            &registry,
            &ctx.state.runtime_service_registry(),
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
                super::send_request_error(ctx.outbound_tx, request_id, error.to_string()).await;
                return;
            }
        }
    };

    let handle = match subscribe_runtime_base_queries(
        ctx.state.service.clone(),
        ctx.tenant_id.clone(),
        setup.base_queries,
        normalize_principal_context(current_auth.as_ref()),
    )
    .await
    {
        Ok(handle) => handle,
        Err(error) => {
            super::send_request_error(ctx.outbound_tx, request_id, error.to_string()).await;
            return;
        }
    };
    let _ = publish_runtime_subscription_setup(
        RuntimeSubscriptionPublishContext {
            outbound_tx: ctx.outbound_tx,
            subscription_tx: ctx.subscription_tx,
            transforms: ctx.transforms,
            active_subscriptions,
        },
        request_id,
        setup.initial_value,
        setup.transform,
        handle,
    )
    .await;
}

async fn publish_runtime_subscription_setup(
    context: RuntimeSubscriptionPublishContext<'_>,
    request_id: String,
    initial_value: Value,
    transform: ConvexSubscriptionTransform,
    mut handle: RuntimeSubscriptionHandle,
) -> bool {
    let primary_subscription_id = handle.primary_subscription_id;
    update_runtime_transform_read_set(context.transforms, primary_subscription_id, transform);
    if context
        .outbound_tx
        .send(ServerMessage::SubscriptionResult {
            subscription_id: primary_subscription_id,
            request_id: Some(request_id),
            data: initial_value,
        })
        .await
        .is_err()
    {
        remove_subscription_transform(context.transforms, primary_subscription_id);
        return false;
    }
    handle.start_forwarding(context.subscription_tx.clone());
    context.active_subscriptions.insert(
        primary_subscription_id,
        ActiveSubscription::from_runtime_handle(handle),
    );
    true
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use nimbus_core::{CommitEntry, SequenceNumber, SubscriptionResultSnapshot, Timestamp};
    use serde_json::json;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn runtime_publish_waits_for_initial_payload_before_forwarding_buffered_catch_up() {
        let (outbound_tx, mut outbound_rx) = mpsc::channel(1);
        let (subscription_tx, mut subscription_rx) =
            mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let transforms = Arc::new(RwLock::new(ConvexSubscriptionTransforms::default()));
        let mut active_subscriptions = ActiveSubscriptions::new();
        let blocked_message =
            ServerMessage::session_error("session.test_blocked_send", "block-initial-send");
        outbound_tx
            .send(blocked_message)
            .await
            .expect("prefill should occupy the outbound channel");

        let (pending_tx, pending_rx) =
            mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        pending_tx
            .send(SubscriptionUpdate::Result {
                subscription_id: 7,
                request_id: None,
                snapshot: SubscriptionResultSnapshot::from_delivery(
                    SequenceNumber(11),
                    Some(&CommitEntry {
                        sequence: SequenceNumber(11),
                        timestamp: Timestamp(110),
                        writes: Vec::new(),
                    }),
                    vec![nimbus_core::Document::new(
                        nimbus_core::TableName::new("tasks").expect("table should be valid"),
                        serde_json::Map::from_iter([("body".to_string(), json!("buffered"))]),
                    )],
                    Vec::new(),
                ),
                commit_hint: Some(CommitEntry {
                    sequence: SequenceNumber(11),
                    timestamp: Timestamp(110),
                    writes: Vec::new(),
                }),
            })
            .await
            .expect("buffered catch-up update should send");

        let publish = tokio::spawn({
            let transforms = transforms.clone();
            let outbound_tx = outbound_tx.clone();
            let subscription_tx = subscription_tx.clone();
            async move {
                let published = publish_runtime_subscription_setup(
                    RuntimeSubscriptionPublishContext {
                        outbound_tx: &outbound_tx,
                        subscription_tx: &subscription_tx,
                        transforms: &transforms,
                        active_subscriptions: &mut active_subscriptions,
                    },
                    "request-1".to_string(),
                    json!({"runtime": true, "value": []}),
                    ConvexSubscriptionTransform::RuntimeNamedQuery {
                        name: "messages:maybeByAuthor".to_string(),
                        args: json!({"author": "Ada"}),
                        auth: None,
                        services: Default::default(),
                        read_set: None,
                        last_value: Some(Arc::new(json!({"runtime": true, "value": []}))),
                    },
                    RuntimeSubscriptionHandle::new_for_testing(42, pending_rx),
                )
                .await;
                (published, active_subscriptions)
            }
        });

        assert!(
            timeout(Duration::from_millis(50), subscription_rx.recv())
                .await
                .is_err(),
            "buffered catch-up updates must stay parked while the initial payload send is blocked",
        );

        let unblocked = outbound_rx
            .recv()
            .await
            .expect("prefilled outbound message should be readable");
        let ServerMessage::Error { request_id, error } = unblocked else {
            panic!("expected the prefilled outbound error message");
        };
        assert_eq!(request_id, None);
        assert_eq!(error.message(), "block-initial-send");

        let (published, mut active_subscriptions) = publish
            .await
            .expect("runtime publish task should complete once the outbound channel drains");
        assert!(published, "runtime publish helper should report success");

        let initial = outbound_rx
            .recv()
            .await
            .expect("request-scoped initial runtime payload should be queued");
        let ServerMessage::SubscriptionResult {
            subscription_id,
            request_id,
            data,
        } = initial
        else {
            panic!("expected the request-scoped runtime bootstrap payload");
        };
        assert_eq!(subscription_id, 42);
        assert_eq!(request_id.as_deref(), Some("request-1"));
        assert_eq!(data, json!({"runtime": true, "value": []}));

        let buffered_catch_up = subscription_rx
            .recv()
            .await
            .expect("buffered catch-up should forward after initial payload publish");
        let SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            snapshot,
            ..
        } = buffered_catch_up
        else {
            panic!("expected a forwarded buffered result");
        };
        let data = snapshot.to_json_documents();
        assert_eq!(subscription_id, 42);
        assert_eq!(request_id, None);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["body"], json!("buffered"));

        let active_subscription = active_subscriptions
            .remove(&42)
            .expect("runtime publish should retain an active subscription until cleanup");
        active_subscription.shutdown_and_drain().await;
    }
}
