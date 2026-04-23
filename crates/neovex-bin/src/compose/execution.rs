use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use neovex::{
    Error, SandboxBackend, SandboxBackendKind, SandboxServiceCatalog, SandboxServiceManager,
    TenantId,
};
use neovex_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxStateView};

use crate::machine::{
    ForwardedMachineApiSandboxBackend, MachineApiClient, ensure_default_machine_api_client_started,
    require_default_machine_api_client,
};

use super::{ComposeProjectContext, ServiceLifecycleTarget, file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServiceHostPlatform {
    Macos,
    Linux,
    Other,
}

impl ServiceHostPlatform {
    pub(super) fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

pub(super) enum ServiceExecutionSurface {
    Krun {
        state_view: KrunSandboxStateView,
        backend: Arc<dyn SandboxBackend>,
    },
    ForwardedContainer {
        client: MachineApiClient,
        backend: Arc<dyn SandboxBackend>,
    },
}

pub(super) fn load_host_backed_sandbox_service_manager_for_platform(
    file: &Path,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<SandboxServiceManager, Error> {
    let context = super::load_compose_project_context(file, control_data_dir)?;
    let machine_api_client = match machine_api_client {
        Some(client) => Some(client),
        None if should_auto_start_default_machine_for_host_loader(&context, host_platform)? => {
            Some(ensure_default_machine_api_client_started()?)
        }
        None => None,
    };
    let backend = load_host_backed_project_backend(&context, host_platform, machine_api_client)?;
    Ok(SandboxServiceManager::new(
        load_sandbox_service_catalog_for_execution_platform(file, host_platform)?,
        backend,
    ))
}

pub(super) fn should_auto_start_default_machine_for_host_loader(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
) -> Result<bool, Error> {
    if host_platform != ServiceHostPlatform::Macos {
        return Ok(false);
    }

    Ok(matches!(
        required_effective_project_backend(
            context,
            None,
            "load a compose-backed sandbox manager",
            host_platform,
        )?,
        SandboxBackendKind::Container
    ))
}

pub(super) fn render_state_lookup_error(operation: &str, error: neovex::SandboxError) -> Error {
    Error::Internal(format!("failed to {operation}: {error}"))
}

pub(super) fn lookup_current_remote_service_details(
    _context: &ComposeProjectContext,
    client: &MachineApiClient,
    tenant: &TenantId,
    service_name: &str,
    operation: &str,
) -> Result<Option<MachineApiServiceSandboxDetails>, Error> {
    client
        .inspect_current_service_sandbox(tenant, service_name)
        .map(|response| response.details)
        .map_err(|error| machine_api_operation_error(operation, client, error))
}

pub(super) fn resolve_remote_service_down_targets(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
    tenant: &TenantId,
    requested_service: Option<&str>,
) -> Result<Vec<ServiceLifecycleTarget>, Error> {
    match requested_service {
        Some(service_name) => {
            let details = lookup_current_remote_service_details(
                context,
                client,
                tenant,
                service_name,
                "resolve persisted sandbox state",
            )?
            .ok_or_else(|| {
                missing_persisted_service_error(
                    &context.control_plane.project_name,
                    tenant,
                    service_name,
                )
            })?;
            Ok(vec![ServiceLifecycleTarget {
                sandbox_id: details.summary.sandbox_id,
                service_name: details.summary.service_name,
                status: details.summary.status,
            }])
        }
        None => {
            let summaries = client
                .list_service_sandboxes(Some(tenant))
                .map_err(|error| {
                    machine_api_operation_error("list persisted sandbox state", client, error)
                })?;
            let service_names = summaries
                .into_iter()
                .map(|summary| summary.service_name)
                .collect::<BTreeSet<_>>();

            service_names
                .into_iter()
                .map(|service_name| {
                    lookup_current_remote_service_details(
                        context,
                        client,
                        tenant,
                        &service_name,
                        "resolve persisted sandbox state",
                    )?
                    .map(|details| ServiceLifecycleTarget {
                        sandbox_id: details.summary.sandbox_id,
                        service_name: details.summary.service_name,
                        status: details.summary.status,
                    })
                    .ok_or_else(|| {
                        Error::Internal(format!(
                            "persisted sandbox state changed while resolving service {} in tenant {} under project {}",
                            service_name, tenant, context.control_plane.project_name
                        ))
                    })
                })
                .collect()
        }
    }
}

pub(super) fn missing_persisted_service_error(
    project_name: &str,
    tenant: &TenantId,
    service_name: &str,
) -> Error {
    Error::InvalidInput(format!(
        "no persisted sandbox state found for service {} in tenant {} under project {}",
        service_name, tenant, project_name
    ))
}

pub(super) fn machine_api_operation_error(
    operation: &str,
    client: &MachineApiClient,
    error: Error,
) -> Error {
    Error::InvalidInput(format!(
        "failed to {operation} through default machine API at {}: {error}",
        client.socket_path().display()
    ))
}

pub(super) fn requested_service_names(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
) -> Result<Vec<String>, Error> {
    match requested_service {
        Some(service_name) => context
            .plan
            .services
            .contains_key(service_name)
            .then(|| vec![service_name.to_owned()])
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                ))
            }),
        None => Ok(context.plan.services.keys().cloned().collect()),
    }
}

pub(super) fn require_krun_backend_for_service_operation(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
) -> Result<(), Error> {
    let backend = required_project_backend(context, requested_service, operation)?;
    if backend == SandboxBackendKind::Krun {
        return Ok(());
    }

    let scope = match requested_service {
        Some(service_name) => format!(
            "service {} in compose project {}",
            service_name, context.control_plane.project_name
        ),
        None => format!("compose project {}", context.control_plane.project_name),
    };

    Err(Error::InvalidInput(format!(
        "{scope} selects sandbox backend {}, but neovex {} only supports the krun backend today",
        sandbox_backend_name(backend),
        operation,
    )))
}

pub(super) fn required_project_backend(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
) -> Result<SandboxBackendKind, Error> {
    match requested_service {
        Some(service_name) => context
            .plan
            .services
            .get(service_name)
            .map(|service| service.backend)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                ))
            }),
        None => {
            let mut services = context.plan.services.iter();
            let Some((_, first_service)) = services.next() else {
                return Err(Error::InvalidInput(format!(
                    "compose project {} does not declare any services",
                    context.control_plane.project_name
                )));
            };
            let first_backend = first_service.backend;
            if services.any(|(_, service)| service.backend != first_backend) {
                return Err(Error::InvalidInput(format!(
                    "compose project {} mixes sandbox backends across services ({}); neovex {} currently requires one backend family per project-wide operation",
                    context.control_plane.project_name,
                    project_backend_assignments(context),
                    operation,
                )));
            }
            Ok(first_backend)
        }
    }
}

pub(super) fn load_sandbox_service_catalog_for_execution_platform(
    file: &Path,
    host_platform: ServiceHostPlatform,
) -> Result<Arc<dyn SandboxServiceCatalog>, Error> {
    let mut plan = file::ComposeProjectPlan::load(file)?;
    apply_platform_backend_defaults(&mut plan, host_platform);
    Ok(Arc::new(plan.into_service_catalog()?))
}

fn apply_platform_backend_defaults(
    plan: &mut file::ComposeProjectPlan,
    host_platform: ServiceHostPlatform,
) {
    if host_platform != ServiceHostPlatform::Macos {
        return;
    }

    for service in plan.services.values_mut() {
        if service.backend == SandboxBackendKind::Krun && !service_declares_backend(service) {
            service.backend = SandboxBackendKind::Container;
        }
    }
}

fn effective_service_backend(
    service: &file::ComposeServicePlan,
    host_platform: ServiceHostPlatform,
) -> SandboxBackendKind {
    if host_platform == ServiceHostPlatform::Macos
        && service.backend == SandboxBackendKind::Krun
        && !service_declares_backend(service)
    {
        SandboxBackendKind::Container
    } else {
        service.backend
    }
}

fn service_declares_backend(service: &file::ComposeServicePlan) -> bool {
    service
        .x_neovex
        .as_ref()
        .and_then(|extensions| extensions.backend)
        .is_some()
}

fn required_effective_project_backend(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
    host_platform: ServiceHostPlatform,
) -> Result<SandboxBackendKind, Error> {
    match requested_service {
        Some(service_name) => context
            .plan
            .services
            .get(service_name)
            .map(|service| effective_service_backend(service, host_platform))
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                ))
            }),
        None => {
            let mut services = context.plan.services.iter();
            let Some((_, first_service)) = services.next() else {
                return Err(Error::InvalidInput(format!(
                    "compose project {} does not declare any services",
                    context.control_plane.project_name
                )));
            };
            let first_backend = effective_service_backend(first_service, host_platform);
            if services.any(|(_, service)| {
                effective_service_backend(service, host_platform) != first_backend
            }) {
                return Err(Error::InvalidInput(format!(
                    "compose project {} mixes sandbox backends across services ({}); neovex {} currently requires one backend family per project-wide operation",
                    context.control_plane.project_name,
                    effective_project_backend_assignments(context, host_platform),
                    operation,
                )));
            }
            Ok(first_backend)
        }
    }
}

fn effective_project_backend_assignments(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
) -> String {
    context
        .plan
        .services
        .iter()
        .map(|(service_name, service)| {
            format!(
                "{service_name}={}",
                sandbox_backend_name(effective_service_backend(service, host_platform))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn load_host_backed_project_backend(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Arc<dyn SandboxBackend>, Error> {
    let backend = required_effective_project_backend(
        context,
        None,
        "load a compose-backed sandbox manager",
        host_platform,
    )?;
    match backend {
        SandboxBackendKind::Krun => Ok(Arc::new(KrunSandboxBackend::new(
            context.control_plane.krun_backend_config(),
        ))),
        SandboxBackendKind::Container => {
            load_forwarded_machine_api_backend(context, host_platform, machine_api_client)
        }
    }
}

pub(super) fn load_forwarded_machine_api_backend(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Arc<dyn SandboxBackend>, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => {
            let client = match machine_api_client {
                Some(client) => client,
                None => require_default_machine_api_client()?,
            };
            validate_forwarded_machine_api_backend(context, &client)?;
            Ok(Arc::new(ForwardedMachineApiSandboxBackend::new(client)))
        }
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex load a compose-backed sandbox manager only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex load a compose-backed sandbox manager does not support the current host platform for forwarded guest execution",
            context.control_plane.project_name
        ))),
    }
}

pub(super) fn validate_forwarded_machine_api_backend(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
) -> Result<(), Error> {
    let capabilities = client.capabilities().map_err(|error| {
        Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not reachable: {error}",
            context.control_plane.project_name,
            client.socket_path().display()
        ))
    })?;
    if !capabilities
        .supported_service_backends
        .contains(&SandboxBackendKind::Container)
    {
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} does not advertise container backend support",
            context.control_plane.project_name,
            client.socket_path().display()
        )));
    }
    if !capabilities.service_execution_ready {
        let blockers = if capabilities.service_execution_blockers.is_empty() {
            "guest machine API did not report readiness blockers".to_owned()
        } else {
            capabilities.service_execution_blockers.join("; ")
        };
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not ready for container-backed service execution: {}",
            context.control_plane.project_name,
            client.socket_path().display(),
            blockers,
        )));
    }
    Ok(())
}

pub(super) fn sandbox_backend_name(backend: SandboxBackendKind) -> &'static str {
    match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

fn project_backend_assignments(context: &ComposeProjectContext) -> String {
    context
        .plan
        .services
        .iter()
        .map(|(service_name, service)| {
            format!("{service_name}={}", sandbox_backend_name(service.backend))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn resolve_service_execution_surface(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<ServiceExecutionSurface, Error> {
    let backend =
        required_effective_project_backend(context, requested_service, operation, host_platform)?;
    match backend {
        SandboxBackendKind::Krun => Ok(ServiceExecutionSurface::Krun {
            state_view: KrunSandboxStateView::from_config(
                &context.control_plane.krun_backend_config(),
            ),
            backend: Arc::new(KrunSandboxBackend::new(
                context.control_plane.krun_backend_config(),
            )),
        }),
        SandboxBackendKind::Container => {
            let client = resolve_forwarded_machine_api_client(
                context,
                host_platform,
                machine_api_client,
                operation,
            )?;
            let backend: Arc<dyn SandboxBackend> =
                Arc::new(ForwardedMachineApiSandboxBackend::new(client.clone()));
            Ok(ServiceExecutionSurface::ForwardedContainer { client, backend })
        }
    }
}

fn resolve_forwarded_machine_api_client(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    operation: &str,
) -> Result<MachineApiClient, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => match machine_api_client {
            Some(client) => Ok(client),
            None => require_default_machine_api_client(),
        },
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex {} only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name, operation,
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex {} does not support the current host platform for forwarded guest execution",
            context.control_plane.project_name, operation,
        ))),
    }
}

pub(super) fn validate_forwarded_machine_api_operations(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
    operation: &str,
    required_operations: &[&str],
) -> Result<(), Error> {
    let capabilities = client.capabilities().map_err(|error| {
        Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not reachable: {error}",
            context.control_plane.project_name,
            client.socket_path().display()
        ))
    })?;
    if !capabilities
        .supported_service_backends
        .contains(&SandboxBackendKind::Container)
    {
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} does not advertise container backend support",
            context.control_plane.project_name,
            client.socket_path().display()
        )));
    }

    let missing = required_operations
        .iter()
        .copied()
        .filter(|required_operation| {
            !capabilities
                .supported_operations
                .iter()
                .any(|advertised| advertised == required_operation)
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let operation_blockers = capabilities.blockers_for_operations(missing.iter().copied());
    let blockers = if !operation_blockers.is_empty() {
        operation_blockers.join("; ")
    } else if capabilities.service_execution_blockers.is_empty() {
        "guest machine API did not report readiness blockers".to_owned()
    } else {
        capabilities.service_execution_blockers.join("; ")
    };
    Err(Error::InvalidInput(format!(
        "compose project {} selects sandbox backend container, but neovex {} requires guest machine API operations [{}] that are not available at {}: {}",
        context.control_plane.project_name,
        operation,
        missing.join(", "),
        client.socket_path().display(),
        blockers,
    )))
}

pub(super) fn resolve_service_down_targets(
    state_view: &KrunSandboxStateView,
    tenant: &TenantId,
    requested_service: Option<&str>,
    project_name: &str,
) -> Result<Vec<ServiceLifecycleTarget>, Error> {
    match requested_service {
        Some(service_name) => {
            let details = state_view
                .inspect_service(tenant, service_name)
                .map_err(|error| render_state_lookup_error("resolve persisted sandbox state", error))?
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "no persisted sandbox state found for service {} in tenant {} under project {}",
                        service_name, tenant, project_name
                    ))
                })?;
            Ok(vec![ServiceLifecycleTarget::from_details(details)])
        }
        None => {
            let service_names = state_view
                .list_for_tenant(tenant)
                .map_err(|error| render_state_lookup_error("list persisted sandbox state", error))?
                .into_iter()
                .map(|summary| summary.service_name)
                .collect::<BTreeSet<_>>();

            service_names
                .into_iter()
                .map(|service_name| {
                    state_view
                        .inspect_service(tenant, &service_name)
                        .map_err(|error| {
                            render_state_lookup_error("resolve persisted sandbox state", error)
                        })?
                        .map(ServiceLifecycleTarget::from_details)
                        .ok_or_else(|| {
                            Error::Internal(format!(
                                "persisted sandbox state changed while resolving service {} in tenant {} under project {}",
                                service_name, tenant, project_name
                            ))
                        })
                })
                .collect()
        }
    }
}

type MachineApiServiceSandboxDetails = crate::machine::MachineApiServiceSandboxDetails;
