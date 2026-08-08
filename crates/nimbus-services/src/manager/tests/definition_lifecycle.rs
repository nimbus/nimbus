use super::super::types::TenantServiceKey;
use super::*;

struct VolumeServiceDefinitionCatalog {
    launches: BTreeMap<String, ServiceBackend>,
    volume_policies: BTreeMap<String, TenantVolumePolicyDecision>,
}

impl ServiceDefinitionCatalog for VolumeServiceDefinitionCatalog {
    fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition> {
        self.launches.get(service_name).cloned().map(|backend| {
            ServiceDefinition::static_catalog(tenant_id.clone(), service_name, backend)
        })
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

#[test]
fn service_source_validation_rejects_volume_without_catalog_policy_before_provider_io() {
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
    let prepared = manager
        .prepare_sandbox_service_provision_source(&tenant_id, "db")
        .expect("complete source should prepare before admission validation");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "service.provision")
        .with_deployment_generation(prepared.definition().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("policy input should form an admitted decision");

    let error = manager
        .validate_sandbox_service_provision_decision(&decision, &prepared)
        .expect_err("undeclared service volume must reject");
    assert!(
        error
            .to_string()
            .contains("did not authorize volume `data`")
    );
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn service_source_validation_accepts_declared_volume_without_starting_provider() {
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
    );
    let prepared = manager
        .prepare_sandbox_service_provision_source(&tenant_id, "db")
        .expect("sandbox service source should prepare");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "service.provision")
        .with_deployment_generation(prepared.definition().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("declared volume policy should admit");

    manager
        .validate_sandbox_service_provision_decision(&decision, &prepared)
        .expect("exact declared volume should validate");
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn sandbox_only_service_preparation_rejects_declared_non_sandbox_backends_without_mutation() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([
                ("browser".to_owned(), ServiceBackend::built_in("browser")),
                (
                    "api".to_owned(),
                    ServiceBackend::external(
                        "https://api.example.com",
                        ExternalAuthPolicy::None,
                        HealthCheckPolicy::Http {
                            path: "/health".to_owned(),
                        },
                    ),
                ),
            ]),
        }),
        backend.clone(),
    );

    for name in ["browser", "api"] {
        assert!(
            manager
                .prepare_sandbox_service_provision_source(&tenant_id, name)
                .is_err(),
            "{name} must remain declared without sandbox coercion"
        );
        assert!(manager.service_declared_for_tenant(&tenant_id, name));
        assert_eq!(
            RuntimeServiceRegistry::resolve_service_binding(&manager, &tenant_id, name)
                .expect("declared read should remain valid"),
            None
        );
    }
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
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
            .contains("cannot declare tenant volume mounts")
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
            .contains("absolute http(s) URL with a host")
    );
}

#[tokio::test]
async fn session_and_delete_serialize_on_definition_mutation_only() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let manager = Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            Arc::new(StubSandboxBackend::new(1)),
        )
        .with_definition_mutation_timeout(Duration::from_secs(1)),
    );
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("dynamic browser definition should create");
    let key = TenantServiceKey::new(&tenant_id, "browser");
    manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .definition_mutations_in_progress
        .insert(key.clone());

    let session_error = manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Service {
                name: "browser".to_owned(),
            },
            vec!["cdp".to_owned()],
            Some(60_000),
        )
        .await
        .expect_err("session open must not cross definition deletion");
    assert!(session_error.to_string().contains("definition mutation"));
    let delete_error = manager
        .delete_service_definition_async(&tenant_id, "browser", 1, false)
        .await
        .expect_err("non-force delete must reject an occupied definition gate");
    assert!(delete_error.to_string().contains("definition mutation"));

    manager.release_definition_mutation(&key);
    manager
        .delete_service_definition_async(&tenant_id, "browser", 1, false)
        .await
        .expect("delete should complete after the definition-only gate releases");
}

#[tokio::test]
async fn cancelling_definition_mutation_owner_releases_the_exact_gate() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let manager = Arc::new(ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    ));
    let key = TenantServiceKey::new(&tenant_id, "browser");
    let claimed = Arc::new(Notify::new());
    let task_manager = Arc::clone(&manager);
    let task_key = key.clone();
    let task_claimed = Arc::clone(&claimed);
    let mutation = tokio::spawn(async move {
        let _claim = task_manager
            .claim_definition_mutation_guard(&task_key, false)
            .await
            .expect("the first exact mutation claim should succeed");
        task_claimed.notify_one();
        std::future::pending::<()>().await;
    });
    tokio::time::timeout(Duration::from_secs(1), claimed.notified())
        .await
        .expect("the mutation task should acquire its exact gate before timeout");
    assert!(
        manager
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .definition_mutations_in_progress
            .contains(&key)
    );

    mutation.abort();
    assert!(
        mutation
            .await
            .expect_err("aborted mutation should be cancelled")
            .is_cancelled(),
        "fixture cancellation should exercise future-drop cleanup"
    );
    assert!(
        !manager
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .definition_mutations_in_progress
            .contains(&key),
        "dropping an async mutation owner must not strand the serialization gate"
    );
    manager
        .claim_definition_mutation(&key, false)
        .await
        .expect("a later exact mutation should acquire the released gate");
    manager.release_definition_mutation(&key);
}

#[tokio::test]
async fn definition_delete_retires_only_the_canonical_observation() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend.clone(),
    );
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:latest"),
            BTreeMap::new(),
        )
        .expect("definition should create");
    let handle = backend.sandbox_handle(&tenant_id, "worker", SandboxStatus::Stopping);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            handle.id.as_str(),
            handle.clone(),
        )
        .expect("canonical observation should project");
    backend.report_inspection(retained_stopping_inspection(handle));

    manager
        .delete_service_definition_async(&tenant_id, "worker", definition.generation, false)
        .await
        .expect("retained cleanup should converge before deletion");
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .is_none()
    );
    assert_eq!(
        manager.service_definition_observation_for_tenant(&tenant_id, "worker"),
        None
    );
}

#[tokio::test]
async fn force_delete_revalidates_generation_before_retirement_effect() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        )
        .with_definition_mutation_timeout(Duration::from_secs(1)),
    );
    let definition = manager
        .create_service_definition(
            &tenant_id,
            "worker",
            image_service_backend("worker", "registry.example.com/worker:latest"),
            BTreeMap::new(),
        )
        .expect("definition should create");
    let handle = backend.sandbox_handle(&tenant_id, "worker", SandboxStatus::Ready);
    manager
        .project_service_definition_execution_observation(
            &tenant_id,
            "worker",
            definition.generation,
            &definition.resource_version,
            handle.id.as_str(),
            handle.clone(),
        )
        .expect("canonical observation should project");
    let key = TenantServiceKey::new(&tenant_id, "worker");
    manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .definition_mutations_in_progress
        .insert(key.clone());
    let waiting = Arc::new(Notify::new());
    manager.set_definition_mutation_wait_observer(waiting.clone());

    let manager_for_delete = manager.clone();
    let tenant_for_delete = tenant_id.clone();
    let deletion = tokio::spawn(async move {
        manager_for_delete
            .delete_service_definition_async(&tenant_for_delete, "worker", 1, true)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), waiting.notified())
        .await
        .expect("force delete should wait on the definition gate");
    {
        let mut state = manager
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let current = state
            .definitions
            .get_mut(&key)
            .expect("definition should remain while gate is occupied");
        current.generation = 2;
        current.resource_version = "svcdef-v2-race".to_owned();
        state.definition_mutations_in_progress.remove(&key);
    }
    manager.definition_mutation_notify.notify_waiters();

    let error = deletion
        .await
        .expect("delete task should join")
        .expect_err("generation race must reject before retirement");
    assert!(matches!(error, Error::PreconditionFailed(_)));
    assert_eq!(backend.inspect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        manager
            .service_definition_for_tenant(&tenant_id, "worker")
            .expect("raced definition should remain")
            .generation,
        2
    );
}
