use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, TenantId};
use nimbus_egress::{EgressPolicy, EgressRule};
use nimbus_network::{EndpointProtocol, PublishedEndpoint};
use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxError, SandboxFuture, SandboxHandle, SandboxId,
    SandboxMountSpec, SandboxOciBuildSpec, SandboxOciImageSource, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus,
};

use crate::{
    ExternalAuthPolicy, HealthCheckPolicy, RuntimeServiceRegistry, ServiceBackend,
    ServiceDefinitionCatalog, SessionLifecycleState, SessionTarget,
};
use nimbus_tenant::{
    TenantImageVerificationEvidence, TenantImageVerificationProvider, TenantIsolationContext,
    TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantVolumePolicyDecision,
    WorkloadAttributes,
};
use tokio::sync::Semaphore;

use super::*;

mod definition_lifecycle;
mod lifecycle;
mod sandbox_resources;
mod sessions;
mod tenant_teardown;

struct StubServiceDefinitionCatalog {
    launches: BTreeMap<String, ServiceBackend>,
}

impl ServiceDefinitionCatalog for StubServiceDefinitionCatalog {
    fn service_backend_for_tenant(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend> {
        self.launches.get(service_name).cloned()
    }
}

struct StopBarrier {
    entered: Semaphore,
    release: Semaphore,
}

impl Default for StopBarrier {
    fn default() -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
}

struct StubSandboxBackend {
    image_starts: AtomicUsize,
    build_starts: AtomicUsize,
    stop_calls: AtomicUsize,
    artifact_cleanup_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
    egress_reloads: Mutex<Vec<(String, EgressPolicy)>>,
    fail_stop_ids: Mutex<BTreeSet<String>>,
    ready_after_inspects: usize,
    handle_tenant_override: Option<TenantId>,
    handle_name_override: Option<String>,
    stop_barrier: Option<Arc<StopBarrier>>,
    handles: Mutex<BTreeMap<String, SandboxHandle>>,
}

impl StubSandboxBackend {
    fn new(ready_after_inspects: usize) -> Self {
        Self {
            image_starts: AtomicUsize::new(0),
            build_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            artifact_cleanup_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
            egress_reloads: Mutex::new(Vec::new()),
            fail_stop_ids: Mutex::new(BTreeSet::new()),
            ready_after_inspects,
            handle_tenant_override: None,
            handle_name_override: None,
            stop_barrier: None,
            handles: Mutex::new(BTreeMap::new()),
        }
    }

    fn with_handle_tenant_override(mut self, tenant_id: TenantId) -> Self {
        self.handle_tenant_override = Some(tenant_id);
        self
    }

    fn with_handle_name_override(mut self, name: impl Into<String>) -> Self {
        self.handle_name_override = Some(name.into());
        self
    }

    fn with_stop_barrier(mut self, barrier: Arc<StopBarrier>) -> Self {
        self.stop_barrier = Some(barrier);
        self
    }

    fn fail_stop_for(&self, id: &str) {
        self.fail_stop_ids
            .lock()
            .expect("failed stop id set should not be poisoned")
            .insert(id.to_owned());
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
        let handle_tenant_id = self
            .handle_tenant_override
            .as_ref()
            .unwrap_or(tenant_id)
            .clone();
        let handle_name = self.handle_name_override.as_deref().unwrap_or(service_name);
        SandboxHandle::new(
            handle_tenant_id.clone(),
            SandboxId::new(format!("sandbox-{handle_tenant_id}-{service_name}")),
            handle_name,
            SandboxBackendKind::Krun,
            status,
            endpoints,
        )
    }
}

struct RecordingImageVerifier {
    evidence: TenantImageVerificationEvidence,
    calls: AtomicUsize,
    references: Mutex<Vec<String>>,
}

impl RecordingImageVerifier {
    fn with_evidence(evidence: TenantImageVerificationEvidence) -> Self {
        Self {
            evidence,
            calls: AtomicUsize::new(0),
            references: Mutex::new(Vec::new()),
        }
    }
}

impl TenantImageVerificationProvider for RecordingImageVerifier {
    fn verify_registry_image(
        &self,
        request: &nimbus_tenant::TenantImageVerificationRequest,
    ) -> nimbus_core::Result<TenantImageVerificationEvidence> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.references
            .lock()
            .expect("image verifier references should not be poisoned")
            .push(request.image_reference().to_string());
        Ok(self.evidence.clone())
    }
}

impl SandboxBackend for StubSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        match &spec.root {
            SandboxRootSpec::Rootfs(_) => {
                let message = format!("rootfs launch unsupported for {}", spec.display_name());
                return Box::pin(async move { Err(SandboxError::InvalidSpec { message }) });
            }
            SandboxRootSpec::OciImage(image) => match &image.source {
                SandboxOciImageSource::Reference(_) => {
                    self.image_starts.fetch_add(1, Ordering::SeqCst);
                }
                SandboxOciImageSource::Build(_) => {
                    self.build_starts.fetch_add(1, Ordering::SeqCst);
                }
            },
        }
        let handle = self.sandbox_handle(
            &spec.tenant_id,
            spec.display_name(),
            SandboxStatus::Starting,
        );
        self.handles
            .lock()
            .expect("backend lock should not be poisoned")
            .insert(handle.id.as_str().to_owned(), handle.clone());
        Box::pin(async move { Ok(handle) })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let inspect_call = self.inspect_calls.fetch_add(1, Ordering::SeqCst) + 1;
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
        Box::pin(async move { Ok(handle) })
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
        let stop_barrier = self.stop_barrier.clone();
        Box::pin(async move {
            if let Some(barrier) = stop_barrier {
                barrier.entered.add_permits(1);
                barrier
                    .release
                    .acquire()
                    .await
                    .expect("stop barrier should remain open")
                    .forget();
            }
            Ok(())
        })
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

fn sparse_build_spec(
    name: &str,
    image_name: impl Into<String>,
    dockerfile_path: impl Into<std::path::PathBuf>,
    context_path: impl Into<std::path::PathBuf>,
) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image(SandboxOciImageSource::Build(SandboxOciBuildSpec::new(
            image_name,
            dockerfile_path,
            context_path,
        ))),
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

fn image_service_backend(name: &str, image_reference: impl Into<String>) -> ServiceBackend {
    ServiceBackend::sandbox(sparse_image_spec_with_reference(name, image_reference))
}

fn build_service_backend(
    name: &str,
    image_name: impl Into<String>,
    dockerfile_path: impl Into<std::path::PathBuf>,
    context_path: impl Into<std::path::PathBuf>,
) -> ServiceBackend {
    ServiceBackend::sandbox(sparse_build_spec(
        name,
        image_name,
        dockerfile_path,
        context_path,
    ))
}
