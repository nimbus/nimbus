//! Process-owned lifetime state for provider-managed machine port proxies.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::TenantId;
use nimbus_network::{PortLeaseBinding, PortLeaseRecoveryGuard, PortLeaseRequest};

use crate::backends::oci::network::{MachinePortProxy, MachinePortProxyRoute};
use crate::backends::oci::port_lease::OciPortBindLifetimeBatch;
use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

pub(crate) type MachinePortProxyKey = (TenantId, SandboxId);
pub(crate) type MachinePortProxyEntries = HashMap<MachinePortProxyKey, MachinePortProxyEntry>;

pub(crate) struct MachinePortProxyRegistration {
    pub(crate) port_bindings: Vec<SandboxPortBinding>,
    pub(crate) port_leases: Vec<PortLeaseRequest>,
    pub(crate) routes: Vec<MachinePortProxyRoute>,
    pub(crate) proxies: Vec<MachinePortProxy>,
    pub(crate) lease_authority: Option<MachinePortProxyLeaseAuthority>,
    pub(crate) publication_may_exist: bool,
}

pub(crate) enum MachinePortProxyLeaseAuthority {
    Live(OciPortBindLifetimeBatch),
    Recovered(Vec<PortLeaseRecoveryGuard>),
}

pub(crate) enum MachinePortProxyEntry {
    Running(MachinePortProxyRegistration),
    Stopping(Arc<Mutex<MachinePortProxyCleanupState>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MachinePortProxyCleanupDisposition {
    Restart,
    Release,
}

pub(crate) struct MachinePortProxyCleanupState {
    pub(crate) disposition: MachinePortProxyCleanupDisposition,
    pub(crate) port_lease_coordinator: OciPortLeaseCoordinator,
    pub(crate) registration: MachinePortProxyRegistration,
    pub(crate) expected_bindings: Vec<PortLeaseBinding>,
    pub(crate) withdraw_complete: bool,
    pub(crate) provider_stopped: bool,
    pub(crate) publication_withdrawn: Vec<bool>,
    pub(crate) durable_transition_complete: bool,
}

/// Shared, fail-closed owner of machine-proxy process lifetimes.
#[derive(Clone, Default)]
pub(crate) struct MachinePortProxyLifetimeRegistry {
    inner: Arc<Mutex<MachinePortProxyEntries>>,
}

impl MachinePortProxyLifetimeRegistry {
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, MachinePortProxyEntries>> {
        self.inner
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "container machine port proxy registry lock is poisoned".to_owned(),
            })
    }

    #[cfg(test)]
    pub(crate) fn try_lock(
        &self,
    ) -> std::sync::TryLockResult<MutexGuard<'_, MachinePortProxyEntries>> {
        self.inner.try_lock()
    }
}
