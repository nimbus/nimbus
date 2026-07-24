//! Durable inspection and cleanup authority for fenced segment allocations.
//!
//! This restricted capability is reconstructed from the checksummed local
//! authority. It deliberately has no assign, acquire, or grow methods, so lease
//! expiry can revoke creation without stranding provider cleanup.

use super::*;

/// Restricted view of durable segment state used after cluster create
/// authority expires.
///
/// Constructing it from the persisted super-net and epoch cannot accidentally
/// grant assign/acquire/grow authority, and it continues to work after restart
/// or after the lease provider stops reporting the old grant.
pub(crate) struct DurableSegmentCleanupAuthority {
    inner: SingleNodeSegmentAllocator,
}

impl SingleNodeSegmentAllocator {
    fn for_durable_cleanup(state_root: &Path, tenant_prefix: u8) -> Result<Option<Self>> {
        let store = LocalNetworkStateStore::open(state_root).map_err(network_store_error)?;
        let Some(state) = store
            .read::<SegmentState>(&NetworkStatePartition::SegmentAllocations)
            .map_err(network_store_error)?
        else {
            return Ok(None);
        };
        let supernet = match (state.supernet_cidr.as_deref(), state.supernet_epoch) {
            (Some(cidr), Some(epoch)) => InstalledSuperNet {
                cidr: Cidr::parse(cidr).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "network segment authority contains invalid durable super-net {cidr:?}: {error}"
                    ),
                })?,
                epoch,
            },
            (None, None) if state.tenants.is_empty() => return Ok(None),
            (cidr, epoch) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "network segment authority has incomplete durable cleanup fencing: super-net={cidr:?}, epoch={epoch:?}"
                    ),
                });
            }
        };
        Ok(Some(Self {
            store,
            supernet: Some(supernet),
            tenant_prefix,
        }))
    }
}

impl DurableSegmentCleanupAuthority {
    pub(crate) fn open(state_root: &Path, tenant_prefix: u8) -> Result<Option<Self>> {
        SingleNodeSegmentAllocator::for_durable_cleanup(state_root, tenant_prefix)
            .map(|inner| inner.map(|inner| Self { inner }))
    }

    pub(crate) fn inspect_segments(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<Vec<OciSegmentRealization>>> {
        self.inner.inspect_segments(tenant)
    }

    pub(crate) fn quarantine(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentQuarantineOutcome> {
        self.inner.quarantine(tenant, attachment_id)
    }

    pub(crate) fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentReleaseOutcome<OciSegmentRealization>> {
        self.inner.release(tenant, attachment_id)
    }

    pub(crate) fn finalize_release(
        &self,
        cleanup: &NetworkSegmentCleanup<OciSegmentRealization>,
    ) -> Result<NetworkSegmentFinalizeOutcome> {
        self.inner.finalize_release(cleanup)
    }

    pub(crate) fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<OciSegmentRealization>> {
        self.inner.reconcile_orphans(live)
    }
}
