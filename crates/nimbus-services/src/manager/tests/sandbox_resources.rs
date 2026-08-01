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
        .create_sandbox_resource_for_context_async(
            &TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create"),
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
        .create_sandbox_resource_for_context_async(
            &TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create"),
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("first standalone sandbox should start");
    let duplicate = manager
        .create_sandbox_resource_for_context_async(
            &TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create"),
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await;

    assert!(
        matches!(&duplicate, Err(Error::Conflict { message, .. }) if message.contains("duplicate sandbox id")),
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
async fn retained_stopping_standalone_sandbox_explicit_stop_converges_once() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let resource = manager
        .create_sandbox_resource_for_context_async(
            &TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create"),
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("standalone sandbox should start");
    let retained = retained_stopping_inspection(resource.handle.clone());
    backend.report_inspection(retained.clone());

    let observed = manager
        .inspect_sandbox_resource_async(&tenant_id, &resource.id)
        .await
        .expect("typed standalone inspection should succeed")
        .expect("retained sandbox should remain visible");
    assert_eq!(observed.1, retained);
    assert_eq!(observed.0.handle.status, SandboxStatus::Stopping);

    let stopped = manager
        .stop_sandbox_resource_async(&tenant_id, &resource.id)
        .await
        .expect("explicit stop should converge retained cleanup")
        .expect("stopped resource should remain recorded");
    assert_eq!(stopped.handle.status, SandboxStatus::Stopped);
    assert!(stopped.handle.published_endpoints.is_empty());
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);

    assert!(
        manager
            .stop_sandbox_resource_async(&tenant_id, &resource.id)
            .await
            .expect("stop replay should be idempotent")
            .is_none()
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "cleanup convergence must execute at most once after backend finality"
    );
}

#[tokio::test]
async fn sandbox_inspection_rejects_crossed_identity_before_resource_or_lifecycle_effects() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");

    for case in ["sandbox-id", "tenant", "name", "backend"] {
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        );
        let resource = manager
            .create_sandbox_resource_for_context_async(
                &TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create"),
                "worker",
                standalone_resource_spec(&tenant_id, "task"),
                BTreeMap::new(),
            )
            .await
            .expect("standalone sandbox should start");
        let before = resource.clone();
        let mut crossed = resource.handle.clone();
        match case {
            "sandbox-id" => crossed.id = SandboxId::new("crossed-resource-sandbox"),
            "tenant" => {
                crossed.tenant_id =
                    TenantId::new("crossed-tenant").expect("crossed tenant should be valid");
            }
            "name" => crossed.name = "crossed-resource".to_owned(),
            "backend" => crossed.backend = SandboxBackendKind::Container,
            _ => unreachable!("the identity table is exhaustive"),
        }
        backend.report_inspection_for(
            &resource.handle.id,
            SandboxInspection::provider_reported(crossed),
        );

        let error = manager
            .inspect_sandbox_resource_async(&tenant_id, &resource.id)
            .await
            .expect_err("crossed standalone inspection identity must fail closed");

        assert!(
            error.to_string().contains("crossed inspection identity"),
            "{case}: rejection must name the backend contract failure: {error}"
        );
        assert_eq!(
            manager
                .state
                .lock()
                .expect("manager state should not be poisoned")
                .sandbox_resources
                .get(&resource.id),
            Some(&before),
            "{case}: rejected evidence must not update or evict the resource"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "{case}: inspection must not start a replacement"
        );
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0, "{case}");
    }
}
