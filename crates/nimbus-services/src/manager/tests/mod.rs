use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, TenantId};
use nimbus_egress::EgressPolicy;
use nimbus_network::{EndpointProtocol, PublishedEndpoint};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxCleanupObservation, SandboxError,
    SandboxExecutionObservation, SandboxFuture, SandboxHandle, SandboxId, SandboxInspection,
    SandboxMountSpec, SandboxOwnerSpec, SandboxProcessSpec, SandboxRestartAssessment,
    SandboxRestartBlocker, SandboxRootSpec, SandboxSpec, SandboxStatus,
};

use crate::{
    ExternalAuthPolicy, HealthCheckPolicy, RuntimeServiceRegistry, ServiceBackend,
    ServiceDefinition, ServiceDefinitionCatalog, SessionLifecycleState, SessionTarget,
};
use nimbus_tenant::{TenantIsolationContext, TenantVolumePolicyDecision};

use super::*;

mod definition_lifecycle;
mod sandbox_resources;
mod sessions;
mod source_projection;
mod tenant_teardown;

struct StubServiceDefinitionCatalog {
    launches: BTreeMap<String, ServiceBackend>,
}

impl ServiceDefinitionCatalog for StubServiceDefinitionCatalog {
    fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition> {
        self.launches.get(service_name).cloned().map(|backend| {
            ServiceDefinition::static_catalog(tenant_id.clone(), service_name, backend)
        })
    }
}

struct StubSandboxBackend {
    image_starts: AtomicUsize,
    stop_calls: AtomicUsize,
    artifact_cleanup_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
    egress_reloads: Mutex<Vec<(String, EgressPolicy)>>,
    fail_stop_ids: Mutex<BTreeSet<String>>,
    ready_after_inspects: usize,
    handles: Mutex<BTreeMap<String, SandboxHandle>>,
    inspection_overrides: Mutex<BTreeMap<String, SandboxInspection>>,
}

impl StubSandboxBackend {
    fn new(ready_after_inspects: usize) -> Self {
        Self {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            artifact_cleanup_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
            egress_reloads: Mutex::new(Vec::new()),
            fail_stop_ids: Mutex::new(BTreeSet::new()),
            ready_after_inspects,
            handles: Mutex::new(BTreeMap::new()),
            inspection_overrides: Mutex::new(BTreeMap::new()),
        }
    }

    fn fail_stop_for(&self, id: &str) {
        self.fail_stop_ids
            .lock()
            .expect("failed stop id set should not be poisoned")
            .insert(id.to_owned());
    }

    fn report_inspection(&self, inspection: SandboxInspection) {
        let id = inspection.handle.id.clone();
        self.report_inspection_for(&id, inspection);
    }

    fn report_inspection_for(&self, id: &SandboxId, inspection: SandboxInspection) {
        self.inspection_overrides
            .lock()
            .expect("inspection override map should not be poisoned")
            .insert(id.as_str().to_owned(), inspection);
    }

    fn sandbox_handle(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        status: SandboxStatus,
    ) -> SandboxHandle {
        let endpoints = if status == SandboxStatus::Ready {
            vec![
                PublishedEndpoint::new(
                    "postgres",
                    EndpointProtocol::Tcp,
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                )
                .with_guest_port(5432),
            ]
        } else {
            Vec::new()
        };
        SandboxHandle::new(
            tenant_id.clone(),
            SandboxId::new(format!("sandbox-{tenant_id}-{service_name}")),
            service_name,
            SandboxBackendKind::Krun,
            status,
            endpoints,
        )
    }
}

impl SandboxBackend for StubSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        let inspect_call = self.inspect_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(inspection) = self
            .inspection_overrides
            .lock()
            .expect("inspection override map should not be poisoned")
            .get(id.as_str())
            .cloned()
        {
            return Box::pin(async move { Ok(Some(inspection)) });
        }
        let mut handles = self
            .handles
            .lock()
            .expect("backend lock should not be poisoned");
        let handle = handles.get_mut(id.as_str()).cloned().map(|mut handle| {
            if inspect_call >= self.ready_after_inspects {
                handle = self.sandbox_handle(&handle.tenant_id, &handle.name, SandboxStatus::Ready);
                handles.insert(id.as_str().to_owned(), handle.clone());
            }
            handle
        });
        Box::pin(async move { Ok(handle.map(SandboxInspection::provider_reported)) })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
        if self
            .fail_stop_ids
            .lock()
            .expect("failed stop id set should not be poisoned")
            .contains(id.as_str())
        {
            let message = format!("stub backend refused to stop sandbox {id}");
            return Box::pin(async move { Err(SandboxError::OperationFailed { message }) });
        }
        self.handles
            .lock()
            .expect("backend lock should not be poisoned")
            .remove(id.as_str());
        self.inspection_overrides
            .lock()
            .expect("inspection override map should not be poisoned")
            .remove(id.as_str());
        Box::pin(async move { Ok(()) })
    }

    fn reload_egress_policy(&self, id: &SandboxId, egress: EgressPolicy) -> SandboxFuture<()> {
        self.egress_reloads
            .lock()
            .expect("backend lock should not be poisoned")
            .push((id.as_str().to_owned(), egress));
        Box::pin(async move { Ok(()) })
    }

    fn remove_tenant_artifacts(&self, _tenant_id: TenantId) -> SandboxFuture<()> {
        self.artifact_cleanup_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(()) })
    }
}

fn sparse_image_spec(name: &str) -> SandboxSpec {
    sparse_image_spec_with_reference(name, "postgres:16")
}

fn sparse_image_spec_with_reference(name: &str, image_reference: impl Into<String>) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(image_reference),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

fn standalone_resource_spec(tenant_id: &TenantId, display_name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named(display_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference("registry.example.com/task:latest"),
        SandboxProcessSpec::new(vec!["task".to_owned()]),
    )
}

fn reserve_standalone_source(
    manager: &ServiceManager,
    tenant_id: &TenantId,
    stable_resource_id: &str,
    profile: &str,
    spec: SandboxSpec,
    labels: BTreeMap<String, String>,
) -> crate::SandboxResourceSource {
    let prepared = manager
        .prepare_standalone_sandbox_provision_source(
            tenant_id,
            stable_resource_id,
            profile,
            spec,
            labels,
        )
        .expect("standalone desired source should prepare");
    let decision =
        TenantIsolationContext::system(tenant_id.clone(), "test.standalone_sandbox.reserve")
            .with_deployment_generation(prepared.source().generation)
            .admit_decision(prepared.policy_input().clone())
            .expect("standalone desired source should admit");
    manager
        .reserve_standalone_sandbox_provision_source(&decision, prepared)
        .expect("standalone desired source should reserve")
}

fn retained_stopping_inspection(mut handle: SandboxHandle) -> SandboxInspection {
    handle.status = SandboxStatus::Stopping;
    handle.published_endpoints.clear();
    SandboxInspection::provider_reported(handle.clone()).with_provider_projection(
        handle,
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            completed_restarts: 1,
            retry_delay_millis: 2_000,
            persisted_not_before_millis: Some(9_000),
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        },
        SandboxCleanupObservation::Retained,
    )
}

fn image_service_backend(name: &str, image_reference: impl Into<String>) -> ServiceBackend {
    ServiceBackend::sandbox(sparse_image_spec_with_reference(name, image_reference))
}
