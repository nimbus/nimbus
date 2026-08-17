//! Parent OS-node composition for managed-machine network authority.
//!
//! This module pairs machine artifact records with the one process-owned
//! `nimbus-network` authority. It does not perform gvproxy, VMM, socket, policy,
//! or service-name effects.

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus::Error;
use nimbus_machine::{MachineConfigRecord, MachineNetworkAuthorityRecord, MachineRootLayout};
use nimbus_network::{
    LocalNetworkAuthority, LocalNetworkManager, NetworkCapabilityRegistry, NetworkProviderHandle,
    NetworkProviderId,
};
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_sandbox::backends::container::OciMachinePortForwarderConfig;
use ulid::Ulid;

/// Process-lifetime owner of the direct CLI's fail-closed parent composition.
///
/// The manager is retained, rather than reconstructed from serialized machine
/// roots, until the direct command has completed. Embedded server composition
/// injects its already-frozen authority through [`HostMachineNetworkAuthority`]
/// instead.
pub(crate) struct HostMachineNetworkComposition {
    _manager: Arc<LocalNetworkManager>,
    authority: HostMachineNetworkAuthority,
}

impl HostMachineNetworkComposition {
    pub(crate) fn claim_default() -> Result<Self, Error> {
        let root = LocalNodeNetworkRoot::resolve_for_current_platform(None).map_err(|error| {
            Error::Internal(format!(
                "failed to resolve the parent machine network authority: {error}"
            ))
        })?;
        Self::claim(root)
    }

    fn claim(root: LocalNodeNetworkRoot) -> Result<Self, Error> {
        let registry = NetworkCapabilityRegistry::new([]).map_err(|error| {
            Error::Internal(format!(
                "failed to freeze the parent machine network capability registry: {error}"
            ))
        })?;
        let manager = LocalNetworkManager::open(root.as_path(), registry).map_err(|error| {
            Error::Internal(format!(
                "failed to claim the parent machine network authority: {error}"
            ))
        })?;
        let authority = HostMachineNetworkAuthority::injected(manager.authority());
        Ok(Self {
            _manager: manager,
            authority,
        })
    }

    pub(crate) fn authority(&self) -> HostMachineNetworkAuthority {
        self.authority.clone()
    }

    #[cfg(test)]
    pub(super) fn claim_at(root: &std::path::Path) -> Result<Self, Error> {
        let root =
            LocalNodeNetworkRoot::resolve_for_current_platform(Some(root)).map_err(|error| {
                Error::Internal(format!(
                    "failed to resolve the test parent machine network authority: {error}"
                ))
            })?;
        Self::claim(root)
    }
}

/// Cloneable authority injected into every parent machine lifecycle path.
#[derive(Clone)]
pub(crate) struct HostMachineNetworkAuthority {
    port_leases: nimbus_network::LocalPortLeaseAuthority,
    // Retain the manager-derived claim for every production authority clone.
    // Isolated unit tests may inject primitive durable handles without
    // manufacturing a second process composition.
    _composition_claim: Option<LocalNetworkAuthority>,
}

/// Manager-derived capabilities needed by one host-machine lifecycle action.
///
/// This handle neither owns nor reconstructs process composition. Production
/// callers can obtain it only from the retained [`HostMachineNetworkAuthority`];
/// the primitive constructor exists solely for isolated manager unit tests.
#[cfg(any(unix, test))]
#[derive(Clone)]
pub(super) struct MachineNetworkLifecycleHandle {
    port_leases: nimbus_network::LocalPortLeaseAuthority,
    publications: super::publication_authority::MachinePublicationIntentStore,
}

#[cfg(any(unix, test))]
impl MachineNetworkLifecycleHandle {
    pub(super) fn port_leases(&self) -> nimbus_network::LocalPortLeaseAuthority {
        self.port_leases.clone()
    }

    pub(super) fn machine_publications(
        &self,
    ) -> super::publication_authority::MachinePublicationIntentStore {
        self.publications.clone()
    }

    #[cfg(test)]
    pub(super) fn from_port_leases_for_test(
        port_leases: nimbus_network::LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        let publications = super::publication_authority::MachinePublicationIntentStore::open(
            port_leases.state_root(),
        )?;
        Ok(Self {
            port_leases,
            publications,
        })
    }
}

impl HostMachineNetworkAuthority {
    pub(crate) fn injected(authority: LocalNetworkAuthority) -> Self {
        Self {
            port_leases: authority.port_leases(),
            _composition_claim: Some(authority),
        }
    }

    #[cfg(test)]
    pub(super) fn from_port_leases_for_test(
        port_leases: nimbus_network::LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        let canonical_root = fs::canonicalize(port_leases.state_root()).map_err(|error| {
            Error::Internal(format!(
                "failed to canonicalize isolated test machine network authority {}: {error}",
                port_leases.state_root().display()
            ))
        })?;
        let port_leases = if port_leases.state_root() == canonical_root {
            port_leases
        } else {
            nimbus_network::LocalPortLeaseAuthority::open(canonical_root).map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen canonical isolated test machine network authority: {error}"
                ))
            })?
        };
        Ok(Self {
            port_leases,
            _composition_claim: None,
        })
    }

    pub(crate) fn port_leases(&self) -> nimbus_network::LocalPortLeaseAuthority {
        self.port_leases.clone()
    }

    pub(super) fn machine_publications(
        &self,
    ) -> Result<super::publication_authority::MachinePublicationIntentStore, Error> {
        super::publication_authority::MachinePublicationIntentStore::open(
            self.port_leases.state_root(),
        )
    }

    #[cfg(any(unix, test))]
    pub(super) fn lifecycle_handle(&self) -> Result<MachineNetworkLifecycleHandle, Error> {
        Ok(MachineNetworkLifecycleHandle {
            port_leases: self.port_leases(),
            publications: self.machine_publications()?,
        })
    }

    /// Authenticate persisted manager and artifact provenance without
    /// creating, opening, or mutating the attempted roots.
    pub(super) fn authenticate_config(
        &self,
        config: &MachineConfigRecord,
        attempted_roots: &MachineRootLayout,
    ) -> Result<(), HostMachineNetworkAuthenticationError> {
        let attempted_authority_path =
            diagnostic_existing_path(config.network_authority.authority_path());
        if attempted_authority_path != self.port_leases.authority_path() {
            return Err(
                HostMachineNetworkAuthenticationError::NetworkAuthorityMismatch {
                    active_authority_path: self.port_leases.authority_path().to_path_buf(),
                    attempted_authority_path,
                },
            );
        }

        let presented_provider = config.network_authority.provider_instance();
        let expected_provider = OciMachinePortForwarderConfig::gvproxy_provider_handle(
            presented_provider.expose_to_provider(),
        )
        .map_err(|_| HostMachineNetworkAuthenticationError::InvalidProviderIdentity)?;
        if expected_provider != *presented_provider {
            return Err(
                HostMachineNetworkAuthenticationError::ProviderIdentityMismatch {
                    expected_provider_id: expected_provider.provider_id().clone(),
                    attempted_provider_id: presented_provider.provider_id().clone(),
                },
            );
        }

        authenticate_artifact_root(
            MachineArtifactRootKind::Config,
            &config.roots.config_root,
            &attempted_roots.config_root,
        )?;
        authenticate_artifact_root(
            MachineArtifactRootKind::State,
            &config.roots.state_root,
            &attempted_roots.state_root,
        )?;
        authenticate_artifact_root(
            MachineArtifactRootKind::Data,
            &config.roots.data_root,
            &attempted_roots.data_root,
        )?;
        authenticate_artifact_root(
            MachineArtifactRootKind::Cache,
            &config.roots.cache_root,
            &attempted_roots.cache_root,
        )?;
        authenticate_artifact_root(
            MachineArtifactRootKind::Runtime,
            &config.roots.runtime_root,
            &attempted_roots.runtime_root,
        )
    }

    pub(super) fn new_machine_record(
        &self,
        machine_name: &str,
    ) -> Result<MachineNetworkAuthorityRecord, Error> {
        let provider_instance = machine_provider_instance(machine_name)?;
        MachineNetworkAuthorityRecord::new(
            self.port_leases.authority_path(),
            provider_instance,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to record parent network authority for machine '{machine_name}': {error}"
            ))
        })
    }

    #[cfg(test)]
    pub(super) fn authority_path(&self) -> &Path {
        self.port_leases.authority_path()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachineArtifactRootKind {
    Config,
    State,
    Data,
    Cache,
    Runtime,
}

impl Display for MachineArtifactRootKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Config => "config",
            Self::State => "state",
            Self::Data => "data",
            Self::Cache => "cache",
            Self::Runtime => "runtime",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HostMachineNetworkAuthenticationError {
    NetworkAuthorityMismatch {
        active_authority_path: PathBuf,
        attempted_authority_path: PathBuf,
    },
    ProviderIdentityMismatch {
        expected_provider_id: NetworkProviderId,
        attempted_provider_id: NetworkProviderId,
    },
    InvalidProviderIdentity,
    ArtifactRootMismatch {
        kind: MachineArtifactRootKind,
        persisted_root: PathBuf,
        attempted_root: PathBuf,
    },
}

impl Display for HostMachineNetworkAuthenticationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NetworkAuthorityMismatch {
                active_authority_path,
                attempted_authority_path,
            } => write!(
                formatter,
                "machine network authority {} does not match active parent authority {}",
                attempted_authority_path.display(),
                active_authority_path.display()
            ),
            Self::ProviderIdentityMismatch {
                expected_provider_id,
                attempted_provider_id,
            } => write!(
                formatter,
                "machine forwarder provider registration {attempted_provider_id} does not match \
                 the gvproxy owner {expected_provider_id}"
            ),
            Self::InvalidProviderIdentity => {
                formatter.write_str("machine forwarder provider identity is invalid")
            }
            Self::ArtifactRootMismatch {
                kind,
                persisted_root,
                attempted_root,
            } => write!(
                formatter,
                "machine {kind} artifact root {} does not match persisted root {}",
                attempted_root.display(),
                persisted_root.display()
            ),
        }
    }
}

impl std::error::Error for HostMachineNetworkAuthenticationError {}

fn authenticate_artifact_root(
    kind: MachineArtifactRootKind,
    persisted_root: &Path,
    attempted_root: &Path,
) -> Result<(), HostMachineNetworkAuthenticationError> {
    let persisted_root = diagnostic_existing_path(persisted_root);
    let attempted_root = diagnostic_existing_path(attempted_root);
    if persisted_root == attempted_root {
        return Ok(());
    }
    Err(
        HostMachineNetworkAuthenticationError::ArtifactRootMismatch {
            kind,
            persisted_root,
            attempted_root,
        },
    )
}

fn diagnostic_existing_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    for ancestor in absolute.ancestors() {
        let Ok(canonical_ancestor) = fs::canonicalize(ancestor) else {
            continue;
        };
        let Ok(unresolved_suffix) = absolute.strip_prefix(ancestor) else {
            continue;
        };
        return canonical_ancestor.join(unresolved_suffix);
    }
    absolute
}

fn machine_provider_instance(machine_name: &str) -> Result<NetworkProviderHandle, Error> {
    OciMachinePortForwarderConfig::gvproxy_provider_handle(format!(
        "managed-machine:{machine_name}:{}",
        Ulid::new()
    ))
    .map_err(|error| {
        Error::Internal(format!(
            "failed to create the gvproxy provider identity for machine '{machine_name}': {error}"
        ))
    })
}
