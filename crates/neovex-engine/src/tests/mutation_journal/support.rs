use super::*;

pub(super) fn new_faulted_service(
    timestamp: u64,
) -> (TempDir, Arc<Service>, TenantId, Arc<BlockingFaultInjector>) {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(timestamp))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    (data_dir, service, tenant_id, faults)
}
