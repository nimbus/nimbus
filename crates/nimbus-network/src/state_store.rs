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
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use nimbus_core::TenantId;
use nimbus_durable_record::WriteCheckpoint;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ulid::Ulid;

mod filesystem_kind;
mod owner_files;

use filesystem_kind::{detect_filesystem_kind, ensure_supported_filesystem};
use owner_files::{
    StagedWriteOperations, replace_owner_file, sync_authority_directory,
    validate_owner_file_permissions,
};
pub(crate) use owner_files::{create_dir_all_owner_only, is_lock_contended, open_owner_file};

const STORE_DIRECTORY: &str = "control-plane";
const STORE_FILE: &str = "state.json";
const LOCK_FILE: &str = "authority.lock";
const FORMAT_MAGIC: &str = "nimbus-network-state";
const FORMAT_VERSION: u32 = 2;
const TEMP_PREFIX: &str = ".nimbus-network-state-";
const PROBE_PREFIX: &str = ".nimbus-network-probe-";

/// A typed partition inside the single node-local network authority.
///
/// Partitions isolate payload schemas while every mutation still shares one
/// lock, revision, checksum, and atomic commit point. New network resource
/// families extend this enum instead of creating another local store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NetworkStatePartition {
    /// Portable tenant-segment assignments and holds.
    SegmentAllocations,
    /// Tenant-qualified durable attachment lifecycle records.
    AttachmentStates,
    /// Provider-adjacent IP allocation state for one tenant.
    TenantIpam(TenantId),
    /// Host-global port lease lifecycle records.
    PortLeases,
}

impl NetworkStatePartition {
    fn key(&self) -> String {
        match self {
            Self::SegmentAllocations => "segment-allocations".to_owned(),
            Self::AttachmentStates => "attachment-states".to_owned(),
            Self::TenantIpam(tenant_id) => format!("tenant-ipam/{}", tenant_id.as_str()),
            Self::PortLeases => "port-leases".to_owned(),
        }
    }

    fn from_key(key: &str) -> Result<Self, String> {
        match key {
            "segment-allocations" => Ok(Self::SegmentAllocations),
            "attachment-states" => Ok(Self::AttachmentStates),
            "port-leases" => Ok(Self::PortLeases),
            _ => {
                let tenant = key
                    .strip_prefix("tenant-ipam/")
                    .ok_or_else(|| format!("unknown network state partition key {key:?}"))?;
                if tenant.is_empty() {
                    return Err("tenant IPAM partition has an empty tenant identity".to_owned());
                }
                let tenant_id = TenantId::new(tenant).map_err(|error| {
                    format!(
                        "tenant IPAM partition key {key:?} contains an invalid tenant identity: \
                         {error}"
                    )
                })?;
                let partition = Self::TenantIpam(tenant_id);
                if partition.key() != key {
                    return Err(format!(
                        "tenant IPAM partition key {key:?} is not in canonical form"
                    ));
                }
                Ok(partition)
            }
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
            // A segment teardown can require several fsync-backed authority
            // transitions. Under legitimate same-node concurrency, a two
            // second waiter could fail while prior owners were still making
            // bounded forward progress. Keep contention fail-closed and
            // bounded, but allow a realistic durability budget.
            lock_timeout: Duration::from_secs(30),
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
    process_lock: Arc<Mutex<()>>,
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

        let lock_path = store_root.join(LOCK_FILE);
        let store = Self {
            state_root,
            state_path: store_root.join(STORE_FILE),
            process_lock: process_lock_for(&lock_path)?,
            lock_path,
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

    /// Enumerate every durable tenant-IPAM partition in deterministic order.
    ///
    /// Enumeration validates all durable partition keys while holding the same
    /// process and cross-process lock used by reads and transactions. Unknown
    /// or malformed keys are corruption: silently skipping one could hide
    /// provider-attempt authority from startup reconciliation.
    pub fn tenant_ipam_tenants(&self) -> Result<Vec<TenantId>, NetworkStateStoreError> {
        let _lock = self.acquire_lock()?;
        let body = self.load_body()?;
        let mut tenants = Vec::new();
        for key in body.records.keys() {
            match NetworkStatePartition::from_key(key).map_err(|reason| {
                NetworkStateStoreError::Corrupt {
                    path: self.state_path.clone(),
                    reason,
                }
            })? {
                NetworkStatePartition::TenantIpam(tenant_id) => tenants.push(tenant_id),
                NetworkStatePartition::SegmentAllocations
                | NetworkStatePartition::AttachmentStates
                | NetworkStatePartition::PortLeases => {}
            }
        }
        Ok(tenants)
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
        let original_payload = body.records.get(&key).cloned();
        let mut payload = match original_payload.clone() {
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
        let absent_default = if original_payload.is_none() {
            Some(serde_json::to_value(T::default()).map_err(|source| {
                NetworkStateTransactionError::Store(NetworkStateStoreError::Serialization {
                    partition: partition.clone(),
                    reason: source.to_string(),
                })
            })?)
        } else {
            None
        };
        // A concept owner may need the exclusive cross-process lock merely to
        // authenticate a no-effect or already-terminal transition. Preserve
        // the exact authority revision and bytes when that locked decision
        // leaves an existing partition unchanged or leaves an absent
        // partition at its default.
        if original_payload.as_ref() == Some(&payload) || absent_default.as_ref() == Some(&payload)
        {
            return Ok(result);
        }
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

    fn acquire_lock(&self) -> Result<AuthorityLock<'_>, NetworkStateStoreError> {
        let started = Instant::now();
        let process_guard = loop {
            match self.process_lock.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= self.options.lock_timeout {
                        return Err(NetworkStateStoreError::LockTimeout {
                            path: self.lock_path.clone(),
                            timeout: self.options.lock_timeout,
                        });
                    }
                    let remaining = self.options.lock_timeout.saturating_sub(started.elapsed());
                    thread::sleep(self.options.lock_retry_interval.min(remaining));
                }
                // This mutex protects no in-memory authority: durable state is
                // loaded and validated only after the OS file lock is held.
                // A panicking prior holder therefore cannot leave protected
                // data inconsistent, so recover the serialization guard.
                Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            }
        };
        let file = open_owner_file(&self.lock_path)?;
        loop {
            match file.try_lock().map_err(std::io::Error::from) {
                Ok(()) => {
                    return Ok(AuthorityLock {
                        _process_guard: process_guard,
                        file,
                    });
                }
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
            sync_authority_directory(&self.store_root)?;
        }
        Ok(())
    }

    fn probe_durable_replace(&self) -> Result<(), NetworkStateStoreError> {
        let token = Ulid::new();
        let source = self.store_root.join(format!("{PROBE_PREFIX}{token}.stage"));
        let destination = self.store_root.join(format!("{PROBE_PREFIX}{token}.done"));
        let probe_result = replace_owner_file(
            &source,
            &destination,
            b"nimbus-network-durability-probe",
            &PROBE_OPERATIONS,
            &mut |_| {},
        )
        .and_then(|()| {
            fs::remove_file(&destination).map_err(|source_error| NetworkStateStoreError::Io {
                operation: "remove durability probe",
                path: destination.clone(),
                source: source_error,
            })?;
            sync_authority_directory(&self.store_root)
        });
        if probe_result.is_err() {
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

const STATE_OPERATIONS: StagedWriteOperations = StagedWriteOperations {
    create: "open owner-only authority file",
    write: "write staged authority state",
    sync: "sync staged authority state",
    replace: "atomically replace authority state",
};

const PROBE_OPERATIONS: StagedWriteOperations = StagedWriteOperations {
    create: "open owner-only authority file",
    write: "write durability probe",
    sync: "sync durability probe",
    replace: "replace durability probe",
};

fn durable_replace(
    parent: &Path,
    destination: &Path,
    bytes: &[u8],
    temp_prefix: &str,
    observer: &dyn Fn(DurabilityEvent),
) -> Result<(), NetworkStateStoreError> {
    let stage = parent.join(format!("{temp_prefix}{}.stage", Ulid::new()));
    replace_owner_file(
        &stage,
        destination,
        bytes,
        &STATE_OPERATIONS,
        &mut |checkpoint| {
            observer(match checkpoint {
                WriteCheckpoint::StageDurable => DurabilityEvent::StateFileSynced,
                WriteCheckpoint::Committed => DurabilityEvent::StateReplaced,
            });
        },
    )?;
    observer(DurabilityEvent::ParentDirectorySynced);
    Ok(())
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

fn process_lock_for(path: &Path) -> Result<Arc<Mutex<()>>, NetworkStateStoreError> {
    static PROCESS_LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

    let parent = path
        .parent()
        .expect("the authority lock path always has its store directory");
    let canonical_parent =
        fs::canonicalize(parent).map_err(|source| NetworkStateStoreError::Io {
            operation: "canonicalize authority lock directory",
            path: parent.to_path_buf(),
            source,
        })?;
    let key = canonical_parent.join(
        path.file_name()
            .expect("the authority lock path always has a file name"),
    );
    let registry = PROCESS_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    Ok(lock)
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

struct AuthorityLock<'a> {
    _process_guard: MutexGuard<'a, ()>,
    file: File,
}

impl Drop for AuthorityLock<'_> {
    fn drop(&mut self) {
        let _ = self.file.unlock();
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
mod tests;
