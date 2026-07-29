//! Retained server-side access to one node-local network authority.
//!
//! This module owns no socket or provider effect. It keeps a manager-derived
//! process claim beside the exact port-lifecycle handle used by listener
//! adapters. The explicitly named direct reconstruction path exists for
//! embedders and tests that do not own a [`nimbus_network::LocalNetworkManager`]; it opens the
//! primitive authority once during construction and never re-resolves a path
//! while listeners are prepared or active.

use std::fmt;
use std::io;
use std::path::Path;

use nimbus_network::{LocalNetworkAuthority, LocalNetworkStateStore, LocalPortLeaseAuthority};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerNetworkAuthorityProvenance {
    ManagerDerived,
    DirectReconstruction,
}

/// One retained authority provenance plus its exact primitive port handle.
#[derive(Clone)]
pub(super) struct RetainedServerNetworkAuthority {
    port_leases: LocalPortLeaseAuthority,
    authority_path: std::path::PathBuf,
    provenance: ServerNetworkAuthorityProvenance,
    // `LocalPortLeaseAuthority` retains the store but not the manager's
    // process-composition claim. Keep the authority itself alive through every
    // prepared and active listener lifetime.
    _manager_authority: Option<LocalNetworkAuthority>,
}

impl RetainedServerNetworkAuthority {
    pub(super) fn manager_derived(authority: LocalNetworkAuthority) -> Self {
        Self {
            port_leases: authority.port_leases(),
            authority_path: authority.authority_path().to_path_buf(),
            provenance: ServerNetworkAuthorityProvenance::ManagerDerived,
            _manager_authority: Some(authority),
        }
    }

    pub(super) fn reconstruct_direct(state_root: impl AsRef<Path>) -> io::Result<Self> {
        let state_root = state_root.as_ref();
        let port_leases = LocalPortLeaseAuthority::open(state_root).map_err(network_error)?;
        let canonical_root = std::fs::canonicalize(state_root).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to canonicalize directly reconstructed network root {}: {error}",
                    state_root.display()
                ),
            )
        })?;
        Ok(Self {
            port_leases,
            authority_path: LocalNetworkStateStore::authority_path_for(canonical_root),
            provenance: ServerNetworkAuthorityProvenance::DirectReconstruction,
            _manager_authority: None,
        })
    }

    pub(super) fn port_leases(&self) -> &LocalPortLeaseAuthority {
        &self.port_leases
    }

    pub(super) fn authenticate_same_authority(&self, attempted: &Self) -> io::Result<()> {
        if self.provenance == attempted.provenance
            && self.authority_path == attempted.authority_path
        {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "server listener network authority mismatch: active {} ({:?}), attempted {} \
                 ({:?}); inject the same LocalNetworkAuthority instead of replacing the root",
                self.authority_path.display(),
                self.provenance,
                attempted.authority_path.display(),
                attempted.provenance
            ),
        ))
    }
}

impl fmt::Debug for RetainedServerNetworkAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetainedServerNetworkAuthority")
            .field("authority_path", &self.authority_path)
            .field("provenance", &self.provenance)
            .finish_non_exhaustive()
    }
}

fn network_error(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
