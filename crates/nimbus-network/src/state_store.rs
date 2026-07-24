//! Crash-safe, node-local authority for portable network resource state.
//!
//! The store deliberately has one file, one checksum/version envelope, and one
//! cross-process lock domain. Payload ownership remains with the concept that
//! understands it; this module provides typed partitions and the durable
//! transaction boundary without learning provider effects.
//!
//! ## Filesystem contract
//!
//! State roots must be on a same-host local filesystem that honors advisory
//! locks, atomic same-directory replacement, file synchronization, and parent
//! directory synchronization. Open rejects filesystem types known to be
//! network mounted (including NFS, SMB/CIFS, 9p, AFS, Coda, NCP, and Ceph)
//! before reading or creating authoritative state. It then exercises the full
//! create → file-sync → rename → directory-sync recipe as a startup capability
//! probe. There is intentionally no override that weakens this contract.
//!
//! The file is a latest-state snapshot, not an event log: successful commits
//! replace the previous revision and startup removes crash-leftover stages.
//! Retention is therefore bounded by live resource records rather than commit
//! count. Payload owners must retain every cleanup-pending resource until its
//! fenced release proof; the store never ages or compacts records on its own.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use nimbus_core::TenantId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ulid::Ulid;

const STORE_DIRECTORY: &str = "control-plane";
const STORE_FILE: &str = "state.json";
const LOCK_FILE: &str = "authority.lock";
const FORMAT_MAGIC: &str = "nimbus-network-state";
const FORMAT_VERSION: u32 = 1;
const TEMP_PREFIX: &str = ".nimbus-network-state-";
const PROBE_PREFIX: &str = ".nimbus-network-probe-";
#[cfg(unix)]
const OWNER_DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const OWNER_FILE_MODE: u32 = 0o600;
// fs2 0.4's Windows `try_lock_exclusive` returns the raw Win32
// `ERROR_LOCK_VIOLATION` from `LockFileEx(..., LOCKFILE_FAIL_IMMEDIATELY)`.
// Rust does not promise to map that code to `ErrorKind::WouldBlock`.
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

/// A typed partition inside the single node-local network authority.
///
/// Partitions isolate payload schemas while every mutation still shares one
/// lock, revision, checksum, and atomic commit point. New network resource
/// families extend this enum instead of creating another local store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NetworkStatePartition {
    /// Portable tenant-segment assignments and holds.
    SegmentAllocations,
    /// Provider-adjacent IP allocation state for one tenant.
    TenantIpam(TenantId),
}

impl NetworkStatePartition {
    fn key(&self) -> String {
        match self {
            Self::SegmentAllocations => "segment-allocations".to_owned(),
            Self::TenantIpam(tenant_id) => format!("tenant-ipam/{}", tenant_id.as_str()),
        }
    }
}

impl Display for NetworkStatePartition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.key())
    }
}

/// Bounded locking options for [`LocalNetworkStateStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalNetworkStateStoreOptions {
    /// Maximum time a transaction may wait for the process-shared authority
    /// lock. Exhaustion fails closed; no unlocked read or mutation is allowed.
    pub lock_timeout: Duration,
    /// Bounded retry interval while another process owns the lock.
    pub lock_retry_interval: Duration,
}

impl Default for LocalNetworkStateStoreOptions {
    fn default() -> Self {
        Self {
            lock_timeout: Duration::from_secs(2),
            lock_retry_interval: Duration::from_millis(10),
        }
    }
}

/// One node-local, crash-safe network state authority.
#[derive(Clone, Debug)]
pub struct LocalNetworkStateStore {
    state_root: PathBuf,
    store_root: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
    filesystem_kind: String,
    options: LocalNetworkStateStoreOptions,
}

impl LocalNetworkStateStore {
    /// Open and validate a node-local authority using bounded default locking.
    pub fn open(state_root: impl AsRef<Path>) -> Result<Self, NetworkStateStoreError> {
        Self::open_with_options(state_root, LocalNetworkStateStoreOptions::default())
    }

    /// Open with explicit bounded lock timing.
    ///
    /// This performs filesystem classification and the durable-write
    /// capability probe before reading authority state. A corrupt,
    /// incompatible, or unsupported root therefore never becomes usable.
    pub fn open_with_options(
        state_root: impl AsRef<Path>,
        options: LocalNetworkStateStoreOptions,
    ) -> Result<Self, NetworkStateStoreError> {
        validate_options(options)?;
        let state_root = absolutize(state_root.as_ref())?;
        let existing_ancestor = nearest_existing_ancestor(&state_root)?;
        let filesystem_kind = detect_filesystem_kind(&existing_ancestor)?;
        ensure_supported_filesystem(&existing_ancestor, &filesystem_kind)?;

        let store_root = state_root.join("networks").join(STORE_DIRECTORY);
        create_dir_all_owner_only(&store_root)?;
        let store_filesystem_kind = detect_filesystem_kind(&store_root)?;
        ensure_supported_filesystem(&store_root, &store_filesystem_kind)?;
        if store_filesystem_kind != filesystem_kind {
            return Err(NetworkStateStoreError::UnsupportedFilesystem {
                path: store_root,
                filesystem_kind: format!(
                    "mount changed from {filesystem_kind} to {store_filesystem_kind} while opening"
                ),
            });
        }

        let store = Self {
            state_root,
            state_path: store_root.join(STORE_FILE),
            lock_path: store_root.join(LOCK_FILE),
            store_root,
            filesystem_kind,
            options,
        };
        store.establish_root()?;
        Ok(store)
    }

    /// Original node state root supplied by the composition owner.
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Canonical authority file. Diagnostics and corruption drills use this
    /// path; callers must never edit it during ordinary operation.
    pub fn authority_path(&self) -> &Path {
        &self.state_path
    }

    /// Detected filesystem type recorded when the startup probe passed.
    pub fn filesystem_kind(&self) -> &str {
        &self.filesystem_kind
    }

    /// Derive the one authority path without opening it.
    pub fn authority_path_for(state_root: impl AsRef<Path>) -> PathBuf {
        state_root
            .as_ref()
            .join("networks")
            .join(STORE_DIRECTORY)
            .join(STORE_FILE)
    }

    /// Read and validate one typed partition under the shared lock.
    pub fn read<T>(
        &self,
        partition: &NetworkStatePartition,
    ) -> Result<Option<T>, NetworkStateStoreError>
    where
        T: DeserializeOwned,
    {
        let _lock = self.acquire_lock()?;
        let body = self.load_body()?;
        body.records
            .get(&partition.key())
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|source| NetworkStateStoreError::Corrupt {
                    path: self.state_path.clone(),
                    reason: format!(
                        "partition {partition} does not match its payload schema: {source}"
                    ),
                })
            })
            .transpose()
    }

    /// Atomically read, mutate, checksum, and publish one typed partition.
    ///
    /// A closure error leaves the existing authority unchanged. A store error
    /// is distinct from the concept-owned mutation error so callers cannot
    /// accidentally downgrade corruption or lock failure into a domain result.
    pub fn transaction<T, R, E>(
        &self,
        partition: &NetworkStatePartition,
        mutator: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<R, NetworkStateTransactionError<E>>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        self.transaction_inner(partition, mutator, &|_| {})
    }

    fn establish_root(&self) -> Result<(), NetworkStateStoreError> {
        let _lock = self.acquire_lock()?;
        self.remove_stale_files()?;
        self.probe_durable_replace()?;
        let _ = self.load_body()?;
        Ok(())
    }

    fn transaction_inner<T, R, E>(
        &self,
        partition: &NetworkStatePartition,
        mutator: impl FnOnce(&mut T) -> Result<R, E>,
        observer: &dyn Fn(DurabilityEvent),
    ) -> Result<R, NetworkStateTransactionError<E>>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        let _lock = self
            .acquire_lock()
            .map_err(NetworkStateTransactionError::Store)?;
        let mut body = self
            .load_body()
            .map_err(NetworkStateTransactionError::Store)?;
        let key = partition.key();
        let mut payload = match body.records.get(&key).cloned() {
            Some(value) => serde_json::from_value(value).map_err(|source| {
                NetworkStateTransactionError::Store(NetworkStateStoreError::Corrupt {
                    path: self.state_path.clone(),
                    reason: format!(
                        "partition {partition} does not match its payload schema: {source}"
                    ),
                })
            })?,
            None => T::default(),
        };
        let result = mutator(&mut payload).map_err(NetworkStateTransactionError::Operation)?;
        let payload = serde_json::to_value(payload).map_err(|source| {
            NetworkStateTransactionError::Store(NetworkStateStoreError::Serialization {
                partition: partition.clone(),
                reason: source.to_string(),
            })
        })?;
        body.records.insert(key, payload);
        body.revision = body.revision.checked_add(1).ok_or_else(|| {
            NetworkStateTransactionError::Store(NetworkStateStoreError::RevisionExhausted {
                path: self.state_path.clone(),
            })
        })?;
        self.persist_body(&body, observer)
            .map_err(NetworkStateTransactionError::Store)?;
        Ok(result)
    }

    fn acquire_lock(&self) -> Result<AuthorityLock, NetworkStateStoreError> {
        let file = open_owner_file(&self.lock_path, false)?;
        let started = Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(AuthorityLock { file }),
                Err(source) if is_lock_contended(&source) => {
                    if started.elapsed() >= self.options.lock_timeout {
                        return Err(NetworkStateStoreError::LockTimeout {
                            path: self.lock_path.clone(),
                            timeout: self.options.lock_timeout,
                        });
                    }
                    let remaining = self.options.lock_timeout.saturating_sub(started.elapsed());
                    thread::sleep(self.options.lock_retry_interval.min(remaining));
                }
                Err(source) => {
                    return Err(NetworkStateStoreError::Io {
                        operation: "acquire authority lock",
                        path: self.lock_path.clone(),
                        source,
                    });
                }
            }
        }
    }

    fn load_body(&self) -> Result<StoreBody, NetworkStateStoreError> {
        let mut file = match File::open(&self.state_path) {
            Ok(file) => file,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(StoreBody::default());
            }
            Err(source) => {
                return Err(NetworkStateStoreError::Io {
                    operation: "open authority state",
                    path: self.state_path.clone(),
                    source,
                });
            }
        };
        validate_owner_file_permissions(&self.state_path, &file)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| NetworkStateStoreError::Io {
                operation: "read authority state",
                path: self.state_path.clone(),
                source,
            })?;
        let envelope: StoreEnvelope =
            serde_json::from_slice(&bytes).map_err(|source| NetworkStateStoreError::Corrupt {
                path: self.state_path.clone(),
                reason: format!("invalid or truncated JSON envelope: {source}"),
            })?;
        if envelope.magic != FORMAT_MAGIC {
            return Err(NetworkStateStoreError::Corrupt {
                path: self.state_path.clone(),
                reason: format!("unexpected format marker {:?}", envelope.magic),
            });
        }
        if envelope.version != FORMAT_VERSION {
            return Err(NetworkStateStoreError::IncompatibleVersion {
                path: self.state_path.clone(),
                found: envelope.version,
                supported: FORMAT_VERSION,
            });
        }
        let expected = checksum_body(&envelope.body)?;
        if envelope.checksum != expected {
            return Err(NetworkStateStoreError::ChecksumMismatch {
                path: self.state_path.clone(),
                expected,
                found: envelope.checksum,
            });
        }
        Ok(envelope.body)
    }

    fn persist_body(
        &self,
        body: &StoreBody,
        observer: &dyn Fn(DurabilityEvent),
    ) -> Result<(), NetworkStateStoreError> {
        let envelope = StoreEnvelope {
            magic: FORMAT_MAGIC.to_owned(),
            version: FORMAT_VERSION,
            checksum: checksum_body(body)?,
            body: body.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&envelope).map_err(|source| {
            NetworkStateStoreError::Serialization {
                partition: NetworkStatePartition::SegmentAllocations,
                reason: format!("serialize authority envelope: {source}"),
            }
        })?;
        durable_replace(
            &self.store_root,
            &self.state_path,
            &bytes,
            TEMP_PREFIX,
            observer,
        )
    }

    fn remove_stale_files(&self) -> Result<(), NetworkStateStoreError> {
        let mut removed = false;
        for entry in
            fs::read_dir(&self.store_root).map_err(|source| NetworkStateStoreError::Io {
                operation: "enumerate authority directory",
                path: self.store_root.clone(),
                source,
            })?
        {
            let entry = entry.map_err(|source| NetworkStateStoreError::Io {
                operation: "read authority directory entry",
                path: self.store_root.clone(),
                source,
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(TEMP_PREFIX) || name.starts_with(PROBE_PREFIX) {
                fs::remove_file(entry.path()).map_err(|source| NetworkStateStoreError::Io {
                    operation: "remove stale authority stage",
                    path: entry.path(),
                    source,
                })?;
                removed = true;
            }
        }
        if removed {
            sync_directory(&self.store_root)?;
        }
        Ok(())
    }

    fn probe_durable_replace(&self) -> Result<(), NetworkStateStoreError> {
        let token = Ulid::new();
        let source = self.store_root.join(format!("{PROBE_PREFIX}{token}.stage"));
        let destination = self.store_root.join(format!("{PROBE_PREFIX}{token}.done"));
        let probe_result = (|| {
            let mut file = open_owner_file(&source, true)?;
            file.write_all(b"nimbus-network-durability-probe")
                .map_err(|source_error| NetworkStateStoreError::Io {
                    operation: "write durability probe",
                    path: source.clone(),
                    source: source_error,
                })?;
            file.sync_all()
                .map_err(|source_error| NetworkStateStoreError::Io {
                    operation: "sync durability probe",
                    path: source.clone(),
                    source: source_error,
                })?;
            drop(file);
            replace_file(&source, &destination).map_err(|source_error| {
                NetworkStateStoreError::Io {
                    operation: "replace durability probe",
                    path: destination.clone(),
                    source: source_error,
                }
            })?;
            sync_directory(&self.store_root)?;
            fs::remove_file(&destination).map_err(|source_error| NetworkStateStoreError::Io {
                operation: "remove durability probe",
                path: destination.clone(),
                source: source_error,
            })?;
            sync_directory(&self.store_root)
        })();
        if probe_result.is_err() {
            let _ = fs::remove_file(&source);
            let _ = fs::remove_file(&destination);
        }
        probe_result
    }

    #[cfg(any(test, feature = "test-support"))]
    fn transaction_observed<T, R, E>(
        &self,
        partition: &NetworkStatePartition,
        observer: &dyn Fn(test_support::NetworkStateDurabilityEvent),
        mutator: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<R, NetworkStateTransactionError<E>>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        self.transaction_inner(partition, mutator, &|event| observer(event.into()))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreBody {
    revision: u64,
    records: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreEnvelope {
    magic: String,
    version: u32,
    checksum: String,
    body: StoreBody,
}

fn checksum_body(body: &StoreBody) -> Result<String, NetworkStateStoreError> {
    let bytes =
        serde_json::to_vec(body).map_err(|source| NetworkStateStoreError::Serialization {
            partition: NetworkStatePartition::SegmentAllocations,
            reason: format!("serialize authority checksum body: {source}"),
        })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DurabilityEvent {
    StateFileSynced,
    StateReplaced,
    ParentDirectorySynced,
}

fn durable_replace(
    parent: &Path,
    destination: &Path,
    bytes: &[u8],
    temp_prefix: &str,
    observer: &dyn Fn(DurabilityEvent),
) -> Result<(), NetworkStateStoreError> {
    let stage = parent.join(format!("{temp_prefix}{}.stage", Ulid::new()));
    let result = (|| {
        let mut file = open_owner_file(&stage, true)?;
        file.write_all(bytes)
            .map_err(|source| NetworkStateStoreError::Io {
                operation: "write staged authority state",
                path: stage.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| NetworkStateStoreError::Io {
                operation: "sync staged authority state",
                path: stage.clone(),
                source,
            })?;
        observer(DurabilityEvent::StateFileSynced);
        drop(file);
        replace_file(&stage, destination).map_err(|source| NetworkStateStoreError::Io {
            operation: "atomically replace authority state",
            path: destination.to_path_buf(),
            source,
        })?;
        observer(DurabilityEvent::StateReplaced);
        sync_directory(parent)?;
        observer(DurabilityEvent::ParentDirectorySynced);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&stage);
    }
    result
}

fn validate_options(options: LocalNetworkStateStoreOptions) -> Result<(), NetworkStateStoreError> {
    if options.lock_timeout.is_zero() {
        return Err(NetworkStateStoreError::InvalidOptions(
            "lock timeout must be greater than zero",
        ));
    }
    if options.lock_retry_interval.is_zero() {
        return Err(NetworkStateStoreError::InvalidOptions(
            "lock retry interval must be greater than zero",
        ));
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf, NetworkStateStoreError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|source| NetworkStateStoreError::Io {
                operation: "resolve current directory",
                path: path.to_path_buf(),
                source,
            })
    }
}

fn nearest_existing_ancestor(path: &Path) -> Result<PathBuf, NetworkStateStoreError> {
    let mut candidate = path;
    loop {
        match fs::canonicalize(candidate) {
            Ok(path) => return Ok(path),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                candidate = candidate
                    .parent()
                    .ok_or_else(|| NetworkStateStoreError::Io {
                        operation: "find existing state-root ancestor",
                        path: path.to_path_buf(),
                        source,
                    })?;
            }
            Err(source) => {
                return Err(NetworkStateStoreError::Io {
                    operation: "canonicalize state-root ancestor",
                    path: candidate.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn create_dir_all_owner_only(path: &Path) -> Result<(), NetworkStateStoreError> {
    let mut missing = Vec::new();
    let mut candidate = path;
    while !candidate.exists() {
        missing.push(candidate.to_path_buf());
        candidate = candidate
            .parent()
            .ok_or_else(|| NetworkStateStoreError::Io {
                operation: "resolve authority directory parent",
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
            })?;
    }
    create_dir_all_with_owner_mode(path).map_err(|source| NetworkStateStoreError::Io {
        operation: "create authority directory",
        path: path.to_path_buf(),
        source,
    })?;
    for directory in missing.iter().rev() {
        set_owner_directory_permissions(directory)?;
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
    }
    set_owner_directory_permissions(path)
}

#[cfg(unix)]
fn create_dir_all_with_owner_mode(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(OWNER_DIRECTORY_MODE);
    builder.create(path)
}

#[cfg(not(unix))]
fn create_dir_all_with_owner_mode(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(unix)]
fn set_owner_directory_permissions(path: &Path) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_DIRECTORY_MODE)).map_err(|source| {
        NetworkStateStoreError::Io {
            operation: "protect authority directory",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_owner_directory_permissions(_path: &Path) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

fn open_owner_file(path: &Path, create_new: bool) -> Result<File, NetworkStateStoreError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OWNER_FILE_MODE);
    }
    let file = options
        .open(path)
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "open owner-only authority file",
            path: path.to_path_buf(),
            source,
        })?;
    set_owner_file_permissions(path, &file)?;
    Ok(file)
}

#[cfg(unix)]
fn set_owner_file_permissions(path: &Path, file: &File) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "protect authority file",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn set_owner_file_permissions(_path: &Path, _file: &File) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

#[cfg(unix)]
fn validate_owner_file_permissions(path: &Path, file: &File) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = file
        .metadata()
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "inspect authority file permissions",
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 == 0 {
        Ok(())
    } else {
        Err(NetworkStateStoreError::InsecurePermissions {
            path: path.to_path_buf(),
            mode,
        })
    }
}

#[cfg(not(unix))]
fn validate_owner_file_permissions(
    _path: &Path,
    _file: &File,
) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), NetworkStateStoreError> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| NetworkStateStoreError::Io {
                operation: "sync authority directory",
                path: path.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().collect();
    source_wide.push(0);
    let mut destination_wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
    destination_wide.push(0);
    // SAFETY: both pointers address null-terminated buffers that remain alive
    // for the duration of the synchronous call.
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn is_lock_contended(source: &io::Error) -> bool {
    source.kind() == io::ErrorKind::WouldBlock
        || matches!(
            source.raw_os_error(),
            Some(11 | 35 | 36 | WINDOWS_ERROR_LOCK_VIOLATION)
        )
}

fn ensure_supported_filesystem(
    path: &Path,
    filesystem_kind: &str,
) -> Result<(), NetworkStateStoreError> {
    let normalized = filesystem_kind.to_ascii_lowercase();
    let unsupported = [
        "nfs",
        "nfs4",
        "smb",
        "smb2",
        "smbfs",
        "cifs",
        "9p",
        "afs",
        "coda",
        "ncp",
        "ceph",
        "webdav",
        "davfs",
        "windows-no-root",
        "windows-unknown",
    ];
    if unsupported
        .iter()
        .any(|kind| normalized == *kind || normalized.starts_with(&format!("{kind}:")))
    {
        Err(NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: filesystem_kind.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: "path contains a NUL byte".to_owned(),
        }
    })?;
    let mut stat = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `encoded` is a live NUL-terminated path and `stat` points to
    // writable storage of the exact structure libc expects.
    let result = unsafe { libc::statfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return Err(NetworkStateStoreError::Io {
            operation: "inspect state-root filesystem",
            path: path.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: successful statfs initialized the structure.
    // Linux filesystem magic values are 32-bit bit patterns even when
    // `f_type` is a signed machine word. Normalize through `u32` before
    // widening so CIFS/SMB2 do not sign-extend on ILP32 targets.
    let magic = unsafe { stat.assume_init() }.f_type as u32;
    Ok(classify_linux_filesystem_magic(magic))
}

#[cfg(any(test, target_os = "linux"))]
fn classify_linux_filesystem_magic(magic: u32) -> String {
    match magic {
        0x0000_6969 => "nfs".to_owned(),
        0xff53_4d42 => "cifs".to_owned(),
        0xfe53_4d42 => "smb2".to_owned(),
        0x0102_1997 => "9p".to_owned(),
        0x5346_414f => "afs".to_owned(),
        0x7375_7245 => "coda".to_owned(),
        0x0000_564c => "ncp".to_owned(),
        0x00c3_6400 => "ceph".to_owned(),
        0x0000_ef53 => "ext".to_owned(),
        0x5846_5342 => "xfs".to_owned(),
        0x9123_683e => "btrfs".to_owned(),
        0x0102_1994 => "tmpfs".to_owned(),
        0x794c_7630 => "overlay".to_owned(),
        other => format!("linux-magic:0x{other:x}"),
    }
}

#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::ffi::{CStr, CString};
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        NetworkStateStoreError::UnsupportedFilesystem {
            path: path.to_path_buf(),
            filesystem_kind: "path contains a NUL byte".to_owned(),
        }
    })?;
    let mut stat = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `encoded` and `stat` satisfy libc::statfs's pointer contract.
    let result = unsafe { libc::statfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return Err(NetworkStateStoreError::Io {
            operation: "inspect state-root filesystem",
            path: path.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    // SAFETY: successful statfs initialized the structure and f_fstypename is
    // a kernel-provided NUL-terminated fixed buffer on supported BSD targets.
    let stat = unsafe { stat.assume_init() };
    let name = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    Ok(name.to_string_lossy().into_owned())
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    Err(NetworkStateStoreError::UnsupportedFilesystem {
        path: path.to_path_buf(),
        filesystem_kind: format!("unsupported-unix-target:{}", std::env::consts::OS),
    })
}

#[cfg(windows)]
fn detect_filesystem_kind(path: &Path) -> Result<String, NetworkStateStoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_NO_ROOT_DIR, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
        DRIVE_UNKNOWN, GetDriveTypeW,
    };

    let canonical = fs::canonicalize(path).map_err(|source| NetworkStateStoreError::Io {
        operation: "canonicalize state-root filesystem",
        path: path.to_path_buf(),
        source,
    })?;
    let canonical_text = canonical.to_string_lossy();
    let root = match windows_classification_root(&canonical_text) {
        Some(WindowsClassificationRoot::Unc) => return Ok("smb".to_owned()),
        Some(WindowsClassificationRoot::Drive(root)) => root,
        None => {
            return Err(NetworkStateStoreError::UnsupportedFilesystem {
                path: canonical,
                filesystem_kind: "windows-unknown-root-shape".to_owned(),
            });
        }
    };
    let mut wide: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a live NUL-terminated root path.
    let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
    Ok(match kind {
        DRIVE_REMOTE => "smb".to_owned(),
        DRIVE_FIXED => "windows-fixed".to_owned(),
        DRIVE_RAMDISK => "windows-ramdisk".to_owned(),
        DRIVE_REMOVABLE => "windows-removable".to_owned(),
        DRIVE_CDROM => "windows-cdrom".to_owned(),
        DRIVE_NO_ROOT_DIR => "windows-no-root".to_owned(),
        DRIVE_UNKNOWN | _ => "windows-unknown".to_owned(),
    })
}

#[cfg(any(test, windows))]
#[derive(Debug, PartialEq, Eq)]
enum WindowsClassificationRoot {
    /// A UNC path is network-mounted by definition; no drive-type lookup can
    /// make it a supported node-local authority root.
    Unc,
    /// Plain drive root accepted by `GetDriveTypeW`, including the trailing
    /// backslash required by that API.
    Drive(String),
}

/// Convert Rust's canonical Windows path shape into the root form accepted by
/// `GetDriveTypeW`.
///
/// `std::fs::canonicalize` returns verbatim paths (`\\?\C:\...` or
/// `\\?\UNC\server\share\...`). `GetDriveTypeW` requires a plain `C:\` root;
/// UNC paths are classified as remote without calling it. Unknown device or
/// volume shapes fail closed at the caller.
#[cfg(any(test, windows))]
fn windows_classification_root(path: &str) -> Option<WindowsClassificationRoot> {
    if path
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        return Some(WindowsClassificationRoot::Unc);
    }
    if path.starts_with(r"\\") && !path.starts_with(r"\\?\") {
        return Some(WindowsClassificationRoot::Unc);
    }
    let drive_path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = drive_path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return Some(WindowsClassificationRoot::Drive(format!(
            "{}:\\",
            bytes[0] as char
        )));
    }
    None
}

/// Failures from the node-local network authority.
#[derive(Debug)]
pub enum NetworkStateStoreError {
    /// Lock timing must be bounded and non-zero.
    InvalidOptions(&'static str),
    /// The state root is on a known network-mounted or otherwise unsupported
    /// filesystem.
    UnsupportedFilesystem {
        path: PathBuf,
        filesystem_kind: String,
    },
    /// Another process retained the one authority lock past the configured
    /// bound.
    LockTimeout { path: PathBuf, timeout: Duration },
    /// The durable envelope is not parseable or a typed payload is malformed.
    Corrupt { path: PathBuf, reason: String },
    /// The record uses a version this build cannot safely interpret.
    IncompatibleVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    /// Body bytes do not match the recorded checksum.
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    /// The monotonic store revision cannot advance.
    RevisionExhausted { path: PathBuf },
    /// A typed payload could not be serialized.
    Serialization {
        partition: NetworkStatePartition,
        reason: String,
    },
    /// Existing authority data is readable by group or other users.
    InsecurePermissions { path: PathBuf, mode: u32 },
    /// A named filesystem operation failed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl Display for NetworkStateStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(reason) => {
                write!(formatter, "invalid network state-store options: {reason}")
            }
            Self::UnsupportedFilesystem {
                path,
                filesystem_kind,
            } => write!(
                formatter,
                "network state root {} uses unsupported filesystem {filesystem_kind:?}; \
                 a same-host local filesystem with advisory locks, atomic replacement, \
                 file sync, and directory sync is required",
                path.display()
            ),
            Self::LockTimeout { path, timeout } => write!(
                formatter,
                "timed out after {timeout:?} waiting for network authority lock {}; \
                 refusing an unlocked mutation",
                path.display()
            ),
            Self::Corrupt { path, reason } => write!(
                formatter,
                "network authority state {} is corrupt: {reason}",
                path.display()
            ),
            Self::IncompatibleVersion {
                path,
                found,
                supported,
            } => write!(
                formatter,
                "network authority state {} has incompatible format version {found}; \
                 this build supports version {supported}",
                path.display()
            ),
            Self::ChecksumMismatch {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "network authority state {} failed checksum validation \
                 (expected {expected}, found {found})",
                path.display()
            ),
            Self::RevisionExhausted { path } => write!(
                formatter,
                "network authority state {} exhausted its monotonic revision",
                path.display()
            ),
            Self::Serialization { partition, reason } => {
                write!(
                    formatter,
                    "failed to serialize network state partition {partition}: {reason}"
                )
            }
            Self::InsecurePermissions { path, mode } => write!(
                formatter,
                "network authority state {} has insecure mode {mode:o}; group/other access \
                 could expose provider handles",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} {}: {source}",
                path.display()
            ),
        }
    }
}

impl StdError for NetworkStateStoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Distinguishes authority failure from a concept-owned transaction rejection.
#[derive(Debug)]
pub enum NetworkStateTransactionError<E> {
    /// Durable state could not be safely read or committed.
    Store(NetworkStateStoreError),
    /// The caller's mutation rejected the proposed change.
    Operation(E),
}

impl<E: Display> Display for NetworkStateTransactionError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::Operation(error) => Display::fmt(error, formatter),
        }
    }
}

impl<E> StdError for NetworkStateTransactionError<E>
where
    E: StdError + 'static,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Operation(error) => Some(error),
        }
    }
}

struct AuthorityLock {
    file: File,
}

impl Drop for AuthorityLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Feature-gated hooks for exact crash-cut tests.
///
/// This is not a production provider seam. It exists only when the
/// `test-support` feature is explicitly enabled and adds no dependency from
/// this low-level crate to an upper-layer harness.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::{
        DurabilityEvent, LocalNetworkStateStore, NetworkStatePartition,
        NetworkStateTransactionError,
    };

    /// Exact durable-replace boundary reached by one transaction.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum NetworkStateDurabilityEvent {
        /// Staged bytes and checksum are durable; authority path is unchanged.
        StateFileSynced,
        /// The atomic replacement is visible; parent directory is not yet
        /// acknowledged durable.
        StateReplaced,
        /// The parent-directory entry is durably synchronized.
        ParentDirectorySynced,
    }

    impl From<DurabilityEvent> for NetworkStateDurabilityEvent {
        fn from(value: DurabilityEvent) -> Self {
            match value {
                DurabilityEvent::StateFileSynced => Self::StateFileSynced,
                DurabilityEvent::StateReplaced => Self::StateReplaced,
                DurabilityEvent::ParentDirectorySynced => Self::ParentDirectorySynced,
            }
        }
    }

    /// Run one transaction while observing exact durability boundaries.
    pub fn transaction_with_durability_observer<T, R, E>(
        store: &LocalNetworkStateStore,
        partition: &NetworkStatePartition,
        observer: impl Fn(NetworkStateDurabilityEvent),
        mutator: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<R, NetworkStateTransactionError<E>>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        store.transaction_observed(partition, &observer, mutator)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::test_support::{NetworkStateDurabilityEvent, transaction_with_durability_observer};
    use super::*;

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct FixtureState {
        owner: Option<String>,
        cleanup_pending: BTreeMap<String, String>,
    }

    #[derive(Default, Deserialize)]
    struct RefusesSerialization;

    impl Serialize for RefusesSerialization {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom(
                "intentional payload serialization failure",
            ))
        }
    }

    fn fixture_partition() -> NetworkStatePartition {
        NetworkStatePartition::TenantIpam(
            TenantId::new("tenant-a").expect("fixture tenant should parse"),
        )
    }

    #[test]
    fn transaction_round_trip_and_restart_share_one_authority() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("attachment-a".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("transaction should commit");

        let restarted =
            LocalNetworkStateStore::open(root.path()).expect("store should restart cleanly");
        let state: FixtureState = restarted
            .read(&fixture_partition())
            .expect("partition should read")
            .expect("partition should exist");
        assert_eq!(state.owner.as_deref(), Some("attachment-a"));
        assert_eq!(
            store.authority_path(),
            restarted.authority_path(),
            "all handles must resolve one authority file"
        );
    }

    #[test]
    fn transactions_preserve_sibling_partitions_in_the_same_envelope() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        let segment_partition = NetworkStatePartition::SegmentAllocations;
        let ipam_partition = fixture_partition();
        store
            .transaction(&segment_partition, |state: &mut FixtureState| {
                state.owner = Some("segment-owner".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("segment partition should commit");
        store
            .transaction(&ipam_partition, |state: &mut FixtureState| {
                state.owner = Some("ipam-owner".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("IPAM partition should commit");

        let segment: FixtureState = store
            .read(&segment_partition)
            .expect("segment partition should read")
            .expect("segment partition should exist");
        let ipam: FixtureState = store
            .read(&ipam_partition)
            .expect("IPAM partition should read")
            .expect("IPAM partition should exist");
        assert_eq!(segment.owner.as_deref(), Some("segment-owner"));
        assert_eq!(ipam.owner.as_deref(), Some("ipam-owner"));
        assert!(
            !store.filesystem_kind().is_empty(),
            "startup must record the detected filesystem kind"
        );
    }

    #[test]
    fn closure_rejection_does_not_publish_partial_state() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("committed".to_owned());
                Ok::<_, &'static str>(())
            })
            .expect("seed should commit");

        let rejected = store.transaction(&fixture_partition(), |state: &mut FixtureState| {
            state.owner = Some("must-not-land".to_owned());
            Err::<(), _>("domain rejection")
        });
        assert!(matches!(
            rejected,
            Err(NetworkStateTransactionError::Operation("domain rejection"))
        ));
        let state: FixtureState = store
            .read(&fixture_partition())
            .expect("partition should read")
            .expect("partition should exist");
        assert_eq!(state.owner.as_deref(), Some("committed"));
    }

    #[test]
    fn payload_serialization_failure_is_a_store_error_and_publishes_nothing() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");

        let error = store
            .transaction(&fixture_partition(), |_state: &mut RefusesSerialization| {
                Ok::<_, Infallible>(())
            })
            .expect_err("payload serialization must fail closed");
        assert!(matches!(
            error,
            NetworkStateTransactionError::Store(NetworkStateStoreError::Serialization {
                partition: NetworkStatePartition::TenantIpam(_),
                ..
            })
        ));
        assert!(
            !store.authority_path().exists(),
            "a serialization failure must not publish an authority file"
        );
    }

    #[test]
    fn exhausted_revision_rejects_mutation_without_replacing_authority() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("still-live".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        let mut envelope: StoreEnvelope =
            serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
                .expect("parse authority");
        envelope.body.revision = u64::MAX;
        envelope.checksum = checksum_body(&envelope.body).expect("checksum should render");
        let exhausted_bytes = serde_json::to_vec_pretty(&envelope).expect("render authority");
        fs::write(store.authority_path(), &exhausted_bytes).expect("write exhausted authority");

        let error = store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("must-not-land".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect_err("revision exhaustion must fail closed");
        assert!(matches!(
            error,
            NetworkStateTransactionError::Store(NetworkStateStoreError::RevisionExhausted { .. })
        ));
        assert_eq!(
            fs::read(store.authority_path()).expect("read authority after rejection"),
            exhausted_bytes,
            "revision exhaustion must not replace durable authority"
        );
    }

    #[test]
    fn durability_events_are_file_sync_then_replace_then_parent_sync() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&events);
        transaction_with_durability_observer(
            &store,
            &fixture_partition(),
            move |event| recorded.lock().expect("event lock").push(event),
            |state: &mut FixtureState| {
                state.owner = Some("durable".to_owned());
                Ok::<_, Infallible>(())
            },
        )
        .expect("observed transaction should commit");

        assert_eq!(
            *events.lock().expect("event lock"),
            [
                NetworkStateDurabilityEvent::StateFileSynced,
                NetworkStateDurabilityEvent::StateReplaced,
                NetworkStateDurabilityEvent::ParentDirectorySynced,
            ]
        );
    }

    #[test]
    fn truncated_state_fails_closed_with_authority_path() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |_state: &mut FixtureState| {
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        fs::write(store.authority_path(), b"{").expect("truncate authority");

        let error = LocalNetworkStateStore::open(root.path())
            .expect_err("truncated authority must refuse startup");
        let rendered = error.to_string();
        assert!(rendered.contains("corrupt"));
        assert!(rendered.contains(&store.authority_path().display().to_string()));
    }

    #[test]
    fn checksum_rejects_semantically_valid_tampering() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("live".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        let mut envelope: Value =
            serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
                .expect("parse authority");
        envelope["body"]["records"][fixture_partition().key()]["owner"] = Value::Null;
        fs::write(
            store.authority_path(),
            serde_json::to_vec_pretty(&envelope).expect("render tampered authority"),
        )
        .expect("write tampered authority");

        let error = LocalNetworkStateStore::open(root.path())
            .expect_err("checksum mismatch must refuse startup");
        assert!(matches!(
            error,
            NetworkStateStoreError::ChecksumMismatch { .. }
        ));
    }

    #[test]
    fn incompatible_version_is_distinct_from_corruption() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |_state: &mut FixtureState| {
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        let mut envelope: Value =
            serde_json::from_slice(&fs::read(store.authority_path()).expect("read authority"))
                .expect("parse authority");
        envelope["version"] = Value::from(FORMAT_VERSION + 1);
        fs::write(
            store.authority_path(),
            serde_json::to_vec_pretty(&envelope).expect("render future authority"),
        )
        .expect("write future authority");

        let error = LocalNetworkStateStore::open(root.path())
            .expect_err("future version must refuse startup");
        assert!(matches!(
            error,
            NetworkStateStoreError::IncompatibleVersion {
                found: 2,
                supported: 1,
                ..
            }
        ));
    }

    #[test]
    fn stale_stage_is_removed_without_changing_committed_state() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.owner = Some("committed".to_owned());
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        let stale = store.store_root.join(format!("{TEMP_PREFIX}crash.stage"));
        fs::write(&stale, b"partial future state").expect("write stale stage");

        let restarted =
            LocalNetworkStateStore::open(root.path()).expect("restart should clean stage");
        assert!(!stale.exists(), "crash stage must be removed under lock");
        let state: FixtureState = restarted
            .read(&fixture_partition())
            .expect("partition should read")
            .expect("partition should exist");
        assert_eq!(state.owner.as_deref(), Some("committed"));
    }

    #[test]
    fn startup_removes_crash_leftovers_from_the_durability_probe() {
        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        let stale_stage = store.store_root.join(format!("{PROBE_PREFIX}crash.stage"));
        let stale_done = store.store_root.join(format!("{PROBE_PREFIX}crash.done"));
        fs::write(&stale_stage, b"partially written probe").expect("write probe stage");
        fs::write(&stale_done, b"renamed probe").expect("write probe destination");

        LocalNetworkStateStore::open(root.path()).expect("restart should clean probe leftovers");

        assert!(!stale_stage.exists(), "stale probe stage must be removed");
        assert!(!stale_done.exists(), "stale renamed probe must be removed");
    }

    #[cfg(unix)]
    #[test]
    fn authority_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |_state: &mut FixtureState| {
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");

        let state_mode = fs::metadata(store.authority_path())
            .expect("state metadata")
            .permissions()
            .mode()
            & 0o777;
        let lock_mode = fs::metadata(&store.lock_path)
            .expect("lock metadata")
            .permissions()
            .mode()
            & 0o777;
        let root_mode = fs::metadata(&store.store_root)
            .expect("root metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(state_mode, OWNER_FILE_MODE);
        assert_eq!(lock_mode, OWNER_FILE_MODE);
        assert_eq!(root_mode, OWNER_DIRECTORY_MODE);
    }

    #[cfg(unix)]
    #[test]
    fn insecure_authority_permissions_fail_closed_on_restart() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempdir().expect("state root");
        let store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |_state: &mut FixtureState| {
                Ok::<_, Infallible>(())
            })
            .expect("seed should commit");
        fs::set_permissions(store.authority_path(), fs::Permissions::from_mode(0o644))
            .expect("weaken authority permissions");

        let error = LocalNetworkStateStore::open(root.path())
            .expect_err("group/world-readable authority must refuse startup");
        assert!(matches!(
            error,
            NetworkStateStoreError::InsecurePermissions { mode: 0o644, .. }
        ));
    }

    #[test]
    fn known_network_filesystems_are_rejected_and_local_types_are_accepted() {
        let root = Path::new("/authority");
        for unsupported in [
            "nfs", "nfs4", "smbfs", "cifs", "9p", "afs", "coda", "ncp", "ceph", "webdav",
        ] {
            assert!(
                matches!(
                    ensure_supported_filesystem(root, unsupported),
                    Err(NetworkStateStoreError::UnsupportedFilesystem { .. })
                ),
                "{unsupported} must fail closed"
            );
        }
        for supported in ["apfs", "ext", "xfs", "btrfs", "tmpfs", "overlay"] {
            ensure_supported_filesystem(root, supported)
                .unwrap_or_else(|error| panic!("{supported} should be accepted: {error}"));
        }
    }

    #[test]
    fn signed_ilp32_linux_magic_preserves_cifs_and_smb2_bit_patterns() {
        for (magic, expected) in [(0xff53_4d42_u32, "cifs"), (0xfe53_4d42_u32, "smb2")] {
            let signed = magic as i32;
            assert!(
                signed.is_negative(),
                "fixture must exercise the ILP32 sign bit"
            );
            assert_eq!(
                classify_linux_filesystem_magic(signed as u32),
                expected,
                "normalizing through u32 must prevent signed-word extension"
            );
            assert!(matches!(
                ensure_supported_filesystem(
                    Path::new("/authority"),
                    &classify_linux_filesystem_magic(signed as u32)
                ),
                Err(NetworkStateStoreError::UnsupportedFilesystem { .. })
            ));
        }
    }

    #[test]
    fn windows_verbatim_drive_and_unc_roots_are_classified_fail_closed() {
        assert_eq!(
            windows_classification_root(r"\\?\C:\Users\Nimbus\state"),
            Some(WindowsClassificationRoot::Drive("C:\\".to_owned()))
        );
        assert_eq!(
            windows_classification_root(r"D:\state"),
            Some(WindowsClassificationRoot::Drive("D:\\".to_owned()))
        );
        assert_eq!(
            windows_classification_root(r"\\?\UNC\server\share\state"),
            Some(WindowsClassificationRoot::Unc)
        );
        assert_eq!(
            windows_classification_root(r"\\server\share\state"),
            Some(WindowsClassificationRoot::Unc)
        );
        assert_eq!(
            windows_classification_root(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\state"),
            None,
            "unknown device/volume shapes must fail closed instead of bypassing classification"
        );
    }

    #[test]
    fn windows_lock_violation_is_classified_as_bounded_contention() {
        let error = io::Error::from_raw_os_error(WINDOWS_ERROR_LOCK_VIOLATION);
        assert!(
            is_lock_contended(&error),
            "fs2's Windows ERROR_LOCK_VIOLATION must enter the bounded retry path"
        );
    }

    #[test]
    fn cleanup_pending_payload_survives_repeated_restart() {
        let root = tempdir().expect("state root");
        let mut store = LocalNetworkStateStore::open(root.path()).expect("store should open");
        store
            .transaction(&fixture_partition(), |state: &mut FixtureState| {
                state.cleanup_pending.insert(
                    "portlease-a".to_owned(),
                    "provider-delete-ambiguous".to_owned(),
                );
                Ok::<_, Infallible>(())
            })
            .expect("cleanup-pending state should commit");

        for _ in 0..3 {
            store = LocalNetworkStateStore::open(root.path()).expect("restart should open");
            let state: FixtureState = store
                .read(&fixture_partition())
                .expect("partition should read")
                .expect("partition should exist");
            assert_eq!(
                state.cleanup_pending.get("portlease-a").map(String::as_str),
                Some("provider-delete-ambiguous")
            );
        }
    }

    #[test]
    fn contended_lock_times_out_without_an_unlocked_read() {
        let root = tempdir().expect("state root");
        let options = LocalNetworkStateStoreOptions {
            lock_timeout: Duration::from_millis(50),
            lock_retry_interval: Duration::from_millis(2),
        };
        let holder =
            LocalNetworkStateStore::open_with_options(root.path(), options).expect("holder open");
        let contender = LocalNetworkStateStore::open_with_options(root.path(), options)
            .expect("contender open");
        let (held_tx, held_rx) = mpsc::sync_channel(0);
        let (release_tx, release_rx) = mpsc::sync_channel(0);

        let holder_thread = std::thread::spawn(move || {
            transaction_with_durability_observer(
                &holder,
                &fixture_partition(),
                |event| {
                    if event == NetworkStateDurabilityEvent::StateFileSynced {
                        held_tx.send(()).expect("held signal should deliver");
                        release_rx.recv().expect("release signal should deliver");
                    }
                },
                |state: &mut FixtureState| {
                    state.owner = Some("holder".to_owned());
                    Ok::<_, Infallible>(())
                },
            )
            .expect("holder transaction should finish after release");
        });
        held_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("holder must reach the synced stage");

        let error = contender
            .read::<FixtureState>(&fixture_partition())
            .expect_err("contender must fail closed while the authority lock is held");
        assert!(
            matches!(error, NetworkStateStoreError::LockTimeout { .. }),
            "contender must report a bounded lock timeout: {error}"
        );
        release_tx.send(()).expect("holder release should deliver");
        holder_thread.join().expect("holder thread should join");

        let state: FixtureState = contender
            .read(&fixture_partition())
            .expect("read should succeed after release")
            .expect("partition should exist");
        assert_eq!(state.owner.as_deref(), Some("holder"));
    }
}
