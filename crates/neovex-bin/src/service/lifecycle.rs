use std::path::Path;

use neovex::{Error, SandboxBackend, SandboxHandle, SandboxServiceLaunch, SandboxStatus, TenantId};

use crate::machine::MachineApiClient;

use super::{
    ServiceDownCommand, ServiceLifecycleAction, ServiceLifecycleOutcome, ServiceLifecycleTarget,
    ServiceUpCommand, load_compose_project_context,
    load_sandbox_service_catalog_for_execution_platform, lookup_current_remote_service_details,
    requested_service_names, resolve_remote_service_down_targets, resolve_service_down_targets,
    resolve_service_execution_surface, validate_forwarded_machine_api_backend,
    validate_forwarded_machine_api_operations,
};

pub(super) async fn service_up_outcomes_for_platform(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
    host_platform: super::ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let service_names = requested_service_names(&context, command.service.as_deref())?;
    let service_catalog =
        load_sandbox_service_catalog_for_execution_platform(&command.file, host_platform)?;

    match resolve_service_execution_surface(
        &context,
        command.service.as_deref(),
        "service up",
        host_platform,
        machine_api_client,
    )? {
        super::ServiceExecutionSurface::Krun {
            state_view,
            backend,
        } => {
            let mut outcomes = Vec::new();
            for service_name in service_names {
                if let Some(handle) = resolve_live_service_handle(
                    &state_view,
                    backend.as_ref(),
                    &tenant,
                    &service_name,
                )
                .await?
                {
                    outcomes.push(ServiceLifecycleOutcome::from_handle(
                        ServiceLifecycleAction::AlreadyRunning,
                        &tenant,
                        &service_name,
                        handle,
                    ));
                    continue;
                }

                let launch = service_catalog
                    .sandbox_service_for_tenant(&tenant, &service_name)
                    .ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "service {} is not declared in compose project {}",
                            service_name, context.control_plane.project_name
                        ))
                    })?;
                let handle =
                    start_service_launch(backend.as_ref(), &tenant, &service_name, launch).await?;
                outcomes.push(ServiceLifecycleOutcome::from_handle(
                    ServiceLifecycleAction::Started,
                    &tenant,
                    &service_name,
                    handle,
                ));
            }
            Ok(outcomes)
        }
        super::ServiceExecutionSurface::ForwardedContainer { client, backend } => {
            validate_forwarded_machine_api_backend(&context, &client)?;
            let mut outcomes = Vec::new();
            for service_name in service_names {
                if let Some(details) = lookup_current_remote_service_details(
                    &context,
                    &client,
                    &tenant,
                    &service_name,
                    "resolve persisted sandbox state",
                )? && is_active_status(details.summary.status)
                {
                    outcomes.push(ServiceLifecycleOutcome {
                        action: ServiceLifecycleAction::AlreadyRunning,
                        tenant_id: details.summary.tenant_id,
                        service_name: details.summary.service_name,
                        sandbox_id: details.summary.sandbox_id,
                        status: details.summary.status,
                    });
                    continue;
                }

                let launch = service_catalog
                    .sandbox_service_for_tenant(&tenant, &service_name)
                    .ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "service {} is not declared in compose project {}",
                            service_name, context.control_plane.project_name
                        ))
                    })?;
                let handle =
                    start_service_launch(backend.as_ref(), &tenant, &service_name, launch).await?;
                outcomes.push(ServiceLifecycleOutcome::from_handle(
                    ServiceLifecycleAction::Started,
                    &tenant,
                    &service_name,
                    handle,
                ));
            }
            Ok(outcomes)
        }
    }
}

pub(super) async fn service_down_outcomes_for_platform(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
    host_platform: super::ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());

    match resolve_service_execution_surface(
        &context,
        command.service.as_deref(),
        "service down",
        host_platform,
        machine_api_client,
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
                "service down",
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

pub(super) async fn start_service_launch(
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    service_name: &str,
    launch: SandboxServiceLaunch,
) -> Result<SandboxHandle, Error> {
    if launch.spec().name != service_name {
        return Err(Error::InvalidInput(format!(
            "sandbox service catalog returned launch spec name {} for requested service {}",
            launch.spec().name,
            service_name
        )));
    }
    if &launch.spec().tenant_id != tenant {
        return Err(Error::InvalidInput(format!(
            "sandbox service catalog returned tenant {} for requested tenant {}",
            launch.spec().tenant_id,
            tenant
        )));
    }
    if launch.spec().backend != backend.kind() {
        return Err(Error::InvalidInput(format!(
            "sandbox service {} for tenant {} requested backend {:?}, but the configured backend is {:?}",
            service_name,
            tenant,
            launch.spec().backend,
            backend.kind()
        )));
    }

    match launch {
        SandboxServiceLaunch::Image(launch) => backend
            .start_from_image(launch)
            .await
            .map_err(|error| backend_operation_error("start", tenant, service_name, error)),
        SandboxServiceLaunch::Build(launch) => backend
            .start_from_build(launch)
            .await
            .map_err(|error| backend_operation_error("start", tenant, service_name, error)),
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

    if refreshed
        .as_ref()
        .is_none_or(|handle| !is_active_status(handle.status))
    {
        let status = refreshed
            .map(|handle| handle.status)
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
    let status = backend
        .inspect(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, &target.service_name, error))?
        .map(|handle| handle.status)
        .unwrap_or(SandboxStatus::Stopped);

    Ok(ServiceLifecycleOutcome {
        action: ServiceLifecycleAction::Stopped,
        tenant_id: tenant.clone(),
        service_name: target.service_name,
        sandbox_id: target.sandbox_id,
        status,
    })
}

async fn resolve_live_service_handle(
    state_view: &super::KrunSandboxStateView,
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    service_name: &str,
) -> Result<Option<SandboxHandle>, Error> {
    let Some(details) = state_view
        .inspect_service(tenant, service_name)
        .map_err(|error| {
            super::render_state_lookup_error("resolve persisted sandbox state", error)
        })?
    else {
        return Ok(None);
    };

    let refreshed = backend
        .inspect(&details.summary.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, service_name, error))?;

    Ok(refreshed.filter(|handle| is_active_status(handle.status)))
}

fn backend_operation_error(
    operation: &str,
    tenant: &TenantId,
    service_name: &str,
    error: neovex::SandboxError,
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
