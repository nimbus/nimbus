//! Effect-free preparation of the exact provider realm used by Compose up.
//!
//! Provider effects begin only after the caller opens the canonical Engine
//! and injects its one durable workload-saga store into the foreground runtime.

use std::path::Path;
use std::sync::Arc;

use nimbus::{Engine, Error, LocalBuildAdmission, SandboxBackendKind, ServiceDefinitionCatalog};
use nimbus_compute::embedded_local_node_identity;
use nimbus_compute::state::ComputeError;
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_server::{
    ServerForegroundWorkloadRuntime, ServerWorkloadComposition,
    nimbus_owned_workload_ingress_registration,
};
use nimbus_tenant::TenantIsolationMode;
use nimbus_workloads::WorkloadSagaStore;

use crate::machine::{
    HostMachineNetworkAuthority, PreparedDefaultMachineProvisionSource,
    prepare_default_machine_provision_source,
};
use crate::network_composition::{
    LocalNetworkCompositionError, PreparedForwardedWorkloadProfile,
    PreparedLocalNetworkComposition, PreparedServerWorkloadProfile, StagedLocalNetworkComposition,
    prepare_forwarded_workload_profile,
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
    Forwarded(Box<PreparedForwardedWorkloadProfile>),
    #[cfg(test)]
    TestComposition(Box<ServerWorkloadComposition>),
    #[cfg(test)]
    TestRuntime(Box<ServerForegroundWorkloadRuntime>),
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
                Ok(Self::Forwarded(Box::new(
                    PreparedForwardedWorkloadProfile::new(
                        network,
                        source,
                        catalog,
                        LocalBuildAdmission::Allowed,
                    ),
                )))
            }
        }
    }

    pub(super) async fn activate<S>(
        self,
        engine: Arc<Engine>,
        saga_store: Arc<S>,
    ) -> Result<ServerForegroundWorkloadRuntime, Error>
    where
        S: WorkloadSagaStore + nimbus_workloads::TenantRetirementStore,
    {
        match self {
            Self::Local(profile) => profile
                .complete_foreground(engine, saga_store)
                .await
                .map_err(|error| Error::InvalidInput(error.to_string())),
            Self::Forwarded(prepared) => {
                let composition = compose_forwarded_foreground(*prepared, engine)
                    .map_err(forwarded_compose_activation_error)?;
                composition
                    .into_foreground_runtime(saga_store)
                    .await
                    .map_err(foreground_runtime_error)
            }
            #[cfg(test)]
            Self::TestComposition(composition) => composition
                .into_foreground_runtime(saga_store)
                .await
                .map_err(foreground_runtime_error),
            #[cfg(test)]
            Self::TestRuntime(runtime) => Ok(*runtime),
        }
    }
}

fn foreground_runtime_error(error: ComputeError) -> Error {
    match error {
        ComputeError::Core(error) => error,
        ComputeError::Unauthorized(message) | ComputeError::Forbidden(message) => {
            Error::PermissionDenied(message)
        }
        ComputeError::NotFound(message) => Error::NotFound(message),
    }
}

fn compose_forwarded_foreground(
    prepared: PreparedForwardedWorkloadProfile,
    engine: Arc<Engine>,
) -> Result<ServerWorkloadComposition, LocalNetworkCompositionError> {
    prepare_forwarded_workload_profile(prepared, engine)
}

fn forwarded_compose_activation_error(error: LocalNetworkCompositionError) -> Error {
    match error {
        LocalNetworkCompositionError::Compose(error) => error,
        error => Error::InvalidInput(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_activation_preserves_operational_errors_and_classifies_validation_errors() {
        let operational =
            forwarded_compose_activation_error(LocalNetworkCompositionError::Compose(
                Error::PreconditionFailed("machine provider identity changed".to_owned()),
            ));
        assert!(matches!(
            operational,
            Error::PreconditionFailed(message) if message == "machine provider identity changed"
        ));

        let validation = forwarded_compose_activation_error(
            LocalNetworkCompositionError::ForwardedServerWorkloadUnavailable,
        );
        assert!(matches!(validation, Error::InvalidInput(_)));
    }
}
