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
        backend.kind(),
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
        backend.kind(),
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
        backend.kind(),
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
        SandboxBackendKind::Krun,
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
        SandboxBackendKind::Krun,
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
