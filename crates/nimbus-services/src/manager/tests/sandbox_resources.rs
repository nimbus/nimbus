use super::*;

#[tokio::test]
async fn create_sandbox_resource_stops_backend_after_post_start_validation_errors() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let other_tenant_id = TenantId::new("other").expect("tenant id should be valid");
    let backend =
        Arc::new(StubSandboxBackend::new(1).with_handle_tenant_override(other_tenant_id.clone()));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let result = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await;

    assert!(
        matches!(&result, Err(Error::InvalidInput(message)) if message.contains(other_tenant_id.as_str())),
        "mismatched post-start handle should return validation error, got {result:?}"
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "post-start validation failure must stop the returned untracked sandbox"
    );
    assert!(
        manager
            .list_sandbox_resources_for_tenant(&tenant_id)
            .is_empty(),
        "failed post-start validation must not record a sandbox resource"
    );
    assert!(
        backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .is_empty(),
        "cleanup should remove the mismatched started handle from the backend"
    );
}

#[tokio::test]
async fn create_sandbox_resource_preserves_existing_backend_after_duplicate_started_id() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );

    manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("first standalone sandbox should start");
    let duplicate = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await;

    assert!(
        matches!(&duplicate, Err(Error::Conflict(message)) if message.contains("duplicate sandbox id")),
        "duplicate post-start id should return conflict, got {duplicate:?}"
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 2);
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "duplicate-id failure must not stop a tracked sandbox through the create path"
    );
    assert_eq!(
        manager.list_sandbox_resources_for_tenant(&tenant_id).len(),
        1,
        "duplicate-id failure must not insert a second sandbox resource"
    );
    assert!(
        backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .contains_key("sandbox-tenant-task"),
        "duplicate-id failure must leave the tracked backend handle intact"
    );
}

#[tokio::test]
async fn sandbox_create_records_desired_workload() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );

    let resource = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("standalone sandbox should start");
    let workload_id = format!("sandbox:{}", resource.id);
    let snapshot = manager.desired_workload_snapshot();
    let desired = snapshot
        .workloads()
        .find(|workload| workload.workload_id() == workload_id)
        .expect("sandbox create should record desired workload state");

    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    assert_eq!(desired.tenant_id(), &tenant_id);
    assert_eq!(
        desired.kind(),
        nimbus_workloads::DesiredWorkloadKind::Sandbox
    );
    assert_eq!(
        desired.desired_state(),
        nimbus_workloads::DesiredWorkloadState::Running
    );
    assert_eq!(desired.generation(), resource.generation);
    assert_eq!(desired.binding_key(), Some(workload_id.as_str()));
}
