//! Effect-free preparation of the exact provider realm used by Compose up.
//!
//! Provider effects begin only after the caller opens the canonical Engine
//! and injects its one durable workload-saga store into the foreground runtime.

use std::path::Path;
use std::sync::Arc;

use nimbus::{
    Engine, Error, LocalBuildAdmission, SandboxBackend, SandboxBackendKind,
    ServiceDefinitionCatalog, ServiceManager,
};
use nimbus_compute::embedded_local_node_identity;
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_server::{
    ServerForegroundWorkloadRuntime, ServerWorkloadComposition, ServerWorkloadProviders,
    nimbus_owned_workload_ingress_registration,
};
use nimbus_tenant::TenantIsolationMode;
use nimbus_workloads::WorkloadSagaStore;

use crate::machine::{
    ForwardedMachineApiSandboxBackend, HostMachineNetworkAuthority,
    PreparedDefaultMachineProvisionSource, prepare_default_machine_provision_source,
};
use crate::network_composition::{
    FrozenLocalNetworkComposition, PreparedLocalNetworkComposition, PreparedServerWorkloadProfile,
    StagedLocalNetworkComposition,
};

use super::discovery::ResolvedComposeSelection;
use super::execution::{
    ServiceHostPlatform, load_service_definition_catalog_for_execution_platform_with_admission,
    required_effective_project_backend,
};
use super::file::ComposeAdmissionMode;

pub(crate) struct PreparedForwardedComposeProvisionSource {
    pub(crate) source: PreparedDefaultMachineProvisionSource,
    pub(crate) catalog: Arc<dyn ServiceDefinitionCatalog>,
    pub(crate) local_build_admission: LocalBuildAdmission,
}

pub(crate) fn prepare_forwarded_compose_provision_source(
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    network_authority: &HostMachineNetworkAuthority,
    tenant_isolation_mode: TenantIsolationMode,
) -> Result<Option<PreparedForwardedComposeProvisionSource>, Error> {
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)?;
    let host_platform = ServiceHostPlatform::current();
    let backend = required_effective_project_backend(
        &context,
        None,
        "prepare forwarded Compose provision authority",
        host_platform,
    )?;
    if backend == SandboxBackendKind::Krun {
        return Ok(None);
    }
    if host_platform != ServiceHostPlatform::Macos {
        return Err(Error::InvalidInput(format!(
            "compose project {} selects container execution, but forwarded Compose provisioning is supported only through the macOS managed machine",
            context.control_plane.project_name
        )));
    }
    let source = prepare_default_machine_provision_source(
        network_authority,
        embedded_local_node_identity(),
    )?;
    let admission_mode = match tenant_isolation_mode {
        TenantIsolationMode::LocalDevelopment => ComposeAdmissionMode::LocalDevelopment,
        TenantIsolationMode::Production => ComposeAdmissionMode::Production,
    };
    let local_build_admission = match tenant_isolation_mode {
        TenantIsolationMode::LocalDevelopment => LocalBuildAdmission::Allowed,
        TenantIsolationMode::Production => LocalBuildAdmission::Denied,
    };
    let catalog = load_service_definition_catalog_for_execution_platform_with_admission(
        selection,
        host_platform,
        admission_mode,
    )?;
    Ok(Some(PreparedForwardedComposeProvisionSource {
        source,
        catalog,
        local_build_admission,
    }))
}

/// Exact effect-free Compose provider source, frozen before Engine/provider work.
pub(super) enum PreparedComposeProvision {
    Local(PreparedServerWorkloadProfile),
    Forwarded {
        network: FrozenLocalNetworkComposition,
        source: Box<PreparedDefaultMachineProvisionSource>,
        catalog: Arc<dyn ServiceDefinitionCatalog>,
    },
}

impl PreparedComposeProvision {
    pub(super) fn prepare(
        selection: &ResolvedComposeSelection,
        control_data_dir: &Path,
        explicit_network_state_dir: Option<&Path>,
    ) -> Result<Self, Error> {
        let context =
            super::load_compose_project_context_for_selection(selection, control_data_dir)?;
        let host_platform = ServiceHostPlatform::current();
        let backend = required_effective_project_backend(
            &context,
            None,
            "prepare Compose provision authority",
            host_platform,
        )?;
        let root = LocalNodeNetworkRoot::resolve_for_current_platform(explicit_network_state_dir)
            .map_err(|error| {
            Error::InvalidInput(format!("invalid local node network root: {error}"))
        })?;
        let staged = StagedLocalNetworkComposition::claim(&root)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;

        match backend {
            SandboxBackendKind::Krun => {
                let prepared = PreparedLocalNetworkComposition::prepare(
                    staged,
                    Some(selection),
                    control_data_dir,
                    TenantIsolationMode::LocalDevelopment,
                    nimbus_owned_workload_ingress_registration(),
                )
                .map_err(|error| Error::InvalidInput(error.to_string()))?;
                let profile = prepared
                    .prepare_server_workload_profile()
                    .map_err(|error| Error::InvalidInput(error.to_string()))?;
                Ok(Self::Local(profile))
            }
            SandboxBackendKind::Container => {
                if host_platform != ServiceHostPlatform::Macos {
                    return Err(Error::InvalidInput(format!(
                        "compose project {} selects container execution, but forwarded Compose provisioning is supported only through the macOS managed machine",
                        context.control_plane.project_name
                    )));
                }
                let network_authority = HostMachineNetworkAuthority::injected(staged.authority());
                let source = prepare_default_machine_provision_source(
                    &network_authority,
                    embedded_local_node_identity(),
                )?;
                let network = staged
                    .freeze_bundle(source.bundle().clone())
                    .map_err(|error| Error::InvalidInput(error.to_string()))?;
                let catalog =
                    load_service_definition_catalog_for_execution_platform_with_admission(
                        selection,
                        host_platform,
                        ComposeAdmissionMode::LocalDevelopment,
                    )?;
                Ok(Self::Forwarded {
                    network,
                    source: Box::new(source),
                    catalog,
                })
            }
        }
    }

    pub(super) fn activate(
        self,
        engine: Arc<Engine>,
        saga_store: Arc<dyn WorkloadSagaStore>,
    ) -> Result<ServerForegroundWorkloadRuntime, Error> {
        match self {
            Self::Local(profile) => profile
                .complete_foreground(engine, saga_store)
                .map_err(|error| Error::InvalidInput(error.to_string())),
            Self::Forwarded {
                network,
                source,
                catalog,
            } => {
                let source = *source;
                let selection = source.selection().clone();
                let requirements = source.requirements().clone();
                let sovereignty = source.sovereignty().clone();
                let local_node = source.node_identity().clone();
                let execution_provider_id = source.execution_provider_id().clone();
                network
                    .manager()
                    .capability_registry()
                    .select_exact(&selection, &requirements)
                    .map_err(|error| Error::InvalidInput(error.to_string()))?;
                let activated = source.activate()?;
                let (client, adapter) = activated.into_parts();
                let parent_network = HostMachineNetworkAuthority::injected(network.authority());
                let read_retirement_backend: Arc<dyn SandboxBackend> =
                    Arc::new(ForwardedMachineApiSandboxBackend::with_provision_adapter(
                        client,
                        &parent_network,
                        Arc::clone(&adapter),
                    )?);
                let service_manager = Arc::new(
                    ServiceManager::new(catalog, read_retirement_backend)
                        .with_local_build_admission(LocalBuildAdmission::Allowed),
                );
                let providers = ServerWorkloadProviders::new(
                    selection.attachment_provider_id().clone(),
                    Arc::clone(&adapter),
                    execution_provider_id,
                    Arc::clone(&adapter),
                    selection.ingress_provider_id().clone(),
                    adapter,
                )
                .with_restart_capabilities();
                let composition = ServerWorkloadComposition::new(
                    engine,
                    network.manager(),
                    service_manager,
                    local_node,
                    selection,
                    sovereignty,
                    providers,
                )
                .map_err(|error| Error::InvalidInput(error.to_string()))?;
                Ok(composition.into_foreground_runtime(saga_store))
            }
        }
    }
}
