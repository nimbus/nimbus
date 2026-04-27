use neovex_runtime::HOST_CALL_ABI_VERSION;
use serde_json::json;

use super::fixture::host_bridge_fixture;
use super::*;

#[test]
fn convex_host_request_roundtrips_between_adapter_wire_names_and_runtime_types() {
    let request = serde_json::from_value::<ConvexHostCallRequest>(json!({
        "abi_version": HOST_CALL_ABI_VERSION,
        "operation": "convex.ctx.db.get",
        "payload": {
            "id": "doc-1",
        },
    }))
    .expect("convex host request should deserialize");
    assert_eq!(
        HostCallRequest::from(request.clone()),
        HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!({
                "id": "doc-1",
            }),
        )
    );
    assert_eq!(
        serde_json::to_value(request).expect("convex host request should serialize"),
        json!({
            "abi_version": HOST_CALL_ABI_VERSION,
            "operation": "convex.ctx.db.get",
            "payload": {
                "id": "doc-1",
            },
        })
    );
}

#[test]
fn convex_host_request_rejects_unknown_operation_names_during_deserialization() {
    let error = serde_json::from_value::<ConvexHostCallRequest>(json!({
        "abi_version": HOST_CALL_ABI_VERSION,
        "operation": "convex.ctx.unknown",
        "payload": {},
    }))
    .expect_err("unknown runtime host operations should fail to deserialize");
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn convex_host_request_defaults_current_abi_version() {
    let request = serde_json::from_value::<ConvexHostCallRequest>(json!({
        "operation": "convex.ctx.db.get",
        "payload": {
            "id": "doc-1",
        },
    }))
    .expect("convex host request should deserialize with default abi version");

    assert_eq!(
        HostCallRequest::from(request).abi_version,
        HOST_CALL_ABI_VERSION
    );
}

#[test]
fn convex_host_operations_classify_into_concept_owned_families() {
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxQuery).family(),
        ConvexHostCallFamily::Function
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::QueryBuilderStart).family(),
        ConvexHostCallFamily::QueryBuilder
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::QueryReadTake).family(),
        ConvexHostCallFamily::QueryRead
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::DocumentInsert).family(),
        ConvexHostCallFamily::Document
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxSchedulerRunAfter).family(),
        ConvexHostCallFamily::Scheduler
    );
}

#[test]
fn dispatch_host_call_rejects_unsupported_runtime_host_abi_version() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();

    let error = bridge
        .dispatch_host_call(HostCallRequest {
            abi_version: HOST_CALL_ABI_VERSION + 1,
            operation: HostCallOperation::DocumentGet,
            payload: json!({
                "table": "messages",
                "id": "doc-1",
            }),
        })
        .expect_err("unsupported ABI version should be rejected before dispatch");

    assert!(
        error
            .to_string()
            .contains("unsupported host call ABI version")
    );
}

#[test]
fn dispatch_host_call_rejects_operation_payload_mismatches_before_handler_dispatch() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();

    let error = bridge
        .dispatch_host_call(HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!({
                "mutation": {
                    "table": "messages",
                }
            }),
        ))
        .expect_err("mismatched payload should be rejected before handler dispatch");

    assert!(error.to_string().contains("missing field"));
}
