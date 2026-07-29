//! Process-owned composition of portable local network authority.
//!
//! [`LocalNetworkManager`] is the one composition façade in a Nimbus process.
//! It freezes one canonical node root, one durable store, one port authority,
//! and one immutable capability registry. It owns no provider effect.
//!
//! Primitive [`LocalNetworkStateStore`] and [`LocalPortLeaseAuthority`] handles
//! remain independently openable: they are transaction adapters over the same
//! process mutex and OS file lock. The manager prevents a second independent
//! *composition* from silently selecting another root or capability view.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::{
    LocalNetworkStateStore, LocalPortLeaseAuthority, NetworkCapabilityRegistry,
    NetworkStateStoreError, PortLeaseError,
};

/// The one process-owned local network composition.
///
/// Construction returns an [`Arc`] deliberately: cloning that `Arc` is the
/// only way to share composition ownership. A second independent construction
/// fails even when it names a different root, because one process must not
/// split host-global resource authority across node stores.
pub struct LocalNetworkManager {
    state_root: PathBuf,
    authority_path: PathBuf,
    state_store: LocalNetworkStateStore,
    port_leases: LocalPortLeaseAuthority,
    capability_registry: NetworkCapabilityRegistry,
}

impl LocalNetworkManager {
    /// Claim the process network composition over one node-local state root.
    ///
    /// Separate OS processes may each call this with the same root. Their
    /// stores coordinate through the existing cross-process file lock. Inside
    /// one process, callers must clone and inject the returned [`Arc`].
    pub fn open(
        state_root: impl AsRef<Path>,
        capability_registry: NetworkCapabilityRegistry,
    ) -> Result<Arc<Self>, LocalNetworkManagerError> {
        static PROCESS_MANAGER: OnceLock<Mutex<Weak<LocalNetworkManager>>> = OnceLock::new();

        let requested_root = state_root.as_ref();
        let slot = PROCESS_MANAGER.get_or_init(|| Mutex::new(Weak::new()));
        let mut active = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(manager) = active.upgrade() {
            return Err(LocalNetworkManagerError::DuplicateProcessComposition {
                active_authority_path: manager.authority_path.clone(),
                attempted_authority_path: diagnostic_authority_path(requested_root),
            });
        }

        // Hold the process-composition mutex through initialization. This is a
        // startup-only path and makes concurrent construction linearizable:
        // no provisional token or stale failed-open claim can escape.
        let state_store = LocalNetworkStateStore::open(requested_root)
            .map_err(LocalNetworkManagerError::Store)?;
        let state_root = fs::canonicalize(state_store.state_root()).map_err(|source| {
            LocalNetworkManagerError::Store(NetworkStateStoreError::Io {
                operation: "canonicalize initialized network manager root",
                path: state_store.state_root().to_path_buf(),
                source,
            })
        })?;
        let authority_path = LocalNetworkStateStore::authority_path_for(&state_root);
        let port_leases = LocalPortLeaseAuthority::from_store(state_store.clone())
            .map_err(LocalNetworkManagerError::PortLease)?;
        let manager = Arc::new(Self {
            state_root,
            authority_path,
            state_store,
            port_leases,
            capability_registry,
        });
        *active = Arc::downgrade(&manager);
        Ok(manager)
    }

    /// Canonical node-local state root owned by this process composition.
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Canonical durable authority path used in diagnostics and fencing.
    pub fn authority_path(&self) -> &Path {
        &self.authority_path
    }

    /// Deliberate handle to the one store/lock domain.
    pub fn state_store(&self) -> LocalNetworkStateStore {
        self.state_store.clone()
    }

    /// Deliberate port-lifecycle handle derived from the manager's store.
    pub fn port_leases(&self) -> LocalPortLeaseAuthority {
        self.port_leases.clone()
    }

    /// Immutable admitted provider-capability compositions for this process.
    pub fn capability_registry(&self) -> &NetworkCapabilityRegistry {
        &self.capability_registry
    }
}

impl fmt::Debug for LocalNetworkManager {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalNetworkManager")
            .field("state_root", &self.state_root)
            .field("authority_path", &self.authority_path)
            .field("capability_registry", &self.capability_registry)
            .finish_non_exhaustive()
    }
}

/// Process-composition construction failure.
#[derive(Debug)]
pub enum LocalNetworkManagerError {
    /// Another live manager already owns this process, regardless of whether
    /// the attempted root is the same, an alias, or divergent.
    DuplicateProcessComposition {
        active_authority_path: PathBuf,
        attempted_authority_path: PathBuf,
    },
    /// The node-local store could not be initialized safely.
    Store(NetworkStateStoreError),
    /// The durable port partition was invalid when the shared handle opened.
    PortLease(PortLeaseError),
}

impl Display for LocalNetworkManagerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateProcessComposition {
                active_authority_path,
                attempted_authority_path,
            } => write!(
                formatter,
                "network composition is already initialized at {}; attempted authority {}; \
                 clone and inject the existing LocalNetworkManager instead of opening an \
                 independent manager",
                active_authority_path.display(),
                attempted_authority_path.display()
            ),
            Self::Store(error) => {
                write!(formatter, "failed to initialize network manager: {error}")
            }
            Self::PortLease(error) => write!(
                formatter,
                "failed to initialize the network manager port authority: {error}"
            ),
        }
    }
}

impl StdError for LocalNetworkManagerError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::DuplicateProcessComposition { .. } => None,
            Self::Store(error) => Some(error),
            Self::PortLease(error) => Some(error),
        }
    }
}

fn diagnostic_authority_path(state_root: &Path) -> PathBuf {
    let absolute = if state_root.is_absolute() {
        state_root.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(state_root))
            .unwrap_or_else(|_| state_root.to_path_buf())
    };
    let diagnostic_root = fs::canonicalize(&absolute).unwrap_or(absolute);
    LocalNetworkStateStore::authority_path_for(diagnostic_root)
}
