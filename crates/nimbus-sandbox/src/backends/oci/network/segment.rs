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

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use nimbus_core::TenantId;
use nimbus_core::net::{Cidr, NetworkSegment};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

/// The default single-node super-net: the node-0 `/16` slice of the cluster pool
/// (`10.0.0.0/8`), so enrolling into a cluster later never re-carves live tenants.
/// The backend configs default `node_network_supernet` to this same value.
#[cfg(test)]
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
    /// Idempotent read-or-assign: return the tenant's segment (assigning a fresh
    /// lowest-free index on first call) WITHOUT taking a sandbox hold. Fail-closed
    /// if no super-net is installed or the pool is exhausted.
    fn segment_for(&self, tenant: &TenantId) -> Result<NetworkSegment>;
    /// Take a sandbox hold on the tenant's segment (assigning the index on the
    /// first hold). Every started sandbox acquires; the index is not freed while
    /// any hold is live — the crash-safe reaper's refcount.
    fn acquire(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<NetworkSegment>;
    /// Drop a sandbox's hold. Returns [`ReleaseOutcome::TenantDrained`] with EVERY
    /// block bridge to reap when the last hold is gone (all indices are then free),
    /// else [`ReleaseOutcome::StillLive`]. Idempotent.
    fn release(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<ReleaseOutcome>;
    /// Append an additional `/24` block (a new sibling bridge) to a tenant that has
    /// exhausted its current block(s). Fail-closed on pool exhaustion or the
    /// per-tenant block cap. On-demand growth is a bridge CREATE, never a mutation
    /// of a live bridge (netavark has no live subnet-add — MTN6). Wired into
    /// block-aware sandbox placement (`place_sandbox_on_block`).
    fn grow_block(&self, tenant: &TenantId) -> Result<NetworkSegment>;
    /// Whether this allocator gates on a committed cluster lease (fail-closed
    /// without one). The single-node allocator has a config-default super-net, so
    /// this defaults to `false`; the cluster allocator returns `true`. Consumed by
    /// fail-closed cluster admission (`assert_cluster_admission`, MTN7).
    fn requires_cluster_lease(&self) -> bool {
        false
    }
}

/// The result of [`NetworkSegmentAllocator::release`].
pub(crate) enum ReleaseOutcome {
    /// The last sandbox released: the caller reaps EVERY listed block bridge and
    /// all of the tenant's indices are now free for reuse.
    TenantDrained { segments: Vec<NetworkSegment> },
    /// Other sandboxes still hold the tenant's segment; keep the bridges.
    StillLive,
}

/// The maximum number of `/24` blocks (bridges) a single tenant may hold. Each
/// block is 253 sandboxes, so 64 blocks is ~16k sandboxes/tenant — generous —
/// while bounding a runaway tenant's consumption of the node super-net.
const MAX_BLOCKS_PER_TENANT: usize = 64;

/// A tenant's allocation: its ordered list of block indices (element 0 is the
/// primary/anchor bridge; additional blocks are appended on-demand by
/// `grow_block` when a block's `/24` exhausts — each index is a self-contained
/// single-subnet bridge) plus the set of live sandbox ids across all its blocks.
/// The whole allocation is freed only when the last hold releases.
#[derive(Default, Serialize, Deserialize)]
struct TenantEntry {
    indices: Vec<u32>,
    #[serde(default)]
    live_sandboxes: BTreeSet<String>,
}

impl TenantEntry {
    /// The tenant's primary/anchor block index (element 0). A persisted entry is
    /// never empty once assigned.
    fn primary(&self) -> u32 {
        *self
            .indices
            .first()
            .expect("a persisted tenant entry always has at least its primary block")
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SegmentState {
    /// The super-net (and its fencing epoch) these assignments were carved under.
    /// A mismatch on load is fail-closed: the node must drain + re-carve, never
    /// silently reuse a stale-epoch block (the cluster reclamation-safety hook).
    supernet_cidr: Option<String>,
    supernet_epoch: Option<u64>,
    /// tenant id → its allocation (index + live-sandbox refcount).
    tenants: BTreeMap<String, TenantEntry>,
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
    #[cfg(test)]
    pub(crate) fn single_node_default(state_root: &Path) -> Self {
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse(DEFAULT_NODE_SUPERNET)
                .expect("the default node super-net constant must be a valid CIDR"),
            epoch: 0,
        };
        Self::new(state_root, Some(supernet), DEFAULT_TENANT_PREFIX)
    }

    /// Build a single-node allocator carving `/24` tenant subnets from the given
    /// node super-net (the configurable knob a backend passes from its config).
    pub(crate) fn for_node_supernet(
        state_root: &Path,
        supernet: &str,
        tenant_prefix: u8,
    ) -> Result<Self> {
        let cidr = Cidr::parse(supernet).map_err(|error| SandboxError::InvalidSpec {
            message: format!("invalid node network super-net {supernet:?}: {error}"),
        })?;
        Ok(Self::new(
            state_root,
            Some(InstalledSuperNet { cidr, epoch: 0 }),
            tenant_prefix,
        ))
    }

    /// Install (or replace) the node super-net. The cluster leg calls this at
    /// membership-commit with its raft-committed, epoch-fenced lease.
    // Wired by the cluster ClusterSegmentAllocator in MTN7; test-only until then.
    #[allow(dead_code)]
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

impl SingleNodeSegmentAllocator {
    /// The lowest index held by NO tenant's block — the M1 collision guard: it
    /// unions every tenant's whole block list, so a grown tenant's extra blocks
    /// are never re-handed to another tenant. Fail-closed if the index overflows
    /// the node super-net.
    fn next_free_index(&self, supernet: &InstalledSuperNet, state: &SegmentState) -> Result<u32> {
        let used: BTreeSet<u32> = state
            .tenants
            .values()
            .flat_map(|entry| entry.indices.iter().copied())
            .collect();
        let index = (0u32..)
            .find(|candidate| !used.contains(candidate))
            .expect("the u32 index space cannot be exhausted by a live tenant set");
        // Fail closed BEFORE the caller commits if this index overflows the pool.
        self.segment_at(supernet, index)?;
        Ok(index)
    }

    /// Get-or-assign the tenant's PRIMARY block index under a held state lock,
    /// fail-closed on a stale super-net or pool exhaustion.
    fn assign_index(
        &self,
        supernet: &InstalledSuperNet,
        state: &mut SegmentState,
        tenant: &TenantId,
    ) -> Result<u32> {
        self.ensure_supernet_matches(supernet, state)?;
        if let Some(entry) = state.tenants.get(tenant.as_str()) {
            return Ok(entry.primary());
        }
        let index = self.next_free_index(supernet, state)?;
        state.tenants.insert(
            tenant.as_str().to_owned(),
            TenantEntry {
                indices: vec![index],
                live_sandboxes: BTreeSet::new(),
            },
        );
        state.supernet_cidr = Some(supernet.cidr.to_string());
        state.supernet_epoch = Some(supernet.epoch);
        Ok(index)
    }

    /// Startup orphan GC: reconcile persisted holds against the set of live
    /// `(tenant_id, sandbox_id)` pairs (from live manifests). Prune crash-leaked
    /// holds and, for every tenant left with no live sandbox, free its index and
    /// return its segment so the caller can reap the orphaned bridge. Fail-closed
    /// on a missing super-net (never reclaim blind). Wired into both backends'
    /// startup via `reconcile_network_segment_orphans`.
    pub(crate) fn reconcile_orphans(
        &self,
        live: &BTreeSet<(String, String)>,
    ) -> Result<Vec<NetworkSegment>> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            let mut drained = Vec::new();
            let tenants: Vec<String> = state.tenants.keys().cloned().collect();
            for tenant in tenants {
                let entry = state
                    .tenants
                    .get_mut(&tenant)
                    .expect("tenant key came from the same map");
                entry
                    .live_sandboxes
                    .retain(|sandbox| live.contains(&(tenant.clone(), sandbox.clone())));
                if entry.live_sandboxes.is_empty() {
                    let indices = entry.indices.clone();
                    state.tenants.remove(&tenant);
                    // A drained tenant releases EVERY block bridge it grew.
                    for index in indices {
                        drained.push(self.segment_at(&supernet, index)?);
                    }
                }
            }
            Ok(drained)
        })
    }
}

impl NetworkSegmentAllocator for SingleNodeSegmentAllocator {
    fn segment_for(&self, tenant: &TenantId) -> Result<NetworkSegment> {
        let supernet = self.installed()?.clone();
        let index = self.with_state(|state| self.assign_index(&supernet, state, tenant))?;
        self.segment_at(&supernet, index)
    }

    fn acquire(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<NetworkSegment> {
        let supernet = self.installed()?.clone();
        let index = self.with_state(|state| {
            let index = self.assign_index(&supernet, state, tenant)?;
            state
                .tenants
                .get_mut(tenant.as_str())
                .expect("assign_index inserts the tenant entry")
                .live_sandboxes
                .insert(sandbox_id.as_str().to_owned());
            Ok(index)
        })?;
        self.segment_at(&supernet, index)
    }

    fn release(&self, tenant: &TenantId, sandbox_id: &SandboxId) -> Result<ReleaseOutcome> {
        let supernet = self.installed()?.clone();
        self.with_state(|state| {
            let Some(entry) = state.tenants.get_mut(tenant.as_str()) else {
                return Ok(ReleaseOutcome::StillLive);
            };
            entry.live_sandboxes.remove(sandbox_id.as_str());
            if entry.live_sandboxes.is_empty() {
                let indices = entry.indices.clone();
                state.tenants.remove(tenant.as_str());
                // Drain releases EVERY block bridge the tenant grew.
                let mut segments = Vec::with_capacity(indices.len());
                for index in indices {
                    segments.push(self.segment_at(&supernet, index)?);
                }
                Ok(ReleaseOutcome::TenantDrained { segments })
            } else {
                Ok(ReleaseOutcome::StillLive)
            }
        })
    }

    fn grow_block(&self, tenant: &TenantId) -> Result<NetworkSegment> {
        let supernet = self.installed()?.clone();
        let index = self.with_state(|state| {
            self.ensure_supernet_matches(&supernet, state)?;
            let entry = state
                .tenants
                .get(tenant.as_str())
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "cannot grow a block for tenant {} before its primary block is assigned",
                        tenant.as_str()
                    ),
                })?;
            if entry.indices.len() >= MAX_BLOCKS_PER_TENANT {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "tenant {} already holds the maximum of {MAX_BLOCKS_PER_TENANT} network blocks; \
                         raise the per-tenant subnet prefix or MAX_BLOCKS_PER_TENANT",
                        tenant.as_str()
                    ),
                });
            }
            // Fail-closed on pool exhaustion happens inside next_free_index.
            let index = self.next_free_index(&supernet, state)?;
            state
                .tenants
                .get_mut(tenant.as_str())
                .expect("tenant entry checked above")
                .indices
                .push(index);
            Ok(index)
        })?;
        self.segment_at(&supernet, index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn sandbox(id: &str) -> SandboxId {
        SandboxId::new(id)
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
    fn refcount_frees_the_index_only_after_the_last_sandbox_releases() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");

        // Two sandboxes of tenant-a hold the same segment.
        let s1 = allocator
            .acquire(&a, &sandbox("sb-1"))
            .expect("acquire sb-1");
        let s2 = allocator
            .acquire(&a, &sandbox("sb-2"))
            .expect("acquire sb-2");
        assert_eq!(s1.cidr().to_string(), "10.0.0.0/24");
        assert_eq!(s1.cidr(), s2.cidr(), "same tenant shares one segment");

        // Releasing one leaves the tenant live — the bridge stays, index held.
        assert!(matches!(
            allocator
                .release(&a, &sandbox("sb-1"))
                .expect("release sb-1"),
            ReleaseOutcome::StillLive
        ));
        // A fresh tenant does NOT get tenant-a's still-held index.
        let b = allocator
            .acquire(&tenant("tenant-b"), &sandbox("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

        // Releasing the LAST sandbox drains the tenant and frees the index.
        assert!(matches!(
            allocator
                .release(&a, &sandbox("sb-2"))
                .expect("release sb-2"),
            ReleaseOutcome::TenantDrained { .. }
        ));
        // The freed lowest index (10.0.0.0/24) is handed to the next new tenant.
        let c = allocator
            .acquire(&tenant("tenant-c"), &sandbox("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.0.0/24");

        // Releasing an unknown sandbox is idempotent.
        assert!(matches!(
            allocator
                .release(&tenant("nobody"), &sandbox("ghost"))
                .expect("release ghost"),
            ReleaseOutcome::StillLive
        ));
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

    #[test]
    fn concurrent_acquire_release_across_threads_stays_consistent_under_the_lock() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().expect("temp dir");
        let root = Arc::new(dir.path().to_path_buf());
        // 8 threads, each a distinct tenant, contending on the shared on-disk
        // segments.json via the fs-exclusive lock: acquire a sole sandbox then
        // release it, which must drain the tenant.
        let handles: Vec<_> = (0..8u32)
            .map(|i| {
                let root = Arc::clone(&root);
                thread::spawn(move || {
                    let allocator = SingleNodeSegmentAllocator::single_node_default(&root);
                    let tenant = tenant(&format!("t-{i}"));
                    let sandbox = sandbox(&format!("sb-{i}"));
                    let segment = allocator.acquire(&tenant, &sandbox).expect("acquire");
                    assert!(segment.cidr().to_string().starts_with("10.0."));
                    matches!(
                        allocator.release(&tenant, &sandbox).expect("release"),
                        ReleaseOutcome::TenantDrained { .. }
                    )
                })
            })
            .collect();
        for handle in handles {
            assert!(handle.join().expect("thread should not panic"));
        }
        // Every tenant drained, so the freed lowest index is reused: the next new
        // tenant gets 10.0.0.0/24 (no leaked reservations under contention).
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let segment = allocator
            .acquire(&tenant("after"), &sandbox("sb"))
            .expect("acquire after drain");
        assert_eq!(segment.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn reconcile_orphans_prunes_leaked_holds_and_drains_empty_tenants() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        // tenant-a holds two sandboxes; sb-1 will be a crash-leaked hold, sb-2 live.
        allocator
            .acquire(&tenant("tenant-a"), &sandbox("sb-1"))
            .expect("acquire a/1");
        allocator
            .acquire(&tenant("tenant-a"), &sandbox("sb-2"))
            .expect("acquire a/2");
        // tenant-b holds one sandbox, fully crash-leaked (nothing live).
        let b = allocator
            .acquire(&tenant("tenant-b"), &sandbox("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.1.0/24");

        // Only tenant-a/sb-2 is actually live at startup.
        let mut live = BTreeSet::new();
        live.insert(("tenant-a".to_owned(), "sb-2".to_owned()));
        let drained = allocator.reconcile_orphans(&live).expect("reconcile");

        // tenant-b fully orphaned -> drained (its bridge must be reaped), segment returned.
        assert_eq!(drained.len(), 1, "only the fully-orphaned tenant drains");
        assert_eq!(drained[0].cidr().to_string(), "10.0.1.0/24");
        // tenant-a still holds sb-2, so its index 0 is retained; the freed index 1
        // is what the next new tenant reuses.
        let c = allocator
            .acquire(&tenant("tenant-c"), &sandbox("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.1.0/24");
        // tenant-a's still-live sandbox keeps its original segment.
        let a = allocator
            .acquire(&tenant("tenant-a"), &sandbox("sb-2"))
            .expect("re-acquire a/2");
        assert_eq!(a.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn grow_block_appends_a_distinct_block_and_never_collides_across_tenants() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");
        // tenant-a's primary block = index 0 (10.0.0.0/24).
        let a0 = allocator.acquire(&a, &sandbox("sb-a")).expect("acquire a");
        assert_eq!(a0.cidr().to_string(), "10.0.0.0/24");
        // Grow tenant-a: a second, distinct block/bridge at index 1.
        let a1 = allocator.grow_block(&a).expect("grow a");
        assert_eq!(a1.cidr().to_string(), "10.0.1.0/24");
        assert_ne!(a0.network_interface(), a1.network_interface());
        assert_ne!(a0.network_id().as_str(), a1.network_id().as_str());
        // The M1 guard: a DIFFERENT tenant must NEVER be handed tenant-a's grown
        // index 1 — the unioned lowest-free scan skips it to index 2.
        let b = allocator
            .acquire(&tenant("tenant-b"), &sandbox("sb-b"))
            .expect("acquire b");
        assert_eq!(b.cidr().to_string(), "10.0.2.0/24");
    }

    #[test]
    fn draining_a_multi_block_tenant_returns_every_block_to_reap() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let a = tenant("tenant-a");
        allocator.acquire(&a, &sandbox("sb-1")).expect("acquire");
        allocator.grow_block(&a).expect("grow to block 1");
        allocator.grow_block(&a).expect("grow to block 2");

        // The sole sandbox's release drains the tenant and returns ALL 3 block
        // bridges so the caller reaps every one.
        let ReleaseOutcome::TenantDrained { segments } =
            allocator.release(&a, &sandbox("sb-1")).expect("release")
        else {
            panic!("expected the last release to drain the tenant");
        };
        let subnets: Vec<String> = segments.iter().map(|s| s.cidr().to_string()).collect();
        assert_eq!(subnets, ["10.0.0.0/24", "10.0.1.0/24", "10.0.2.0/24"]);
        // All 3 indices are freed, so the next new tenant reuses index 0.
        let c = allocator
            .acquire(&tenant("tenant-c"), &sandbox("sb-c"))
            .expect("acquire c");
        assert_eq!(c.cidr().to_string(), "10.0.0.0/24");
    }

    #[test]
    fn grow_block_fails_closed_at_pool_exhaustion() {
        let dir = tempdir().expect("temp dir");
        // A /24 super-net carved into /24 blocks holds exactly ONE block.
        let supernet = InstalledSuperNet {
            cidr: Cidr::parse("10.9.0.0/24").unwrap(),
            epoch: 0,
        };
        let allocator = SingleNodeSegmentAllocator::new(dir.path(), Some(supernet), 24);
        let t = tenant("t0");
        allocator
            .acquire(&t, &sandbox("sb"))
            .expect("primary block fits");
        let error = allocator
            .grow_block(&t)
            .expect_err("a second block must not fit a single-child super-net");
        assert!(
            format!("{error}").contains("pool exhausted"),
            "grow must fail closed on exhaustion, got: {error}"
        );
    }

    #[test]
    // NNC0.4 fail-before: the current parser does reject torn JSON, but its
    // diagnostic omits the authority path. NNC2.1 owns the actionable,
    // versioned corruption error and removal of this ignore marker.
    #[ignore = "NNC0.4 expected red until torn segment state names its authority path"]
    fn torn_segment_state_error_must_name_the_authority_path() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        allocator
            .acquire(&tenant("tenant-original"), &sandbox("sandbox-original"))
            .expect("original segment should allocate");
        let state_path = allocator.state_path();
        fs::write(&state_path, b"{").expect("torn state should be installed");

        let restarted = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let error = restarted
            .acquire(
                &tenant("tenant-replacement"),
                &sandbox("sandbox-replacement"),
            )
            .expect_err("torn segment JSON must fail closed");
        let rendered = error.to_string();
        assert!(
            rendered.contains("failed to parse network segment state"),
            "the failure must reach the segment-state parse boundary: {rendered}"
        );
        assert!(
            rendered.contains(&state_path.display().to_string()),
            "a corruption diagnostic must name the affected authority path: {rendered}"
        );
    }

    #[test]
    // NNC0.4 fail-before: this valid JSON erases the committed owner without
    // tripping serde. NNC2.1 owns the checksum/version envelope, will make the
    // final fail-closed assertion pass, and must remove this ignore marker.
    #[ignore = "NNC0.4 expected red until checksums reject valid segment-state corruption"]
    fn semantically_valid_segment_state_corruption_must_not_reissue_a_live_segment() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let original = allocator
            .acquire(&tenant("tenant-original"), &sandbox("sandbox-original"))
            .expect("original segment should allocate");
        fs::write(
            allocator.state_path(),
            br#"{
  "supernet_cidr": "10.0.0.0/16",
  "supernet_epoch": 0,
  "tenants": {}
}"#,
        )
        .expect("semantically corrupt state should be installed");

        let restarted = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let replacement = restarted.acquire(
            &tenant("tenant-replacement"),
            &sandbox("sandbox-replacement"),
        );
        match replacement.as_ref() {
            Ok(segment) => assert_eq!(
                segment.cidr(),
                original.cidr(),
                "the unchecked corruption must expose the audited live-segment reuse"
            ),
            Err(error) => {
                let rendered = error.to_string();
                assert!(
                    ["checksum", "corrupt", "integrity", "version"]
                        .iter()
                        .any(|needle| rendered.to_ascii_lowercase().contains(needle)),
                    "a fixed store must reject corruption with a named integrity error: {rendered}"
                );
            }
        }
        assert!(
            replacement.is_err(),
            "semantically valid corruption must fail closed instead of reissuing a live segment"
        );
    }
}
