use super::*;

#[test]
fn final_inventory_rejects_a_key_inserted_during_retirement() {
    let initial = BTreeMap::new();
    let record = crate::workload_saga::test_support::restart_observed_record(
        "tenant-inserted-child",
        nimbus_workloads::WorkloadRestartPolicy::Never,
    );
    let final_records = BTreeMap::from([(record.key().clone(), record)]);

    assert!(matches!(
        require_all_recorded_before_finish_tenant_delete(&initial, &final_records),
        Err(TenantRetirementError::InvalidInventory(
            "durable workload key set changed during tenant retirement"
        ))
    ));
}

#[test]
fn final_inventory_requires_recorded_stopped_truth() {
    let running = crate::workload_saga::test_support::restart_observed_record(
        "tenant-running-child",
        nimbus_workloads::WorkloadRestartPolicy::Never,
    );
    let initial = BTreeMap::from([(running.key().clone(), running.clone())]);
    let final_records = BTreeMap::from([(running.key().clone(), running)]);

    assert!(matches!(
        require_all_recorded_before_finish_tenant_delete(&initial, &final_records),
        Err(TenantRetirementError::InvalidInventory(
            "tenant deletion requires every durable workload to be Recorded and stopped"
        ))
    ));
}

#[test]
fn final_inventory_rejects_missing_successor_and_cleanup_pending_truth() {
    let first = crate::workload_saga::test_support::teardown_fixture_record(
        "tenant-final-first",
        WorkloadSagaPhase::Recorded,
    );
    let second = crate::workload_saga::test_support::teardown_fixture_record(
        "tenant-final-second",
        WorkloadSagaPhase::Recorded,
    );
    let initial = BTreeMap::from([
        (first.key().clone(), first.clone()),
        (second.key().clone(), second),
    ]);
    let missing = BTreeMap::from([(first.key().clone(), first)]);
    assert!(matches!(
        require_all_recorded_before_finish_tenant_delete(&initial, &missing),
        Err(TenantRetirementError::InvalidInventory(
            "durable workload key set changed during tenant retirement"
        ))
    ));

    let successor = crate::workload_saga::test_support::teardown_fixture_record(
        "tenant-final-successor",
        WorkloadSagaPhase::Recorded,
    );
    assert!(successor.successor_intent().is_some());
    let successor_map = BTreeMap::from([(successor.key().clone(), successor.clone())]);
    assert!(matches!(
        require_all_recorded_before_finish_tenant_delete(&successor_map, &successor_map),
        Err(TenantRetirementError::InvalidInventory(
            "tenant deletion requires every durable workload to be Recorded and stopped"
        ))
    ));

    let cleanup =
        crate::workload_saga::test_support::cleanup_pending_fixture_record("tenant-final-cleanup");
    let cleanup_map = BTreeMap::from([(cleanup.key().clone(), cleanup)]);
    assert!(matches!(
        require_all_recorded_before_finish_tenant_delete(&cleanup_map, &cleanup_map),
        Err(TenantRetirementError::InvalidInventory(
            "tenant deletion requires every durable workload to be Recorded and stopped"
        ))
    ));
}
