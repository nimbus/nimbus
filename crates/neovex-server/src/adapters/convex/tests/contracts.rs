use serde_json::json;

use super::*;

#[test]
fn runtime_host_request_rejects_unknown_operation_names_during_deserialization() {
    let error = serde_json::from_value::<HostCallRequest>(json!({
        "operation": "convex.ctx.unknown",
        "payload": {},
    }))
    .expect_err("unknown runtime host operations should fail to deserialize");
    assert!(error.to_string().contains("unknown variant"));
}
