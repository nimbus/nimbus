//! Per-tenant network segment allocation.
//!
//! Ends the M1 collision: instead of every tenant drawing from one constant
//! subnet on one shared bridge, each tenant is assigned a distinct subnet carved
//! from the node's super-net, plus a collision-free bridge identity
//! ([`nimbus_core::net::NetworkSegment`]). The allocator is injected
//! HostBridge-style so the single-node implementation here and the future
//! cluster leader — which hands each node a raft-committed, epoch-fenced
//! super-net — satisfy the SAME trait; the sandbox backend consumes it unchanged.
//!
//! State lives at `<state_root>/networks/segments.json` (ABOVE `tenant_root`)
//! guarded by an fs2-exclusive lock, mirroring the IPAM state pattern. Assign is
//! fail-closed until a super-net is installed, so the cluster leg cannot start a
//! workload before its committed lease arrives, and a single-node node installs
//! the node-0 slice at startup so the code path is identical.

// MTN2 lands the allocator + its tests; MTN3 wires `segment_for`/`release` into
// both OCI backends' `network_config(tenant)`. Until that wiring lands the
// allocator is only exercised by unit tests, so the non-test build sees it as
// dead code under `-D unused`. Drop this allow in MTN3 once the backends consume it.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use nimbus_core::TenantId;
use nimbus_core::net::{Cidr, NetworkSegment};

use crate::error::{Result, SandboxError};

/// The default single-node super-net: the node-0 `/16` slice of the cluster pool
/// (`10.0.0.0/8`), so enrolling into a cluster later never re-carves live tenants.
pub(crate) const DEFAULT_NODE_SUPERNET: &str = "10.0.0.0/16";
/// The default per-tenant subnet prefix (`/24` = 253 sandboxes). On-demand block
/// growth for denser packing is MTN6.
pub(crate) const DEFAULT_TENANT_PREFIX: u8 = 24;

/// The node super-net this allocator may carve tenant subnets from. Single-node
/// installs the node-0 slice at epoch 0; the cluster leg installs a raft-committed
/// epoch-fenced lease behind the same type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledSuperNet {
    pub(crate) cidr: Cidr,
    pub(crate) epoch: u64,
}

/// Assigns a distinct, cluster-ready network segment per tenant. Injected into
/// the OCI-family backends so the tenant→segment policy is defined once.
pub(crate) trait NetworkSegmentAllocator: Send + Sync {
    /// Idempotent: return the tenant's segment, assigning a fresh lowest-free
    /// index on first call. Fail-closed if no super-net is installed or the pool
    /// is exhausted.
    fn segment_for(&self, tenant: &TenantId) -> Result<NetworkSegment>;
    /// Free the tenant's index for reuse. MUST be called only after the tenant's
    /// bridge/netns is torn down (the reaper's job, MTN4).
    fn release(&self, tenant: &TenantId) -> Result<()>;
}

#[derive(Default, Serialize, Deserialize)]
struct SegmentState {
    /// The super-net (and its fencing epoch) these assignments were carved under.
    /// A mismatch on load is fail-closed: the node must drain + re-carve, never
    /// silently reuse a stale-epoch block (the cluster reclamation-safety hook).
    supernet_cidr: Option<String>,
    supernet_epoch: Option<u64>,
    /// tenant id → assigned index.
    assignments: BTreeMap<String, u32>,
}

/// Single-node allocator: carves per-tenant subnets from a locally-installed
/// super-net, persisting assignments under `<state_root>/networks/segments.json`.
pub(crate) struct SingleNodeSegmentAllocator {
    networks_root: PathBuf,
    supernet: Option<InstalledSuperNet>,
    tenant_prefix: u8,
}

impl SingleNodeSegmentAllocator {
    pub(crate) fn new(
        state_root: &Path,
        supernet: Option<InstalledSuperNet>,
        tenant_prefix: u8,
    ) -> Self {
        Self {
            networks_root: state_root.join("networks"),
            supernet,
            tenant_prefix,
        }
    }

    /// The launch default: node-0 `/16` at epoch 0, `/24` per tenant.
    pub(crate) fn single_node_default(state_root: &Path) -> Self {
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET)
                .expect("the default node super-net constant must be a valid CIDR"),
            epoch: 0,
        };
        Self::new(state_root, Some(supernet), DEFAULT_TENANT_PREFIX)
    }

    /// Install (or replace) the node super-net. The cluster leg calls this at
    /// membership-commit with its raft-committed, epoch-fenced lease.
    pub(crate) fn install_supernet(&mut self, supernet: InstalledSuperNet) {
        self.supernet = Some(supernet);
    }

    fn state_path(&self) -> PathBuf {
        self.networks_root.join("segments.json")
    }

    fn lock_path(&self) -> PathBuf {
        self.networks_root.join("segments.lock")
    }

    fn installed(&self) -> Result<&InstalledSuperNet> {
        self.supernet
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message:
                    "network segment pool is unassigned: no super-net is installed on this node \
                     (the cluster lease has not arrived); refusing to assign a tenant segment"
                        .to_owned(),
            })
    }

    /// Build the segment for an index, or fail closed if it overflows the pool.
    fn segment_at(&self, supernet: &InstalledSuperNet, index: u32) -> Result<NetworkSegment> {
        let subnet = supernet
            .cidr
            .nth_subnet(self.tenant_prefix, u64::from(index))
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "network segment pool exhausted: node super-net {} cannot carve a /{} at index {index}; raise the node super-net or the per-tenant prefix",
                    supernet.cidr, self.tenant_prefix
                ),
            })?;
        Ok(NetworkSegment::from_index(subnet, index))
    }

    /// Fail closed if the persisted state was carved under a different super-net
    /// or epoch than the one this allocator has installed.
    fn ensure_supernet_matches(
        &self,
        supernet: &InstalledSuperNet,
        state: &SegmentState,
    ) -> Result<()> {
        if let Some(cidr) = state.supernet_cidr.as_deref()
            && cidr != supernet.cidr.to_string()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment state was carved under super-net {cidr}, not the installed {}; drain and re-carve before reuse",
                    supernet.cidr
                ),
            });
        }
        if let Some(epoch) = state.supernet_epoch
            && epoch != supernet.epoch
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "network segment state was carved under super-net epoch {epoch}, not the installed {}; a stale-epoch block must not be reused",
                    supernet.epoch
                ),
            });
        }
        Ok(())
    }

    fn with_state<T>(&self, mutator: impl FnOnce(&mut SegmentState) -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.networks_root).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create network segment directory {}: {error}",
                self.networks_root.display()
            ),
        })?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to open network segment lock: {error}"),
            })?;
        lock.lock_exclusive()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to lock network segment state: {error}"),
            })?;

        let state_path = self.state_path();
        let mut state = if state_path.exists() {
            let contents =
                fs::read(&state_path).map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to read network segment state: {error}"),
                })?;
            serde_json::from_slice::<SegmentState>(&contents).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to parse network segment state: {error}"),
                }
            })?
        } else {
            SegmentState::default()
        };

        let result = mutator(&mut state);
        if result.is_ok() {
            let rendered = serde_json::to_vec_pretty(&state).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to serialize network segment state: {error}"),
                }
            })?;
            fs::write(&state_path, rendered).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to persist network segment state: {error}"),
            })?;
        }
        lock.unlock()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to unlock network segment state: {error}"),
            })?;
        result
    }
}

impl NetworkSegmentAllocator for SingleNodeSegmentAllocator {
    fn segment_for(&self, tenant: &TenantId) -> Result<NetworkSegment> {
        let supernet = self.installed()?.clone();
        let index = self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            if let Some(&index) = state.assignments.get(tenant.as_str()) {
                return Ok(index);
            }
            let used: BTreeSet<u32> = state.assignments.values().copied().collect();
            let index = (0u32..)
                .find(|candidate| !used.contains(candidate))
                .expect("the u32 index space cannot be exhausted by a live tenant set");
            // Fail closed BEFORE committing if this index overflows the pool.
            self.segment_at(&supernet, index)?;
            state.assignments.insert(tenant.as_str().to_owned(), index);
            state.supernet_cidr = Some(supernet.cidr.to_string());
            state.supernet_epoch = Some(supernet.epoch);
            Ok(index)
        })?;
        self.segment_at(&supernet, index)
    }

    fn release(&self, tenant: &TenantId) -> Result<()> {
        self.with_state(|state| {
            state.assignments.remove(tenant.as_str());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    #[test]
    fn two_tenants_get_distinct_subnets_bridges_and_ids() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("assign a");
        let b = allocator
            .segment_for(&tenant("tenant-b"))
            .expect("assign b");

        assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");
        assert!(
            !a.cidr().overlaps(&b.cidr()),
            "tenant subnets must not overlap"
        );
        assert_ne!(a.network_interface(), b.network_interface());
        assert_ne!(a.network_id().as_str(), b.network_id().as_str());
    }

    #[test]
    fn assign_is_idempotent_per_tenant() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let first = allocator.segment_for(&tenant("tenant-a")).expect("assign");
        let again = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("re-assign");
        assert_eq!(first.cidr(), again.cidr());
        assert_eq!(first.network_id().as_str(), again.network_id().as_str());
    }

    #[test]
    fn release_frees_the_index_for_reuse() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("assign a");
        assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
        allocator.release(&tenant("tenant-a")).expect("release a");
        // The freed lowest index is handed to the next new tenant.
        let c = allocator
            .segment_for(&tenant("tenant-c"))
            .expect("assign c");
        assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn exhaustion_fails_closed() {
        let dir = tempdir().expect("temp dir");
        // A /30 super-net carved into /30 tenant subnets holds exactly one tenant.
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse("10.9.0.0/30").unwrap(),
            epoch: 0,
        };
        let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 30);
        allocator
            .segment_for(&tenant("t0"))
            .expect("first tenant fits");
        let error = allocator
            .segment_for(&tenant("t1"))
            .expect_err("second tenant must not fit a single-child super-net");
        assert!(
            format!("{error}").contains("pool exhausted"),
            "exhaustion must fail closed, got: {error}"
        );
    }

    #[test]
    fn assign_fails_closed_until_a_supernet_is_installed() {
        let dir = tempdir().expect("temp dir");
        let mut allocator =
            SingleNodeSegmentAllocator::new(dir.path(), None, DEFAULT_TENANT_PREFIX);
        let error = allocator
            .segment_for(&tenant("tenant-a"))
            .expect_err("no super-net installed must fail closed");
        assert!(
            format!("{error}").contains("unassigned"),
            "must fail closed as unassigned, got: {error}"
        );
        allocator.install_supernet(InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
            epoch: 0,
        });
        let seg = allocator
            .segment_for(&tenant("tenant-a"))
            .expect("assign after install");
        assert_eq!(seg.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn a_stale_epoch_carve_fails_closed_on_load() {
        let dir = tempdir().expect("temp dir");
        let epoch0 = SingleNodeSegmentAllocator::new(
            dir.path(),
            Some(InstalledSuperNet {
                cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
                epoch: 0,
            }),
            DEFAULT_TENANT_PREFIX,
        );
        epoch0
            .segment_for(&tenant("tenant-a"))
            .expect("carve at epoch 0");
        // A later allocator with a bumped epoch must refuse the stale state.
        let epoch1 = SingleNodeSegmentAllocator::new(
            dir.path(),
            Some(InstalledSuperNet {
                cidr: Cidr::parse(DEFAULT_NODE_SUPERNET).unwrap(),
                epoch: 1,
            }),
            DEFAULT_TENANT_PREFIX,
        );
        let error = epoch1
            .segment_for(&tenant("tenant-b"))
            .expect_err("stale-epoch state must fail closed");
        assert!(
            format!("{error}").contains("epoch"),
            "must fail closed on epoch mismatch, got: {error}"
        );
    }
}
