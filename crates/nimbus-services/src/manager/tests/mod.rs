use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, TenantId};
use nimbus_egress::EgressPolicy;
use nimbus_network::{
    EndpointProtocol, NetworkResourceGeneration, PublishedEndpoint, PublishedEndpointHandle,
    PublishedEndpointId,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxFuture, SandboxHandle, SandboxId, SandboxInspection,
    SandboxMountSpec, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
    SandboxStatus,
};
use nimbus_workloads::{
    NodeIdentity, WorkloadDesiredDigest, WorkloadExecutionAttemptId, WorkloadExecutionId,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadRestartEpoch,
};
use sha2::{Digest, Sha256};

use crate::{
    ExternalAuthPolicy, HealthCheckPolicy, RuntimeServiceRegistry, ServiceBackend,
    ServiceDefinition, ServiceDefinitionCatalog, ServiceInstanceObservation, SessionLifecycleState,
    SessionTarget,
};
use nimbus_tenant::{TenantIsolationContext, TenantVolumePolicyDecision};

use super::*;

mod definition_lifecycle;
mod sandbox_resources;
mod sessions;
mod source_projection;
mod source_retirement;

pub(super) fn execution_reference_for_handle(
    handle: &mut SandboxHandle,
    generation: u64,
    restart_epoch: u64,
) -> WorkloadExecutionReference {
    let identity_seed = format!("{}\0{}", handle.tenant_id, handle.name);
    let workload_uid = format!("twu_{:x}", Sha256::digest(identity_seed.as_bytes()))
        .try_into()
        .expect("fixture workload uid should validate");
    let node_identity =
        NodeIdentity::new("services-test-node").expect("fixture node identity should validate");
    let generation = WorkloadGeneration::new(generation);
    let restart_epoch = WorkloadRestartEpoch::new(restart_epoch);
    let execution_id =
        WorkloadExecutionId::for_execution(&workload_uid, &node_identity, generation);
    let attempt_id = WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch);
    let desired_digest = WorkloadDesiredDigest::sha256(identity_seed);
    handle.id = SandboxId::new(execution_id.as_str());
    serde_json::from_value(serde_json::json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "restartEpoch": restart_epoch,
        "attemptId": attempt_id,
        "generation": generation,
        "desiredDigest": desired_digest,
    }))
    .expect("fixture execution reference should validate")
}

pub(super) fn endpoint_handles_for_handle(
    handle: &SandboxHandle,
    generation: u64,
) -> Vec<PublishedEndpointHandle> {
    let incarnation = format!(
        "nimbus.services.test-incarnation.v1:{}:{}",
        handle.tenant_id, handle.name
    );
    handle
        .published_endpoints
        .iter()
        .cloned()
        .map(|endpoint| {
            PublishedEndpointHandle::new(
                PublishedEndpointId::for_workload_endpoint(&incarnation, &endpoint.name),
                NetworkResourceGeneration::new(generation),
                endpoint,
            )
        })
        .collect()
}

pub(super) fn service_instance_observation(
    handle: SandboxHandle,
    published_endpoints: Vec<PublishedEndpointHandle>,
) -> ServiceInstanceObservation {
    ServiceInstanceObservation::new(handle, published_endpoints)
        .expect("fixture service instance observation should validate")
}

pub(super) struct StubServiceDefinitionCatalog {
    pub(super) launches: BTreeMap<String, ServiceBackend>,
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

pub(super) struct StubSandboxBackend {
    image_starts: AtomicUsize,
    stop_calls: AtomicUsize,
    artifact_cleanup_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
    egress_reloads: Mutex<Vec<(String, EgressPolicy)>>,
    ready_after_inspects: usize,
    handles: Mutex<BTreeMap<String, SandboxHandle>>,
    inspection_overrides: Mutex<BTreeMap<String, SandboxInspection>>,
}

impl StubSandboxBackend {
    pub(super) fn new(ready_after_inspects: usize) -> Self {
        Self {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            artifact_cleanup_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
            egress_reloads: Mutex::new(Vec::new()),
            ready_after_inspects,
            handles: Mutex::new(BTreeMap::new()),
            inspection_overrides: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn retirement_effect_counts(&self) -> (usize, usize, usize) {
        (
            self.inspect_calls.load(Ordering::SeqCst),
            self.stop_calls.load(Ordering::SeqCst),
            self.artifact_cleanup_calls.load(Ordering::SeqCst),
        )
    }

    pub(super) fn sandbox_handle(
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

pub(super) fn standalone_resource_spec(tenant_id: &TenantId, display_name: &str) -> SandboxSpec {
    SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named(display_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference("registry.example.com/task:latest"),
        SandboxProcessSpec::new(vec!["task".to_owned()]),
    )
}

pub(super) fn reserve_standalone_source(
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

pub(super) fn image_service_backend(
    name: &str,
    image_reference: impl Into<String>,
) -> ServiceBackend {
    ServiceBackend::sandbox(sparse_image_spec_with_reference(name, image_reference))
}
