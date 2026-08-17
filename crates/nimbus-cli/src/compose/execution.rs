use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use nimbus::SandboxBackend;
use nimbus::{
    Error, LocalBuildAdmission, SandboxBackendKind, ServiceDefinitionCatalog, ServiceManager,
    TenantId,
};
use nimbus_network::NetworkAttachmentProviderRegistration;
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxStateView};

use crate::compose::discovery::ResolvedComposeSelection;
#[cfg(test)]
use crate::machine::ForwardedMachineApiSandboxBackend;
#[cfg(test)]
use crate::machine::HostMachineNetworkComposition;
#[cfg(test)]
use crate::machine::ensure_default_machine_api_client_started;
use crate::machine::{
    HostMachineNetworkAuthority, MachineApiClient, require_default_machine_api_client,
};
use crate::network_composition::StagedLocalNetworkComposition;

use super::{ComposeProjectContext, file};

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
    Krun { state_view: KrunSandboxStateView },
    ForwardedContainer { client: MachineApiClient },
}

#[derive(Clone)]
pub(crate) struct LocalKrunExecutionSurface {
    pub(crate) state_view: KrunSandboxStateView,
}

/// A local service manager prepared under the staged OS-node authority.
///
/// The concrete backend remains retained by `ServiceManager`; its exact
/// source-owned attachment report is kept separately until the CLI freezes
/// the complete local capability registry.
pub(crate) struct PreparedLocalServiceManager {
    pub(crate) manager: ServiceManager,
    pub(crate) attachment: NetworkAttachmentProviderRegistration,
    pub(crate) backend: Arc<KrunSandboxBackend>,
    pub(crate) state_view: KrunSandboxStateView,
}

/// Prepare a host-managed local backend without performing machine or socket
/// effects.
///
/// `None` means this selection is not a host-managed local composition (for
/// example the macOS forwarded-machine path). The caller freezes an honest
/// empty local registry before constructing that separate provider realm.
pub(crate) fn prepare_local_service_manager_for_selection_with_isolation_mode(
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
    staged_network: &mut StagedLocalNetworkComposition,
) -> Result<Option<PreparedLocalServiceManager>, Error> {
    let host_platform = ServiceHostPlatform::current();
    let admission_mode = match tenant_isolation_mode {
        nimbus_tenant::TenantIsolationMode::LocalDevelopment => {
            file::ComposeAdmissionMode::LocalDevelopment
        }
        nimbus_tenant::TenantIsolationMode::Production => file::ComposeAdmissionMode::Production,
    };
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)?;
    let backend_kind = required_effective_project_backend(
        &context,
        None,
        "prepare a compose-backed local network composition",
        host_platform,
    )?;
    if backend_kind != SandboxBackendKind::Krun {
        return Ok(None);
    }

    let catalog = load_service_definition_catalog_for_execution_platform_with_admission(
        selection,
        host_platform,
        admission_mode,
    )?;
    let config = context
        .control_plane
        .krun_backend_config_with_network_authority(staged_network.authority().state_root());
    let process = staged_network
        .prepare_krun_process(&config)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let state_view = KrunSandboxStateView::from_config(&config);
    let backend = Arc::new(
        KrunSandboxBackend::with_network_process(config, process)
            .map_err(|error| Error::InvalidInput(error.to_string()))?,
    );
    let attachment = backend
        .host_managed_attachment_registration()
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let local_build_admission = match admission_mode {
        file::ComposeAdmissionMode::LocalDevelopment => LocalBuildAdmission::Allowed,
        file::ComposeAdmissionMode::Production => LocalBuildAdmission::Denied,
    };
    Ok(Some(PreparedLocalServiceManager {
        manager: ServiceManager::new(catalog, backend_kind)
            .with_local_build_admission(local_build_admission),
        attachment,
        backend,
        state_view,
    }))
}

#[cfg(test)]
pub(super) fn load_host_backed_service_manager_for_platform(
    file: &Path,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<ServiceManager, Error> {
    load_host_backed_service_manager_for_platform_selection_with_admission(
        &ResolvedComposeSelection::explicit(file.to_path_buf()),
        control_data_dir,
        host_platform,
        machine_api_client,
        file::ComposeAdmissionMode::LocalDevelopment,
    )
}

#[cfg(test)]
pub(super) fn load_host_backed_service_manager_for_platform_selection_with_admission(
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    admission_mode: file::ComposeAdmissionMode,
) -> Result<ServiceManager, Error> {
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)?;
    let catalog = load_service_definition_catalog_for_execution_platform_with_admission(
        selection,
        host_platform,
        admission_mode,
    )?;
    let machine_api_client = match machine_api_client {
        Some(client) => Some(client),
        None if should_auto_start_default_machine_for_host_loader(&context, host_platform)? => {
            let network = HostMachineNetworkComposition::claim_default()?;
            Some(ensure_default_machine_api_client_started(
                &network.authority(),
            )?)
        }
        None => None,
    };
    let backend = load_host_backed_project_backend(&context, host_platform, machine_api_client)?;
    let local_build_admission = match admission_mode {
        file::ComposeAdmissionMode::LocalDevelopment => LocalBuildAdmission::Allowed,
        file::ComposeAdmissionMode::Production => LocalBuildAdmission::Denied,
    };
    Ok(ServiceManager::new(catalog, backend.kind())
        .with_local_build_admission(local_build_admission))
}

#[cfg(test)]
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

pub(super) fn render_state_lookup_error(operation: &str, error: nimbus::SandboxError) -> Error {
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
        "{scope} selects sandbox backend {}, but nimbus {} only supports the krun backend today",
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
                    "compose project {} mixes sandbox backends across services ({}); nimbus {} currently requires one backend family per project-wide operation",
                    context.control_plane.project_name,
                    project_backend_assignments(context),
                    operation,
                )));
            }
            Ok(first_backend)
        }
    }
}

pub(super) fn load_service_definition_catalog_for_execution_platform_with_admission(
    selection: &ResolvedComposeSelection,
    host_platform: ServiceHostPlatform,
    admission_mode: file::ComposeAdmissionMode,
) -> Result<Arc<dyn ServiceDefinitionCatalog>, Error> {
    let mut plan =
        file::ComposeProjectPlan::load_selection_with_admission(selection, admission_mode)?;
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
        .x_nimbus
        .as_ref()
        .and_then(|extensions| extensions.backend)
        .is_some()
}

pub(super) fn required_effective_project_backend(
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
                    "compose project {} mixes sandbox backends across services ({}); nimbus {} currently requires one backend family per project-wide operation",
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

#[cfg(test)]
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
            context
                .control_plane
                .reconstruct_direct_krun_backend_config(),
        ))),
        SandboxBackendKind::Container => {
            load_forwarded_machine_api_backend(context, host_platform, machine_api_client, None)
        }
    }
}

#[cfg(test)]
pub(super) fn load_forwarded_machine_api_backend(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    network: Option<&HostMachineNetworkAuthority>,
) -> Result<Arc<dyn SandboxBackend>, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => {
            let client = match machine_api_client {
                Some(client) => client,
                None => require_default_machine_api_client(network.ok_or_else(|| {
                    Error::Internal(
                        "forwarded machine backend requires the retained parent network authority"
                            .to_owned(),
                    )
                })?)?,
            };
            validate_forwarded_machine_api_backend(context, &client)?;
            let backend = match network {
                Some(network) => ForwardedMachineApiSandboxBackend::new(client, network)?,
                #[cfg(test)]
                None => ForwardedMachineApiSandboxBackend::new_for_test(
                    client,
                    nimbus_network::LocalPortLeaseAuthority::open(
                        context
                            .control_plane
                            .project_root
                            .join("test-parent-machine-network"),
                    )
                    .map_err(|error| {
                        Error::Internal(format!(
                            "failed to open isolated test parent publication authority: {error}"
                        ))
                    })?,
                )?,
                #[cfg(not(test))]
                None => {
                    return Err(Error::Internal(
                        "forwarded machine backend requires the retained parent network authority"
                            .to_owned(),
                    ));
                }
            };
            Ok(Arc::new(backend))
        }
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but nimbus load a compose-backed sandbox manager only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but nimbus load a compose-backed sandbox manager does not support the current host platform for forwarded guest execution",
            context.control_plane.project_name
        ))),
    }
}

#[cfg(test)]
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
    local_krun: Option<LocalKrunExecutionSurface>,
    network: Option<&HostMachineNetworkAuthority>,
) -> Result<ServiceExecutionSurface, Error> {
    let backend =
        required_effective_project_backend(context, requested_service, operation, host_platform)?;
    match backend {
        SandboxBackendKind::Krun => {
            let local_krun = require_local_krun_execution(context, operation, local_krun)?;
            Ok(ServiceExecutionSurface::Krun {
                state_view: local_krun.state_view,
            })
        }
        SandboxBackendKind::Container => {
            let client = resolve_forwarded_machine_api_client(
                context,
                host_platform,
                machine_api_client,
                operation,
                network,
            )?;
            Ok(ServiceExecutionSurface::ForwardedContainer { client })
        }
    }
}

fn require_local_krun_execution(
    context: &ComposeProjectContext,
    _operation: &str,
    local_krun: Option<LocalKrunExecutionSurface>,
) -> Result<LocalKrunExecutionSurface, Error> {
    if let Some(local_krun) = local_krun {
        return Ok(local_krun);
    }
    #[cfg(test)]
    {
        let config = context
            .control_plane
            .reconstruct_direct_krun_backend_config();
        Ok(LocalKrunExecutionSurface {
            state_view: KrunSandboxStateView::from_config(&config),
        })
    }
    #[cfg(not(test))]
    Err(Error::Internal(format!(
        "nimbus {_operation} requires the manager-derived local krun composition for project {}",
        context.control_plane.project_name
    )))
}

fn resolve_forwarded_machine_api_client(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    operation: &str,
    network: Option<&HostMachineNetworkAuthority>,
) -> Result<MachineApiClient, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => match machine_api_client {
            Some(client) => Ok(client),
            None => require_default_machine_api_client(network.ok_or_else(|| {
                Error::Internal(format!(
                    "nimbus {operation} requires the retained parent network authority"
                ))
            })?),
        },
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but nimbus {} only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name, operation,
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but nimbus {} does not support the current host platform for forwarded guest execution",
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
        "compose project {} selects sandbox backend container, but nimbus {} requires guest machine API operations [{}] that are not available at {}: {}",
        context.control_plane.project_name,
        operation,
        missing.join(", "),
        client.socket_path().display(),
        blockers,
    )))
}

type MachineApiServiceSandboxDetails = crate::machine::MachineApiServiceSandboxDetails;
