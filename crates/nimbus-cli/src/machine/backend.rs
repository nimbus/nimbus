//! Read and retirement adapter for guest-owned machine workloads.
//!
//! Provisioning deliberately does not enter through `SandboxBackend`. The
//! compute-owned saga uses the narrow exact-phase adapter in [`provision`].

use std::fmt;
use std::sync::Arc;

use nimbus::{
    CommitErrorClass, Error, SandboxBackend, SandboxBackendKind, SandboxError, SandboxId,
};
use nimbus_sandbox::SandboxFuture;

use super::client::MachineApiClient;
use super::network_composition::HostMachineNetworkAuthority;
use provision::ForwardedMachineProvisionAdapter;
use teardown::{ForwardedMachineTeardownAdapter, ForwardedMachineTeardownRegistrations};

pub(crate) mod provision;
pub(crate) mod teardown;

#[derive(Clone)]
pub(crate) struct ForwardedMachineApiSandboxBackend {
    client: MachineApiClient,
    // Retain the process-composition claim for the lifetime of the adapter.
    _parent_network: Option<HostMachineNetworkAuthority>,
    // Band 8 composes and retains the exact teardown sink. Band 9 transfers
    // these traits into the compute registry without reopening its authorities.
    // NNC6.5f owns the product composition-root cutover.
    teardown_adapter: Option<Arc<ForwardedMachineTeardownAdapter>>,
}

impl ForwardedMachineApiSandboxBackend {
    #[cfg(test)]
    pub(crate) fn new(
        client: MachineApiClient,
        network: &HostMachineNetworkAuthority,
    ) -> Result<Self, Error> {
        Self::open(client, Some(network.clone()), None)
    }

    pub(crate) fn with_provision_adapter(
        client: MachineApiClient,
        network: &HostMachineNetworkAuthority,
        provision_adapter: Arc<ForwardedMachineProvisionAdapter>,
    ) -> Result<Self, Error> {
        Self::open(client, Some(network.clone()), Some(provision_adapter))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        client: MachineApiClient,
        _port_leases: nimbus_network::LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        Self::open(client, None, None)
    }

    fn open(
        client: MachineApiClient,
        parent_network: Option<HostMachineNetworkAuthority>,
        provision_adapter: Option<Arc<ForwardedMachineProvisionAdapter>>,
    ) -> Result<Self, Error> {
        let teardown_adapter = provision_adapter
            .as_ref()
            .map(|adapter| ForwardedMachineTeardownAdapter::new(Arc::clone(adapter)).map(Arc::new))
            .transpose()?;
        Ok(Self {
            client,
            _parent_network: parent_network,
            teardown_adapter,
        })
    }

    pub(crate) fn teardown_capabilities(
        &self,
    ) -> Result<ForwardedMachineTeardownRegistrations, Error> {
        self.teardown_adapter
            .as_ref()
            .map(|adapter| Arc::clone(adapter).registrations())
            .ok_or_else(|| {
                Error::PreconditionFailed(
                    "forwarded machine teardown capabilities require the exact provision authority"
                        .to_owned(),
                )
            })
    }
}

impl fmt::Debug for ForwardedMachineApiSandboxBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardedMachineApiSandboxBackend")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl SandboxBackend for ForwardedMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<nimbus_sandbox::SandboxInspection>> {
        let sandbox_id = id.clone();
        let client = self.client.clone();
        spawn_machine_api_operation("inspect", move || {
            client.inspect_service_sandbox(&sandbox_id)
        })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let sandbox_id = id.clone();
        Box::pin(async move {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "coarse forwarded stop is unavailable for sandbox {sandbox_id}; use the exact compute teardown facade"
                ),
            })
        })
    }
}

fn spawn_machine_api_operation<T, F>(operation: &'static str, callback: F) -> SandboxFuture<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Error> + Send + 'static,
{
    Box::pin(async move {
        tokio::task::spawn_blocking(callback)
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("forwarded machine API {operation} task failed to join: {error}"),
            })?
            .map_err(machine_client_error_to_sandbox_error)
    })
}

fn machine_client_error_to_sandbox_error(error: Error) -> SandboxError {
    let rendered = error.to_string();
    if let Some(class) = error.commit_class() {
        return match class {
            CommitErrorClass::Conflict | CommitErrorClass::OutOfRetention => {
                SandboxError::OperationFailed { message: rendered }
            }
            CommitErrorClass::Overloaded
            | CommitErrorClass::CommitterFull
            | CommitErrorClass::RejectedBeforeExecution
            | CommitErrorClass::RateLimited => {
                SandboxError::BackendUnavailable { message: rendered }
            }
            CommitErrorClass::CapExceeded => SandboxError::InvalidSpec { message: rendered },
        };
    }

    match error {
        Error::InvalidInput(_)
        | Error::MissingIndex { .. }
        | Error::SchemaValidation(_)
        | Error::SchemaNotFound(_)
        | Error::HistoricalRead { .. }
        | Error::Serialization(_) => SandboxError::InvalidSpec { message: rendered },
        Error::ResourceExhausted(_)
        | Error::PermissionDenied(_)
        | Error::Storage { .. }
        | Error::Transport(_) => SandboxError::BackendUnavailable { message: rendered },
        Error::Internal(message)
            if message.contains("failed to connect to machine API socket")
                || message.contains("failed to read machine API response")
                || message.contains("machine API response from")
                || message.contains("machine API request") =>
        {
            SandboxError::BackendUnavailable { message: rendered }
        }
        _ => SandboxError::OperationFailed { message: rendered },
    }
}
