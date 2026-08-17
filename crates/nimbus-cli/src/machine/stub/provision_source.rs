//! Fail-closed non-Unix shape of the managed-machine provision source.

use nimbus::Error;
use nimbus_network::NetworkCapabilityBundle;
use nimbus_workloads::NodeIdentity;

use super::network_composition::HostMachineNetworkAuthority;

/// An opaque source keeps cross-platform composition type-correct. Its only
/// constructor fails, so a non-Unix host cannot obtain activation authority.
#[derive(Clone)]
pub(crate) struct PreparedDefaultMachineProvisionSource {
    bundle: NetworkCapabilityBundle,
}

impl PreparedDefaultMachineProvisionSource {
    pub(crate) fn bundle(&self) -> &NetworkCapabilityBundle {
        &self.bundle
    }
}

pub(crate) fn prepare_default_machine_provision_source(
    _network: &HostMachineNetworkAuthority,
    _node_identity: NodeIdentity,
) -> Result<PreparedDefaultMachineProvisionSource, Error> {
    Err(Error::InvalidInput(
        "forwarded managed-machine workload provisioning is available only on macOS".to_owned(),
    ))
}
