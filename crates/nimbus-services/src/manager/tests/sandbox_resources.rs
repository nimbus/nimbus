use super::*;

#[test]
fn standalone_source_owns_initial_generation_and_exact_replay_version() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let prepared = manager
        .prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "stable-resource",
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::from([("team".to_owned(), "runtime".to_owned())]),
        )
        .expect("exact desired source should prepare");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "sandbox.reserve")
        .with_deployment_generation(prepared.source().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("exact desired source should admit");
    let first = manager
        .reserve_standalone_sandbox_provision_source(&decision, prepared)
        .expect("exact desired source should reserve");

    let replay = manager
        .prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "stable-resource",
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::from([("team".to_owned(), "runtime".to_owned())]),
        )
        .expect("exact replay should prepare retained source");
    let replay_decision = TenantIsolationContext::system(tenant_id.clone(), "sandbox.reserve")
        .with_deployment_generation(replay.source().generation)
        .admit_decision(replay.policy_input().clone())
        .expect("exact replay should admit");
    let replayed = manager
        .reserve_standalone_sandbox_provision_source(&replay_decision, replay)
        .expect("exact replay should adopt retained source");
    assert_eq!(first.generation, 1);
    assert_eq!(replayed.generation, first.generation);
    assert_eq!(replayed.resource_version, first.resource_version);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn crossed_standalone_decision_rejects_before_source_mutation() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let other_tenant = TenantId::new("other").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let prepared = manager
        .prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "stable-resource",
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .expect("desired source should prepare");
    let crossed = TenantIsolationContext::system(other_tenant, "sandbox.reserve")
        .with_deployment_generation(prepared.source().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("crossed tenant context can form its own decision");

    assert!(
        manager
            .reserve_standalone_sandbox_provision_source(&crossed, prepared)
            .is_err()
    );
    assert_eq!(
        manager
            .sandbox_resource_snapshot_for_tenant(&tenant_id, "stable-resource")
            .expect("source lookup should succeed"),
        None
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_standalone_retirement_is_the_only_provider_inspecting_read() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let source = reserve_standalone_source(
        &manager,
        &tenant_id,
        "stable-resource",
        "worker",
        standalone_resource_spec(&tenant_id, "task"),
        BTreeMap::new(),
    );
    let handle = backend.sandbox_handle(&tenant_id, "task", SandboxStatus::Starting);
    manager
        .project_sandbox_resource_execution_observation(
            &tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            handle.id.as_str(),
            handle.clone(),
        )
        .expect("exact execution observation should establish the projection");
    backend.report_inspection(SandboxInspection::provider_reported(handle));

    let first = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("read should succeed")
        .expect("snapshot should exist");
    let second = manager
        .sandbox_resource_snapshot_for_tenant(&tenant_id, &source.id)
        .expect("repeat read should succeed")
        .expect("snapshot should remain");
    assert_eq!(first, second);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);

    let retired = manager
        .retire_sandbox_resource_async(&tenant_id, &source.id)
        .await
        .expect("explicit retirement should succeed")
        .expect("retired snapshot should remain observable");
    assert_eq!(
        retired
            .observation
            .expect("retirement should retain stopped observation")
            .handle
            .status,
        SandboxStatus::Stopped
    );
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
}
