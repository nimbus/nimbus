use super::super::types::TenantServiceKey;
use super::*;

struct VolumeServiceDefinitionCatalog {
    launches: BTreeMap<String, ServiceBackend>,
    volume_policies: BTreeMap<String, TenantVolumePolicyDecision>,
}

impl ServiceDefinitionCatalog for VolumeServiceDefinitionCatalog {
    fn service_backend_for_tenant(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend> {
        self.launches.get(service_name).cloned()
    }

    fn service_volume_policy_for_tenant(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> TenantVolumePolicyDecision {
        self.volume_policies
            .get(service_name)
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct RecordingServiceEvidenceWriter {
    handles: Mutex<Vec<SandboxHandle>>,
}

impl ServiceEvidenceWriter for RecordingServiceEvidenceWriter {
    fn record_service_handle<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a> {
        Box::pin(async move {
            self.handles
                .lock()
                .expect("recorded handles lock should not be poisoned")
                .push(handle.clone());
            Ok(())
        })
    }
}

#[tokio::test]
async fn start_service_for_decision_rejects_service_volume_without_catalog_policy() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                ServiceBackend::sandbox(
                    sparse_image_spec("db")
                        .with_mount(SandboxMountSpec::tenant_volume("data", "/var/lib/db")),
                ),
            )]),
        }),
        backend.clone(),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = manager
        .service_lifecycle_decision(&isolation, "db")
        .expect("db service activation decision should build");

    let error = manager
        .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
        .await
        .expect_err("service volume should require catalog volume admission");

    assert!(
        error
            .to_string()
            .contains("did not authorize volume `data`"),
        "volume admission should fail closed against the catalog policy: {error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "unadmitted service volume must fail before sandbox start"
    );
}

#[tokio::test]
async fn start_service_for_decision_accepts_declared_service_tenant_volume() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(VolumeServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                ServiceBackend::sandbox(
                    sparse_image_spec("db")
                        .with_mount(SandboxMountSpec::tenant_volume("data", "/var/lib/db")),
                ),
            )]),
            volume_policies: BTreeMap::from([(
                "db".to_owned(),
                TenantVolumePolicyDecision::new(["data"]),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = manager
        .service_lifecycle_decision(&isolation, "db")
        .expect("db service activation decision should build");

    manager
        .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
        .await
        .expect("declared service volume should pass service-definition admission")
        .expect("handle should be returned");

    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
}

#[test]
fn create_service_definition_rejects_dynamic_tenant_volume_mounts() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    );

    let error = manager
        .create_service_definition(
            &tenant_id,
            "db",
            ServiceBackend::sandbox(
                sparse_image_spec("db")
                    .with_mount(SandboxMountSpec::tenant_volume("data", "/var/lib/db")),
            ),
            BTreeMap::new(),
        )
        .expect_err("dynamic service definitions must not self-authorize tenant volumes");

    assert!(
        error
            .to_string()
            .contains("cannot declare tenant volume mounts"),
        "dynamic volume admission error should be explicit: {error}"
    );
}

#[test]
fn create_service_definition_rejects_malformed_external_endpoint() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    );

    let error = manager
        .create_service_definition(
            &tenant_id,
            "api",
            ServiceBackend::external(
                "https://",
                ExternalAuthPolicy::None,
                HealthCheckPolicy::Http {
                    path: "/health".to_owned(),
                },
            ),
            BTreeMap::new(),
        )
        .expect_err("external endpoint without a host must be rejected");

    assert!(
        error
            .to_string()
            .contains("absolute http(s) URL with a host"),
        "malformed URL error should describe the endpoint contract: {error}"
    );
}

#[tokio::test]
async fn open_service_session_rejects_in_flight_definition_delete() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    );
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("dynamic browser service definition should create");
    let key = TenantServiceKey::new(&tenant_id, "browser");
    manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .activations_in_progress
        .insert(key);

    let error = manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Service {
                name: "browser".to_owned(),
            },
            vec!["cdp".to_owned()],
            Some(60_000),
        )
        .await
        .expect_err("session open must not race a service definition delete");

    assert!(
        error
            .to_string()
            .contains("lifecycle operation in progress"),
        "session open should fail closed while service lifecycle slot is held: {error}"
    );
    assert!(
        manager.list_sessions_for_tenant(&tenant_id).is_empty(),
        "rejected service session must not leave a session resource"
    );
}

#[tokio::test]
async fn delete_service_definition_serializes_with_in_flight_activation() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1)),
    );
    let evidence = Arc::new(RecordingServiceEvidenceWriter::default());
    manager.set_service_evidence_writer_arc(evidence.clone());
    manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:latest"),
            BTreeMap::new(),
        )
        .expect("dynamic service definition should create");
    let key = TenantServiceKey::new(&tenant_id, "worker");
    manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .activations_in_progress
        .insert(key.clone());
    let delete_waiting = Arc::new(Notify::new());
    manager.set_activation_wait_observer(delete_waiting.clone());

    let non_force = manager
        .delete_service_definition_async(&tenant_id, "worker", 1, false)
        .await
        .expect_err("non-force delete must reject in-flight activation");
    assert!(
        matches!(&non_force, Error::Conflict { message, .. } if message.contains("activation in progress")),
        "non-force delete should fail closed on activation race, got {non_force:?}"
    );
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .is_some(),
        "conflicted delete must preserve the service definition"
    );

    let manager_for_delete = manager.clone();
    let tenant_for_delete = tenant_id.clone();
    let force_delete = tokio::spawn(async move {
        manager_for_delete
            .delete_service_definition_async(&tenant_for_delete, "worker", 1, true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), delete_waiting.notified())
        .await
        .expect("force delete should reach the lifecycle-slot wait");
    assert!(
        !force_delete.is_finished(),
        "force delete should wait while the lifecycle slot is still held"
    );

    let handle = backend.sandbox_handle(&tenant_id, "worker", SandboxStatus::Ready);
    backend
        .handles
        .lock()
        .expect("backend lock should not be poisoned")
        .insert(handle.id.as_str().to_owned(), handle.clone());
    {
        let mut state = manager
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state.handles.insert(key.clone(), handle);
        state.activations_in_progress.remove(&key);
    }
    manager.activation_notify.notify_waiters();

    let removed = force_delete
        .await
        .expect("force delete task should join")
        .expect("force delete should complete after activation settles");
    assert_eq!(removed.name, "worker");
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "force delete must stop the backend that became active while waiting"
    );
    assert!(
        evidence
            .handles
            .lock()
            .expect("recorded handles lock should not be poisoned")
            .iter()
            .any(|handle| {
                handle.name == "worker"
                    && handle.status == SandboxStatus::Stopped
                    && handle.published_endpoints.is_empty()
            }),
        "force delete must record a stopped handle with endpoints cleared"
    );
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .is_none(),
        "force delete should remove the definition only after coordinating with activation"
    );
}

#[tokio::test]
async fn delete_service_definition_converges_retained_cleanup_before_removal() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:latest"),
            BTreeMap::new(),
        )
        .expect("dynamic service definition should create");
    let key = TenantServiceKey::new(&tenant_id, "worker");
    let handle = backend.sandbox_handle(&tenant_id, "worker", SandboxStatus::Stopping);
    backend
        .handles
        .lock()
        .expect("backend lock should not be poisoned")
        .insert(handle.id.as_str().to_owned(), handle.clone());
    manager
        .state
        .lock()
        .expect("manager state should not be poisoned")
        .handles
        .insert(key, handle.clone());
    backend.report_inspection(retained_stopping_inspection(handle));

    let removed = manager
        .delete_service_definition_async(&tenant_id, "worker", 1, false)
        .await
        .expect("delete should explicitly converge retained cleanup before removal");

    assert_eq!(removed.name, "worker");
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "a Stopping projection is not cleanup finality and cannot skip backend teardown"
    );
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .is_none()
    );
}

#[tokio::test]
async fn force_delete_revalidates_generation_before_stopping_backend() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1)),
    );
    let evidence = Arc::new(RecordingServiceEvidenceWriter::default());
    manager.set_service_evidence_writer_arc(evidence.clone());
    manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:latest"),
            BTreeMap::new(),
        )
        .expect("dynamic service definition should create");
    let key = TenantServiceKey::new(&tenant_id, "worker");
    manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .activations_in_progress
        .insert(key.clone());
    let delete_waiting = Arc::new(Notify::new());
    manager.set_activation_wait_observer(delete_waiting.clone());

    let manager_for_delete = manager.clone();
    let tenant_for_delete = tenant_id.clone();
    let force_delete = tokio::spawn(async move {
        manager_for_delete
            .delete_service_definition_async(&tenant_for_delete, "worker", 1, true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), delete_waiting.notified())
        .await
        .expect("force delete should reach the lifecycle-slot wait");

    let handle = backend.sandbox_handle(&tenant_id, "worker", SandboxStatus::Ready);
    backend
        .handles
        .lock()
        .expect("backend lock should not be poisoned")
        .insert(handle.id.as_str().to_owned(), handle.clone());
    {
        let mut state = manager
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let definition = state
            .definitions
            .get_mut(&key)
            .expect("dynamic definition should still be present");
        definition.generation = 2;
        definition.resource_version = "svcdef-v2-race".to_owned();
        definition.updated_at_millis = definition.updated_at_millis.saturating_add(1);
        state.handles.insert(key.clone(), handle);
        state.activations_in_progress.remove(&key);
    }
    manager.activation_notify.notify_waiters();

    let error = force_delete
        .await
        .expect("force delete task should join")
        .expect_err("generation race must fail before stopping the backend");
    assert!(
        matches!(&error, Error::PreconditionFailed(message) if message.contains("generation 2") && message.contains("expected generation 1")),
        "force delete should report the raced generation without stopping: {error:?}"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "force delete must re-check generation before the irreversible backend stop"
    );
    let definition = manager
        .service_definition_for_tenant(&tenant_id, "worker")
        .expect("generation-raced definition should remain");
    assert_eq!(definition.generation, 2);
    assert!(
        evidence
            .handles
            .lock()
            .expect("recorded handles lock should not be poisoned")
            .is_empty(),
        "generation-raced force delete must not record a stopped handle"
    );
    let cached_handle = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .handles
        .get(&key)
        .cloned()
        .expect("cached running handle should remain for the updated definition");
    assert_eq!(cached_handle.status, SandboxStatus::Ready);
}
