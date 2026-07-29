//! Guest-node network composition for the Machine API process.
//!
//! Parent boot evidence is authenticated before this module is entered. This
//! owner then claims the guest OS-node authority before constructing any
//! workload backend, listener, or provider effect.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::Cidr;
use nimbus_machine::{MachineBootAuthorityEvidence, MachineForwarderAuthority};
use nimbus_network::{LocalNetworkManager, LocalNetworkManagerError, NetworkCapabilityRegistry};
use nimbus_sandbox::backends::OciNetworkProcess;
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, OciMachinePortForwarderConfig,
};

use nimbus::Error;

pub(super) const MACHINE_API_AUTHORITY_FILE_NAME: &str = "machine-api-authority.json";

/// Retained guest OS-node composition.
///
/// The manager, OCI process, and backend are deliberately retained together:
/// dropping or reconstructing any one independently would split the guest
/// network authority.
pub(super) struct GuestMachineNetworkComposition {
    _manager: Arc<LocalNetworkManager>,
    _network_process: Arc<OciNetworkProcess>,
    backend: Arc<ContainerSandboxBackend>,
    #[cfg(test)]
    network_state_root: PathBuf,
    #[cfg(test)]
    workload_state_root: PathBuf,
}

impl GuestMachineNetworkComposition {
    pub(super) fn claim(
        control_data_dir: &Path,
        mut container_config: ContainerSandboxBackendConfig,
    ) -> Result<Self, Error> {
        let expected_workload_parent = control_data_dir.join("service-sandboxes");
        if container_config.workload_state_root == control_data_dir
            || !container_config
                .workload_state_root
                .starts_with(&expected_workload_parent)
        {
            return Err(Error::InvalidInput(format!(
                "guest workload artifacts must remain below {}; network authority owns {}",
                expected_workload_parent.display(),
                control_data_dir.display()
            )));
        }
        let bootstrap =
            LocalNetworkManager::bootstrap(control_data_dir).map_err(|error| match error {
                error @ LocalNetworkManagerError::DuplicateProcessComposition { .. } => {
                    Error::Internal(format!(
                        "guest process already owns network composition; independent Machine API \
                         composition refused: {error}"
                    ))
                }
                error => Error::Internal(format!(
                    "failed to claim guest network composition before Machine API effects: {error}"
                )),
            })?;
        container_config.network_state_root = bootstrap.authority().state_root().to_path_buf();
        let node_supernet =
            Cidr::parse(&container_config.node_network_supernet).map_err(|error| {
                Error::Internal(format!(
                    "failed to validate guest node network super-net: {error}"
                ))
            })?;
        let network_process = OciNetworkProcess::new(
            bootstrap.authority(),
            node_supernet,
            container_config.node_tenant_subnet_prefix,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to compose guest OCI network authority: {error}"
            ))
        })?;
        let registry = NetworkCapabilityRegistry::new(std::iter::empty()).map_err(|error| {
            Error::Internal(format!(
                "failed to freeze guest network capability registry: {error}"
            ))
        })?;
        let manager = bootstrap.freeze(registry);
        #[cfg(test)]
        let network_state_root = manager.state_root().to_path_buf();
        #[cfg(test)]
        let workload_state_root = container_config.workload_state_root.clone();
        let backend = Arc::new(
            ContainerSandboxBackend::with_network_process(
                container_config,
                Arc::clone(&network_process),
            )
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to inject guest OCI network composition into container backend: \
                     {error}"
                ))
            })?,
        );
        Ok(Self {
            _manager: manager,
            _network_process: network_process,
            backend,
            #[cfg(test)]
            network_state_root,
            #[cfg(test)]
            workload_state_root,
        })
    }

    pub(super) fn backend(&self) -> Arc<ContainerSandboxBackend> {
        Arc::clone(&self.backend)
    }

    #[cfg(test)]
    pub(super) fn state_roots(&self) -> (&Path, &Path) {
        (&self.network_state_root, &self.workload_state_root)
    }
}

pub(super) fn machine_api_authority_path(control_data_dir: &Path) -> PathBuf {
    control_data_dir
        .parent()
        .unwrap_or(control_data_dir)
        .join(MACHINE_API_AUTHORITY_FILE_NAME)
}

/// Load strict parent boot evidence and authenticate that its provider handle
/// belongs to the built-in gvproxy adapter.
///
/// This performs no mutation. Callers must invoke it before claiming the guest
/// manager or creating listener/workload artifacts.
pub(super) fn load_parent_forwarder_authority(
    control_data_dir: &Path,
) -> Result<(MachineForwarderAuthority, OciMachinePortForwarderConfig), Error> {
    let authority_path = machine_api_authority_path(control_data_dir);
    let bytes = fs::read(&authority_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read parent-issued machine boot authority at {}: {error}",
            authority_path.display()
        ))
    })?;
    let evidence: MachineBootAuthorityEvidence =
        serde_json::from_slice(&bytes).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to decode parent-issued machine boot authority at {}: {error}",
                authority_path.display()
            ))
        })?;
    evidence.validate().map_err(|error| {
        Error::InvalidInput(format!(
            "invalid parent-issued machine boot authority at {}: {error}",
            authority_path.display()
        ))
    })?;
    let authority = evidence.forwarder_authority().clone();
    let forwarder = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
        authority.provider_instance().expose_to_provider(),
        authority.generation(),
    )
    .map_err(|error| {
        Error::InvalidInput(format!(
            "invalid parent-issued machine forwarder provider authority: {error}"
        ))
    })?;
    if forwarder.provider_instance() != authority.provider_instance() {
        return Err(Error::InvalidInput(
            "parent-issued machine forwarder provider does not belong to the gvproxy adapter"
                .to_owned(),
        ));
    }
    Ok((authority, forwarder))
}
