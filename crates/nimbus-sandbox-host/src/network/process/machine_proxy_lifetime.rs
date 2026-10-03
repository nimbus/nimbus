//! Process-owned lifetime state for provider-managed machine port proxies.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkProviderHandle, NetworkResourceGeneration, PortLeaseBinding, PortLeaseRecoveryGuard,
    PortLeaseRequest,
};

use crate::network::{
    MachinePortForwardReceipt, MachinePortProxy, MachinePortProxyRoute,
    OciMachinePortForwarderConfig, inspect_machine_ports, machine_port_proxy_routes,
};
use crate::port_lease::OciPortBindLifetimeBatch;
use crate::port_lifecycle::OciPortLeaseCoordinator;
use nimbus_sandbox::SandboxId;
use nimbus_sandbox::SandboxPortBinding;
use nimbus_sandbox::{Result, SandboxError};

pub type MachinePortProxyKey = (TenantId, SandboxId);
pub type MachinePortProxyEntries = HashMap<MachinePortProxyKey, MachinePortProxyEntry>;

pub struct MachinePortProxyRegistration {
    pub port_bindings: Vec<SandboxPortBinding>,
    pub port_leases: Vec<PortLeaseRequest>,
    pub routes: Vec<MachinePortProxyRoute>,
    pub proxies: Vec<MachinePortProxy>,
    pub lease_authority: Option<MachinePortProxyLeaseAuthority>,
}

pub enum MachinePortProxyLeaseAuthority {
    Live(OciPortBindLifetimeBatch),
    Recovered(Vec<PortLeaseRecoveryGuard>),
}

pub enum MachinePortProxyEntry {
    Running(MachinePortProxyRegistration),
    Stopping(Arc<Mutex<MachinePortProxyCleanupState>>),
}

/// Exact inputs for one read-only machine publication observation.
pub struct MachineForwardedPublicationInspection<'a> {
    pub tenant_id: &'a TenantId,
    pub sandbox_id: &'a SandboxId,
    pub assigned_ips: &'a [Ipv4Addr],
    pub bindings: &'a [SandboxPortBinding],
    pub leases: &'a [PortLeaseRequest],
    pub durable_receipts: &'a [MachinePortForwardReceipt],
    pub forwarder: &'a OciMachinePortForwarderConfig,
    pub port_leases: &'a OciPortLeaseCoordinator,
    /// Complete compiler-owned launch membership for a PlanOnly provision.
    /// Legacy coarse launches retain their sandbox-derived authority until the
    /// NNC6.4 deletion gate removes that path.
    pub planned_members: Option<&'a [PortLeaseRequest]>,
}

/// Non-serializable proof that one exact machine publication is current.
///
/// The private fields can be constructed only after the process-lifetime
/// registry, listener leases, local workers, durable receipts, and provider
/// observation have all authenticated as one generation.
#[derive(Debug)]
pub struct MachineForwardedPublicationReadiness {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
}

impl MachineForwardedPublicationReadiness {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub fn provider_instance(&self) -> &NetworkProviderHandle {
        &self.provider_instance
    }

    pub fn provider_generation(&self) -> NetworkResourceGeneration {
        self.provider_generation
    }

    #[cfg(test)]
    pub(crate) fn exact_for_test(
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        forwarder: &OciMachinePortForwarderConfig,
    ) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            provider_instance: forwarder.provider_instance().clone(),
            provider_generation: forwarder.provider_generation(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachinePortProxyCleanupDisposition {
    Restart,
    Release,
}

pub struct MachinePortProxyCleanupState {
    pub disposition: MachinePortProxyCleanupDisposition,
    pub port_lease_coordinator: OciPortLeaseCoordinator,
    pub registration: MachinePortProxyRegistration,
    pub expected_bindings: Vec<PortLeaseBinding>,
    pub withdraw_complete: bool,
    pub provider_stopped: bool,
    pub durable_transition_complete: bool,
}

/// Shared, fail-closed owner of machine-proxy process lifetimes.
#[derive(Clone, Default)]
pub struct MachinePortProxyLifetimeRegistry {
    inner: Arc<Mutex<MachinePortProxyEntries>>,
}

impl MachinePortProxyLifetimeRegistry {
    pub fn lock(&self) -> Result<MutexGuard<'_, MachinePortProxyEntries>> {
        self.inner
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "container machine port proxy registry lock is poisoned".to_owned(),
            })
    }

    pub fn inspect_current_publication(
        &self,
        inspection: MachineForwardedPublicationInspection<'_>,
    ) -> Result<MachineForwardedPublicationReadiness> {
        let expected_routes =
            machine_port_proxy_routes(inspection.assigned_ips, inspection.bindings)?;
        let registrations = self.lock()?;
        let key = (inspection.tenant_id.clone(), inspection.sandbox_id.clone());
        let registration = match registrations.get(&key) {
            Some(MachinePortProxyEntry::Running(registration)) => registration,
            Some(MachinePortProxyEntry::Stopping(_)) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port publication for tenant {} sandbox {} is stopping",
                        inspection.tenant_id, inspection.sandbox_id
                    ),
                });
            }
            None => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port publication for tenant {} sandbox {} has no live \
                         process-local registration",
                        inspection.tenant_id, inspection.sandbox_id
                    ),
                });
            }
        };
        if registration.port_bindings != inspection.bindings
            || registration.port_leases != inspection.leases
            || registration.routes != expected_routes
            || registration.proxies.len() != expected_routes.len()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container machine port publication for tenant {} sandbox {} does not match \
                     the exact binding, lease, route, and worker generation",
                    inspection.tenant_id, inspection.sandbox_id
                ),
            });
        }
        let live_authority = match registration.lease_authority.as_ref() {
            Some(MachinePortProxyLeaseAuthority::Live(authority)) => authority,
            Some(MachinePortProxyLeaseAuthority::Recovered(_)) | None => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port publication for tenant {} sandbox {} lacks its \
                         exact live process lifetime",
                        inspection.tenant_id, inspection.sandbox_id
                    ),
                });
            }
        };
        match inspection.planned_members {
            Some(plan_members) => inspection
                .port_leases
                .require_active_planned_machine_bindings_with_lifetimes(
                    inspection.tenant_id,
                    inspection.bindings,
                    inspection.leases,
                    plan_members,
                    live_authority,
                )?,
            None => inspection
                .port_leases
                .require_active_machine_bindings_with_lifetimes(
                    inspection.tenant_id,
                    inspection.sandbox_id,
                    inspection.bindings,
                    inspection.leases,
                    live_authority,
                )?,
        };
        if registration
            .proxies
            .iter()
            .any(|proxy| !proxy.provider_is_running())
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container machine port publication for tenant {} sandbox {} has an exited \
                     local provider worker or listener",
                    inspection.tenant_id, inspection.sandbox_id
                ),
            });
        }

        // Keep the lifecycle registry locked across the bounded read-only
        // provider inspection. Cleanup must not withdraw or stop the exact
        // local generation between its validation and the final observation.
        let current = inspect_machine_ports(
            inspection.forwarder,
            inspection.tenant_id,
            inspection.sandbox_id,
            inspection.bindings,
        )?;
        if current.provider_instance() != inspection.forwarder.provider_instance()
            || current.provider_generation() != inspection.forwarder.provider_generation()
            || current.receipts() != inspection.durable_receipts
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container machine port publication for tenant {} sandbox {} has crossed, \
                     stale, or non-current provider evidence",
                    inspection.tenant_id, inspection.sandbox_id
                ),
            });
        }
        if registration
            .proxies
            .iter()
            .any(|proxy| !proxy.provider_is_running())
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container machine port publication for tenant {} sandbox {} lost its local \
                     provider worker or listener during current inspection",
                    inspection.tenant_id, inspection.sandbox_id
                ),
            });
        }
        Ok(MachineForwardedPublicationReadiness {
            tenant_id: inspection.tenant_id.clone(),
            sandbox_id: inspection.sandbox_id.clone(),
            provider_instance: current.provider_instance().clone(),
            provider_generation: current.provider_generation(),
        })
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn try_lock(&self) -> std::sync::TryLockResult<MutexGuard<'_, MachinePortProxyEntries>> {
        self.inner.try_lock()
    }
}
