//! Netavark process-lifetime ownership and cleanup reconciliation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::*;

type NetavarkLifetimeKey = (TenantId, SandboxId);

/// Process-local ownership of active Netavark publication lease generations.
#[derive(Clone, Default)]
pub(crate) struct NetavarkPortLifetimeRegistry {
    inner: Arc<Mutex<HashMap<NetavarkLifetimeKey, OciPortBindLifetimeBatch>>>,
}

enum NetavarkPortLeaseAuthority {
    Live(OciPortBindLifetimeBatch),
    Recovered(Vec<PortLeaseRecoveryGuard>),
}

/// Exact process-generation authority retained across one Netavark teardown.
pub(crate) struct NetavarkPortCleanup {
    expected_bindings: Vec<PortLeaseBinding>,
    authority: NetavarkPortLeaseAuthority,
}

impl NetavarkPortLifetimeRegistry {
    pub(crate) fn insert(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        batch: OciPortBindLifetimeBatch,
    ) -> std::result::Result<(), (SandboxError, OciPortBindLifetimeBatch)> {
        let mut entries = match self.inner.lock() {
            Ok(entries) => entries,
            Err(_) => {
                return Err((
                    SandboxError::OperationFailed {
                        message: "Netavark port-lifetime registry lock is poisoned".to_owned(),
                    },
                    batch,
                ));
            }
        };
        let key = (tenant_id.clone(), sandbox_id.clone());
        if entries.contains_key(&key) {
            return Err((
                SandboxError::OperationFailed {
                    message: format!(
                        "Netavark port-lifetime registry already owns tenant {tenant_id} sandbox \
                         {sandbox_id}"
                    ),
                },
                batch,
            ));
        }
        entries.insert(key, batch);
        Ok(())
    }

    pub(crate) fn take(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
    ) -> Result<Option<OciPortBindLifetimeBatch>> {
        self.inner
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "Netavark port-lifetime registry lock is poisoned".to_owned(),
            })
            .map(|mut entries| entries.remove(&(tenant_id.clone(), sandbox_id.clone())))
    }
}

impl PortManager {
    fn require_releasable_netavark_bindings_with_lifetimes(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
        batch: &OciPortBindLifetimeBatch,
    ) -> Result<Vec<PortLeaseBinding>> {
        self.require_published_listener_provider(PublishedListenerProvider::Netavark)?;
        self.require_binding_lease_identities(tenant_id, sandbox_id, bindings, leases)?;
        if leases.len() != batch.lifetimes().len() || leases.len() != batch.claims().len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "sandbox has {} Netavark listener leases but {} live process lifetimes and {} \
                     bind claims",
                    leases.len(),
                    batch.lifetimes().len(),
                    batch.claims().len()
                ),
            });
        }
        bindings
            .iter()
            .zip(leases)
            .zip(batch.lifetimes())
            .map(|((binding, request), lifetime)| {
                let record = crate::backends::oci::port_lease::require_releasable_provider_binding(
                    &self.state_root,
                    request,
                    binding.host_socket_addr(),
                    OciPortProvider::Netavark,
                )?;
                if lifetime.request() != request
                    || record.active_lifetime() != Some(lifetime.lifetime())
                {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "sandbox Netavark port lease {} is not owned by the retained live \
                             process lifetime",
                            request.lease_id()
                        ),
                    });
                }
                record
                    .binding()
                    .cloned()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "releasable Netavark port lease {} lost exact provider binding \
                             evidence",
                            request.lease_id()
                        ),
                    })
            })
            .collect()
    }

    /// Acquire the only valid cleanup capability for one provider-owned batch.
    ///
    /// Same-process cleanup retains the non-cloneable live lifetime guards.
    /// A fresh process must prove owner death, which quarantines the batch in
    /// `CleanupPending`; neither path treats process death as provider absence.
    pub(crate) fn begin_netavark_cleanup(
        &self,
        registry: &NetavarkPortLifetimeRegistry,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Option<NetavarkPortCleanup>> {
        if leases.is_empty() {
            return Ok(None);
        }
        if let Some(batch) = registry.take(tenant_id, sandbox_id)? {
            match self.require_releasable_netavark_bindings_with_lifetimes(
                tenant_id, sandbox_id, bindings, leases, &batch,
            ) {
                Ok(expected_bindings) => {
                    return Ok(Some(NetavarkPortCleanup {
                        expected_bindings,
                        authority: NetavarkPortLeaseAuthority::Live(batch),
                    }));
                }
                Err(primary) => {
                    return match registry.insert(tenant_id, sandbox_id, batch) {
                        Ok(()) => Err(primary),
                        Err((retention, _batch)) => Err(SandboxError::OperationFailed {
                            message: format!(
                                "{primary}; retaining the live Netavark lifetime batch also \
                                 failed: {retention}"
                            ),
                        }),
                    };
                }
            }
        }
        let (expected_bindings, recoveries) = self
            .recover_netavark_bindings_after_owner_death(tenant_id, sandbox_id, bindings, leases)?;
        Ok(Some(NetavarkPortCleanup {
            expected_bindings,
            authority: NetavarkPortLeaseAuthority::Recovered(recoveries),
        }))
    }

    /// Retain cleanup authority after an ambiguous provider teardown.
    pub(crate) fn retain_ambiguous_netavark_cleanup(
        &self,
        registry: &NetavarkPortLifetimeRegistry,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        cleanup: Option<NetavarkPortCleanup>,
    ) -> Result<()> {
        let Some(cleanup) = cleanup else {
            return Ok(());
        };
        match cleanup.authority {
            NetavarkPortLeaseAuthority::Live(batch) => registry
                .insert(tenant_id, sandbox_id, batch)
                .map_err(|(error, _batch)| error),
            NetavarkPortLeaseAuthority::Recovered(_recoveries) => {
                // Dropping only the exclusive recovery locks leaves the exact
                // durable batch quarantined in CleanupPending for a retry.
                Ok(())
            }
        }
    }

    /// Apply an exact provider-absence receipt to the retained cleanup batch.
    pub(crate) fn complete_netavark_cleanup(
        &self,
        leases: &[PortLeaseRequest],
        cleanup: Option<&NetavarkPortCleanup>,
        release: bool,
    ) -> Result<()> {
        let Some(cleanup) = cleanup else {
            return Ok(());
        };
        match (&cleanup.authority, release) {
            (NetavarkPortLeaseAuthority::Live(batch), false) => self
                .prepare_netavark_bindings_for_rebind_with_lifetimes(
                    leases,
                    &cleanup.expected_bindings,
                    batch,
                ),
            (NetavarkPortLeaseAuthority::Live(batch), true) => self
                .release_netavark_bindings_after_confirmed_stop_with_lifetimes(
                    leases,
                    &cleanup.expected_bindings,
                    batch,
                ),
            (NetavarkPortLeaseAuthority::Recovered(recoveries), false) => self
                .prepare_recovered_netavark_bindings_for_rebind(
                    leases,
                    &cleanup.expected_bindings,
                    recoveries,
                ),
            (NetavarkPortLeaseAuthority::Recovered(recoveries), true) => {
                self.release_recovered_netavark_bindings(leases, recoveries)
            }
        }
    }
}
