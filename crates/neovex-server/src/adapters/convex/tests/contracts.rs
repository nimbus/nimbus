use serde_json::json;

use super::fixture::host_bridge_fixture;
use super::*;

#[test]
fn runtime_cancellable_unknown_operation_is_rejected() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();

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
