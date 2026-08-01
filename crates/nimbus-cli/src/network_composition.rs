//! CLI-owned staged composition of one local OS-node network authority.
//!
//! This module orders existing portable and effect-owner seams. It does not
//! implement sockets, provider effects, policy, service naming, or forwarding.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus::{Error, ServiceManager};
use nimbus_core::{Cidr, CidrError};
use nimbus_network::{
    LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch, LocalNetworkManager,
    LocalNetworkManagerBootstrap, LocalNetworkManagerError, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilityRegistryError,
    NetworkIngressProviderRegistration,
};
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_sandbox::backends::krun::{
    KrunSandboxBackend, KrunSandboxBackendConfig, KrunSandboxStateView,
};
use nimbus_sandbox::{OciNetworkProcess, OciNetworkProcessError};

use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::prepare_local_service_manager_for_selection_with_isolation_mode;

/// Exact source set admitted by the outer local-node composition.
///
/// A partial provider set is deliberately unrepresentable. Processes without
/// both roles freeze an empty registry rather than synthesizing a counterpart.
enum LocalCapabilitySources {
    Empty,
    Complete(Box<CompleteLocalCapabilitySources>),
}

struct CompleteLocalCapabilitySources {
    attachment: NetworkAttachmentProviderRegistration,
    ingress: NetworkIngressProviderRegistration,
}

impl LocalCapabilitySources {
    pub(crate) const fn empty() -> Self {
        Self::Empty
    }

    pub(crate) fn complete(
        attachment: NetworkAttachmentProviderRegistration,
        ingress: NetworkIngressProviderRegistration,
    ) -> Self {
        Self::Complete(Box::new(CompleteLocalCapabilitySources {
            attachment,
            ingress,
        }))
    }

    fn into_registry(self) -> Result<NetworkCapabilityRegistry, NetworkCapabilityRegistryError> {
        let bundles = match self {
            Self::Empty => Vec::new(),
            Self::Complete(sources) => vec![NetworkCapabilityBundle::new(
                sources.attachment,
                sources.ingress,
            )],
        };
        NetworkCapabilityRegistry::new(bundles)
    }
}

/// One claimed authority while source-owned capability evidence is assembled.
#[derive(Debug)]
pub(crate) struct StagedLocalNetworkComposition {
    bootstrap: LocalNetworkManagerBootstrap,
    authority: LocalNetworkAuthority,
    oci_process: Option<Arc<OciNetworkProcess>>,
}

impl StagedLocalNetworkComposition {
    /// Claim the typed OS-node authority before any dependent durable or
    /// provider work.
    pub(crate) fn claim(root: &LocalNodeNetworkRoot) -> Result<Self, LocalNetworkCompositionError> {
        let bootstrap = LocalNetworkManager::bootstrap(root.as_path())
            .map_err(LocalNetworkCompositionError::Manager)?;
        let authority = bootstrap.authority();
        Ok(Self {
            bootstrap,
            authority,
            oci_process: None,
        })
    }

    pub(crate) fn authority(&self) -> LocalNetworkAuthority {
        self.authority.clone()
    }

    /// Prepare the one OCI-family process composition from a real local krun
    /// configuration. Construction performs no provider effect.
    pub(crate) fn prepare_krun_process(
        &mut self,
        config: &KrunSandboxBackendConfig,
    ) -> Result<Arc<OciNetworkProcess>, LocalNetworkCompositionError> {
        if let Some(process) = self.oci_process.as_ref() {
            process
                .authenticate_backend_config(
                    &config.network_state_root,
                    &config.node_network_supernet,
                    config.node_tenant_subnet_prefix,
                )
                .map_err(LocalNetworkCompositionError::Oci)?;
            return Ok(Arc::clone(process));
        }
        self.authority
            .authenticate_state_root(&config.network_state_root)
            .map_err(|source| {
                LocalNetworkCompositionError::Oci(OciNetworkProcessError::AuthorityRootMismatch(
                    source,
                ))
            })?;
        let node_supernet = Cidr::parse(&config.node_network_supernet)
            .map_err(LocalNetworkCompositionError::InvalidNodeSupernet)?;
        let process = OciNetworkProcess::new(
            self.authority(),
            node_supernet,
            config.node_tenant_subnet_prefix,
        )
        .map_err(LocalNetworkCompositionError::Oci)?;
        self.oci_process = Some(Arc::clone(&process));
        Ok(process)
    }

    /// Consume the staged claim and freeze the exact complete source set.
    fn freeze(
        self,
        sources: LocalCapabilitySources,
    ) -> Result<FrozenLocalNetworkComposition, LocalNetworkCompositionError> {
        let registry = sources
            .into_registry()
            .map_err(LocalNetworkCompositionError::Registry)?;
        let manager = self.bootstrap.freeze(registry);
        Ok(FrozenLocalNetworkComposition {
            manager,
            _oci_process: self.oci_process,
        })
    }
}

/// Process-lifetime retention of the frozen manager and optional OCI process.
#[derive(Debug)]
pub(crate) struct FrozenLocalNetworkComposition {
    manager: Arc<LocalNetworkManager>,
    _oci_process: Option<Arc<OciNetworkProcess>>,
}

impl FrozenLocalNetworkComposition {
    pub(crate) fn manager(&self) -> Arc<LocalNetworkManager> {
        Arc::clone(&self.manager)
    }

    pub(crate) fn authority(&self) -> LocalNetworkAuthority {
        self.manager.authority()
    }

    #[cfg(test)]
    pub(crate) fn oci_process(&self) -> Option<Arc<OciNetworkProcess>> {
        self._oci_process.as_ref().map(Arc::clone)
    }
}

/// Frozen local-node composition prepared before any listener/provider effect.
///
/// Dev prepares this value before its prebound sibling sockets. Start either
/// receives that exact value or creates one itself, then verifies that its
/// finalized server ingress report still equals the source report admitted at
/// freeze time.
pub(crate) struct PreparedLocalNetworkComposition {
    frozen: FrozenLocalNetworkComposition,
    local_service_manager: Option<Arc<ServiceManager>>,
    local_krun_backend: Option<Arc<KrunSandboxBackend>>,
    local_krun_state_view: Option<KrunSandboxStateView>,
    compose_selection: Option<ResolvedComposeSelection>,
    control_data_dir: PathBuf,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
    admitted_ingress: Option<NetworkIngressProviderRegistration>,
}

impl PreparedLocalNetworkComposition {
    pub(crate) fn prepare(
        staged: StagedLocalNetworkComposition,
        compose_selection: Option<&ResolvedComposeSelection>,
        control_data_dir: &Path,
        tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
        ingress: NetworkIngressProviderRegistration,
    ) -> Result<Self, LocalNetworkCompositionError> {
        Self::prepare_with_optional_ingress(
            staged,
            compose_selection,
            control_data_dir,
            tenant_isolation_mode,
            Some(ingress),
        )
    }

    /// Prepare a standalone Compose composition.
    ///
    /// The attachment is retained for real execution but the registry freezes
    /// empty because no ingress source exists in this process.
    pub(crate) fn prepare_attachment_only(
        staged: StagedLocalNetworkComposition,
        compose_selection: &ResolvedComposeSelection,
        control_data_dir: &Path,
    ) -> Result<Self, LocalNetworkCompositionError> {
        Self::prepare_with_optional_ingress(
            staged,
            Some(compose_selection),
            control_data_dir,
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            None,
        )
    }

    fn prepare_with_optional_ingress(
        mut staged: StagedLocalNetworkComposition,
        compose_selection: Option<&ResolvedComposeSelection>,
        control_data_dir: &Path,
        tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
        ingress: Option<NetworkIngressProviderRegistration>,
    ) -> Result<Self, LocalNetworkCompositionError> {
        let prepared_local_service = compose_selection
            .map(|selection| {
                prepare_local_service_manager_for_selection_with_isolation_mode(
                    selection,
                    control_data_dir,
                    tenant_isolation_mode,
                    &mut staged,
                )
            })
            .transpose()
            .map_err(LocalNetworkCompositionError::Compose)?
            .flatten();
        let (
            local_service_manager,
            local_krun_backend,
            local_krun_state_view,
            sources,
            admitted_ingress,
        ) = match prepared_local_service {
            Some(prepared) => {
                let (sources, admitted_ingress) = match ingress {
                    Some(ingress) => (
                        LocalCapabilitySources::complete(prepared.attachment, ingress.clone()),
                        Some(ingress),
                    ),
                    None => (LocalCapabilitySources::empty(), None),
                };
                (
                    Some(Arc::new(prepared.manager)),
                    Some(prepared.backend),
                    Some(prepared.state_view),
                    sources,
                    admitted_ingress,
                )
            }
            None => (None, None, None, LocalCapabilitySources::empty(), None),
        };
        let frozen = staged.freeze(sources)?;
        Ok(Self {
            frozen,
            local_service_manager,
            local_krun_backend,
            local_krun_state_view,
            compose_selection: compose_selection.cloned(),
            control_data_dir: control_data_dir.to_path_buf(),
            tenant_isolation_mode,
            admitted_ingress,
        })
    }

    pub(crate) fn authority(&self) -> LocalNetworkAuthority {
        self.frozen.authority()
    }

    pub(crate) fn manager(&self) -> Arc<LocalNetworkManager> {
        self.frozen.manager()
    }

    pub(crate) fn authenticate_requested_root(
        &self,
        root: &LocalNodeNetworkRoot,
    ) -> Result<(), LocalNetworkCompositionError> {
        self.authority()
            .authenticate_state_root(root.as_path())
            .map_err(LocalNetworkCompositionError::PreparedAuthorityRootMismatch)
    }

    pub(crate) fn local_service_manager(&self) -> Option<Arc<ServiceManager>> {
        self.local_service_manager.as_ref().map(Arc::clone)
    }

    pub(crate) fn local_krun_backend(&self) -> Option<Arc<KrunSandboxBackend>> {
        self.local_krun_backend.as_ref().map(Arc::clone)
    }

    pub(crate) fn local_krun_state_view(&self) -> Option<KrunSandboxStateView> {
        self.local_krun_state_view.clone()
    }

    pub(crate) fn requires_forwarded_service_manager(&self) -> bool {
        self.compose_selection.is_some() && self.local_service_manager.is_none()
    }

    pub(crate) fn validate_start_context(
        &self,
        compose_selection: Option<&ResolvedComposeSelection>,
        control_data_dir: &Path,
        tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
        actual_ingress: &NetworkIngressProviderRegistration,
    ) -> Result<(), LocalNetworkCompositionError> {
        if self.compose_selection.as_ref() != compose_selection {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the resolved Compose selection changed after capability freeze".to_owned(),
            ));
        }
        if self.control_data_dir != control_data_dir {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the control-data root changed after capability freeze".to_owned(),
            ));
        }
        if self.tenant_isolation_mode != tenant_isolation_mode {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the tenant-isolation mode changed after capability freeze".to_owned(),
            ));
        }
        if self
            .admitted_ingress
            .as_ref()
            .is_some_and(|admitted| admitted != actual_ingress)
        {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the finalized server ingress report changed after capability freeze".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum LocalNetworkCompositionError {
    Manager(LocalNetworkManagerError),
    Compose(Error),
    InvalidNodeSupernet(CidrError),
    Oci(OciNetworkProcessError),
    PreparedAuthorityRootMismatch(LocalNetworkAuthorityRootMismatch),
    PreparedContextMismatch(String),
    Registry(NetworkCapabilityRegistryError),
}

impl Display for LocalNetworkCompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manager(error) => {
                write!(formatter, "failed to claim local network manager: {error}")
            }
            Self::Compose(error) => {
                write!(
                    formatter,
                    "failed to prepare local network sources: {error}"
                )
            }
            Self::InvalidNodeSupernet(error) => {
                write!(formatter, "invalid local OCI node super-net: {error}")
            }
            Self::Oci(error) => write!(
                formatter,
                "failed to prepare local OCI network process: {error}"
            ),
            Self::PreparedAuthorityRootMismatch(error) => write!(
                formatter,
                "prepared local network authority does not match the requested start root: {error}"
            ),
            Self::PreparedContextMismatch(reason) => {
                write!(
                    formatter,
                    "prepared local network context mismatch: {reason}"
                )
            }
            Self::Registry(error) => {
                write!(
                    formatter,
                    "failed to freeze local network capabilities: {error}"
                )
            }
        }
    }
}

impl StdError for LocalNetworkCompositionError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Manager(error) => Some(error),
            Self::Compose(error) => Some(error),
            Self::InvalidNodeSupernet(error) => Some(error),
            Self::Oci(error) => Some(error),
            Self::PreparedAuthorityRootMismatch(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::PreparedContextMismatch(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serial_test::serial;

    use super::{
        LocalCapabilitySources, LocalNetworkCompositionError, PreparedLocalNetworkComposition,
        StagedLocalNetworkComposition,
    };
    use crate::compose::discovery::ResolvedComposeSelection;
    use nimbus_network::{
        LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
        NetworkAttachmentMode, NetworkAttachmentProviderRegistration, NetworkControlPlaneLocality,
        NetworkIsolationMode, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
        NetworkManagementMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    };
    #[cfg(target_os = "linux")]
    use nimbus_network::{
        NetworkBindRealmKind, NetworkCapabilityRequirements, NetworkEndpointCapabilitySet,
        NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
        NetworkIngressFeature, NetworkPortAssignmentMode, NetworkSovereigntyRequirements,
        PortProtocol,
    };
    use nimbus_operator::LocalNodeNetworkRoot;
    use nimbus_sandbox::backends::krun::KrunSandboxBackendConfig;

    fn fixture_attachment() -> NetworkAttachmentProviderRegistration {
        NetworkAttachmentProviderRegistration::new(
            NetworkProviderId::for_registration_key("nnc4.6d.fixture-attachment"),
            NetworkAttachmentCapabilitySet::new(
                NetworkManagementMode::NimbusHostManaged,
                [NetworkAttachmentMode::IsolatedNamespace],
                [
                    NetworkIsolationMode::WorkloadNamespace,
                    NetworkIsolationMode::TenantSegment,
                ],
            ),
            [NetworkAddressFamily::Ipv4],
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        )
    }

    #[cfg(target_os = "linux")]
    fn local_requirements() -> NetworkCapabilityRequirements {
        NetworkCapabilityRequirements::new(
            NetworkAttachmentCapabilitySet::new(
                NetworkManagementMode::NimbusHostManaged,
                [NetworkAttachmentMode::IsolatedNamespace],
                [
                    NetworkIsolationMode::WorkloadNamespace,
                    NetworkIsolationMode::TenantSegment,
                ],
            ),
            NetworkEndpointCapabilitySet::new(
                [NetworkAddressFamily::Ipv4],
                [NetworkBindRealmKind::Host],
                [NetworkExposure::Loopback],
                [PortProtocol::Tcp],
                [NetworkPortAssignmentMode::ProviderAssigned],
            ),
            NetworkIngressCapabilitySet::new([
                NetworkIngressFeature::PathRouting,
                NetworkIngressFeature::WebSocket,
                NetworkIngressFeature::Streaming,
            ]),
            NetworkForwardingCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
            NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        )
    }

    #[test]
    #[serial]
    fn empty_source_set_freezes_without_fabricated_capabilities_and_retains_claim() {
        let root_dir = tempfile::tempdir().expect("network root parent");
        let root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(root_dir.path()))
            .expect("absolute explicit root should resolve");
        let staged = StagedLocalNetworkComposition::claim(&root).expect("staged claim should open");
        let authority_path = staged.authority().authority_path().to_path_buf();

        let frozen = staged
            .freeze(LocalCapabilitySources::empty())
            .expect("empty source set should freeze honestly");
        assert_eq!(
            frozen.manager().capability_registry().selections().count(),
            0
        );
        assert_eq!(frozen.authority().authority_path(), authority_path);
        assert!(frozen.oci_process().is_none());

        let duplicate = LocalNetworkManager::bootstrap(root.as_path())
            .expect_err("frozen composition must retain the process claim");
        assert!(duplicate.to_string().contains("already initialized"));
        drop(frozen);

        let reopened = LocalNetworkManager::bootstrap(root.as_path())
            .expect("final drop should permit reopen");
        assert_eq!(reopened.authority().authority_path(), authority_path);
        drop(reopened);
    }

    #[test]
    #[serial]
    fn staged_claim_preserves_typed_duplicate_evidence_before_attempted_root_mutation() {
        let root = tempfile::tempdir().expect("fixture root");
        let active_path = root.path().join("active-node");
        let attempted_path = root.path().join("attempted-node");
        let active_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(&active_path))
            .expect("active node root should resolve");
        let attempted_root =
            LocalNodeNetworkRoot::resolve_for_current_platform(Some(&attempted_path))
                .expect("attempted node root should resolve");
        let active =
            StagedLocalNetworkComposition::claim(&active_root).expect("first claim should win");

        let error = StagedLocalNetworkComposition::claim(&attempted_root)
            .expect_err("a second node composition must fail before effects");
        match error {
            LocalNetworkCompositionError::Manager(
                nimbus_network::LocalNetworkManagerError::DuplicateProcessComposition {
                    active_authority_path,
                    attempted_authority_path,
                },
            ) => {
                assert_eq!(active_authority_path, active.authority().authority_path());
                assert_eq!(
                    attempted_authority_path,
                    nimbus_network::LocalNetworkStateStore::authority_path_for(&attempted_path)
                );
            }
            other => panic!("expected typed duplicate composition evidence, got {other}"),
        }
        assert!(
            !attempted_path.exists(),
            "the losing claim must not create or mutate its attempted root"
        );
    }

    #[test]
    #[serial]
    fn distinct_project_workload_roots_share_one_exact_oci_process_and_node_authority() {
        let node_dir = tempfile::tempdir().expect("node network root");
        let projects = tempfile::tempdir().expect("project roots");
        let node_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(node_dir.path()))
            .expect("absolute node root should resolve");
        let mut staged =
            StagedLocalNetworkComposition::claim(&node_root).expect("node claim should succeed");
        let first = KrunSandboxBackendConfig::under_root(projects.path().join("first"))
            .with_network_state_root(staged.authority().state_root());
        let second = KrunSandboxBackendConfig::under_root(projects.path().join("second"))
            .with_network_state_root(staged.authority().state_root());

        let first_process = staged
            .prepare_krun_process(&first)
            .expect("first project should prepare the process");
        let second_process = staged
            .prepare_krun_process(&second)
            .expect("second project should reuse the process");

        assert!(Arc::ptr_eq(&first_process, &second_process));
        assert_ne!(first.workload_state_root, second.workload_state_root);
        assert_eq!(first.network_state_root, second.network_state_root);
        assert_eq!(
            first.network_state_root,
            staged.authority().state_root(),
            "both project configs must retain the canonical node authority"
        );
        assert!(
            !first
                .workload_state_root
                .join("network-authority.json")
                .exists()
                && !second
                    .workload_state_root
                    .join("network-authority.json")
                    .exists(),
            "project workload roots must not receive node authority files"
        );
    }

    #[test]
    #[serial]
    fn incompatible_second_krun_config_is_refused_without_replacing_the_first_process() {
        let node_dir = tempfile::tempdir().expect("node network root");
        let workloads = tempfile::tempdir().expect("workload roots");
        let node_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(node_dir.path()))
            .expect("absolute node root should resolve");
        let mut staged =
            StagedLocalNetworkComposition::claim(&node_root).expect("node claim should succeed");
        let first = KrunSandboxBackendConfig::under_root(workloads.path().join("first"))
            .with_network_state_root(staged.authority().state_root());
        let first_process = staged
            .prepare_krun_process(&first)
            .expect("first configuration should install the process");

        let mut incompatible_supernet =
            KrunSandboxBackendConfig::under_root(workloads.path().join("incompatible"))
                .with_network_state_root(staged.authority().state_root());
        incompatible_supernet.node_network_supernet = "10.44.0.0/16".to_owned();
        let mut incompatible_prefix = first.clone();
        incompatible_prefix.node_tenant_subnet_prefix += 1;
        for incompatible in [&incompatible_supernet, &incompatible_prefix] {
            let error = staged
                .prepare_krun_process(incompatible)
                .expect_err("different CIDR geometry must not replace the installed process");
            assert!(matches!(error, LocalNetworkCompositionError::Oci(_)));
        }

        let replay = staged
            .prepare_krun_process(&first)
            .expect("the original configuration should remain reusable");
        assert!(
            Arc::ptr_eq(&first_process, &replay),
            "refusing drift must retain the exact first OCI process"
        );
        drop(first_process);
        drop(replay);
        drop(staged);
        drop(
            LocalNetworkManager::bootstrap(node_root.as_path())
                .expect("final process drop should permit deterministic manager reopen"),
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn alias_retarget_cannot_redirect_later_backend_construction() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("fixture root");
        let original = root.path().join("original-node");
        let foreign = root.path().join("foreign-node");
        std::fs::create_dir_all(&original).expect("original node root should exist");
        std::fs::create_dir_all(&foreign).expect("foreign node root should exist");
        let alias = root.path().join("node-alias");
        symlink(&original, &alias).expect("node alias should exist");
        let node_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(&alias))
            .expect("alias root should resolve");
        let mut staged =
            StagedLocalNetworkComposition::claim(&node_root).expect("alias claim should succeed");
        let config = KrunSandboxBackendConfig::under_root(root.path().join("workload"))
            .with_network_state_root(staged.authority().state_root());
        let process = staged
            .prepare_krun_process(&config)
            .expect("canonical process should prepare");

        std::fs::remove_file(&alias).expect("old alias should be removed");
        symlink(&foreign, &alias).expect("alias should retarget");
        let _backend = nimbus_sandbox::backends::krun::KrunSandboxBackend::with_network_process(
            config, process,
        )
        .expect("backend must retain the canonical authority after alias retarget");
        assert!(
            !nimbus_network::LocalNetworkStateStore::authority_path_for(&foreign).exists(),
            "retargeted foreign root must remain untouched"
        );
    }

    #[test]
    #[serial]
    fn server_only_and_forwarded_shapes_freeze_empty_without_fabricated_local_sources() {
        let root = tempfile::tempdir().expect("fixture root");
        let server_node = root.path().join("server-node");
        let server_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(&server_node))
            .expect("server node root should resolve");
        let server_only = PreparedLocalNetworkComposition::prepare(
            StagedLocalNetworkComposition::claim(&server_root)
                .expect("server-only claim should succeed"),
            None,
            &root.path().join("server-control"),
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            nimbus_server::nimbus_owned_local_ingress_registration(false),
        )
        .expect("server-only composition should freeze");
        let authority_existed_before = server_only.authority().authority_path().exists();
        let first_manager = server_only.manager();
        let second_manager = server_only.manager();
        assert!(Arc::ptr_eq(&first_manager, &second_manager));
        assert!(std::ptr::eq(
            first_manager.capability_registry(),
            server_only.frozen.manager().capability_registry()
        ));
        assert_eq!(first_manager.capability_registry().selections().count(), 0);
        assert_eq!(
            server_only.authority().authority_path().exists(),
            authority_existed_before,
            "repeated manager access must not materialize durable authority"
        );
        assert!(server_only.local_service_manager().is_none());
        assert!(!server_only.requires_forwarded_service_manager());
        drop(first_manager);
        drop(second_manager);
        drop(server_only);

        let compose_path = root.path().join("compose.yaml");
        std::fs::write(
            &compose_path,
            r#"
name: Forwarded
services:
  api:
    image: busybox:latest
    x_nimbus:
      backend: container
"#,
        )
        .expect("forwarded Compose fixture should write");
        let selection = ResolvedComposeSelection::explicit(compose_path);
        let forwarded_node = root.path().join("forwarded-node");
        let forwarded_root =
            LocalNodeNetworkRoot::resolve_for_current_platform(Some(&forwarded_node))
                .expect("forwarded node root should resolve");
        let forwarded = PreparedLocalNetworkComposition::prepare(
            StagedLocalNetworkComposition::claim(&forwarded_root)
                .expect("forwarded claim should succeed"),
            Some(&selection),
            &root.path().join("forwarded-control"),
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            nimbus_server::nimbus_owned_local_ingress_registration(false),
        )
        .expect("forwarded composition should freeze before machine effects");
        assert_eq!(
            forwarded
                .frozen
                .manager()
                .capability_registry()
                .selections()
                .count(),
            0
        );
        assert!(forwarded.local_service_manager().is_none());
        assert!(forwarded.requires_forwarded_service_manager());
    }

    #[test]
    #[serial]
    fn finalized_ingress_drift_is_rejected_and_final_drop_permits_reopen() {
        let root_dir = tempfile::tempdir().expect("network root parent");
        let root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(root_dir.path()))
            .expect("absolute explicit root should resolve");
        let ingress = nimbus_server::nimbus_owned_local_ingress_registration(false);
        let frozen = StagedLocalNetworkComposition::claim(&root)
            .expect("staged claim should open")
            .freeze(LocalCapabilitySources::complete(
                fixture_attachment(),
                ingress.clone(),
            ))
            .expect("complete fixture evidence should freeze");
        let prepared = PreparedLocalNetworkComposition {
            frozen,
            local_service_manager: None,
            local_krun_backend: None,
            local_krun_state_view: None,
            compose_selection: None,
            control_data_dir: root_dir.path().join("control"),
            tenant_isolation_mode: nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            admitted_ingress: Some(ingress),
        };

        let error = prepared
            .validate_start_context(
                None,
                &root_dir.path().join("control"),
                nimbus_tenant::TenantIsolationMode::LocalDevelopment,
                &nimbus_server::nimbus_owned_local_ingress_registration(true),
            )
            .expect_err("TLS/report drift must fail before listener effects");
        assert!(
            error
                .to_string()
                .contains("finalized server ingress report changed")
        );
        LocalNetworkManager::bootstrap(root.as_path())
            .expect_err("the rejected but retained composition must still own the claim");
        drop(prepared);
        drop(
            LocalNetworkManager::bootstrap(root.as_path())
                .expect("final composition drop should permit deterministic reopen"),
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[serial]
    fn real_local_krun_and_server_reports_freeze_one_exact_selectable_bundle() {
        let root = tempfile::tempdir().expect("fixture root");
        let compose_path = root.path().join("compose.yaml");
        std::fs::write(
            &compose_path,
            r#"
name: Local
services:
  api:
    image: busybox:latest
    x_nimbus:
      backend: krun
"#,
        )
        .expect("local Compose fixture should write");
        let selection = ResolvedComposeSelection::explicit(compose_path);
        let node_path = root.path().join("node");
        let node_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(&node_path))
            .expect("node root should resolve");
        let ingress = nimbus_server::nimbus_owned_local_ingress_registration(false);
        let prepared = PreparedLocalNetworkComposition::prepare(
            StagedLocalNetworkComposition::claim(&node_root)
                .expect("local manager should claim before source collection"),
            Some(&selection),
            &root.path().join("control"),
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            ingress.clone(),
        )
        .expect("real local sources should freeze");
        let attachment = prepared
            .local_krun_backend()
            .expect("the concrete local backend must remain retained")
            .host_managed_attachment_registration()
            .expect("the retained Linux Execute backend should report");
        let expected_selection = nimbus_network::NetworkCapabilitySelection::new(
            attachment.provider_id().clone(),
            ingress.provider_id().clone(),
        );
        let selected = prepared
            .frozen
            .manager()
            .capability_registry()
            .select_exact(&expected_selection, &local_requirements())
            .expect("the exact real source pair should satisfy");
        assert_eq!(selected.attachment(), &attachment);
        assert_eq!(selected.ingress(), &ingress);
        assert_eq!(
            prepared
                .frozen
                .manager()
                .capability_registry()
                .selections()
                .count(),
            1
        );
        let authority_path = prepared.authority().authority_path().to_path_buf();
        let durable_before = std::fs::read(&authority_path).ok();
        LocalNetworkManager::bootstrap(node_root.as_path())
            .expect_err("attachment-bearing prepared composition must retain the process claim");
        drop(prepared);
        let reopened = LocalNetworkManager::bootstrap(node_root.as_path())
            .expect("final attachment-bearing composition drop should permit reopen");
        assert_eq!(
            std::fs::read(reopened.authority().authority_path()).ok(),
            durable_before,
            "manager reopen must not mutate durable network authority"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[serial]
    fn standalone_attachment_only_composition_retains_backend_but_freezes_empty() {
        let root = tempfile::tempdir().expect("fixture root");
        let compose_path = root.path().join("compose.yaml");
        std::fs::write(
            &compose_path,
            r#"
name: AttachmentOnly
services:
  api:
    image: busybox:latest
    x_nimbus:
      backend: krun
"#,
        )
        .expect("attachment-only Compose fixture should write");
        let selection = ResolvedComposeSelection::explicit(compose_path);
        let node_path = root.path().join("node");
        let node_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(&node_path))
            .expect("node root should resolve");
        let prepared = PreparedLocalNetworkComposition::prepare_attachment_only(
            StagedLocalNetworkComposition::claim(&node_root)
                .expect("standalone manager should claim"),
            &selection,
            &root.path().join("control"),
        )
        .expect("standalone local attachment should prepare");

        assert!(prepared.local_krun_backend().is_some());
        assert_eq!(
            prepared
                .frozen
                .manager()
                .capability_registry()
                .selections()
                .count(),
            0,
            "attachment-only composition must not fabricate ingress"
        );
    }
}
