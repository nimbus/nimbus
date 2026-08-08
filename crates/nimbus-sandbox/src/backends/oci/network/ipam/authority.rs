//! Process-derived access to the durable OCI IPAM partition.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAuthority, LocalNetworkStateStore, NetworkStatePartition,
    NetworkStateTransactionError,
};

use crate::error::{Result, SandboxError};

use super::super::dto::IpamState;
use super::super::layout::OciNetworkLayout;

/// Narrow OCI adapter over the process-owned network authority.
///
/// The persisted [`OciNetworkLayout`] remains lifecycle evidence, but never
/// selects the durable authority. Every state access first authenticates that
/// evidence against the injected process handle.
#[derive(Clone, Debug)]
pub(crate) struct OciIpamAuthority {
    source: OciIpamAuthoritySource,
    state_store: std::result::Result<LocalNetworkStateStore, Arc<str>>,
}

#[derive(Clone, Debug)]
enum OciIpamAuthoritySource {
    Process(LocalNetworkAuthority),
    #[cfg(test)]
    DirectTest {
        canonical_state_root: PathBuf,
    },
    Direct {
        attempted_state_root: PathBuf,
        boundary: &'static str,
    },
}

impl OciIpamAuthority {
    /// Derive the OCI IPAM adapter from the one process-owned authority.
    pub(crate) fn from_process(authority: &LocalNetworkAuthority) -> Self {
        Self {
            source: OciIpamAuthoritySource::Process(authority.clone()),
            state_store: Ok(authority.state_store()),
        }
    }

    /// Reconstruct once at an explicitly selected direct-adapter boundary.
    pub(crate) fn reconstruct_direct(state_root: impl AsRef<Path>) -> Self {
        Self::reconstruct("direct adapter", state_root.as_ref())
    }

    /// Reconstruct once in the separate container-runner OS process.
    pub(crate) fn reconstruct_for_runner(state_root: impl AsRef<Path>) -> Self {
        Self::reconstruct("container runner", state_root.as_ref())
    }

    fn reconstruct(boundary: &'static str, state_root: &Path) -> Self {
        Self {
            source: OciIpamAuthoritySource::Direct {
                attempted_state_root: state_root.to_path_buf(),
                boundary,
            },
            state_store: LocalNetworkStateStore::open(state_root).map_err(|error| {
                Arc::<str>::from(format!(
                    "failed to reconstruct OCI IPAM authority for {boundary} at {}: {error}",
                    state_root.display()
                ))
            }),
        }
    }

    /// Reconstruct a direct authority only inside this module's state-machine
    /// tests, where no process composition exists.
    #[cfg(test)]
    pub(crate) fn reconstruct_for_direct_test(layout: &OciNetworkLayout) -> Result<Self> {
        let state_store = LocalNetworkStateStore::open(&layout.network_state_root)
            .map_err(super::ipam_store_error)?;
        let canonical_state_root =
            std::fs::canonicalize(state_store.state_root()).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "OCI IPAM direct-test authority could not authenticate its retained root \
                         {}: {error}",
                        state_store.state_root().display()
                    ),
                }
            })?;
        Ok(Self {
            source: OciIpamAuthoritySource::DirectTest {
                canonical_state_root,
            },
            state_store: Ok(state_store),
        })
    }

    /// Authenticate persisted layout evidence without selecting or opening an
    /// authority.
    pub(crate) fn authenticate_layout(&self, layout: &OciNetworkLayout) -> Result<()> {
        match &self.source {
            OciIpamAuthoritySource::Process(authority) => authority
                .authenticate_state_root(&layout.network_state_root)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "OCI IPAM rejected network layout authority before state access: {error}"
                    ),
                }),
            #[cfg(test)]
            OciIpamAuthoritySource::DirectTest {
                canonical_state_root,
            } => {
                let attempted = std::fs::canonicalize(&layout.network_state_root).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "OCI IPAM direct-test authority could not authenticate layout root {}: {error}",
                            layout.network_state_root.display()
                        ),
                    }
                })?;
                if &attempted == canonical_state_root {
                    Ok(())
                } else {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "OCI IPAM direct-test authority rejected layout root {}; active root is {}",
                            attempted.display(),
                            canonical_state_root.display()
                        ),
                    })
                }
            }
            OciIpamAuthoritySource::Direct {
                attempted_state_root,
                boundary,
            } => {
                let state_store = self.state_store()?;
                let active = std::fs::canonicalize(state_store.state_root()).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "OCI IPAM {boundary} authority could not authenticate its retained \
                             root {}: {error}",
                            state_store.state_root().display()
                        ),
                    }
                })?;
                let attempted = std::fs::canonicalize(&layout.network_state_root).map_err(
                    |error| SandboxError::OperationFailed {
                        message: format!(
                            "OCI IPAM {boundary} authority could not authenticate layout root {}: \
                             {error}",
                            layout.network_state_root.display()
                        ),
                    },
                )?;
                if attempted == active {
                    Ok(())
                } else {
                    Err(SandboxError::OperationFailed {
                        message: format!(
                            "OCI IPAM {boundary} authority rejected layout root {}; configured \
                             root is {} and active root is {}",
                            attempted.display(),
                            attempted_state_root.display(),
                            active.display()
                        ),
                    })
                }
            }
        }
    }

    /// Authenticate and transact against this layout's tenant IPAM partition.
    pub(super) fn transaction<T>(
        &self,
        layout: &OciNetworkLayout,
        mutator: impl FnOnce(&mut IpamState) -> Result<T>,
    ) -> Result<T> {
        self.authenticate_layout(layout)?;
        match self.state_store()?.transaction(
            &NetworkStatePartition::TenantIpam(layout.tenant_id.clone()),
            mutator,
        ) {
            Ok(result) => Ok(result),
            Err(NetworkStateTransactionError::Operation(error)) => Err(error),
            Err(NetworkStateTransactionError::Store(error)) => Err(super::ipam_store_error(error)),
        }
    }

    /// Authenticate and read this layout's tenant IPAM partition.
    pub(super) fn read(&self, layout: &OciNetworkLayout) -> Result<IpamState> {
        self.authenticate_layout(layout)?;
        self.state_store()?
            .read(&NetworkStatePartition::TenantIpam(layout.tenant_id.clone()))
            .map_err(super::ipam_store_error)
            .map(Option::unwrap_or_default)
    }

    /// List every typed tenant-IPAM partition under the one retained store.
    pub(super) fn tenant_ipam_tenants(&self) -> Result<Vec<TenantId>> {
        self.state_store()?
            .tenant_ipam_tenants()
            .map_err(super::ipam_store_error)
    }

    /// Read one enumerated tenant partition without reconstructing a layout
    /// from provider artifacts.
    pub(super) fn read_tenant(&self, tenant_id: &TenantId) -> Result<IpamState> {
        self.state_store()?
            .read(&NetworkStatePartition::TenantIpam(tenant_id.clone()))
            .map_err(super::ipam_store_error)
            .map(Option::unwrap_or_default)
    }

    /// Canonical root selected by the injected process authority.
    pub(crate) fn state_root(&self) -> &Path {
        match &self.state_store {
            Ok(store) => store.state_root(),
            Err(_) => match &self.source {
                OciIpamAuthoritySource::Direct {
                    attempted_state_root,
                    ..
                } => attempted_state_root,
                OciIpamAuthoritySource::Process(authority) => authority.state_root(),
                #[cfg(test)]
                OciIpamAuthoritySource::DirectTest {
                    canonical_state_root,
                } => canonical_state_root,
            },
        }
    }

    fn state_store(&self) -> Result<&LocalNetworkStateStore> {
        self.state_store
            .as_ref()
            .map_err(|reason| SandboxError::OperationFailed {
                message: reason.to_string(),
            })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use nimbus_core::TenantId;
    use tempfile::tempdir;

    use super::*;
    use crate::instance::SandboxId;

    #[test]
    fn direct_authority_accepts_a_canonical_alias_for_its_retained_root() {
        let parent = tempdir().expect("temporary parent should create");
        let canonical_root = parent.path().join("network-state");
        fs::create_dir_all(&canonical_root).expect("canonical network root should create");
        let alias_root = parent.path().join("network-state-alias");
        symlink(&canonical_root, &alias_root).expect("network root alias should create");

        let authority = OciIpamAuthority::reconstruct_direct(&alias_root);
        let tenant = TenantId::new("tenant-canonical-alias").expect("tenant should parse");
        let sandbox = SandboxId::new("sandbox-canonical-alias");
        let layout = OciNetworkLayout::with_roots(
            parent.path().join("workloads"),
            &canonical_root,
            &tenant,
            &sandbox,
        );

        authority
            .authenticate_layout(&layout)
            .expect("canonical aliases must authenticate as one retained authority");
    }

    #[test]
    fn terminal_reconciliation_accepts_a_persisted_canonical_network_root_alias() {
        let parent = tempdir().expect("temporary parent should create");
        let workload_root = parent.path().join("workloads");
        let canonical_root = parent.path().join("network-state");
        fs::create_dir_all(&workload_root).expect("workload root should create");
        fs::create_dir_all(&canonical_root).expect("canonical network root should create");
        let alias_root = parent.path().join("network-state-alias");
        symlink(&canonical_root, &alias_root).expect("network root alias should create");

        let tenant = TenantId::new("tenant-reconciliation-alias").expect("tenant should parse");
        let sandbox = SandboxId::new("sandbox-reconciliation-alias");
        let canonical_layout =
            OciNetworkLayout::with_roots(&workload_root, &canonical_root, &tenant, &sandbox);
        let aliased_layout =
            OciNetworkLayout::with_roots(&workload_root, &alias_root, &tenant, &sandbox);
        let authority = OciIpamAuthority::reconstruct_for_direct_test(&canonical_layout)
            .expect("canonical authority should open");
        let config = crate::backends::oci::network::OciNetworkConfig::default();

        super::super::allocate_container_ips(&authority, &aliased_layout, &config, &sandbox)
            .expect("the canonical alias should reserve IPAM");
        super::super::deallocate_container_ips_after_confirmed_detach(
            &authority,
            &aliased_layout,
            &sandbox,
            &config.attachment_id,
            &config.reservation_claim,
            config.provider_kind(),
        )
        .expect("the canonical alias should publish terminal IPAM evidence");

        let manifest_path = crate::artifact_paths::manifest_path(&workload_root, &tenant, &sandbox);
        fs::create_dir_all(
            manifest_path
                .parent()
                .expect("manifest parent should exist"),
        )
        .expect("manifest parent should create");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "handle": {"id": &sandbox, "tenant_id": &tenant},
                "spec": {"tenant_id": &tenant},
                "network_layout": &aliased_layout,
                "network_config": &config,
                "network_cleanup_complete": true,
                "launch_artifact": null,
                "launch_reservation_claim": null,
                "status": "failed"
            }))
            .expect("manifest should render"),
        )
        .expect("manifest should write");

        assert_eq!(
            super::super::reconcile_terminal_container_ipam_releases(&authority, &workload_root,)
                .expect(
                    "a persisted canonical alias must reconcile through the retained authority"
                ),
            1
        );
        assert!(
            super::super::read_ipam_state(&authority, &canonical_layout)
                .expect("canonical authority should inspect")
                .released_allocations
                .is_empty(),
            "successful reconciliation must retire the exact terminal witness"
        );
        assert!(
            !LocalNetworkStateStore::authority_path_for(&workload_root).exists(),
            "alias reconciliation must not redirect authority beneath the workload root"
        );
    }
}
