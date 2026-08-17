//! Fail-closed non-Unix shape of physical-machine stop authorization.

use std::sync::Arc;

use nimbus::{Engine, Error};
use nimbus_compute::machine_stop_authority::ConfirmedMachineStopAuthorization;
use nimbus_machine::MachineForwarderAuthority;

use super::network_composition::HostMachineNetworkAuthority;

#[derive(Clone)]
pub(super) struct HostMachineStopAuthority;

impl HostMachineStopAuthority {
    pub(super) fn new(
        _network: &HostMachineNetworkAuthority,
        _engine: Arc<Engine>,
    ) -> Result<Self, Error> {
        Ok(Self)
    }

    pub(super) async fn authorize(
        &self,
        _machine_name: &str,
        _forwarder_authority: &MachineForwarderAuthority,
    ) -> Result<ConfirmedMachineStopAuthorization, Error> {
        Err(unsupported_machine_stop_error())
    }

    pub(super) async fn cancel_effect_free_stop(
        &self,
        _authorization: &ConfirmedMachineStopAuthorization,
    ) -> Result<(), Error> {
        Err(unsupported_machine_stop_error())
    }
}

fn unsupported_machine_stop_error() -> Error {
    Error::InvalidInput(
        "physical managed-machine stop is unavailable because this host does not provide the Unix machine effect owner"
            .to_owned(),
    )
}
