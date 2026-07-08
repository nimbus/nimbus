//! Tenant byte-root ownership and format identity for [`crate::LocalPackStore`].
//!
//! A local pack root is operator-proof only if the store can prove three
//! things before serving bytes from it:
//!
//! 1. **It is ours.** A format marker (`format.nblfmt`) names the on-disk
//!    format version and, optionally, the tenant identity the root was
//!    provisioned for. A foreign or future-versioned marker refuses to open.
//! 2. **We are alone.** An advisory `flock` on `lock` refuses a second
//!    writable open of the same root — from another process or another handle
//!    in this process — with [`StorageErrorKind::Busy`]. Read-only inspection
//!    handles take no lock (see `LocalPackStore::open_read_only`).
//! 3. **Crash leftovers are gone.** Temp files from interrupted durable
//!    replaces (the [`crate::disk::TMP_PREFIX`] marker) are deleted, and the
//!    count is reported.
//!
//! Root/format concepts follow the local-disk ownership discipline of
//! RustFS's bare-metal store (rustfs/rustfs@bd5d3c5d,
//! `crates/ecstore/src/disk/local.rs` + `store/init_format.rs`) as
//! architecture patterns only; the marker layout and semantics here are
//! Nimbus's own (no `format.json`, no erasure-set identity, no global state).

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

use fs2::FileExt;
use nimbus_core::{Error, Result, StorageErrorKind};

use crate::disk::{self, SyncObserver};

/// File name of the root format marker.
pub(crate) const FORMAT_FILE: &str = "format.nblfmt";
/// File name of the advisory root lock.
pub(crate) const LOCK_FILE: &str = "lock";

const FORMAT_MAGIC: &[u8] = b"NBLFMT1\n";
const FORMAT_VERSION: u32 = 1;
/// magic + version + created_at + identity + reserved flags.
const FORMAT_LEN: usize = 8 + 4 + 8 + 32 + 4;

/// Identity a root is bound to (BLAKE3 of the owning tenant id). All-zero
/// means the root is unbound (opened without a declared identity).
pub(crate) type RootIdentity = [u8; 32];

const UNBOUND: RootIdentity = [0u8; 32];

/// Options for opening a [`crate::LocalPackStore`].
#[derive(Clone, Debug)]
pub struct LocalPackStoreOptions {
    /// Target size at which the active pack rolls over.
    pub pack_target_bytes: u64,
    /// Identity to bind the root to (typically BLAKE3 of the tenant id).
    /// `None` accepts any marker; `Some` stamps an unbound root and refuses a
    /// root bound to a different identity.
    pub identity: Option<[u8; 32]>,
    /// Permit a symlinked root directory. Off by default: a symlinked root is
    /// how a misconfigured deployment silently serves another tenant's bytes.
    pub allow_symlinked_root: bool,
}

impl Default for LocalPackStoreOptions {
    fn default() -> Self {
        Self {
            pack_target_bytes: crate::local::DEFAULT_PACK_TARGET_BYTES,
            identity: None,
            allow_symlinked_root: false,
        }
    }
}

/// What opening a root observed and repaired.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// Crash-leftover temp files removed from the root and packs directory.
    pub stale_temp_files_removed: usize,
    /// Bytes of a torn (crash-truncated) trailing index record dropped and
    /// truncated away at open.
    pub torn_index_bytes_truncated: u64,
}

/// The root format marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormatMarker {
    pub(crate) version: u32,
    pub(crate) created_at_millis: u64,
    pub(crate) identity: RootIdentity,
}

impl FormatMarker {
    fn new(created_at_millis: u64, identity: RootIdentity) -> Self {
        Self {
            version: FORMAT_VERSION,
            created_at_millis,
            identity,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FORMAT_LEN);
        out.extend_from_slice(FORMAT_MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.created_at_millis.to_le_bytes());
        out.extend_from_slice(&self.identity);
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    fn decode(path: &Path, bytes: &[u8]) -> Result<Self> {
        if !bytes.starts_with(FORMAT_MAGIC) {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!(
                    "root marker {} is not a nimbus-blob format marker (foreign root?)",
                    path.display()
                ),
            ));
        }
        if bytes.len() < FORMAT_LEN {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("root marker {} is truncated", path.display()),
            ));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().expect("sliced 4 bytes"));
        if version == 0 || version > FORMAT_VERSION {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!(
                    "root marker {} carries format version {version}; this build supports \
                     1..={FORMAT_VERSION} (refusing a future or invalid format fail-closed)",
                    path.display()
                ),
            ));
        }
        let created_at_millis =
            u64::from_le_bytes(bytes[12..20].try_into().expect("sliced 8 bytes"));
        let mut identity = [0u8; 32];
        identity.copy_from_slice(&bytes[20..52]);
        Ok(Self {
            version,
            created_at_millis,
            identity,
        })
    }
}

/// Advisory exclusive lock on a root. Held for the store's lifetime; the
/// flock is released when the handle (and its clones) drop.
#[derive(Debug)]
pub(crate) struct RootLock {
    _file: File,
}

/// Everything the guard established about a root at open time.
#[derive(Debug)]
pub(crate) struct RootGuard {
    /// `None` for read-only inspection handles.
    pub(crate) lock: Option<RootLock>,
    pub(crate) report: OpenReport,
}

fn guard_error(err: std::io::Error, context: String) -> Error {
    Error::storage(StorageErrorKind::Io, format!("{context}: {err}"))
}

/// Rejects a symlinked root unless the options allow it.
fn check_root_shape(root: &Path, allow_symlinked_root: bool) -> Result<()> {
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.file_type().is_symlink() && !allow_symlinked_root => {
            Err(Error::InvalidInput(format!(
                "blob root {} is a symlink; refusing (set allow_symlinked_root if the \
                 deployment's capability policy permits it)",
                root.display()
            )))
        }
        Ok(meta) if meta.is_file() => Err(Error::InvalidInput(format!(
            "blob root {} is a file, not a directory",
            root.display()
        ))),
        _ => Ok(()),
    }
}

/// Takes the exclusive advisory lock, or fails with [`StorageErrorKind::Busy`].
fn take_lock(root: &Path) -> Result<RootLock> {
    let path = root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|err| guard_error(err, format!("open root lock {}", path.display())))?;
    file.try_lock_exclusive().map_err(|_| {
        Error::storage(
            StorageErrorKind::Busy,
            format!(
                "blob root {} is exclusively owned by another live handle or process; \
                 close it first (read-only inspection does not take the lock)",
                root.display()
            ),
        )
    })?;
    Ok(RootLock { _file: file })
}

/// Loads, validates, stamps, or upgrades the format marker per the identity
/// rules. Read-only handles pass `identity = None` and never write.
fn establish_marker(
    root: &Path,
    identity: Option<RootIdentity>,
    created_at_millis: u64,
    writable: bool,
    observer: &dyn SyncObserver,
) -> Result<()> {
    let path = root.join(FORMAT_FILE);
    let existing = match File::open(&path) {
        Ok(mut file) => {
            let mut bytes = Vec::with_capacity(FORMAT_LEN);
            file.read_to_end(&mut bytes)
                .map_err(|err| guard_error(err, format!("read root marker {}", path.display())))?;
            Some(FormatMarker::decode(&path, &bytes)?)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(guard_error(
                err,
                format!("open root marker {}", path.display()),
            ));
        }
    };

    let desired_identity = identity.unwrap_or(UNBOUND);
    match existing {
        Some(marker) => {
            let bound = marker.identity != UNBOUND;
            let declared = desired_identity != UNBOUND;
            if bound && declared && marker.identity != desired_identity {
                return Err(Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "blob root {} is bound to a different identity; refusing to open a \
                         foreign tenant root",
                        root.display()
                    ),
                ));
            }
            // Unbound root + declared identity: bind it now (one-time upgrade).
            if !bound && declared && writable {
                let upgraded = FormatMarker {
                    identity: desired_identity,
                    ..marker
                };
                disk::write_replace_durable(&path, &upgraded.encode(), observer).map_err(
                    |err| guard_error(err, format!("bind root marker {}", path.display())),
                )?;
            }
            Ok(())
        }
        None if writable => {
            let marker = FormatMarker::new(created_at_millis, desired_identity);
            disk::write_replace_durable(&path, &marker.encode(), observer)
                .map_err(|err| guard_error(err, format!("write root marker {}", path.display())))?;
            Ok(())
        }
        // Read-only inspection of a root that predates markers is allowed.
        None => Ok(()),
    }
}

/// Deletes crash-leftover temp files directly inside `dir`.
fn sweep_stale_temps(dir: &Path) -> Result<usize> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(guard_error(err, format!("scan {}", dir.display()))),
    };
    let mut removed = 0usize;
    for entry in entries {
        let entry = entry.map_err(|err| guard_error(err, format!("scan {}", dir.display())))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if disk::is_stale_temp(name) && entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            fs::remove_file(entry.path()).map_err(|err| {
                guard_error(err, format!("remove stale temp {}", entry.path().display()))
            })?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Shape-checks and creates a writable root directory so the caller can
/// canonicalize it (the open-root registry keys on the canonical path).
pub(crate) fn check_writable_root_shape(
    root: &Path,
    options: &LocalPackStoreOptions,
) -> Result<()> {
    check_root_shape(root, options.allow_symlinked_root)?;
    fs::create_dir_all(root)
        .map_err(|err| guard_error(err, format!("create blob root {}", root.display())))
}

/// Marker/identity validation for an open that aliases an already-live
/// same-process state: no lock is taken (the shared state holds it) and no
/// cleanup runs, but a foreign-identity open is still refused and an unbound
/// marker is still bound when an identity is declared.
pub(crate) fn validate_marker_for_shared_open(
    root: &Path,
    options: &LocalPackStoreOptions,
    observer: &dyn SyncObserver,
) -> Result<()> {
    establish_marker(root, options.identity, 0, true, observer)
}

/// Establishes ownership of a writable root: shape check → lock → marker →
/// stale-temp cleanup. `packs_dir` is swept too (it holds no temps today, but
/// future compaction/scrub work writes there through the same recipe).
pub(crate) fn guard_writable_root(
    root: &Path,
    packs_dir: &Path,
    options: &LocalPackStoreOptions,
    created_at_millis: u64,
    observer: &dyn SyncObserver,
) -> Result<RootGuard> {
    check_root_shape(root, options.allow_symlinked_root)?;
    fs::create_dir_all(root)
        .map_err(|err| guard_error(err, format!("create blob root {}", root.display())))?;
    let lock = take_lock(root)?;
    establish_marker(root, options.identity, created_at_millis, true, observer)?;
    let report = OpenReport {
        stale_temp_files_removed: sweep_stale_temps(root)? + sweep_stale_temps(packs_dir)?,
        ..OpenReport::default()
    };
    Ok(RootGuard {
        lock: Some(lock),
        report,
    })
}

/// Validates a root for read-only inspection: shape and marker checks only —
/// no lock, no writes, no cleanup.
pub(crate) fn guard_read_only_root(root: &Path) -> Result<RootGuard> {
    check_root_shape(root, false)?;
    establish_marker(root, None, 0, false, &disk::NoopSyncObserver)?;
    Ok(RootGuard {
        lock: None,
        report: OpenReport::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::NoopSyncObserver;

    fn options() -> LocalPackStoreOptions {
        LocalPackStoreOptions::default()
    }

    fn open_guard(root: &Path) -> Result<RootGuard> {
        guard_writable_root(root, &root.join("packs"), &options(), 42, &NoopSyncObserver)
    }

    #[test]
    fn marker_roundtrip() {
        let marker = FormatMarker::new(1234, [7u8; 32]);
        let decoded = FormatMarker::decode(Path::new("x"), &marker.encode()).unwrap();
        assert_eq!(decoded, marker);
    }

    #[test]
    fn marker_rejects_foreign_magic_and_future_version() {
        let err = FormatMarker::decode(Path::new("x"), b"NOTAMRK\n_______________").unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

        let mut future = FormatMarker::new(1, UNBOUND).encode();
        future[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        let err = FormatMarker::decode(Path::new("x"), &future).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[test]
    fn second_writable_guard_is_busy_until_first_drops() {
        let dir = tempfile::tempdir().unwrap();
        let first = open_guard(dir.path()).unwrap();

        let err = open_guard(dir.path()).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));

        drop(first);
        open_guard(dir.path()).unwrap();
    }

    #[test]
    fn read_only_guard_coexists_with_writable_owner() {
        let dir = tempfile::tempdir().unwrap();
        let _owner = open_guard(dir.path()).unwrap();
        guard_read_only_root(dir.path()).unwrap();
    }

    #[test]
    fn identity_binds_upgrades_and_refuses_foreign() {
        let dir = tempfile::tempdir().unwrap();
        // First open unbound.
        drop(open_guard(dir.path()).unwrap());

        // Reopen with an identity: binds the unbound root.
        let mut bound = options();
        bound.identity = Some([9u8; 32]);
        drop(
            guard_writable_root(
                dir.path(),
                &dir.path().join("packs"),
                &bound,
                42,
                &NoopSyncObserver,
            )
            .unwrap(),
        );

        // Same identity reopens.
        drop(
            guard_writable_root(
                dir.path(),
                &dir.path().join("packs"),
                &bound,
                42,
                &NoopSyncObserver,
            )
            .unwrap(),
        );

        // A different identity is a foreign root.
        let mut foreign = options();
        foreign.identity = Some([1u8; 32]);
        let err = guard_writable_root(
            dir.path(),
            &dir.path().join("packs"),
            &foreign,
            42,
            &NoopSyncObserver,
        )
        .unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

        // No declared identity still opens a bound root (identity-agnostic
        // tools like backup enumerate roots without knowing tenants).
        drop(open_guard(dir.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_is_rejected_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let err = open_guard(&link).unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));

        let mut allowed = options();
        allowed.allow_symlinked_root = true;
        guard_writable_root(&link, &link.join("packs"), &allowed, 42, &NoopSyncObserver).unwrap();
    }
}
