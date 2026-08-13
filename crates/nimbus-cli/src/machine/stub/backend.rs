use nimbus::{Error, SandboxBackend, SandboxBackendKind, SandboxError, SandboxId};
use nimbus_sandbox::{SandboxFuture, SandboxInspection};

use super::client::MachineApiClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardedMachineApiSandboxBackend {
    // realized by WIN2: the unix twin forwards through `client`; this stub
    // returns a canned error from every method without dispatching through it.
    #[allow(dead_code)]
    client: MachineApiClient,
}

impl ForwardedMachineApiSandboxBackend {
    pub(crate) fn new(
        client: MachineApiClient,
        _network: &super::HostMachineNetworkAuthority,
    ) -> Result<Self, Error> {
        Ok(Self { client })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        client: MachineApiClient,
        _port_leases: nimbus_network::LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        Ok(Self { client })
    }
}

impl SandboxBackend for ForwardedMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        Box::pin(async move {
            Err(SandboxError::BackendUnavailable {
                message: unsupported_machine_api_backend_message(),
            })
        })
    }
}

fn unsupported_machine_api_backend_message() -> String {
    "forwarded machine API backend is only available on unix hosts; Windows builds do not provide machine-backed guest execution"
        .to_owned()
}
