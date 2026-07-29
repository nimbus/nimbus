//! Construction and access for one retained portable port authority.

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;

use nimbus_network::LocalPortLeaseAuthority;

use crate::error::{Result, SandboxError};

use super::{OciPortLeaseCoordinator, PublishedListenerProvider};

impl OciPortLeaseCoordinator {
    pub(crate) fn from_authority(
        authority: LocalPortLeaseAuthority,
        range: RangeInclusive<u16>,
    ) -> Self {
        Self {
            range,
            authority: Ok(authority),
            max_ports_per_tenant: None,
            published_listener_provider: PublishedListenerProvider::Netavark,
        }
    }

    /// Reconstruct once at an explicitly selected direct-adapter boundary.
    pub(crate) fn reconstruct_direct(
        state_root: impl AsRef<Path>,
        range: RangeInclusive<u16>,
    ) -> Self {
        Self::from_reconstructed_authority("direct adapter", state_root.as_ref(), range)
    }

    /// Reconstruct once in the separate container-runner OS process.
    pub(crate) fn reconstruct_for_runner(
        state_root: impl AsRef<Path>,
        range: RangeInclusive<u16>,
    ) -> Self {
        Self::from_reconstructed_authority("container runner", state_root.as_ref(), range)
    }

    #[cfg(test)]
    pub(crate) fn new(state_root: impl AsRef<Path>, range: RangeInclusive<u16>) -> Self {
        Self::from_reconstructed_authority("test fixture", state_root.as_ref(), range)
    }

    fn from_reconstructed_authority(
        boundary: &'static str,
        state_root: &Path,
        range: RangeInclusive<u16>,
    ) -> Self {
        let authority = LocalPortLeaseAuthority::open(state_root).map_err(|error| {
            Arc::<str>::from(format!(
                "failed to reconstruct port authority for {boundary} at {}: {error}",
                state_root.display()
            ))
        });
        Self {
            range,
            authority,
            max_ports_per_tenant: None,
            published_listener_provider: PublishedListenerProvider::Netavark,
        }
    }

    pub(crate) fn authority(&self) -> Result<&LocalPortLeaseAuthority> {
        self.authority
            .as_ref()
            .map_err(|reason| SandboxError::OperationFailed {
                message: reason.to_string(),
            })
    }

    pub(crate) fn cloned_authority(
        &self,
    ) -> std::result::Result<LocalPortLeaseAuthority, Arc<str>> {
        self.authority.clone()
    }
}
