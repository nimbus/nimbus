//! Seam E: `VolumeProvider` — the durable sandbox filesystem seam.
//!
//! A sandbox declares a filesystem by a **persistence policy** plus a
//! pluggable **backing**; the provider yields a mount source the sandbox
//! layer binds into the guest and owns snapshot / fork / migrate. This is
//! Seam E of the storage-seams spec, deliberately distinct from the
//! in-process `nimbus-fs` isolate filesystem: the CAS face there is
//! read-only, while the live mutable layer is a REAL filesystem behind this
//! seam — never CAS read-modify-rewrite.
//!
//! v1 lands the seam plus the launch-default backing:
//! - [`LocalDirVolume`] — a real host directory (ext4/xfs semantics, true
//!   random-write), serving `Ephemeral`, `Snapshot`, and
//!   `Persistent { backing: LocalDir }` — persistence is a POLICY, not a
//!   different storage engine.
//! - Snapshots are **content-addressed blobs** on the Seam A byte plane
//!   (`nimbus-blob`, BLAKE3): `snapshot()` serializes the volume with a
//!   deterministic v1 archive format (sorted walk), so identical content
//!   yields the identical `SnapshotId`; `fork()` materializes a snapshot
//!   into a new sandbox's volume — the S-band zeroboot filesystem leg.
//! - `ObjectFs` / `ClusterFs` backings, `External` mounts, and
//!   `migrate_in` are NAMED and fail closed (`unsupported`) until their
//!   owning lanes land them (sandbox-plan bands / HS). No silent fallback.
//!
//! The `nimbus-sandbox → nimbus-blob` edge introduced here is sanctioned by
//! the storage-seams spec and is acyclic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_blob::{BlobHash, BlobStore};
use nimbus_core::TenantId;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

/// Opaque identity of a provisioned volume. v1 derives it deterministically
/// from `(tenant, sandbox)` at provision time (one primary volume per
/// sandbox); the derivation is a provider detail, not a contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VolumeId(String);

impl VolumeId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn derive(tenant: &TenantId, sandbox: &SandboxId) -> Self {
        Self(format!("{}/{}", tenant.as_str(), sandbox.as_str()))
    }
}

/// Content-addressed snapshot identity: the BLAKE3 hash of the volume's
/// deterministic archive on the Seam A byte plane. Shareable, verifiable,
/// cluster-replicable by construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotId(BlobHash);

impl SnapshotId {
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    pub fn blob_hash(&self) -> &BlobHash {
        &self.0
    }
}

/// What the sandbox layer binds into the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountSource {
    /// Host directory for virtio-fs passthrough (the `LocalDir` shape).
    HostPath(PathBuf),
    /// Block device for virtio-blk (future `ObjectFs` shape).
    BlockDevice(PathBuf),
}

/// Persistence policy — the lifecycle is the policy, not a storage engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumePolicy {
    /// Scratch; wiped on teardown.
    Ephemeral,
    /// Real host FS, snapshotted to the byte plane on demand (the caller
    /// drives `snapshot()`); data is retained on teardown.
    Snapshot,
    /// CoW from a content-addressed snapshot (zeroboot / S-band fork path).
    Fork { from: SnapshotId },
    /// Survives teardown; backing is pluggable.
    Persistent { backing: VolumeBacking },
    /// NAS / NFS / SMB / external S3-backed FS (named; not yet supported).
    External { mount: String },
}

/// Pluggable backing for `Persistent` volumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeBacking {
    /// Host ext4/xfs directory (launch default; disk-limit quota home).
    LocalDir,
    /// Live mutable FS over object storage (named; owning lane not landed).
    ObjectFs,
    /// S3-as-FS across cluster nodes (named; HS-gated).
    ClusterFs,
}

/// Where a migrating volume's bytes come from (HS lane; named only in v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementSource {
    CloudPrimary,
    Peer(String),
}

/// Seam E. Consumed as `Arc<dyn VolumeProvider>`; object-safe.
#[async_trait]
pub trait VolumeProvider: Send + Sync {
    /// Provision a mount source for this sandbox under the policy.
    async fn provision(
        &self,
        tenant: &TenantId,
        sandbox: &SandboxId,
        policy: VolumePolicy,
    ) -> Result<MountSource>;

    /// Snapshot current FS state into the byte plane (content-addressed).
    async fn snapshot(&self, vol: &VolumeId) -> Result<SnapshotId>;

    /// Fork a snapshot into a new CoW volume for `into`.
    async fn fork(&self, snap: &SnapshotId, into: &SandboxId) -> Result<MountSource>;

    /// Make a volume available on this node (cluster migration; HS lane).
    async fn migrate_in(&self, vol: &VolumeId, from: PlacementSource) -> Result<()>;

    /// Tear a volume down per its policy (`Ephemeral` wipes; others retain).
    async fn teardown(&self, vol: &VolumeId) -> Result<()>;
}

#[derive(Debug, Clone)]
struct ProvisionedVolume {
    path: PathBuf,
    wipe_on_teardown: bool,
}

/// The launch-default backing: a real host directory per volume under a
/// provider root. True random-write semantics; the disk-limit project-quota
/// knob applies HERE (host filesystem quota), not to object/cluster
/// backings.
pub struct LocalDirVolume {
    root: PathBuf,
    /// Seam A handle for snapshot/fork. `None` = no byte plane configured:
    /// snapshot/fork fail closed instead of inventing local state.
    blob_store: Option<Arc<dyn BlobStore>>,
    volumes: Mutex<HashMap<VolumeId, ProvisionedVolume>>,
}

impl LocalDirVolume {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            blob_store: None,
            volumes: Mutex::new(HashMap::new()),
        }
    }

    /// Attach the Seam A byte plane, enabling snapshot/fork.
    pub fn with_blob_store(mut self, blob_store: Arc<dyn BlobStore>) -> Self {
        self.blob_store = Some(blob_store);
        self
    }

    fn volume_dir(&self, tenant: &TenantId, sandbox: &SandboxId) -> PathBuf {
        self.root.join(tenant.as_str()).join(sandbox.as_str())
    }

    fn register(&self, id: VolumeId, path: PathBuf, wipe_on_teardown: bool) -> Result<()> {
        let mut volumes = self.volumes.lock().map_err(|_| poisoned())?;
        volumes.insert(
            id,
            ProvisionedVolume {
                path,
                wipe_on_teardown,
            },
        );
        Ok(())
    }

    fn resolve(&self, vol: &VolumeId) -> Result<ProvisionedVolume> {
        let volumes = self.volumes.lock().map_err(|_| poisoned())?;
        volumes
            .get(vol)
            .cloned()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "volume {} is not provisioned on this provider",
                    vol.as_str()
                ),
            })
    }

    fn store(&self) -> Result<&Arc<dyn BlobStore>> {
        self.blob_store
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "volume snapshot/fork requires a configured byte plane (BlobStore); \
                          none is attached — failing closed"
                    .to_owned(),
            })
    }
}

fn poisoned() -> SandboxError {
    SandboxError::OperationFailed {
        message: "volume provider registry lock is poisoned".to_owned(),
    }
}

#[async_trait]
impl VolumeProvider for LocalDirVolume {
    async fn provision(
        &self,
        tenant: &TenantId,
        sandbox: &SandboxId,
        policy: VolumePolicy,
    ) -> Result<MountSource> {
        match policy {
            VolumePolicy::Ephemeral | VolumePolicy::Snapshot => {
                let dir = self.volume_dir(tenant, sandbox);
                create_dir_all(&dir).await?;
                let wipe = matches!(policy, VolumePolicy::Ephemeral);
                self.register(VolumeId::derive(tenant, sandbox), dir.clone(), wipe)?;
                Ok(MountSource::HostPath(dir))
            }
            VolumePolicy::Persistent {
                backing: VolumeBacking::LocalDir,
            } => {
                let dir = self.volume_dir(tenant, sandbox);
                create_dir_all(&dir).await?;
                self.register(VolumeId::derive(tenant, sandbox), dir.clone(), false)?;
                Ok(MountSource::HostPath(dir))
            }
            VolumePolicy::Fork { from } => {
                let source = self.fork(&from, sandbox).await?;
                // fork() registered the volume under `into`'s derived id with
                // retain semantics; a forked workspace is not scratch.
                let _ = tenant; // identity carried by the derived id below
                Ok(source)
            }
            VolumePolicy::Persistent {
                backing: VolumeBacking::ObjectFs,
            } => Err(unsupported(
                "Persistent{ObjectFs}",
                "sandbox-plan ObjectFs lane",
            )),
            VolumePolicy::Persistent {
                backing: VolumeBacking::ClusterFs,
            } => Err(unsupported(
                "Persistent{ClusterFs}",
                "horizontal-scaling lane",
            )),
            VolumePolicy::External { .. } => {
                Err(unsupported("External mounts", "external-mount lane"))
            }
        }
    }

    async fn snapshot(&self, vol: &VolumeId) -> Result<SnapshotId> {
        let store = self.store()?;
        let provisioned = self.resolve(vol)?;
        let archive = serialize_volume(&provisioned.path).await?;
        let hash = store
            .put(archive)
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to write volume snapshot blob: {error}"),
            })?;
        Ok(SnapshotId(hash))
    }

    async fn fork(&self, snap: &SnapshotId, into: &SandboxId) -> Result<MountSource> {
        let store = self.store()?;
        let archive = store
            .get(&snap.0)
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to read volume snapshot {}: {error}", snap.to_hex()),
            })?;
        let dir = self.root.join("forks").join(into.as_str());
        materialize_volume(&archive, &dir).await?;
        self.register(
            VolumeId(format!("forks/{}", into.as_str())),
            dir.clone(),
            false,
        )?;
        Ok(MountSource::HostPath(dir))
    }

    async fn migrate_in(&self, _vol: &VolumeId, _from: PlacementSource) -> Result<()> {
        Err(unsupported("volume migration", "horizontal-scaling lane"))
    }

    async fn teardown(&self, vol: &VolumeId) -> Result<()> {
        let provisioned = self.resolve(vol)?;
        if provisioned.wipe_on_teardown {
            tokio::fs::remove_dir_all(&provisioned.path)
                .await
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to wipe ephemeral volume {}: {error}",
                        provisioned.path.display()
                    ),
                })?;
        }
        let mut volumes = self.volumes.lock().map_err(|_| poisoned())?;
        volumes.remove(vol);
        Ok(())
    }
}

fn unsupported(what: &str, owner: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "{what} are not supported by LocalDirVolume yet (named follow-on: {owner}); \
             failing closed"
        ),
    }
}

async fn create_dir_all(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to create volume dir {}: {error}", dir.display()),
        })
}

// ---------------------------------------------------------------------------
// v1 deterministic archive format (snapshot wire format)
//
// [u32 path_len][path bytes (relative, `/`-separated)][u64 file_len][bytes]…
// Entries are emitted in sorted path order, so identical content always
// produces identical bytes and therefore an identical content address. Only
// regular files and directories are supported; symlinks fail closed (a
// snapshot must not silently capture link targets outside the volume). The
// chunked/CDC layout question is an open item in the storage-seams spec (§6)
// and can replace this format behind the same SnapshotId contract.
// ---------------------------------------------------------------------------

async fn serialize_volume(dir: &Path) -> Result<Bytes> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let mut entries =
            tokio::fs::read_dir(&current)
                .await
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to walk volume {}: {error}", current.display()),
                })?;
        while let Some(entry) =
            entries
                .next_entry()
                .await
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to walk volume {}: {error}", current.display()),
                })?
        {
            let path = entry.path();
            let kind = entry
                .file_type()
                .await
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to stat {}: {error}", path.display()),
                })?;
            if kind.is_symlink() {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "volume snapshot refuses symlink {} (fail-closed: a snapshot must not \
                         capture targets outside the volume)",
                        path.display()
                    ),
                });
            }
            if kind.is_dir() {
                pending.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|_| SandboxError::OperationFailed {
                        message: format!("volume walk escaped root at {}", path.display()),
                    })?
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                files.push((rel, path));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out: Vec<u8> = Vec::new();
    for (rel, path) in files {
        let contents =
            tokio::fs::read(&path)
                .await
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to read {}: {error}", path.display()),
                })?;
        out.extend_from_slice(&(rel.len() as u32).to_le_bytes());
        out.extend_from_slice(rel.as_bytes());
        out.extend_from_slice(&(contents.len() as u64).to_le_bytes());
        out.extend_from_slice(&contents);
    }
    Ok(Bytes::from(out))
}

async fn materialize_volume(archive: &Bytes, dir: &Path) -> Result<()> {
    create_dir_all(dir).await?;
    let mut cursor = 0usize;
    let data = archive.as_ref();
    while cursor < data.len() {
        let take = |from: usize, len: usize| -> Result<&[u8]> {
            data.get(from..from + len)
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "volume snapshot archive is truncated".to_owned(),
                })
        };
        let path_len = u32::from_le_bytes(take(cursor, 4)?.try_into().unwrap()) as usize;
        cursor += 4;
        let rel = std::str::from_utf8(take(cursor, path_len)?).map_err(|_| {
            SandboxError::OperationFailed {
                message: "volume snapshot archive contains a non-UTF8 path".to_owned(),
            }
        })?;
        if rel.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "volume snapshot archive path {rel:?} is not a clean relative path"
                ),
            });
        }
        cursor += path_len;
        let file_len = u64::from_le_bytes(take(cursor, 8)?.try_into().unwrap()) as usize;
        cursor += 8;
        let contents = take(cursor, file_len)?;
        cursor += file_len;

        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            create_dir_all(parent).await?;
        }
        tokio::fs::write(&target, contents).await.map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to materialize {}: {error}", target.display()),
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_blob::MemoryBlobStore;

    fn provider(root: &Path) -> LocalDirVolume {
        LocalDirVolume::new(root).with_blob_store(Arc::new(MemoryBlobStore::new()))
    }

    fn tenant() -> TenantId {
        TenantId::new("tenant-a").expect("test tenant")
    }

    fn sandbox(raw: &str) -> SandboxId {
        SandboxId::new(raw)
    }

    #[tokio::test]
    async fn ephemeral_volume_is_wiped_on_teardown_persistent_survives() {
        let root = tempfile::tempdir().expect("tempdir");
        let provider = provider(root.path());

        let MountSource::HostPath(eph) = provider
            .provision(&tenant(), &sandbox("scratch"), VolumePolicy::Ephemeral)
            .await
            .expect("ephemeral provisions")
        else {
            panic!("LocalDir yields HostPath");
        };
        tokio::fs::write(eph.join("work.txt"), b"scratch")
            .await
            .unwrap();
        provider
            .teardown(&VolumeId::derive(&tenant(), &sandbox("scratch")))
            .await
            .expect("teardown");
        assert!(!eph.exists(), "ephemeral volume must be wiped on teardown");

        let MountSource::HostPath(per) = provider
            .provision(
                &tenant(),
                &sandbox("keep"),
                VolumePolicy::Persistent {
                    backing: VolumeBacking::LocalDir,
                },
            )
            .await
            .expect("persistent provisions")
        else {
            panic!("HostPath expected");
        };
        tokio::fs::write(per.join("data.txt"), b"durable")
            .await
            .unwrap();
        provider
            .teardown(&VolumeId::derive(&tenant(), &sandbox("keep")))
            .await
            .expect("teardown");
        assert!(
            per.join("data.txt").exists(),
            "persistent volume data survives teardown (persistence is the policy)"
        );
    }

    #[tokio::test]
    async fn snapshot_is_content_addressed_and_fork_round_trips() {
        let root = tempfile::tempdir().expect("tempdir");
        let provider = provider(root.path());
        let MountSource::HostPath(dir) = provider
            .provision(&tenant(), &sandbox("src"), VolumePolicy::Snapshot)
            .await
            .expect("provision")
        else {
            panic!("HostPath expected");
        };
        tokio::fs::create_dir_all(dir.join("nested/deep"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("a.txt"), b"alpha").await.unwrap();
        tokio::fs::write(dir.join("nested/deep/b.bin"), vec![7u8; 1024])
            .await
            .unwrap();

        let vol = VolumeId::derive(&tenant(), &sandbox("src"));
        let snap1 = provider.snapshot(&vol).await.expect("snapshot");
        let snap2 = provider.snapshot(&vol).await.expect("snapshot again");
        assert_eq!(
            snap1, snap2,
            "identical content must produce the identical content address"
        );

        let MountSource::HostPath(forked) = provider
            .fork(&snap1, &sandbox("child"))
            .await
            .expect("fork materializes")
        else {
            panic!("HostPath expected");
        };
        assert_eq!(
            tokio::fs::read(forked.join("a.txt")).await.unwrap(),
            b"alpha"
        );
        assert_eq!(
            tokio::fs::read(forked.join("nested/deep/b.bin"))
                .await
                .unwrap(),
            vec![7u8; 1024],
            "forked volume must byte-match the snapshot"
        );

        // Divergence after fork: the child edit must not affect a re-snapshot
        // of the parent (CoW-by-materialization in v1).
        tokio::fs::write(forked.join("a.txt"), b"changed")
            .await
            .unwrap();
        let snap3 = provider.snapshot(&vol).await.expect("parent re-snapshot");
        assert_eq!(
            snap1, snap3,
            "child divergence must not leak into the parent"
        );
    }

    #[tokio::test]
    async fn unsupported_backings_and_migration_fail_closed() {
        let root = tempfile::tempdir().expect("tempdir");
        let provider = provider(root.path());
        for policy in [
            VolumePolicy::Persistent {
                backing: VolumeBacking::ObjectFs,
            },
            VolumePolicy::Persistent {
                backing: VolumeBacking::ClusterFs,
            },
            VolumePolicy::External {
                mount: "nfs://filer/vol".to_owned(),
            },
        ] {
            let err = provider
                .provision(&tenant(), &sandbox("x"), policy.clone())
                .await
                .expect_err("unsupported backing must fail closed");
            assert!(
                err.to_string().contains("failing closed"),
                "error must state fail-closed intent, got: {err}"
            );
        }
        let err = provider
            .migrate_in(
                &VolumeId::derive(&tenant(), &sandbox("x")),
                PlacementSource::CloudPrimary,
            )
            .await
            .expect_err("migration is HS-gated");
        assert!(err.to_string().contains("failing closed"));
    }

    #[tokio::test]
    async fn snapshot_without_byte_plane_fails_closed_and_symlinks_refused() {
        let root = tempfile::tempdir().expect("tempdir");
        let bare = LocalDirVolume::new(root.path()); // no blob store
        let MountSource::HostPath(dir) = bare
            .provision(&tenant(), &sandbox("s"), VolumePolicy::Snapshot)
            .await
            .expect("provision works without byte plane")
        else {
            panic!("HostPath expected");
        };
        let vol = VolumeId::derive(&tenant(), &sandbox("s"));
        let err = bare.snapshot(&vol).await.expect_err("no byte plane");
        assert!(err.to_string().contains("failing closed"));

        // Symlink refusal (fail-closed capture boundary).
        let provider = provider(root.path().join("p2").as_path());
        let MountSource::HostPath(dir2) = provider
            .provision(&tenant(), &sandbox("sym"), VolumePolicy::Snapshot)
            .await
            .expect("provision")
        else {
            panic!("HostPath expected");
        };
        tokio::fs::write(dir.join("outside.txt"), b"secret")
            .await
            .unwrap();
        tokio::fs::symlink(dir.join("outside.txt"), dir2.join("link"))
            .await
            .unwrap();
        let vol2 = VolumeId::derive(&tenant(), &sandbox("sym"));
        let err = provider.snapshot(&vol2).await.expect_err("symlink refused");
        assert!(err.to_string().contains("symlink"));
    }
}
