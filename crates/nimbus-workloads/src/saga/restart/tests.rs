use super::*;

#[test]
fn restart_policy_wire_is_closed() {
    let encoded = serde_json::to_value(WorkloadRestartPolicy::OnFailure { max_restarts: 3 })
        .expect("restart policy should encode");
    assert_eq!(
        serde_json::from_value::<WorkloadRestartPolicy>(encoded)
            .expect("restart policy should decode"),
        WorkloadRestartPolicy::OnFailure { max_restarts: 3 }
    );
    assert!(
        serde_json::from_value::<WorkloadRestartPolicy>(serde_json::json!({
            "policy": "on_failure",
            "maxRestarts": 3,
            "unknown": true
        }))
        .is_err()
    );
}
