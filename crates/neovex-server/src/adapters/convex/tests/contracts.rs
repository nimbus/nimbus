use serde_json::json;

use super::*;

#[test]
fn convex_host_request_roundtrips_between_adapter_wire_names_and_runtime_types() {
    let request = serde_json::from_value::<ConvexHostCallRequest>(json!({
        "operation": "convex.ctx.db.get",
        "payload": {
            "id": "doc-1",
        },
    }))
    .expect("convex host request should deserialize");
    assert_eq!(
        HostCallRequest::from(request.clone()),
        HostCallRequest {
            operation: HostCallOperation::CtxDbGet,
            payload: json!({
                "id": "doc-1",
            }),
        }
    );
    assert_eq!(
        serde_json::to_value(request).expect("convex host request should serialize"),
        json!({
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
        "operation": "convex.ctx.unknown",
        "payload": {},
    }))
    .expect_err("unknown runtime host operations should fail to deserialize");
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn convex_host_operations_classify_into_concept_owned_families() {
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxQuery).family(),
        ConvexHostCallFamily::Function
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxDbQueryStart).family(),
        ConvexHostCallFamily::QueryBuilder
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxDbQueryTake).family(),
        ConvexHostCallFamily::QueryRead
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxDbInsert).family(),
        ConvexHostCallFamily::Document
    );
    assert_eq!(
        ConvexHostCallOperation::from(HostCallOperation::CtxSchedulerRunAfter).family(),
        ConvexHostCallFamily::Scheduler
    );
}
