//! Process-owned composition of portable local network authority.
//!
//! [`LocalNetworkManagerBootstrap`] claims the process composition before
//! dependent adapters are assembled. It exposes one paired
//! [`LocalNetworkAuthority`] over the canonical node root, durable store,
//! attachment authority, and port authority. Consuming the bootstrap then freezes one immutable
//! capability registry into [`LocalNetworkManager`]. None owns provider
//! effects.
//!
//! Primitive [`LocalNetworkStateStore`], [`LocalNetworkAttachmentAuthority`],
//! and [`LocalPortLeaseAuthority`] handles remain independently openable: they
//! are transaction adapters over the same process mutex and OS file lock. The
//! manager prevents a second independent *composition* from silently selecting
//! another root or capability view.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::{
    LocalNetworkAttachmentAuthority, LocalNetworkStateStore, LocalPortLeaseAuthority,
    NetworkAttachmentStateError, NetworkCapabilityRegistry, NetworkStateStoreError, PortLeaseError,
};

static PROCESS_COMPOSITION: OnceLock<Mutex<Weak<LocalNetworkComposition>>> = OnceLock::new();

struct LocalNetworkComposition {
    state_root: PathBuf,
    authority_path: PathBuf,
    state_store: LocalNetworkStateStore,
    attachments: LocalNetworkAttachmentAuthority,
    port_leases: LocalPortLeaseAuthority,
}

/// Portable handles paired to the one process-owned local network authority.
///
/// Cloning this value deliberately retains the process claim. The final
/// bootstrap, manager, or authority handle must be dropped before a new
/// process composition can open.
#[derive(Clone)]
pub struct LocalNetworkAuthority {
    composition: Arc<LocalNetworkComposition>,
}

impl LocalNetworkAuthority {
    /// Canonical node-local state root owned by this process composition.
    pub fn state_root(&self) -> &Path {
        &self.composition.state_root
    }

    /// Canonical durable authority path used in diagnostics and fencing.
    pub fn authority_path(&self) -> &Path {
        &self.composition.authority_path
    }

    /// Deliberate handle to the one store/lock domain.
    pub fn state_store(&self) -> LocalNetworkStateStore {
        self.composition.state_store.clone()
    }

    /// Deliberate attachment-lifecycle handle derived from the same store.
    pub fn attachments(&self) -> LocalNetworkAttachmentAuthority {
        self.composition.attachments.clone()
    }

    /// Deliberate port-lifecycle handle derived from the same store.
    pub fn port_leases(&self) -> LocalPortLeaseAuthority {
        self.composition.port_leases.clone()
    }

    /// Authenticate a configured state root without creating or mutating it.
    ///
    /// Existing filesystem aliases resolve to the selected authority. A
    /// divergent or missing path returns typed diagnostic evidence.
    pub fn authenticate_state_root(
        &self,
        attempted_state_root: impl AsRef<Path>,
    ) -> Result<(), LocalNetworkAuthorityRootMismatch> {
        let attempted_authority_path = diagnostic_authority_path(attempted_state_root.as_ref());
        if attempted_authority_path == self.authority_path() {
            return Ok(());
        }

        Err(LocalNetworkAuthorityRootMismatch {
            active_authority_path: self.authority_path().to_path_buf(),
            attempted_authority_path,
        })
    }
}

impl fmt::Debug for LocalNetworkAuthority {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalNetworkAuthority")
            .field("state_root", &self.state_root())
            .field("authority_path", &self.authority_path())
            .finish_non_exhaustive()
    }
}

/// Provisional owner used while source capabilities are assembled.
///
/// The bootstrap already owns the durable process authority, so dependent
/// construction cannot race a second composition. [`Self::freeze`] consumes
/// the provisional state and installs the final immutable registry exactly
/// once.
pub struct LocalNetworkManagerBootstrap {
    authority: LocalNetworkAuthority,
}

impl LocalNetworkManagerBootstrap {
    /// Clone the paired authority while dependent adapters are assembled.
    pub fn authority(&self) -> LocalNetworkAuthority {
        self.authority.clone()
    }

    /// Consume the bootstrap and freeze the supplied immutable registry.
    pub fn freeze(
        self,
        capability_registry: NetworkCapabilityRegistry,
    ) -> Arc<LocalNetworkManager> {
        Arc::new(LocalNetworkManager {
            authority: self.authority,
            capability_registry,
        })
    }
}

impl fmt::Debug for LocalNetworkManagerBootstrap {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalNetworkManagerBootstrap")
            .field("authority", &self.authority)
            .finish_non_exhaustive()
    }
}

/// The one process-owned local network composition.
///
/// Construction returns an [`Arc`] deliberately: cloning that `Arc` is the
/// only way to share composition ownership. A second independent construction
/// fails even when it names a different root, because one process must not
/// split host-global resource authority across node stores.
pub struct LocalNetworkManager {
    authority: LocalNetworkAuthority,
    capability_registry: NetworkCapabilityRegistry,
}

impl LocalNetworkManager {
    /// Claim the process authority before dependent capability assembly.
    ///
    /// The returned bootstrap and every authority clone share the same private
    /// claim token. Separate OS processes may bootstrap the same root because
    /// their stores coordinate through the durable cross-process file lock.
    pub fn bootstrap(
        state_root: impl AsRef<Path>,
    ) -> Result<LocalNetworkManagerBootstrap, LocalNetworkManagerError> {
        let requested_root = state_root.as_ref();
        let slot = PROCESS_COMPOSITION.get_or_init(|| Mutex::new(Weak::new()));
        let mut active = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(composition) = active.upgrade() {
            return Err(LocalNetworkManagerError::DuplicateProcessComposition {
                active_authority_path: composition.authority_path.clone(),
                attempted_authority_path: diagnostic_authority_path(requested_root),
            });
        }

        // Hold the process-composition mutex through initialization. This is a
        // startup-only path and makes concurrent construction linearizable:
        // no provisional token or stale failed-open claim can escape.
        let initialized_store = LocalNetworkStateStore::open(requested_root)
            .map_err(LocalNetworkManagerError::Store)?;
        let state_root = fs::canonicalize(initialized_store.state_root()).map_err(|source| {
            LocalNetworkManagerError::Store(NetworkStateStoreError::Io {
                operation: "canonicalize initialized network manager root",
                path: initialized_store.state_root().to_path_buf(),
                source,
            })
        })?;
        let state_store = if initialized_store.state_root() == state_root {
            initialized_store
        } else {
            // The store deliberately preserves its supplied diagnostic path.
            // Reopen through the now-existing canonical root so every paired
            // manager handle renders the same authority even on platforms
            // whose temporary-directory prefix is a filesystem alias.
            LocalNetworkStateStore::open(&state_root).map_err(LocalNetworkManagerError::Store)?
        };
        let authority_path = LocalNetworkStateStore::authority_path_for(&state_root);
        let attachments = LocalNetworkAttachmentAuthority::from_store(state_store.clone())
            .map_err(LocalNetworkManagerError::AttachmentState)?;
        let port_leases = LocalPortLeaseAuthority::from_store(state_store.clone())
            .map_err(LocalNetworkManagerError::PortLease)?;
        let composition = Arc::new(LocalNetworkComposition {
            state_root,
            authority_path,
            state_store,
            attachments,
            port_leases,
        });
        *active = Arc::downgrade(&composition);
        Ok(LocalNetworkManagerBootstrap {
            authority: LocalNetworkAuthority { composition },
        })
    }

    /// Claim the process network composition over one node-local state root.
    ///
    /// This convenience path delegates to [`Self::bootstrap`] and immediately
    /// freezes the supplied registry. Callers that must assemble source-owned
    /// capabilities under the process claim use the staged API instead.
    pub fn open(
        state_root: impl AsRef<Path>,
        capability_registry: NetworkCapabilityRegistry,
    ) -> Result<Arc<Self>, LocalNetworkManagerError> {
        Ok(Self::bootstrap(state_root)?.freeze(capability_registry))
    }

    /// Portable authority paired to this frozen manager.
    pub fn authority(&self) -> LocalNetworkAuthority {
        self.authority.clone()
    }

    /// Canonical node-local state root owned by this process composition.
    pub fn state_root(&self) -> &Path {
        self.authority.state_root()
    }

    /// Canonical durable authority path used in diagnostics and fencing.
    pub fn authority_path(&self) -> &Path {
        self.authority.authority_path()
    }

    /// Deliberate handle to the one store/lock domain.
    pub fn state_store(&self) -> LocalNetworkStateStore {
        self.authority.state_store()
    }

    /// Deliberate attachment-lifecycle handle derived from the manager's store.
    pub fn attachments(&self) -> LocalNetworkAttachmentAuthority {
        self.authority.attachments()
    }

    /// Deliberate port-lifecycle handle derived from the manager's store.
    pub fn port_leases(&self) -> LocalPortLeaseAuthority {
        self.authority.port_leases()
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
            .field("state_root", &self.state_root())
            .field("authority_path", &self.authority_path())
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
    /// The durable attachment partition violates attachment invariants.
    AttachmentState(NetworkAttachmentStateError),
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
            Self::AttachmentState(error) => write!(
                formatter,
                "failed to initialize the network manager attachment authority: {error}"
            ),
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
            Self::AttachmentState(error) => Some(error),
            Self::PortLease(error) => Some(error),
        }
    }
}

/// A configured root does not name the selected process authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalNetworkAuthorityRootMismatch {
    active_authority_path: PathBuf,
    attempted_authority_path: PathBuf,
}

impl LocalNetworkAuthorityRootMismatch {
    /// Canonical authority path selected by the process composition.
    pub fn active_authority_path(&self) -> &Path {
        &self.active_authority_path
    }

    /// Best-effort non-mutating authority path derived from the attempted root.
    pub fn attempted_authority_path(&self) -> &Path {
        &self.attempted_authority_path
    }
}

impl Display for LocalNetworkAuthorityRootMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "network state root resolves to authority {}; active process authority is {}; \
             inject the existing LocalNetworkAuthority instead of selecting another root",
            self.attempted_authority_path.display(),
            self.active_authority_path.display()
        )
    }
}

impl StdError for LocalNetworkAuthorityRootMismatch {}

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
