use super::*;
use crate::ServiceInstanceCatalog;

fn manager_with_backend() -> (ServiceManager, Arc<StubSandboxBackend>) {
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.kind(),
    );
    (manager, backend)
}

fn declare_ready_service(
    manager: &ServiceManager,
    backend: &StubSandboxBackend,
    tenant_id: &TenantId,
    service_name: &str,
) -> crate::ServiceDefinition {
    let definition = manager
        .create_service_definition(
            tenant_id,
            service_name,
            ServiceBackend::sandbox(SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::service(service_name),
                SandboxBackendKind::Krun,
                SandboxRootSpec::oci_image_reference("registry.example.com/service:1"),
                SandboxProcessSpec::new(["/bin/service"]),
            )),
            BTreeMap::new(),
        )
        .expect("fixture service definition should create");
    let mut handle = backend.sandbox_handle(tenant_id, service_name, SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, definition.generation, 0);
    manager
        .project_service_definition_execution_observation(
            tenant_id,
            service_name,
            definition.generation,
            &definition.resource_version,
            &execution,
            service_instance_observation(
                handle.clone(),
                endpoint_handles_for_handle(&handle, definition.generation),
            ),
        )
        .expect("fixture service observation should project");
    definition
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
    let mut starting = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Starting);
    let execution = execution_reference_for_handle(&mut starting, source.generation, 0);
    manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &execution,
            starting.clone(),
        )
        .expect("exact execution observation should establish the first projection");

    let mut ready = starting.clone();
    ready.status = SandboxStatus::Ready;
    let projected = manager
        .project_sandbox_resource_observation(
            &tenant_id,
            &source.id,
            source.generation,
            execution.attempt_id(),
            ready,
        )
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
        execution.attempt_id(),
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
        execution.attempt_id(),
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
    let mut starting = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Starting);
    let execution = execution_reference_for_handle(&mut starting, definition.generation, 0);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &execution,
            service_instance_observation(
                starting.clone(),
                endpoint_handles_for_handle(&starting, definition.generation),
            ),
        )
        .expect("exact execution observation should establish the first service projection");
    let mut ready = starting.clone();
    ready.status = SandboxStatus::Ready;
    let ready = manager
        .project_service_definition_observation(
            &tenant_id,
            "api",
            definition.generation,
            execution.attempt_id(),
            ready.clone(),
            endpoint_handles_for_handle(&ready, definition.generation),
        )
        .expect("same provider identity may advance service status");
    assert_eq!(ready.handle.status, SandboxStatus::Ready);
    let before_rejections = manager
        .service_definition_observation_for_tenant(&tenant_id, "api")
        .expect("observation should exist");

    let stale = manager.project_service_definition_observation(
        &tenant_id,
        "api",
        definition.generation + 1,
        execution.attempt_id(),
        starting.clone(),
        endpoint_handles_for_handle(&starting, definition.generation + 1),
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
        execution.attempt_id(),
        crossed.clone(),
        endpoint_handles_for_handle(&crossed, definition.generation),
    );
    assert!(matches!(crossed_result, Err(Error::Conflict { .. })));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(before_rejections)
    );
}

#[test]
fn service_projection_rejects_stale_endpoint_generation_before_mutation() {
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
    let mut handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, definition.generation, 0);
    let stale_generation = nimbus_network::NetworkResourceGeneration::new(
        execution
            .generation()
            .as_u64()
            .checked_sub(1)
            .expect("fixture execution generation should be positive"),
    );
    let stale_endpoints = endpoint_handles_for_handle(&handle, definition.generation)
        .into_iter()
        .map(|endpoint| {
            nimbus_network::PublishedEndpointHandle::new(
                endpoint.endpoint_id().clone(),
                stale_generation,
                endpoint.endpoint().clone(),
            )
        })
        .collect();

    let rejected = manager.project_service_definition_execution_observation(
        &tenant_id,
        "api",
        definition.generation,
        &definition.resource_version,
        &execution,
        service_instance_observation(handle, stale_endpoints),
    );

    assert!(
        matches!(rejected, Err(Error::PreconditionFailed(_))),
        "a stale network generation must fail closed: {rejected:?}"
    );
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        None,
        "stale endpoint evidence must not create observed service state"
    );
}

#[test]
fn service_projection_rejects_crossed_endpoint_identity_without_mutation() {
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
    let mut handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, definition.generation, 0);
    let endpoint_handles = endpoint_handles_for_handle(&handle, definition.generation);
    let accepted = manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &execution,
            service_instance_observation(handle.clone(), endpoint_handles.clone()),
        )
        .expect("exact endpoint identity should project");
    let crossed_endpoints = endpoint_handles
        .into_iter()
        .map(|endpoint| {
            nimbus_network::PublishedEndpointHandle::new(
                nimbus_network::PublishedEndpointId::for_workload_endpoint(
                    "crossed-service-incarnation",
                    &endpoint.endpoint().name,
                ),
                endpoint.generation(),
                endpoint.endpoint().clone(),
            )
        })
        .collect();

    let rejected = manager.project_service_definition_execution_observation(
        &tenant_id,
        "api",
        definition.generation,
        &definition.resource_version,
        &execution,
        service_instance_observation(handle, crossed_endpoints),
    );

    assert!(matches!(rejected, Err(Error::Conflict { .. })));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(accepted),
        "crossed stable identity must leave the exact projection unchanged"
    );
}

#[test]
fn service_projection_retains_endpoint_identity_fence_across_withdrawal() {
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
    let mut ready = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut ready, definition.generation, 0);
    let endpoint_handles = endpoint_handles_for_handle(&ready, definition.generation);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &execution,
            service_instance_observation(ready.clone(), endpoint_handles.clone()),
        )
        .expect("exact endpoint identity should project");
    let mut withdrawn = ready.clone();
    withdrawn.status = SandboxStatus::NotReady;
    withdrawn.published_endpoints.clear();
    let withdrawn = manager
        .project_service_definition_observation(
            &tenant_id,
            "api",
            definition.generation,
            execution.attempt_id(),
            withdrawn,
            Vec::new(),
        )
        .expect("same-generation withdrawal should project");
    let crossed_endpoints = endpoint_handles
        .into_iter()
        .map(|endpoint| {
            nimbus_network::PublishedEndpointHandle::new(
                nimbus_network::PublishedEndpointId::for_workload_endpoint(
                    "crossed-after-withdrawal",
                    &endpoint.endpoint().name,
                ),
                endpoint.generation(),
                endpoint.endpoint().clone(),
            )
        })
        .collect();

    let rejected = manager.project_service_definition_observation(
        &tenant_id,
        "api",
        definition.generation,
        execution.attempt_id(),
        ready.clone(),
        crossed_endpoints,
    );

    assert!(matches!(rejected, Err(Error::Conflict { .. })));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(withdrawn.clone()),
        "withdrawal must retain the stable identity fence and reject crossed republish"
    );
    let restored = manager
        .project_service_definition_observation(
            &tenant_id,
            "api",
            definition.generation,
            execution.attempt_id(),
            ready.clone(),
            endpoint_handles_for_handle(&ready, definition.generation),
        )
        .expect("the authenticated identity may republish after withdrawal");
    assert_eq!(restored.handle, ready);
    assert_eq!(
        restored.endpoint_identity_fence,
        withdrawn.endpoint_identity_fence
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
    let mut handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, definition.generation, 0);
    let endpoint_handles = endpoint_handles_for_handle(&handle, definition.generation);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &execution,
            service_instance_observation(handle.clone(), endpoint_handles.clone()),
        )
        .expect("exact service observation should project");

    let instance = ServiceInstanceCatalog::service_instance_for_name(&manager, &tenant_id, "api")
        .expect("the exact service instance should be visible");
    assert_eq!(instance.handle(), &handle);
    assert_eq!(instance.published_endpoints(), endpoint_handles);
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
fn service_retirement_claim_fences_only_the_exact_service_resolution_key() {
    let tenant_a = TenantId::new("tenant-a").expect("tenant ID should validate");
    let tenant_b = TenantId::new("tenant-b").expect("tenant ID should validate");
    let (manager, backend) = manager_with_backend();
    let retiring = declare_ready_service(&manager, &backend, &tenant_a, "api");
    declare_ready_service(&manager, &backend, &tenant_a, "peer");
    declare_ready_service(&manager, &backend, &tenant_b, "api");

    let claim = manager
        .claim_service_definition_retirement(
            &tenant_a,
            "api",
            retiring.generation,
            &retiring.resource_version,
            WorkloadSourceRetirementOperation::Stop,
            WorkloadGeneration::new(0),
            nimbus_workloads::WorkloadSagaRevision::new(0),
        )
        .expect("exact service retirement should claim");

    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_a, "api")
            .expect("fenced lookup should not error")
            .is_none()
    );
    assert!(!RuntimeServiceRegistry::snapshot_for_tenant(&manager, &tenant_a).contains_key("api"));
    assert!(
        ServiceInstanceCatalog::service_instance_for_name(&manager, &tenant_a, "api").is_none()
    );
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_a, "peer")
            .expect("unrelated service lookup should succeed")
            .is_some()
    );
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_b, "api")
            .expect("other-tenant lookup should succeed")
            .is_some()
    );
    assert_eq!(
        manager
            .service_definition_observation_for_tenant(&tenant_a, "api")
            .expect("fencing must retain the observed projection")
            .handle
            .status,
        SandboxStatus::Ready
    );

    manager
        .release_unadvanced_source_retirement_claim(&claim)
        .expect("pre-effect claim release should succeed");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_a, "api")
            .expect("released service lookup should succeed")
            .is_some()
    );
}

#[test]
fn tenant_retirement_barrier_fences_only_the_exact_tenant_service_set() {
    let tenant_a = TenantId::new("tenant-a").expect("tenant ID should validate");
    let tenant_b = TenantId::new("tenant-b").expect("tenant ID should validate");
    let (manager, backend) = manager_with_backend();
    declare_ready_service(&manager, &backend, &tenant_a, "api");
    declare_ready_service(&manager, &backend, &tenant_a, "peer");
    declare_ready_service(&manager, &backend, &tenant_b, "api");

    manager
        .claim_tenant_source_retirement(
            &tenant_a,
            std::num::NonZeroU64::new(1).expect("tenant incarnation should be nonzero"),
        )
        .expect("tenant source retirement should claim");

    assert!(RuntimeServiceRegistry::snapshot_for_tenant(&manager, &tenant_a).is_empty());
    assert!(ServiceInstanceCatalog::service_instances_for_tenant(&manager, &tenant_a).is_empty());
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_b, "api")
            .expect("other-tenant lookup should succeed")
            .is_some()
    );
    assert_eq!(
        manager
            .service_definition_observation_for_tenant(&tenant_a, "api")
            .expect("tenant fencing must retain the observed projection")
            .handle
            .status,
        SandboxStatus::Ready
    );
}

#[test]
fn restart_resolution_withdrawal_claims_before_first_observation() {
    let tenant_id = TenantId::new("tenant").expect("tenant ID should validate");
    let (manager, backend) = manager_with_backend();
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "api",
            ServiceBackend::sandbox(SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::service("api"),
                SandboxBackendKind::Krun,
                SandboxRootSpec::oci_image_reference("registry.example.com/service:1"),
                SandboxProcessSpec::new(["/bin/service"]),
            )),
            BTreeMap::new(),
        )
        .expect("fixture service definition should create");
    let mut source_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let source_execution =
        execution_reference_for_handle(&mut source_handle, definition.generation, 0);
    let source_attempt = source_execution.attempt_id().clone();
    let target_attempt = WorkloadExecutionAttemptId::for_execution(
        source_execution.execution_id(),
        WorkloadRestartEpoch::new(1),
    );

    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("unobserved service lookup should succeed")
            .is_none()
    );
    for _ in 0..2 {
        manager
            .claim_service_resolution_withdrawal(
                &tenant_id,
                "api",
                definition.generation,
                &definition.resource_version,
                &source_attempt,
                &target_attempt,
            )
            .expect("an already absent exact resolution should claim or replay");
    }
    assert!(
        manager
            .service_resolution_withdrawal_requires_restore(
                &tenant_id,
                "api",
                definition.generation,
                &definition.resource_version,
                &target_attempt,
            )
            .expect("exact active fence check should succeed")
    );

    let mut target_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let target_execution =
        execution_reference_for_handle(&mut target_handle, definition.generation, 1);
    assert_eq!(target_execution.attempt_id(), &target_attempt);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_execution,
            service_instance_observation(
                target_handle.clone(),
                endpoint_handles_for_handle(&target_handle, definition.generation),
            ),
        )
        .expect("restart target observation should project while fenced");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("active restart fence lookup should succeed")
            .is_none()
    );
    manager
        .release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_attempt,
        )
        .expect("exact target observation should release the fence");
    assert!(
        !manager
            .service_resolution_withdrawal_requires_restore(
                &tenant_id,
                "api",
                definition.generation,
                &definition.resource_version,
                &target_attempt,
            )
            .expect("exact released fence check should succeed")
    );
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("released target lookup should succeed")
            .is_some()
    );
}

#[test]
fn restart_resolution_withdrawal_is_attempt_fenced_and_replay_safe() {
    let tenant_id = TenantId::new("tenant").expect("tenant ID should validate");
    let (manager, backend) = manager_with_backend();
    let definition = declare_ready_service(&manager, &backend, &tenant_id, "api");
    let observation = manager
        .service_definition_observation_for_tenant(&tenant_id, "api")
        .expect("ready service should have an observation");
    let source_attempt = observation.execution.attempt_id().clone();
    let target_attempt = WorkloadExecutionAttemptId::for_execution(
        observation.execution.execution_id(),
        WorkloadRestartEpoch::new(1),
    );

    for _ in 0..2 {
        manager
            .claim_service_resolution_withdrawal(
                &tenant_id,
                "api",
                definition.generation,
                &definition.resource_version,
                &source_attempt,
                &target_attempt,
            )
            .expect("exact resolution withdrawal should claim or replay");
    }
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("fenced lookup should not error")
            .is_none()
    );
    assert_eq!(
        manager
            .service_definition_observation_for_tenant(&tenant_id, "api")
            .expect("restart fence must retain the observation"),
        observation
    );

    let crossed_target = WorkloadExecutionAttemptId::for_execution(
        observation.execution.execution_id(),
        WorkloadRestartEpoch::new(2),
    );
    assert!(matches!(
        manager.release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &crossed_target,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert!(matches!(
        manager.release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_attempt,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    let mut target_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let target_execution =
        execution_reference_for_handle(&mut target_handle, definition.generation, 1);
    assert_eq!(target_execution.attempt_id(), &target_attempt);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_execution,
            service_instance_observation(
                target_handle.clone(),
                endpoint_handles_for_handle(&target_handle, definition.generation),
            ),
        )
        .expect("restart target observation should project before release");
    manager
        .release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_attempt,
        )
        .expect("exact resolution release should succeed");
    manager
        .release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_attempt,
        )
        .expect("exact resolution release should replay");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("released lookup should succeed")
            .is_some()
    );

    assert!(matches!(
        manager.claim_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &source_attempt,
            &target_attempt,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    manager
        .claim_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_attempt,
            &crossed_target,
        )
        .expect("the next exact restart attempt should extend the completed chain");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("second fenced lookup should not error")
            .is_none()
    );
    let mut crossed_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let crossed_execution =
        execution_reference_for_handle(&mut crossed_handle, definition.generation, 2);
    assert_eq!(crossed_execution.attempt_id(), &crossed_target);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &crossed_execution,
            service_instance_observation(
                crossed_handle.clone(),
                endpoint_handles_for_handle(&crossed_handle, definition.generation),
            ),
        )
        .expect("second restart target observation should project before release");
    manager
        .release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &crossed_target,
        )
        .expect("the next exact restart release should succeed");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("second released lookup should succeed")
            .is_some()
    );
}

#[test]
fn active_restart_resolution_withdrawal_hands_off_without_reopening() {
    let tenant_id = TenantId::new("tenant").expect("tenant ID should validate");
    let (manager, backend) = manager_with_backend();
    let definition = declare_ready_service(&manager, &backend, &tenant_id, "api");
    let observation = manager
        .service_definition_observation_for_tenant(&tenant_id, "api")
        .expect("ready service should have an observation");
    let source_attempt = observation.execution.attempt_id().clone();
    let first_target = WorkloadExecutionAttemptId::for_execution(
        observation.execution.execution_id(),
        WorkloadRestartEpoch::new(1),
    );
    let second_target = WorkloadExecutionAttemptId::for_execution(
        observation.execution.execution_id(),
        WorkloadRestartEpoch::new(2),
    );

    manager
        .claim_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &source_attempt,
            &first_target,
        )
        .expect("first restart should fence resolution");
    manager
        .claim_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &first_target,
            &second_target,
        )
        .expect("successor restart should atomically extend the active fence");
    assert!(matches!(
        manager.release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &first_target,
        ),
        Err(Error::PreconditionFailed(_))
    ));
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("successor-fenced lookup should not error")
            .is_none()
    );

    let mut target_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let target_execution =
        execution_reference_for_handle(&mut target_handle, definition.generation, 2);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_execution,
            service_instance_observation(
                target_handle.clone(),
                endpoint_handles_for_handle(&target_handle, definition.generation),
            ),
        )
        .expect("successor target observation should project");
    manager
        .release_service_resolution_withdrawal(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &second_target,
        )
        .expect("successor target should release the exact active fence");
    assert!(
        RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, "api")
            .expect("successor release lookup should not error")
            .is_some()
    );
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
    let sandbox_execution =
        execution_reference_for_handle(&mut sandbox_handle, sandbox_source.generation, 0);
    let mut crossed_sandbox_handle =
        backend.sandbox_handle(&tenant_id, "crossed-task", SandboxStatus::Ready);
    let crossed_sandbox_execution =
        execution_reference_for_handle(&mut crossed_sandbox_handle, sandbox_source.generation, 0);

    assert!(matches!(
        manager.project_sandbox_resource_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            sandbox_execution.attempt_id(),
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
            &sandbox_execution,
            sandbox_handle.clone(),
        ),
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &sandbox_source.id,
            sandbox_source.generation,
            &sandbox_source.resource_version,
            &crossed_sandbox_execution,
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
            &sandbox_execution,
            sandbox_handle.clone(),
        )
        .expect("exact sandbox execution should project");
    assert_eq!(sandbox_projection.handle, sandbox_handle);
    assert_eq!(
        manager
            .project_sandbox_resource_execution_observation(
                &tenant_id,
                &sandbox_source.id,
                sandbox_source.generation,
                &sandbox_source.resource_version,
                &sandbox_execution,
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
    let service_execution =
        execution_reference_for_handle(&mut service_handle, service_definition.generation, 0);
    let mut crossed_service_handle =
        backend.sandbox_handle(&tenant_id, "crossed-api", SandboxStatus::Ready);
    let crossed_service_execution = execution_reference_for_handle(
        &mut crossed_service_handle,
        service_definition.generation,
        0,
    );
    assert!(matches!(
        manager.project_service_definition_observation(
            &tenant_id,
            "api",
            service_definition.generation,
            service_execution.attempt_id(),
            service_handle.clone(),
            endpoint_handles_for_handle(&service_handle, service_definition.generation),
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
        &crossed_service_execution,
        service_instance_observation(
            service_handle.clone(),
            endpoint_handles_for_handle(&service_handle, service_definition.generation),
        ),
    );
    assert!(matches!(rejected, Err(Error::InvalidInput(_))));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        None,
        "crossed service first-write evidence must leave projection absent"
    );
    let expected_execution_id = service_execution.execution_id().as_str().to_owned();
    let service_projection = manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            service_definition.generation,
            &service_definition.resource_version,
            &service_execution,
            service_instance_observation(
                service_handle.clone(),
                endpoint_handles_for_handle(&service_handle, service_definition.generation),
            ),
        )
        .expect("exact service execution should project");
    assert_eq!(service_projection.handle.id.as_str(), expected_execution_id);
}

#[test]
fn sandbox_projection_rejects_delayed_attempts_and_preserves_target_snapshot() {
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
    let mut source_handle = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Starting);
    let source_execution = execution_reference_for_handle(&mut source_handle, source.generation, 0);
    manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &source_execution,
            source_handle.clone(),
        )
        .expect("source attempt should project");

    let mut target_handle = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Ready);
    let target_execution = execution_reference_for_handle(&mut target_handle, source.generation, 1);
    let target = manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &target_execution,
            target_handle.clone(),
        )
        .expect("newer target attempt should project");
    let target_snapshot = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("target snapshot lookup should succeed")
        .expect("target snapshot should exist");
    assert_eq!(target.execution, target_execution);

    let mut delayed_source = source_handle;
    delayed_source.status = SandboxStatus::Failed;
    for rejected in [
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &source_execution,
            delayed_source,
        ),
        manager.project_sandbox_resource_observation(
            &tenant_id,
            &source.id,
            source.generation,
            source_execution.attempt_id(),
            target_handle.clone(),
        ),
    ] {
        assert!(matches!(rejected, Err(Error::PreconditionFailed(_))));
        assert_eq!(
            manager
                .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
                .expect("snapshot lookup should succeed")
                .expect("snapshot should remain"),
            target_snapshot,
            "an old-attempt callback must leave target projection bytes unchanged"
        );
    }

    assert_eq!(
        manager
            .project_sandbox_resource_execution_observation(
                &tenant_id,
                &source.id,
                source.generation,
                &source.resource_version,
                &target_execution,
                target_handle.clone(),
            )
            .expect("exact target replay should be idempotent"),
        target
    );
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
            .expect("snapshot lookup should succeed")
            .expect("snapshot should remain"),
        target_snapshot
    );

    let crossed_attempt = WorkloadExecutionAttemptId::for_execution(
        target_execution.execution_id(),
        WorkloadRestartEpoch::new(2),
    );
    let mut crossed_workload_handle =
        backend.sandbox_handle(&tenant_id, "other-task", SandboxStatus::Ready);
    let crossed_workload =
        execution_reference_for_handle(&mut crossed_workload_handle, source.generation, 2);
    let mut crossed_generation_handle = target_handle.clone();
    let crossed_generation =
        execution_reference_for_handle(&mut crossed_generation_handle, source.generation + 1, 2);
    for rejected in [
        manager.project_sandbox_resource_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &crossed_attempt,
            target_handle.clone(),
        ),
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &crossed_workload,
            target_handle.clone(),
        ),
        manager.project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &crossed_generation,
            target_handle.clone(),
        ),
    ] {
        assert!(rejected.is_err());
        assert_eq!(
            manager
                .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
                .expect("snapshot lookup should succeed")
                .expect("snapshot should remain"),
            target_snapshot,
            "crossed execution evidence must preserve the target snapshot"
        );
    }
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn service_name_reads_only_the_newest_attempt_without_provider_io() {
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
    let mut source_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Starting);
    let source_execution =
        execution_reference_for_handle(&mut source_handle, definition.generation, 0);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &source_execution,
            service_instance_observation(
                source_handle.clone(),
                endpoint_handles_for_handle(&source_handle, definition.generation),
            ),
        )
        .expect("source attempt should project");
    let mut target_handle = backend.sandbox_handle(&tenant_id, "api", SandboxStatus::Ready);
    let target_execution =
        execution_reference_for_handle(&mut target_handle, definition.generation, 1);
    let target = manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "api",
            definition.generation,
            &definition.resource_version,
            &target_execution,
            service_instance_observation(
                target_handle.clone(),
                endpoint_handles_for_handle(&target_handle, definition.generation),
            ),
        )
        .expect("target attempt should project");

    let delayed = manager.project_service_definition_execution_observation(
        &tenant_id,
        "api",
        definition.generation,
        &definition.resource_version,
        &source_execution,
        service_instance_observation(
            source_handle.clone(),
            endpoint_handles_for_handle(&source_handle, definition.generation),
        ),
    );
    assert!(matches!(delayed, Err(Error::PreconditionFailed(_))));
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "api"),
        Some(target.clone())
    );
    assert_eq!(
        manager
            .project_service_definition_execution_observation(
                &tenant_id,
                "api",
                definition.generation,
                &definition.resource_version,
                &target_execution,
                service_instance_observation(
                    target_handle.clone(),
                    endpoint_handles_for_handle(&target_handle, definition.generation),
                ),
            )
            .expect("exact target replay should be idempotent"),
        target
    );
    assert_eq!(
        ServiceInstanceCatalog::service_instance_for_name(&manager, &tenant_id, "api")
            .map(|observation| observation.handle().clone()),
        Some(target_handle)
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}
