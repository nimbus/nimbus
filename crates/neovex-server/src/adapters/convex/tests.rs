use std::sync::Arc;

use neovex_engine::Service;
use serde_json::json;
use tempfile::{TempDir, tempdir};

use super::dispatch::execute_convex_action_cancellable;
use super::*;

fn runtime_bridge_fixture() -> (TempDir, Arc<Service>, TenantId, ConvexRuntimeBridge) {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let service = Arc::new(Service::new(tempdir.path()).expect("service should build"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should be created");
    let registry = Arc::new(ConvexRegistry::empty());
    let bridge = ConvexRuntimeBridge::new(service.clone(), registry, tenant_id.clone(), None);
    (tempdir, service, tenant_id, bridge)
}

#[test]
fn execute_convex_action_cancellable_short_circuits_before_mutation_dispatch() {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let service = Service::new(tempdir.path()).expect("service should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should be created");
    let registry = ConvexRegistry::empty();
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = execute_convex_action_cancellable(
        &service,
        &registry,
        &tenant_id,
        ConvexExecutableAction::Action(ConvexAction::Mutation {
            mutation: Mutation::Insert {
                table: TableName::new("messages").expect("table should build"),
                fields: serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
            },
        }),
        &cancellation,
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: TableName::new("messages").expect("table should build"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("document query should succeed");
    assert!(documents.is_empty());
}

#[test]
fn runtime_cancellable_db_get_short_circuits_before_dispatch() {
    let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("messages").expect("table should build"),
            serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
        )
        .expect("document insert should succeed");
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge.dispatch_host_call_cancellable(
        HostCallRequest {
            operation: "convex.ctx.db.get".to_string(),
            payload: json!({
                "table": "messages",
                "id": document_id.to_string(),
            }),
        },
        &cancellation,
    );

    assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
}

#[test]
fn runtime_cancellable_query_builder_start_short_circuits_before_dispatch() {
    let (_tempdir, _service, _tenant_id, bridge) = runtime_bridge_fixture();
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = neovex_runtime::HostBridge::call_cancellable(
        &bridge,
        HostCallRequest {
            operation: "convex.ctx.db.query.start".to_string(),
            payload: json!({
                "table": "messages",
            }),
        },
        &cancellation,
    );

    assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
    let metrics = bridge.registry.runtime_metrics_snapshot();
    assert_eq!(metrics.precanceled_host_ops, 1);
    assert_eq!(
        metrics
            .host_operations
            .get("convex.ctx.db.query.start")
            .expect("query start metrics should be present")
            .canceled_before_start,
        1
    );
}

#[tokio::test]
async fn runtime_async_db_get_precancel_records_canceled_host_op_metric() {
    let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("messages").expect("table should build"),
            serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
        )
        .expect("document insert should succeed");
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge
        .call_async(
            HostCallRequest {
                operation: "convex.ctx.db.get".to_string(),
                payload: json!({
                    "table": "messages",
                    "id": document_id.to_string(),
                }),
            },
            cancellation,
        )
        .await;

    assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
    let metrics = bridge.registry.runtime_metrics_snapshot();
    assert_eq!(metrics.canceled_host_ops, 1);
    assert_eq!(metrics.precanceled_host_ops, 1);
    assert_eq!(metrics.in_flight_canceled_host_ops, 0);
    let db_get_metrics = metrics
        .host_operations
        .get("convex.ctx.db.get")
        .copied()
        .expect("db.get host op metrics should be present");
    assert_eq!(db_get_metrics.started, 0);
    assert_eq!(db_get_metrics.canceled_before_start, 1);
}

#[test]
fn runtime_cancellable_http_route_short_circuits_before_mutation_dispatch() {
    let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge.dispatch_host_call_cancellable(
        HostCallRequest {
            operation: "convex.http_route".to_string(),
            payload: json!({
                "request": {
                    "kind": "action",
                    "function_name": "messages:send",
                    "args": {
                        "method": "POST",
                        "url": "http://localhost/messages",
                        "pathname": "/messages",
                        "query": {},
                        "headers": {},
                        "body_bytes": [],
                        "body_text": ""
                    }
                },
                "route": {
                    "name": "messages:send",
                    "method": "POST",
                    "path": "/messages",
                    "plan": {
                        "operation": {
                            "type": "mutation",
                            "mutation": {
                                "type": "insert",
                                "table": "messages",
                                "fields": {
                                    "body": "hello"
                                }
                            }
                        },
                        "response": {
                            "kind": "json",
                            "body": {
                                "ok": true
                            }
                        }
                    }
                }
            }),
        },
        &cancellation,
    );

    assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: TableName::new("messages").expect("table should build"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("document query should succeed");
    assert!(documents.is_empty());
}

#[test]
fn runtime_cancellable_unknown_operation_is_rejected() {
    let (_tempdir, _service, _tenant_id, bridge) = runtime_bridge_fixture();

    let result = neovex_runtime::HostBridge::call_cancellable(
        &bridge,
        HostCallRequest {
            operation: "convex.ctx.unknown".to_string(),
            payload: json!({}),
        },
        &HostCallCancellation::default(),
    );

    match result {
        Err(NeovexRuntimeError::Contract(message)) => {
            assert!(message.contains("unsupported convex runtime operation"));
        }
        other => panic!("unexpected unsupported-op result: {other:?}"),
    }
    let metrics = bridge.registry.runtime_metrics_snapshot();
    let unknown_metrics = metrics
        .host_operations
        .get("convex.ctx.unknown")
        .expect("unknown op metrics should be present");
    assert_eq!(unknown_metrics.started, 1);
    assert_eq!(unknown_metrics.failed, 1);
}
