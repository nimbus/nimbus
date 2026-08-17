//! Prepared machine-forwarded server workload composition.
//!
//! The parent module owns local network capability freeze and profile
//! selection. This child owns the forwarded profile's delayed activation and
//! exact provider composition after the caller supplies its `Engine`.

mod profile;

use std::path::Path;
use std::sync::Arc;

use nimbus::Engine;
use nimbus_server::{ServeOptions, ServerWorkloadComposition};

pub(crate) use profile::{PreparedForwardedWorkloadProfile, prepare_forwarded_workload_profile};

use super::{
    LocalNetworkCompositionError, PreparedLocalNetworkComposition, PreparedServerWorkloadProfile,
    StagedLocalNetworkComposition,
};
use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::{
    PreparedForwardedComposeProvisionSource, prepare_forwarded_compose_provision_source,
};
use crate::machine::HostMachineNetworkAuthority;

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
        PreparedForwardedWorkloadProfile::new(
            composition.frozen.clone(),
            forwarded.source.clone(),
            Arc::clone(&forwarded.catalog),
            forwarded.local_build_admission,
        ),
    ))))
}

impl PreparedForwardedWorkloadProfile {
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
        compose_forwarded_server(self, engine)
    }
}

fn compose_forwarded_server(
    prepared: PreparedForwardedWorkloadProfile,
    engine: Arc<Engine>,
) -> Result<ServerWorkloadComposition, LocalNetworkCompositionError> {
    prepare_forwarded_workload_profile(prepared, engine)
}

#[cfg(test)]
pub(crate) fn prepare_forwarded_server_profile_for_test(
    staged: StagedLocalNetworkComposition,
    source: crate::machine::PreparedDefaultMachineProvisionSource,
) -> Result<PreparedServerWorkloadProfile, LocalNetworkCompositionError> {
    let network = staged.freeze_bundle(source.bundle().clone())?;
    Ok(PreparedServerWorkloadProfile::Forwarded(Box::new(
        PreparedForwardedWorkloadProfile::new(
            network,
            source,
            Arc::new(nimbus::EmptyServiceDefinitionCatalog),
            nimbus::LocalBuildAdmission::Denied,
        ),
    )))
}
