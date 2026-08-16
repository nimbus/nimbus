//! Fail-closed non-Unix shape of the managed-machine provision source.

use nimbus::Error;
use nimbus_network::NetworkCapabilityBundle;
use nimbus_workloads::NodeIdentity;

use super::network_composition::HostMachineNetworkAuthority;

/// An uninhabited source keeps cross-platform composition type-correct while
/// making activation impossible on a host that cannot prepare the source.
#[derive(Clone)]
pub(crate) enum PreparedDefaultMachineProvisionSource {}

impl PreparedDefaultMachineProvisionSource {
    pub(crate) fn bundle(&self) -> &NetworkCapabilityBundle {
        match *self {}
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
