//! Canonical forwarded workload provider composition.
//!
//! Server start and foreground Compose retain different preparation roots,
//! but both complete through this one exact provider realm after the caller
//! supplies its canonical `Engine`.

use std::sync::Arc;

use nimbus::{
    Engine, LocalBuildAdmission, SandboxBackend, ServiceDefinitionCatalog, ServiceManager,
};
use nimbus_compute::workload_saga::WorkloadTeardownCapabilityRegistry;
use nimbus_server::{
    ServerWorkloadComposition, ServerWorkloadCompositionError, ServerWorkloadProviders,
};

use super::super::{FrozenLocalNetworkComposition, LocalNetworkCompositionError};
use crate::machine::{
    ForwardedMachineApiSandboxBackend, HostMachineNetworkAuthority,
    PreparedDefaultMachineProvisionSource,
};

/// Effect-free forwarded sources retained until the caller supplies an Engine.
pub(crate) struct PreparedForwardedWorkloadProfile {
    network: FrozenLocalNetworkComposition,
    source: PreparedDefaultMachineProvisionSource,
    catalog: Arc<dyn ServiceDefinitionCatalog>,
    local_build_admission: LocalBuildAdmission,
}

impl PreparedForwardedWorkloadProfile {
    pub(crate) fn new(
        network: FrozenLocalNetworkComposition,
        source: PreparedDefaultMachineProvisionSource,
        catalog: Arc<dyn ServiceDefinitionCatalog>,
        local_build_admission: LocalBuildAdmission,
    ) -> Self {
        Self {
            network,
            source,
            catalog,
            local_build_admission,
        }
    }
}

/// Complete one exact forwarded provider realm after Engine construction.
pub(crate) fn prepare_forwarded_workload_profile(
    prepared: PreparedForwardedWorkloadProfile,
    engine: Arc<Engine>,
) -> Result<ServerWorkloadComposition, LocalNetworkCompositionError> {
    let selection = prepared.source.selection().clone();
    let requirements = prepared.source.requirements().clone();
    let sovereignty = prepared.source.sovereignty().clone();
    let local_node = prepared.source.node_identity().clone();
    let execution_provider_id = prepared.source.execution_provider_id().clone();
    prepared
        .network
        .manager()
        .capability_registry()
        .select_exact(&selection, &requirements)
        .map_err(LocalNetworkCompositionError::CapabilitySelection)?;
    let activated = prepared
        .source
        .activate()
        .map_err(LocalNetworkCompositionError::Compose)?;
    let (client, adapter) = activated.into_parts();
    let desire_admission_guard = adapter
        .desire_admission_guard()
        .map_err(LocalNetworkCompositionError::Compose)?;
    let parent_network = HostMachineNetworkAuthority::injected(prepared.network.authority());
    let backend = Arc::new(
        ForwardedMachineApiSandboxBackend::with_provision_adapter(
            client,
            &parent_network,
            Arc::clone(&adapter),
        )
        .map_err(LocalNetworkCompositionError::Compose)?,
    );
    let (attachment, execution, ingress) = backend
        .teardown_capabilities()
        .map_err(LocalNetworkCompositionError::Compose)?
        .into_parts();
    let teardown_capabilities =
        WorkloadTeardownCapabilityRegistry::new([attachment], [execution], [ingress])
            .map_err(ServerWorkloadCompositionError::from)
            .map_err(LocalNetworkCompositionError::ServerWorkload)?;
    let service_manager = Arc::new(
        ServiceManager::new(prepared.catalog, backend.kind())
            .with_local_build_admission(prepared.local_build_admission),
    );
    let providers = ServerWorkloadProviders::new(
        selection.attachment_provider_id().clone(),
        Arc::clone(&adapter),
        execution_provider_id,
        Arc::clone(&adapter),
        selection.ingress_provider_id().clone(),
        adapter,
    )
    .with_restart_capabilities()
    .with_teardown_capabilities(teardown_capabilities)
    .with_desire_admission_guard(desire_admission_guard);
    ServerWorkloadComposition::new(
        engine,
        prepared.network.manager(),
        service_manager,
        local_node,
        selection,
        sovereignty,
        providers,
    )
    .map_err(LocalNetworkCompositionError::ServerWorkload)
}
