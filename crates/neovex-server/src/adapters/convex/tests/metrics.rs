use serde_json::json;

use super::fixture::host_bridge_fixture;
use super::*;

#[test]
fn runtime_cancellable_query_builder_start_short_circuits_before_dispatch() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();
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
    let (_tempdir, service, tenant_id, bridge) = host_bridge_fixture();
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
fn runtime_metrics_snapshot_surfaces_rejected_invocation_counts() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();

    bridge
        .registry
        .runtime_policy()
        .metrics()
        .record_rejected_invocation_for_tenant(Some("demo"));

    let metrics = bridge.registry.runtime_metrics_snapshot();
    assert_eq!(metrics.rejected_invocations, 1);
    assert_eq!(
        metrics
            .tenants
            .get("demo")
            .expect("tenant runtime metrics should be present")
            .rejected_invocations,
        1
    );
}
