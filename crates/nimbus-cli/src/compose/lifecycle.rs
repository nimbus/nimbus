use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use nimbus::{Engine, Error, SandboxBackend, SandboxHandle, SandboxStatus, TenantId};
use nimbus_compute::{
    ComputeResourceProvisioner, SandboxServiceProvisionSnapshot, WorkloadProvisionCancellation,
};
use nimbus_server::{EngineWorkloadSagaStore, ServerForegroundWorkloadRuntime};
use nimbus_services::{ServiceDefinition, ServiceDefinitionObservation};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::WorkloadSagaStore;
use serde::Serialize;

use crate::compose::discovery::ResolvedComposeSelection;
use crate::machine::{HostMachineNetworkAuthority, MachineApiClient};

use super::provision::PreparedComposeProvision;
use super::{
    ComposeDownCommand, ComposeUpCommand, LocalKrunExecutionSurface, requested_service_names,
    resolve_remote_service_down_targets, resolve_service_down_targets,
    resolve_service_execution_surface, validate_forwarded_machine_api_operations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ServiceLifecycleAction {
    Started,
    AlreadyRunning,
    Stopped,
    AlreadyStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ServiceLifecycleOutcome {
    pub(super) action: ServiceLifecycleAction,
    pub(super) tenant_id: TenantId,
    pub(super) service_name: String,
    pub(super) sandbox_id: nimbus::SandboxId,
    pub(super) status: SandboxStatus,
}

impl ServiceLifecycleOutcome {
    pub(super) fn from_handle(
        action: ServiceLifecycleAction,
        tenant_id: &TenantId,
        service_name: &str,
        handle: SandboxHandle,
    ) -> Self {
        Self {
            action,
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
            sandbox_id: handle.id,
            status: handle.status,
        }
    }
}

impl ServiceLifecycleAction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::AlreadyRunning => "already_running",
            Self::Stopped => "stopped",
            Self::AlreadyStopped => "already_stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ServiceLifecycleTarget {
    pub(super) sandbox_id: nimbus::SandboxId,
    pub(super) service_name: String,
    pub(super) status: SandboxStatus,
}

impl ServiceLifecycleTarget {
    pub(super) fn from_details(
        details: nimbus_sandbox::backends::krun::KrunSandboxDetails,
    ) -> Self {
        Self {
            sandbox_id: details.summary.sandbox_id,
            service_name: details.summary.service_name,
            status: details.summary.status,
        }
    }
}

/// Foreground owner for one exact Compose workload realm.
pub(super) struct ComposeForegroundOwner {
    runtime: ServerForegroundWorkloadRuntime,
    cancellation: WorkloadProvisionCancellation,
}

impl ComposeForegroundOwner {
    pub(super) fn open(
        engine: Arc<Engine>,
        prepared: PreparedComposeProvision,
    ) -> Result<Self, Error> {
        let saga_store: Arc<dyn WorkloadSagaStore> =
            Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
        let runtime = prepared.activate(engine, saga_store)?;
        Ok(Self {
            runtime,
            cancellation: WorkloadProvisionCancellation::default(),
        })
    }

    pub(super) fn cancellation(&self) -> WorkloadProvisionCancellation {
        self.cancellation.clone()
    }

    /// Settle process-bound ownership before the foreground command returns.
    pub(super) async fn shutdown(self, engine: &Engine) {
        self.cancellation.cancel();
        drop(self);
        engine.quiesce().await;
    }

    #[cfg(test)]
    pub(super) fn open_composition_for_test(
        engine: Arc<Engine>,
        composition: nimbus_server::ServerWorkloadComposition,
    ) -> Self {
        let saga_store: Arc<dyn WorkloadSagaStore> =
            Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
        Self {
            runtime: composition.into_foreground_runtime(saga_store),
            cancellation: WorkloadProvisionCancellation::default(),
        }
    }
}

pub(super) type ComposeProvisionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SandboxServiceProvisionSnapshot, Error>> + Send + 'a>>;

pub(super) trait ComposeServiceProvision {
    fn definition(&self, tenant_id: &TenantId, service_name: &str) -> Option<ServiceDefinition>;

    fn observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinitionObservation>;

    fn provision<'a>(
        &'a self,
        context: &'a TenantIsolationContext,
        service_name: &'a str,
    ) -> ComposeProvisionFuture<'a>;
}

impl ComposeServiceProvision for ComposeForegroundOwner {
    fn definition(&self, tenant_id: &TenantId, service_name: &str) -> Option<ServiceDefinition> {
        self.runtime
            .service_manager()
            .service_definition_for_tenant(tenant_id, service_name)
    }

    fn observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinitionObservation> {
        self.runtime
            .service_manager()
            .service_definition_observation_for_tenant(tenant_id, service_name)
    }

    fn provision<'a>(
        &'a self,
        context: &'a TenantIsolationContext,
        service_name: &'a str,
    ) -> ComposeProvisionFuture<'a> {
        let resource_provisioner: &'a ComputeResourceProvisioner =
            self.runtime.resource_provisioner();
        Box::pin(async move {
            resource_provisioner
                .provision_sandbox_service(context, service_name, &self.cancellation)
                .await
                .map_err(|error| Error::Internal(error.to_string()))
        })
    }
}

pub(super) async fn service_up_outcomes_for_selection(
    command: &ComposeUpCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    provision: &impl ComposeServiceProvision,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let service_names = requested_service_names(&context, command.service.as_deref())?;
    let tenant_context = TenantIsolationContext::system(tenant.clone(), "compose-up");
    let mut outcomes = Vec::with_capacity(service_names.len());
    for service_name in service_names {
        outcomes.push(provision_compose_service(provision, &tenant_context, &service_name).await?);
    }
    Ok(outcomes)
}

pub(super) async fn provision_compose_service(
    provision: &impl ComposeServiceProvision,
    context: &TenantIsolationContext,
    service_name: &str,
) -> Result<ServiceLifecycleOutcome, Error> {
    let tenant_id = context.tenant_id();
    let definition = provision
        .definition(tenant_id, service_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "service {service_name} is not declared for tenant {tenant_id}"
            ))
        })?;
    if let Some(observation) = provision.observation(tenant_id, service_name)
        && observation.observed_generation == definition.generation
    {
        validate_service_observation(&definition, &observation)?;
        if matches!(
            observation.handle.status,
            SandboxStatus::Starting | SandboxStatus::Ready | SandboxStatus::NotReady
        ) {
            return Ok(ServiceLifecycleOutcome::from_handle(
                ServiceLifecycleAction::AlreadyRunning,
                tenant_id,
                service_name,
                observation.handle,
            ));
        }
        return Err(Error::PreconditionFailed(format!(
            "compose service {service_name} for tenant {tenant_id} generation {} retains terminal or stopping status {:?}; restart is not a provision action",
            definition.generation, observation.handle.status
        )));
    }

    let snapshot = provision.provision(context, service_name).await?;
    if snapshot.definition != definition {
        return Err(Error::PreconditionFailed(format!(
            "compose service {service_name} for tenant {tenant_id} changed while provisioning"
        )));
    }
    let observation = snapshot.observation.ok_or_else(|| {
        Error::PreconditionFailed(format!(
            "compose service {service_name} for tenant {tenant_id} was accepted but has no exact observed projection"
        ))
    })?;
    validate_service_observation(&definition, &observation)?;
    Ok(ServiceLifecycleOutcome::from_handle(
        ServiceLifecycleAction::Started,
        tenant_id,
        service_name,
        observation.handle,
    ))
}

fn validate_service_observation(
    definition: &ServiceDefinition,
    observation: &ServiceDefinitionObservation,
) -> Result<(), Error> {
    let Some(spec) = definition.backend.sandbox_spec() else {
        return Err(Error::InvalidInput(format!(
            "compose service {} for tenant {} is not sandbox-backed",
            definition.name, definition.tenant_id
        )));
    };
    if observation.tenant_id != definition.tenant_id
        || observation.name != definition.name
        || observation.observed_generation != definition.generation
        || observation.handle.tenant_id != definition.tenant_id
        || observation.handle.name != definition.name
        || observation.handle.backend != spec.backend
    {
        return Err(Error::PreconditionFailed(format!(
            "compose service {} for tenant {} rejected crossed observed projection",
            definition.name, definition.tenant_id
        )));
    }
    Ok(())
}

pub(super) async fn service_down_outcomes_for_selection(
    command: &ComposeDownCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: super::ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    local_krun: Option<LocalKrunExecutionSurface>,
    network: Option<&HostMachineNetworkAuthority>,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());

    match resolve_service_execution_surface(
        &context,
        command.service.as_deref(),
        "compose down",
        host_platform,
        machine_api_client,
        local_krun,
        network,
    )? {
        super::ServiceExecutionSurface::Krun {
            state_view,
            backend,
        } => {
            let targets = resolve_service_down_targets(
                &state_view,
                &tenant,
                command.service.as_deref(),
                &context.control_plane.project_name,
            )?;
            let mut outcomes = Vec::new();
            for target in targets {
                outcomes.push(stop_service_target(backend.as_ref(), &tenant, target).await?);
            }
            Ok(outcomes)
        }
        super::ServiceExecutionSurface::ForwardedContainer { client, backend } => {
            let required_operations = if command.service.is_some() {
                vec![
                    "service-sandboxes.inspect-current",
                    "service-sandboxes.stop",
                ]
            } else {
                vec![
                    "service-sandboxes.list",
                    "service-sandboxes.inspect-current",
                    "service-sandboxes.stop",
                ]
            };
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "compose down",
                &required_operations,
            )?;
            let targets = resolve_remote_service_down_targets(
                &context,
                &client,
                &tenant,
                command.service.as_deref(),
            )?;
            let mut outcomes = Vec::new();
            for target in targets {
                outcomes.push(stop_service_target(backend.as_ref(), &tenant, target).await?);
            }
            Ok(outcomes)
        }
    }
}

pub(super) async fn stop_service_target(
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    target: ServiceLifecycleTarget,
) -> Result<ServiceLifecycleOutcome, Error> {
    let refreshed = backend
        .inspect(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, &target.service_name, error))?;
    if let Some(inspection) = refreshed.as_ref() {
        validate_compose_inspection_identity(
            backend,
            tenant,
            &target.service_name,
            &target.sandbox_id,
            inspection,
        )?;
    }

    if refreshed.as_ref().is_none_or(|inspection| {
        !is_active_status(inspection.handle.status)
            && inspection.cleanup == nimbus_sandbox::SandboxCleanupObservation::Finalized
    }) {
        let status = refreshed
            .map(|inspection| inspection.handle.status)
            .unwrap_or(target.status);
        return Ok(ServiceLifecycleOutcome {
            action: ServiceLifecycleAction::AlreadyStopped,
            tenant_id: tenant.clone(),
            service_name: target.service_name,
            sandbox_id: target.sandbox_id,
            status,
        });
    }

    backend
        .stop(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("stop", tenant, &target.service_name, error))?;
    let stopped_inspection = backend
        .inspect(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, &target.service_name, error))?;
    if let Some(inspection) = stopped_inspection.as_ref() {
        validate_compose_inspection_identity(
            backend,
            tenant,
            &target.service_name,
            &target.sandbox_id,
            inspection,
        )?;
    }
    let status = stopped_inspection
        .map(|inspection| inspection.handle.status)
        .unwrap_or(SandboxStatus::Stopped);

    Ok(ServiceLifecycleOutcome {
        action: ServiceLifecycleAction::Stopped,
        tenant_id: tenant.clone(),
        service_name: target.service_name,
        sandbox_id: target.sandbox_id,
        status,
    })
}

fn validate_compose_inspection_identity(
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    service_name: &str,
    sandbox_id: &nimbus::SandboxId,
    inspection: &nimbus_sandbox::SandboxInspection,
) -> Result<(), Error> {
    let observed = &inspection.handle;
    if observed.id == *sandbox_id
        && observed.tenant_id == *tenant
        && observed.name == service_name
        && observed.backend == backend.kind()
    {
        return Ok(());
    }

    Err(Error::Internal(format!(
        "sandbox backend returned crossed inspection identity for compose service {service_name} \
         tenant {tenant}: expected sandbox {sandbox_id} tenant {tenant} name {service_name} \
         backend {:?}, observed sandbox {} tenant {} name {} backend {:?}",
        backend.kind(),
        observed.id,
        observed.tenant_id,
        observed.name,
        observed.backend
    )))
}

fn backend_operation_error(
    operation: &str,
    tenant: &TenantId,
    service_name: &str,
    error: nimbus::SandboxError,
) -> Error {
    Error::Internal(format!(
        "failed to {operation} service {} for tenant {}: {error}",
        service_name, tenant
    ))
}

fn is_active_status(status: SandboxStatus) -> bool {
    matches!(
        status,
        SandboxStatus::Starting
            | SandboxStatus::Ready
            | SandboxStatus::NotReady
            | SandboxStatus::Stopping
    )
}
