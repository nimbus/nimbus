//! Process-owned composition for OCI-family network adapters.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use nimbus_core::{Cidr, CidrError};
use nimbus_network::{
    LocalNetworkAttachmentAuthority, LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch,
};

use crate::backends::oci::egress::{EgressProxyProcess, EgressProxyRegistry};
use crate::backends::oci::network::ipam::OciIpamAuthority;
use crate::backends::oci::network::{ConfiguredSegmentAllocator, OciSegmentAllocator};
use crate::backends::oci::port_lifecycle::NetavarkPortLifetimeRegistry;
use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
use crate::error::SandboxError;

mod machine_proxy_lifetime;
pub(crate) use machine_proxy_lifetime::{
    MachineForwardedPublicationInspection, MachineForwardedPublicationReadiness,
    MachinePortProxyCleanupDisposition, MachinePortProxyCleanupState, MachinePortProxyEntries,
    MachinePortProxyEntry, MachinePortProxyKey, MachinePortProxyLeaseAuthority,
    MachinePortProxyLifetimeRegistry, MachinePortProxyRegistration,
};

static OCI_NETWORK_PROCESS: OnceLock<Mutex<Weak<OciNetworkProcess>>> = OnceLock::new();

/// One process-owned composition for OCI-family network adapters.
///
/// This object retains the manager-derived authority and the process-local
/// lifecycle registries shared by every injected container and krun backend.
/// It performs no provider effect and persists no state during construction.
pub struct OciNetworkProcess {
    authority: LocalNetworkAuthority,
    node_supernet: Cidr,
    tenant_prefix: u8,
    ipam: OciIpamAuthority,
    segment_allocator: Arc<OciSegmentAllocator>,
    egress: EgressProxyProcess,
    netavark_port_lifetimes: NetavarkPortLifetimeRegistry,
    machine_port_proxy_lifetimes: MachinePortProxyLifetimeRegistry,
}

impl OciNetworkProcess {
    #[cfg(test)]
    pub(crate) fn lock_test_process_claim() -> std::sync::MutexGuard<'static, ()> {
        static SERIALIZER: Mutex<()> = Mutex::new(());
        SERIALIZER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Claim the sole OCI network composition for this process.
    pub fn new(
        authority: LocalNetworkAuthority,
        node_supernet: Cidr,
        tenant_prefix: u8,
    ) -> Result<Arc<Self>, OciNetworkProcessError> {
        validate_tenant_prefix(node_supernet, tenant_prefix)?;
        let slot = OCI_NETWORK_PROCESS.get_or_init(|| Mutex::new(Weak::new()));
        let mut active = slot
            .lock()
            .map_err(|_| OciNetworkProcessError::ProcessClaimPoisoned)?;
        if let Some(process) = active.upgrade() {
            return Err(OciNetworkProcessError::DuplicateProcessComposition {
                active_authority_path: process.authority.authority_path().to_path_buf(),
                attempted_authority_path: authority.authority_path().to_path_buf(),
                active_supernet: process.node_supernet,
                attempted_supernet: node_supernet,
                active_tenant_prefix: process.tenant_prefix,
                attempted_tenant_prefix: tenant_prefix,
            });
        }

        let ipam = OciIpamAuthority::from_process(&authority);
        let segment_allocator: Arc<OciSegmentAllocator> = Arc::new(
            ConfiguredSegmentAllocator::from_store(
                authority.state_store(),
                node_supernet,
                tenant_prefix,
            )
            .map_err(OciNetworkProcessError::SegmentAdapter)?,
        );
        let process = Arc::new(Self {
            authority,
            node_supernet,
            tenant_prefix,
            ipam,
            segment_allocator,
            egress: EgressProxyProcess::new(),
            netavark_port_lifetimes: NetavarkPortLifetimeRegistry::default(),
            machine_port_proxy_lifetimes: MachinePortProxyLifetimeRegistry::default(),
        });
        *active = Arc::downgrade(&process);
        Ok(process)
    }

    #[cfg(test)]
    pub(crate) fn authority(&self) -> LocalNetworkAuthority {
        self.authority.clone()
    }

    /// Authenticate configured topology and return the immutable process root
    /// that injected backends must retain for every later path-based handoff.
    pub fn authenticate_backend_config(
        &self,
        network_state_root: impl AsRef<Path>,
        node_supernet: &str,
        tenant_prefix: u8,
    ) -> Result<PathBuf, OciNetworkProcessError> {
        self.authority
            .authenticate_state_root(network_state_root)
            .map_err(OciNetworkProcessError::AuthorityRootMismatch)?;
        let attempted_supernet = Cidr::parse(node_supernet).map_err(|source| {
            OciNetworkProcessError::InvalidNodeSupernet {
                attempted: node_supernet.to_owned(),
                source,
            }
        })?;
        if attempted_supernet != self.node_supernet {
            return Err(OciNetworkProcessError::NodeSupernetMismatch {
                active: self.node_supernet,
                attempted: attempted_supernet,
            });
        }
        if tenant_prefix != self.tenant_prefix {
            return Err(OciNetworkProcessError::TenantPrefixMismatch {
                active: self.tenant_prefix,
                attempted: tenant_prefix,
            });
        }
        Ok(self.authority.state_root().to_path_buf())
    }

    pub(crate) fn egress_registry(
        &self,
        decision_log_root: impl Into<PathBuf>,
        trust_anchor_root: impl Into<PathBuf>,
    ) -> EgressProxyRegistry {
        EgressProxyRegistry::from_process(
            self.egress.clone(),
            decision_log_root,
            trust_anchor_root,
            &self.authority,
        )
    }

    pub(crate) fn segment_allocator(&self) -> Arc<OciSegmentAllocator> {
        Arc::clone(&self.segment_allocator)
    }

    pub(crate) fn ipam_authority(&self) -> OciIpamAuthority {
        self.ipam.clone()
    }

    pub(crate) fn attachment_authority(&self) -> LocalNetworkAttachmentAuthority {
        self.authority.attachments()
    }

    pub(crate) fn netavark_port_lifetimes(&self) -> NetavarkPortLifetimeRegistry {
        self.netavark_port_lifetimes.clone()
    }

    pub(crate) fn machine_port_proxy_lifetimes(&self) -> MachinePortProxyLifetimeRegistry {
        self.machine_port_proxy_lifetimes.clone()
    }

    pub(crate) fn port_lease_coordinator(
        &self,
        range: RangeInclusive<u16>,
        max_ports_per_tenant: Option<usize>,
    ) -> OciPortLeaseCoordinator {
        OciPortLeaseCoordinator::from_authority(self.authority.port_leases(), range)
            .with_max_ports_per_tenant(max_ports_per_tenant)
    }
}

impl fmt::Debug for OciNetworkProcess {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OciNetworkProcess")
            .field("authority", &self.authority)
            .field("node_supernet", &self.node_supernet)
            .field("tenant_prefix", &self.tenant_prefix)
            .finish_non_exhaustive()
    }
}

/// Failure to construct or authenticate the OCI process composition.
#[derive(Debug)]
pub enum OciNetworkProcessError {
    InvalidTenantPrefix {
        node_supernet: Cidr,
        attempted: u8,
    },
    ProcessClaimPoisoned,
    DuplicateProcessComposition {
        active_authority_path: PathBuf,
        attempted_authority_path: PathBuf,
        active_supernet: Cidr,
        attempted_supernet: Cidr,
        active_tenant_prefix: u8,
        attempted_tenant_prefix: u8,
    },
    AuthorityRootMismatch(LocalNetworkAuthorityRootMismatch),
    InvalidNodeSupernet {
        attempted: String,
        source: CidrError,
    },
    SegmentAdapter(SandboxError),
    NodeSupernetMismatch {
        active: Cidr,
        attempted: Cidr,
    },
    TenantPrefixMismatch {
        active: u8,
        attempted: u8,
    },
}

impl Display for OciNetworkProcessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTenantPrefix {
                node_supernet,
                attempted,
            } => write!(
                formatter,
                "tenant prefix /{attempted} is not a child of node super-net {node_supernet}"
            ),
            Self::ProcessClaimPoisoned => {
                formatter.write_str("OCI network process claim lock is poisoned")
            }
            Self::DuplicateProcessComposition {
                active_authority_path,
                attempted_authority_path,
                active_supernet,
                attempted_supernet,
                active_tenant_prefix,
                attempted_tenant_prefix,
            } => write!(
                formatter,
                "OCI network process is already composed at {} with {active_supernet} -> \
                 /{active_tenant_prefix}; attempted {} with {attempted_supernet} -> \
                 /{attempted_tenant_prefix}",
                active_authority_path.display(),
                attempted_authority_path.display()
            ),
            Self::AuthorityRootMismatch(error) => Display::fmt(error, formatter),
            Self::InvalidNodeSupernet { attempted, source } => {
                write!(
                    formatter,
                    "invalid configured node super-net {attempted:?}: {source}"
                )
            }
            Self::SegmentAdapter(error) => {
                write!(
                    formatter,
                    "failed to compose the OCI segment adapter: {error}"
                )
            }
            Self::NodeSupernetMismatch { active, attempted } => write!(
                formatter,
                "configured node super-net {attempted} differs from active process super-net \
                 {active}"
            ),
            Self::TenantPrefixMismatch { active, attempted } => write!(
                formatter,
                "configured tenant prefix /{attempted} differs from active process prefix \
                 /{active}"
            ),
        }
    }
}

impl StdError for OciNetworkProcessError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::AuthorityRootMismatch(error) => Some(error),
            Self::InvalidNodeSupernet { source, .. } => Some(source),
            Self::SegmentAdapter(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_tenant_prefix(
    node_supernet: Cidr,
    tenant_prefix: u8,
) -> Result<(), OciNetworkProcessError> {
    if tenant_prefix < node_supernet.prefix() || tenant_prefix > 32 {
        return Err(OciNetworkProcessError::InvalidTenantPrefix {
            node_supernet,
            attempted: tenant_prefix,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
