use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxFuture, SandboxId, SandboxInspection,
    SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceBackend, ServiceManager};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::{
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity, WorkloadSagaKey,
};

use super::*;

#[derive(Default)]
struct RecordingSandboxBackend {
    inspects: AtomicUsize,
}

impl SandboxBackend for RecordingSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        self.inspects.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        Box::pin(async { Ok(()) })
    }
}

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).expect("fixture tenant should validate")
}

fn key(tenant_id: &TenantId, workload: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(workload).expect("fixture workload should validate"),
    )
}

fn service_spec(tenant_id: &TenantId, name: &str, image: &str) -> SandboxSpec {
    service_spec_for_backend(tenant_id, name, image, SandboxBackendKind::Krun)
}

fn service_spec_for_backend(
    tenant_id: &TenantId,
    name: &str,
    image: &str,
    backend: SandboxBackendKind,
) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service(name),
        backend,
        SandboxRootSpec::oci_image_reference(image),
        SandboxProcessSpec::new(["serve"]),
    )
}

fn standalone_spec(tenant_id: &TenantId, name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(format!(
            "registry.example.com/worker@sha256:{}",
            "42".repeat(32)
        )),
        SandboxProcessSpec::new(["work"]),
    )
}

fn authority(manager: Arc<ServiceManager>) -> ServiceManagerWorkloadProvisionSourceAuthority {
    ServiceManagerWorkloadProvisionSourceAuthority::new(manager)
}

#[tokio::test]
async fn service_source_is_tenant_qualified_fresh_and_effect_free() {
    let tenant_a = tenant("tenant-a");
    let tenant_b = tenant("tenant-b");
    let backend = Arc::new(RecordingSandboxBackend::default());
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        backend.clone(),
    ));
    let created_a = manager
        .create_service_definition(
            &tenant_a,
            "worker",
            ServiceBackend::sandbox(service_spec(
                &tenant_a,
                "worker",
                "registry.example.com/worker:a",
            )),
            BTreeMap::new(),
        )
        .expect("tenant A service definition should be created");
    manager
        .create_service_definition(
            &tenant_b,
            "worker",
            ServiceBackend::sandbox(service_spec(
                &tenant_b,
                "worker",
                "registry.example.com/worker:b",
            )),
            BTreeMap::new(),
        )
        .expect("tenant B service definition should be created");
    let authority = authority(manager.clone());
    let identity = WorkloadProvisionSourceIdentity::sandbox_backed_service("worker")
        .expect("fixture source identity should validate");

    let evidence_a = authority
        .current_source(&key(&tenant_a, "worker"), &identity)
        .await
        .expect("tenant A source should resolve");
    let evidence_b = authority
        .current_source(&key(&tenant_b, "worker"), &identity)
        .await
        .expect("tenant B source should resolve");

    assert_ne!(
        evidence_a.source_digest(),
        evidence_b.source_digest(),
        "same-name services in different tenants must authenticate different executable sources"
    );
    assert_eq!(
        evidence_a.source_generation(),
        WorkloadProvisionSourceGeneration::new(1)
    );
    assert_eq!(
        evidence_a.execution_provider_id(),
        &sandbox_execution_provider_id(SandboxBackendKind::Krun)
    );
    assert_eq!(
        evidence_a.attachment_provider_id(),
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .required_attachment_provider_id()
    );
    assert_eq!(
        backend.inspects.load(Ordering::SeqCst),
        0,
        "freshness reads must not inspect, restart, or otherwise invoke the sandbox provider"
    );

    let updated = manager
        .update_service_definition(
            &tenant_a,
            "worker",
            created_a.generation,
            ServiceBackend::sandbox(service_spec(
                &tenant_a,
                "worker",
                "registry.example.com/worker:a2",
            )),
            BTreeMap::new(),
        )
        .expect("tenant A service definition should update");
    let refreshed = authority
        .current_source(&key(&tenant_a, "worker"), &identity)
        .await
        .expect("updated tenant A source should resolve");
    assert_eq!(
        refreshed.source_generation(),
        WorkloadProvisionSourceGeneration::new(updated.generation)
    );
    assert_eq!(
        refreshed.resource_version().as_str(),
        updated.resource_version
    );
    assert_ne!(evidence_a.source_digest(), refreshed.source_digest());
    assert_eq!(backend.inspects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_authority_rejects_crossed_missing_and_non_sandbox_sources() {
    let tenant_id = tenant("tenant-a");
    let missing_tenant = tenant("tenant-missing");
    let backend = Arc::new(RecordingSandboxBackend::default());
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        backend.clone(),
    ));
    manager
        .create_service_definition(
            &tenant_id,
            "builtin",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("built-in definition should be created");
    let authority = authority(manager);
    let worker = WorkloadProvisionSourceIdentity::sandbox_backed_service("worker")
        .expect("fixture source identity should validate");
    let builtin = WorkloadProvisionSourceIdentity::sandbox_backed_service("builtin")
        .expect("fixture source identity should validate");

    assert_eq!(
        authority
            .current_source(&key(&tenant_id, "crossed"), &worker)
            .await,
        Err(WorkloadProvisionSourceAuthorityError::Corrupt)
    );
    assert_eq!(
        authority
            .current_source(&key(&missing_tenant, "worker"), &worker)
            .await,
        Err(WorkloadProvisionSourceAuthorityError::NotFound)
    );
    assert_eq!(
        authority
            .current_source(&key(&tenant_id, "builtin"), &builtin)
            .await,
        Err(WorkloadProvisionSourceAuthorityError::Corrupt)
    );
    assert_eq!(backend.inspects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_authority_derives_provider_identity_from_each_service_backend() {
    let tenant_id = tenant("tenant-a");
    let backend = Arc::new(RecordingSandboxBackend::default());
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        backend.clone(),
    ));
    for (name, kind) in [
        ("container-worker", SandboxBackendKind::Container),
        ("krun-worker", SandboxBackendKind::Krun),
    ] {
        manager
            .create_service_definition(
                &tenant_id,
                name,
                ServiceBackend::sandbox(service_spec_for_backend(
                    &tenant_id,
                    name,
                    &format!("registry.example.com/{name}:latest"),
                    kind,
                )),
                BTreeMap::new(),
            )
            .expect("mixed-backend service definition should be created");
    }
    let authority = authority(manager);

    for (name, kind) in [
        ("container-worker", SandboxBackendKind::Container),
        ("krun-worker", SandboxBackendKind::Krun),
    ] {
        let identity = WorkloadProvisionSourceIdentity::sandbox_backed_service(name)
            .expect("fixture source identity should validate");
        let evidence = authority
            .current_source(&key(&tenant_id, name), &identity)
            .await
            .expect("mixed-backend source should resolve");
        assert_eq!(
            evidence.attachment_provider_id(),
            nimbus_sandbox::sandbox_network_plan_requirements(kind)
                .required_attachment_provider_id()
        );
        assert_eq!(
            evidence.execution_provider_id(),
            &sandbox_execution_provider_id(kind)
        );
    }
    assert_eq!(backend.inspects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn standalone_source_authenticates_profile_without_provider_inspection() {
    let tenant_id = tenant("tenant-a");
    let other_tenant = tenant("tenant-b");
    let backend = Arc::new(RecordingSandboxBackend::default());
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        backend.clone(),
    ));
    let prepared = manager
        .prepare_standalone_sandbox_provision_source(
            &tenant_id,
            "standalone-worker",
            "worker-profile",
            standalone_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .expect("standalone desired source should prepare without provider work");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "source-authority-test")
        .with_deployment_generation(prepared.source().generation)
        .admit_decision(prepared.policy_input().clone())
        .expect("standalone desired source should admit");
    let resource = manager
        .reserve_standalone_sandbox_provision_source(&decision, prepared)
        .expect("standalone desired source should reserve without provider work");
    let authority = authority(manager);
    let identity =
        WorkloadProvisionSourceIdentity::standalone_sandbox(resource.id.clone(), "worker-profile")
            .expect("fixture source identity should validate");

    let evidence = authority
        .current_source(&key(&tenant_id, &resource.id), &identity)
        .await
        .expect("standalone source should resolve");
    assert_eq!(
        evidence.source_generation(),
        WorkloadProvisionSourceGeneration::new(1)
    );
    assert_eq!(
        evidence.resource_version().as_str(),
        resource.resource_version
    );
    assert_eq!(
        backend.inspects.load(Ordering::SeqCst),
        0,
        "source evidence must come from the services-owned snapshot, not side-effecting inspect"
    );

    let wrong_profile =
        WorkloadProvisionSourceIdentity::standalone_sandbox(resource.id.clone(), "other-profile")
            .expect("fixture source identity should validate");
    assert_eq!(
        authority
            .current_source(&key(&tenant_id, &resource.id), &wrong_profile)
            .await,
        Err(WorkloadProvisionSourceAuthorityError::Corrupt)
    );
    assert_eq!(
        authority
            .current_source(&key(&other_tenant, &resource.id), &identity)
            .await,
        Err(WorkloadProvisionSourceAuthorityError::NotFound)
    );
    assert_eq!(backend.inspects.load(Ordering::SeqCst), 0);
}
