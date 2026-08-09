//! Prepared machine-forwarded server workload composition.
//!
//! The parent module owns local network capability freeze and profile
//! selection. This child owns the forwarded profile's delayed activation and
//! exact provider composition after the caller supplies its `Engine`.

use std::path::Path;
use std::sync::Arc;

use nimbus::{
    Engine, LocalBuildAdmission, SandboxBackend, ServiceDefinitionCatalog, ServiceManager,
};
use nimbus_server::{ServeOptions, ServerWorkloadComposition, ServerWorkloadProviders};

use super::{
    FrozenLocalNetworkComposition, LocalNetworkCompositionError, PreparedLocalNetworkComposition,
    PreparedServerWorkloadProfile, StagedLocalNetworkComposition,
};
use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::{
    PreparedForwardedComposeProvisionSource, prepare_forwarded_compose_provision_source,
};
use crate::machine::{
    ForwardedMachineApiSandboxBackend, HostMachineNetworkAuthority,
    PreparedDefaultMachineProvisionSource,
};

pub(crate) struct PreparedForwardedServerWorkload {
    pub(super) network: FrozenLocalNetworkComposition,
    pub(super) source: PreparedDefaultMachineProvisionSource,
    pub(super) catalog: Arc<dyn ServiceDefinitionCatalog>,
    pub(super) local_build_admission: LocalBuildAdmission,
}

pub(super) fn prepare_source(
    staged: &StagedLocalNetworkComposition,
    compose_selection: Option<&ResolvedComposeSelection>,
    control_data_dir: &Path,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
) -> Result<Option<PreparedForwardedComposeProvisionSource>, LocalNetworkCompositionError> {
    compose_selection
        .map(|selection| {
            let network = HostMachineNetworkAuthority::injected(staged.authority());
            prepare_forwarded_compose_provision_source(
                selection,
                control_data_dir,
                &network,
                tenant_isolation_mode,
            )
        })
        .transpose()
        .map_err(LocalNetworkCompositionError::Compose)
        .map(Option::flatten)
}

pub(super) fn prepare_server_workload_profile(
    composition: &PreparedLocalNetworkComposition,
    registry_is_empty: bool,
) -> Result<Option<PreparedServerWorkloadProfile>, LocalNetworkCompositionError> {
    let Some(forwarded) = composition.forwarded.as_ref() else {
        return Ok(None);
    };
    if composition.local_service_manager.is_some()
        || composition.local_krun_backend.is_some()
        || composition.local_krun_state_view.is_some()
        || composition.local_krun_network_root.is_some()
        || composition.admitted_ingress.is_some()
        || registry_is_empty
    {
        return Err(
            LocalNetworkCompositionError::IncompleteServerWorkloadSources {
                reason: "a forwarded profile must retain only its machine source, catalog, and exact provider bundle",
            },
        );
    }
    Ok(Some(PreparedServerWorkloadProfile::Forwarded(Box::new(
        PreparedForwardedServerWorkload {
            network: composition.frozen.clone(),
            source: forwarded.source.clone(),
            catalog: Arc::clone(&forwarded.catalog),
            local_build_admission: forwarded.local_build_admission,
        },
    ))))
}

impl PreparedForwardedServerWorkload {
    pub(super) fn complete(
        self,
        engine: Arc<Engine>,
    ) -> Result<ServeOptions, LocalNetworkCompositionError> {
        Ok(ServeOptions::managed(
            self.into_workload_composition(engine)?,
        ))
    }

    pub(super) fn into_workload_composition(
        self,
        engine: Arc<Engine>,
    ) -> Result<ServerWorkloadComposition, LocalNetworkCompositionError> {
        let selection = self.source.selection().clone();
        let requirements = self.source.requirements().clone();
        let sovereignty = self.source.sovereignty().clone();
        let local_node = self.source.node_identity().clone();
        let execution_provider_id = self.source.execution_provider_id().clone();
        self.network
            .manager()
            .capability_registry()
            .select_exact(&selection, &requirements)
            .map_err(LocalNetworkCompositionError::CapabilitySelection)?;
        let activated = self
            .source
            .activate()
            .map_err(LocalNetworkCompositionError::Compose)?;
        let (client, adapter) = activated.into_parts();
        let parent_network = HostMachineNetworkAuthority::injected(self.network.authority());
        let read_retirement_backend: Arc<dyn SandboxBackend> = Arc::new(
            ForwardedMachineApiSandboxBackend::with_provision_adapter(
                client,
                &parent_network,
                Arc::clone(&adapter),
            )
            .map_err(LocalNetworkCompositionError::Compose)?,
        );
        let service_manager = Arc::new(
            ServiceManager::new(self.catalog, read_retirement_backend)
                .with_local_build_admission(self.local_build_admission),
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
        ServerWorkloadComposition::new(
            engine,
            self.network.manager(),
            service_manager,
            local_node,
            selection,
            sovereignty,
            providers,
        )
        .map_err(LocalNetworkCompositionError::ServerWorkload)
    }
}

#[cfg(test)]
pub(crate) fn prepare_forwarded_server_profile_for_test(
    staged: StagedLocalNetworkComposition,
    source: PreparedDefaultMachineProvisionSource,
) -> Result<PreparedServerWorkloadProfile, LocalNetworkCompositionError> {
    let network = staged.freeze_bundle(source.bundle().clone())?;
    Ok(PreparedServerWorkloadProfile::Forwarded(Box::new(
        PreparedForwardedServerWorkload {
            network,
            source,
            catalog: Arc::new(nimbus::EmptyServiceDefinitionCatalog),
            local_build_admission: LocalBuildAdmission::Denied,
        },
    )))
}
