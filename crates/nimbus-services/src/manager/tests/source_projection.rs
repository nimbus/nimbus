use super::*;
use crate::ServiceInstanceCatalog;

fn manager_with_backend() -> (ServiceManager, Arc<StubSandboxBackend>) {
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    (manager, backend)
}

#[test]
fn desired_source_exists_before_first_provider_callback() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();

    let source = reserve_standalone_source(
        &manager,
        &tenant_id,
        "stable-resource",
        "worker",
        standalone_resource_spec(&tenant_id, "task"),
        BTreeMap::from([("team".to_owned(), "runtime".to_owned())]),
    );

    assert_eq!(source.generation, 1);
    assert_eq!(source.id, "stable-resource");
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
            .expect("source lookup should succeed")
            .expect("source should exist")
            .observation,
        None
    );
}

#[test]
fn exact_reservation_is_idempotent_and_resource_identity_is_tenant_qualified() {
    let tenant_a = TenantId::new("tenant-a").expect("tenant id should be valid");
    let tenant_b = TenantId::new("tenant-b").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let labels = BTreeMap::from([("team".to_owned(), "runtime".to_owned())]);
    let first = reserve_standalone_source(
        &manager,
        &tenant_a,
        "shared-logical-id",
        "worker",
        standalone_resource_spec(&tenant_a, "task-a"),
        labels.clone(),
    );

    {
        let mut state = manager
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let stored = state
            .sandbox_resource_sources
            .values_mut()
            .find(|source| source.tenant_id == tenant_a && source.id == first.id)
            .expect("first source should be retained");
        stored.created_at_millis = 7;
        stored.updated_at_millis = 7;
    }
    let exact_retry = reserve_standalone_source(
        &manager,
        &tenant_a,
        "shared-logical-id",
        "worker",
        standalone_resource_spec(&tenant_a, "task-a"),
        labels,
    );
    assert_eq!(exact_retry.created_at_millis, 7);
    assert_eq!(exact_retry.updated_at_millis, 7);

    let tenant_b_source = reserve_standalone_source(
        &manager,
        &tenant_b,
        "shared-logical-id",
        "worker",
        standalone_resource_spec(&tenant_b, "task-b"),
        BTreeMap::new(),
    );
    assert_eq!(tenant_b_source.id, first.id);
    assert_ne!(tenant_b_source.tenant_id, first.tenant_id);
    assert_eq!(
        manager
            .sandbox_resource_source_for_tenant(&tenant_a, &first.id)
            .expect("tenant-a lookup should succeed")
            .expect("tenant-a source should exist")
            .tenant_id,
        tenant_a
    );
    assert_eq!(
        manager
            .sandbox_resource_source_for_tenant(&tenant_b, &tenant_b_source.id)
            .expect("tenant-b lookup should succeed")
            .expect("tenant-b source should exist")
            .tenant_id,
        tenant_b
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
}

#[test]
fn sandbox_projection_updates_status_and_rejects_stale_or_crossed_evidence_unchanged() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let source = reserve_standalone_source(
        &manager,
        &tenant_id,
        "stable-resource",
        "worker",
        standalone_resource_spec(&tenant_id, "task"),
        BTreeMap::new(),
    );
    let starting = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Starting);
    manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            starting.id.as_str(),
            starting.clone(),
        )
        .expect("exact execution observation should establish the first projection");

    let mut ready = starting.clone();
    ready.status = SandboxStatus::Ready;
    let projected = manager
        .project_sandbox_resource_observation(&tenant_id, &source.id, source.generation, ready)
        .expect("same provider identity may advance observed status");
    assert_eq!(projected.handle.status, SandboxStatus::Ready);
    let after_update = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("snapshot lookup should succeed")
        .expect("snapshot should exist");
    assert_eq!(after_update.source, source);

    let stale = manager.project_sandbox_resource_observation(
        &tenant_id,
        &source.id,
        source.generation + 1,
        starting.clone(),
    );
    assert!(matches!(stale, Err(Error::PreconditionFailed(_))));
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
            .expect("snapshot lookup should succeed"),
        Some(after_update.clone()),
        "stale observation must leave desired and observed bytes unchanged"
    );

    let mut crossed = starting;
    crossed.id = SandboxId::new("crossed-provider-id");
    let crossed_result = manager.project_sandbox_resource_observation(
        &tenant_id,
        &source.id,
        source.generation,
        crossed,
    );
    assert!(matches!(crossed_result, Err(Error::Conflict { .. })));
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
            .expect("snapshot lookup should succeed"),
        Some(after_update),
        "crossed observation must leave desired and observed bytes unchanged"
    );
}

#[test]
fn service_projection_is_generation_fenced_and_status_mutable() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "api",
            image_service_backend("api", "registry.example.com/api:1"),
            BTreeMap::new(),
        )
        .expect("definition should create");
    let starting = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Starting);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            starting.id.as_str(),
            starting.clone(),
        )
        .expect("exact execution observation should establish the first service projection");
    let mut ready = starting.clone();
    ready.status = SandboxStatus::Ready;
    let ready = manager
        .project_service_definition_observation(&tenant_id, "api", definition.generation, ready)
        .expect("same provider identity may advance service status");
    assert_eq!(ready.handle.status, SandboxStatus::Ready);
    let before_rejections = manager
        .service_definition_observation_for_tenant(&tenant_id, "api")
        .expect("observation should exist");

    let stale = manager.project_service_definition_observation(
        &tenant_id,
        "api",
        definition.generation + 1,
        starting.clone(),
    );
    assert!(matches!(stale, Err(Error::PreconditionFailed(_))));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(before_rejections.clone())
    );

    let mut crossed = starting;
    crossed.id = SandboxId::new("crossed-provider-id");
    let crossed_result = manager.project_service_definition_observation(
        &tenant_id,
        "api",
        definition.generation,
        crossed,
    );
    assert!(matches!(crossed_result, Err(Error::Conflict { .. })));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(before_rejections)
    );
}

#[test]
fn exact_service_projection_is_immediately_visible_to_every_read_model_without_provider_io() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "api",
            image_service_backend("api", "registry.example.com/api:1"),
            BTreeMap::new(),
        )
        .expect("definition should create");
    let handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            handle.id.as_str(),
            handle.clone(),
        )
        .expect("exact service observation should project");

    assert_eq!(
        ServiceInstanceCatalog::service_instance_for_name(&manager, &tenant_id, "api"),
        Some(handle)
    );
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("runtime binding lookup should succeed")
            .is_some(),
        "the canonical observation must be visible to runtime lookup"
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn source_only_sandbox_reads_are_truthful_repeatable_and_effect_free() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let source = reserve_standalone_source(
        &manager,
        &tenant_id,
        "stable-resource",
        "worker",
        standalone_resource_spec(&tenant_id, "task"),
        BTreeMap::new(),
    );

    let first = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("first read should succeed")
        .expect("desired source should be visible");
    let listed = manager.list_sandbox_resource_snapshots_for_tenant(&tenant_id);
    let second = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("second read should succeed")
        .expect("desired source should remain visible");

    assert_eq!(first, second);
    assert_eq!(listed, vec![first]);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn compute_projection_authenticates_source_version_and_execution_id_before_first_write() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let (manager, backend) = manager_with_backend();
    let sandbox_source = reserve_standalone_source(
        &manager,
        &tenant_id,
        "stable-resource",
        "worker",
        standalone_resource_spec(&tenant_id, "task"),
        BTreeMap::new(),
    );
    let mut sandbox_handle = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Ready);
    sandbox_handle.id = SandboxId::new("execution-sandbox-17");

    assert!(matches!(
        manager.project_sandbox_resource_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            sandbox_handle.clone(),
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, &sandbox_source.id)
            .expect("sandbox snapshot lookup should succeed")
            .expect("sandbox source should remain")
            .observation,
        None,
        "unfenced transitional projection cannot become the first writer"
    );

    for rejected in [
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            "crossed-resource-version",
            sandbox_handle.id.as_str(),
            sandbox_handle.clone(),
        ),
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            &sandbox_source.resource_version,
            "crossed-execution-id",
            sandbox_handle.clone(),
        ),
    ] {
        assert!(rejected.is_err());
        assert_eq!(
            manager
                .sandbox_resource_snapshot_for_tenant(&tenant_id, &sandbox_source.id)
                .expect("sandbox snapshot lookup should succeed")
                .expect("sandbox source should remain")
                .observation,
            None,
            "crossed first-write evidence must leave projection absent"
        );
    }
    let sandbox_projection = manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            &sandbox_source.resource_version,
            sandbox_handle.id.as_str(),
            sandbox_handle.clone(),
        )
        .expect("exact sandbox execution should project");
    assert_eq!(sandbox_projection.handle, sandbox_handle);
    let sandbox_execution_id = sandbox_handle.id.as_str().to_owned();
    assert_eq!(
        manager
            .project_sandbox_resource_execution_observation(
                &tenant_id,
                &sandbox_source.id,
                sandbox_source.generation,
                &sandbox_source.resource_version,
                &sandbox_execution_id,
                sandbox_handle,
            )
            .expect("exact sandbox replay should be idempotent"),
        sandbox_projection
    );

    let service_definition = manager
        .create_service_definition(
            &tenant_id,
            "api",
            image_service_backend("api", "registry.example.com/api:1"),
            BTreeMap::new(),
        )
        .expect("service definition should create");
    let mut service_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    service_handle.id = SandboxId::new("execution-service-17");
    assert!(matches!(
        manager.project_service_definition_observation(
            &tenant_id,
            "api",
            service_definition.generation,
            service_handle.clone(),
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        None,
        "unfenced transitional service projection cannot become the first writer"
    );
    let rejected = manager.project_service_definition_execution_observation(
        &tenant_id,
        "api",
        service_definition.generation,
        &service_definition.resource_version,
        "crossed-execution-id",
        service_handle.clone(),
    );
    assert!(matches!(rejected, Err(Error::InvalidInput(_))));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        None,
        "crossed service first-write evidence must leave projection absent"
    );
    let service_execution_id = service_handle.id.as_str().to_owned();
    let service_projection = manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            service_definition.generation,
            &service_definition.resource_version,
            &service_execution_id,
            service_handle,
        )
        .expect("exact service execution should project");
    assert_eq!(
        service_projection.handle.id.as_str(),
        "execution-service-17"
    );
}
