//! CLI-owned staged composition of one local OS-node network authority.
//!
//! This module orders existing portable and effect-owner seams. It does not
//! implement sockets, provider effects, policy, service naming, or forwarding.

mod forwarded;

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus::{Engine, Error, ServiceManager};
use nimbus_compute::embedded_local_node_identity;
use nimbus_compute::workload_saga::{KrunProvisionAdapter, sandbox_execution_provider_id};
use nimbus_core::{Cidr, CidrError};
use nimbus_network::{
    LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch, LocalNetworkManager,
    LocalNetworkManagerBootstrap, LocalNetworkManagerError, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilityRegistryError,
    NetworkCapabilityRequirements, NetworkCapabilitySelection, NetworkCapabilitySelectionError,
    NetworkIngressProviderRegistration,
};
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_sandbox::backends::SandboxAttachmentRegistrationError;
use nimbus_sandbox::backends::krun::{
    KrunSandboxBackend, KrunSandboxBackendConfig, KrunSandboxStateView,
};
use nimbus_sandbox::{
    OciNetworkProcess, OciNetworkProcessError, ProviderProvisionJournalError, SandboxBackendKind,
    sandbox_network_plan_requirements,
};
use nimbus_server::{
    ServeOptions, ServerForegroundWorkloadRuntime, ServerIngressPublicationAdapter,
    ServerWorkloadComposition, ServerWorkloadCompositionError, ServerWorkloadProviders,
};
use nimbus_workloads::{NodeIdentity, WorkloadSagaStore};

use self::forwarded::PreparedForwardedServerWorkload;
#[cfg(test)]
pub(crate) use self::forwarded::prepare_forwarded_server_profile_for_test;
use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::{
    PreparedForwardedComposeProvisionSource,
    prepare_local_service_manager_for_selection_with_isolation_mode,
};

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

    pub(crate) fn freeze_bundle(
        self,
        bundle: NetworkCapabilityBundle,
    ) -> Result<FrozenLocalNetworkComposition, LocalNetworkCompositionError> {
        self.freeze(LocalCapabilitySources::complete(
            bundle.attachment().clone(),
            bundle.ingress().clone(),
        ))
    }
}

/// Process-lifetime retention of the frozen manager and optional OCI process.
#[derive(Debug, Clone)]
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
    local_krun_network_root: Option<PathBuf>,
    compose_selection: Option<ResolvedComposeSelection>,
    control_data_dir: PathBuf,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
    admitted_ingress: Option<NetworkIngressProviderRegistration>,
    forwarded: Option<PreparedForwardedComposeProvisionSource>,
}

/// Server profile prepared before provider journals or listener effects exist.
///
/// The protocol-only shape retains the exact frozen manager so its process
/// claim and empty capability report survive until serving starts. The local
/// shape retains every source that earned the selected provider evidence; it
/// can only be completed after those sources authenticate again.
pub(crate) enum PreparedServerWorkloadProfile {
    ProtocolOnly {
        network_manager: Arc<LocalNetworkManager>,
    },
    LocalKrun(Box<PreparedLocalKrunServerWorkload>),
    Forwarded(Box<PreparedForwardedServerWorkload>),
}

pub(crate) struct PreparedLocalKrunServerWorkload {
    network_manager: Arc<LocalNetworkManager>,
    service_manager: Arc<ServiceManager>,
    backend: Arc<KrunSandboxBackend>,
    network_root: PathBuf,
    attachment: NetworkAttachmentProviderRegistration,
    ingress: NetworkIngressProviderRegistration,
    selection: NetworkCapabilitySelection,
    requirements: NetworkCapabilityRequirements,
    local_node: NodeIdentity,
}

impl PreparedServerWorkloadProfile {
    /// Whether this profile owns the complete workload lifecycle composition.
    pub(crate) const fn is_managed(&self) -> bool {
        matches!(self, Self::LocalKrun(_) | Self::Forwarded(_))
    }

    /// Complete the prepared profile with the caller's exact Engine.
    ///
    /// Complete validation runs before adapters and performs no provider effect.
    pub(crate) fn complete(
        self,
        engine: Arc<Engine>,
    ) -> Result<ServeOptions, LocalNetworkCompositionError> {
        match self {
            Self::ProtocolOnly { network_manager } => {
                if network_manager
                    .capability_registry()
                    .selections()
                    .next()
                    .is_some()
                {
                    return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                        "a protocol-only server profile cannot expose workload provider reports"
                            .to_owned(),
                    ));
                }
                Ok(ServeOptions::protocol_only_with_authority(
                    engine,
                    network_manager.authority(),
                ))
            }
            Self::LocalKrun(prepared) => prepared.complete(engine),
            Self::Forwarded(prepared) => prepared.complete(engine),
        }
    }

    pub(crate) fn complete_foreground(
        self,
        engine: Arc<Engine>,
        saga_store: Arc<dyn WorkloadSagaStore>,
    ) -> Result<ServerForegroundWorkloadRuntime, LocalNetworkCompositionError> {
        match self {
            Self::ProtocolOnly { .. } => {
                Err(LocalNetworkCompositionError::ForegroundWorkloadSourcesUnavailable)
            }
            Self::LocalKrun(prepared) => Ok(prepared
                .into_workload_composition(engine)?
                .into_foreground_runtime(saga_store)),
            Self::Forwarded(prepared) => Ok(prepared
                .into_workload_composition(engine)?
                .into_foreground_runtime(saga_store)),
        }
    }
}

impl PreparedLocalKrunServerWorkload {
    fn validate(&self) -> Result<(), LocalNetworkCompositionError> {
        self.network_manager
            .authority()
            .authenticate_state_root(&self.network_root)
            .map_err(LocalNetworkCompositionError::PreparedAuthorityRootMismatch)?;

        let actual_attachment = self
            .backend
            .host_managed_attachment_registration()
            .map_err(LocalNetworkCompositionError::AttachmentRegistration)?;
        if actual_attachment != self.attachment {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the retained krun attachment report changed after capability freeze".to_owned(),
            ));
        }

        let actual_ingress = nimbus_server::nimbus_owned_workload_ingress_registration();
        if actual_ingress != self.ingress {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the retained server ingress report changed after capability freeze".to_owned(),
            ));
        }

        let expected_selection = NetworkCapabilitySelection::new(
            self.attachment.provider_id().clone(),
            self.ingress.provider_id().clone(),
        );
        if self.selection != expected_selection {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the prepared provider selection does not name the retained source reports"
                    .to_owned(),
            ));
        }

        let registry = self.network_manager.capability_registry();
        let selected = registry
            .select_exact(&self.selection, &self.requirements)
            .map_err(LocalNetworkCompositionError::CapabilitySelection)?;
        let selection_count = registry.selections().count();
        if selection_count != 1 {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                format!(
                    "a local krun server profile requires exactly one admitted provider bundle; found {selection_count}"
                ),
            ));
        }
        if selected.attachment() != &self.attachment || selected.ingress() != &self.ingress {
            return Err(LocalNetworkCompositionError::PreparedContextMismatch(
                "the exact registry selection does not equal the retained source reports"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn complete(self, engine: Arc<Engine>) -> Result<ServeOptions, LocalNetworkCompositionError> {
        Ok(ServeOptions::managed(
            self.into_workload_composition(engine)?,
        ))
    }

    fn into_workload_composition(
        self,
        engine: Arc<Engine>,
    ) -> Result<ServerWorkloadComposition, LocalNetworkCompositionError> {
        self.validate()?;

        let execution = Arc::new(
            KrunProvisionAdapter::new(Arc::clone(&self.backend))
                .map_err(LocalNetworkCompositionError::ProviderJournal)?,
        );
        let ingress = Arc::new(
            ServerIngressPublicationAdapter::new(
                Arc::clone(&self.backend),
                self.network_manager.authority(),
            )
            .map_err(LocalNetworkCompositionError::ProviderJournal)?,
        );
        let providers = ServerWorkloadProviders::new(
            self.selection.attachment_provider_id().clone(),
            Arc::clone(&execution),
            sandbox_execution_provider_id(SandboxBackendKind::Krun),
            execution,
            self.selection.ingress_provider_id().clone(),
            ingress,
        );
        ServerWorkloadComposition::new(
            engine,
            self.network_manager,
            self.service_manager,
            self.local_node,
            self.selection,
            self.requirements.sovereignty().clone(),
            providers,
        )
        .map_err(LocalNetworkCompositionError::ServerWorkload)
    }
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
    /// Local attachment-only sources freeze no provider report. A machine
    /// forwarded selection instead freezes its exact complete provider bundle
    /// while retaining activation until the caller supplies its `Engine`.
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
        let prepared_forwarded = if prepared_local_service.is_none() {
            forwarded::prepare_source(
                &staged,
                compose_selection,
                control_data_dir,
                tenant_isolation_mode,
            )?
        } else {
            None
        };
        let (
            local_service_manager,
            local_krun_backend,
            local_krun_state_view,
            local_krun_network_root,
            sources,
            admitted_ingress,
            forwarded,
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
                    Some(staged.authority().state_root().to_path_buf()),
                    sources,
                    admitted_ingress,
                    None,
                )
            }
            None => match prepared_forwarded {
                Some(prepared) => (
                    None,
                    None,
                    None,
                    None,
                    LocalCapabilitySources::complete(
                        prepared.source.bundle().attachment().clone(),
                        prepared.source.bundle().ingress().clone(),
                    ),
                    None,
                    Some(prepared),
                ),
                None => (
                    None,
                    None,
                    None,
                    None,
                    LocalCapabilitySources::empty(),
                    None,
                    None,
                ),
            },
        };
        let frozen = staged.freeze(sources)?;
        Ok(Self {
            frozen,
            local_service_manager,
            local_krun_backend,
            local_krun_state_view,
            local_krun_network_root,
            compose_selection: compose_selection.cloned(),
            control_data_dir: control_data_dir.to_path_buf(),
            tenant_isolation_mode,
            admitted_ingress,
            forwarded,
        })
    }

    pub(crate) fn authority(&self) -> LocalNetworkAuthority {
        self.frozen.authority()
    }

    #[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn requires_forwarded_service_manager(&self) -> bool {
        self.forwarded.is_some()
    }

    /// Prepare the exact server workload profile without opening provider
    /// journals or binding listeners.
    pub(crate) fn prepare_server_workload_profile(
        &self,
    ) -> Result<PreparedServerWorkloadProfile, LocalNetworkCompositionError> {
        let has_compose_selection = self.compose_selection.is_some();
        let has_service_manager = self.local_service_manager.is_some();
        let has_backend = self.local_krun_backend.is_some();
        let has_state_view = self.local_krun_state_view.is_some();
        let has_network_root = self.local_krun_network_root.is_some();
        let has_ingress = self.admitted_ingress.is_some();
        let has_forwarded = self.forwarded.is_some();
        let registry_is_empty = self
            .frozen
            .manager
            .capability_registry()
            .selections()
            .next()
            .is_none();

        if !has_compose_selection {
            if has_service_manager
                || has_backend
                || has_state_view
                || has_network_root
                || has_ingress
                || has_forwarded
                || !registry_is_empty
            {
                return Err(
                    LocalNetworkCompositionError::IncompleteServerWorkloadSources {
                        reason: "a protocol-only profile retained local workload sources or provider reports",
                    },
                );
            }
            return Ok(PreparedServerWorkloadProfile::ProtocolOnly {
                network_manager: self.frozen.manager(),
            });
        }

        if let Some(profile) = forwarded::prepare_server_workload_profile(self, registry_is_empty)?
        {
            return Ok(profile);
        }

        if !has_service_manager
            && !has_backend
            && !has_state_view
            && !has_network_root
            && !has_ingress
            && !has_forwarded
            && registry_is_empty
        {
            return Err(LocalNetworkCompositionError::ForwardedServerWorkloadUnavailable);
        }

        let (
            Some(service_manager),
            Some(backend),
            Some(_state_view),
            Some(network_root),
            Some(ingress),
        ) = (
            self.local_service_manager(),
            self.local_krun_backend(),
            self.local_krun_state_view(),
            self.local_krun_network_root.clone(),
            self.admitted_ingress.clone(),
        )
        else {
            return Err(
                LocalNetworkCompositionError::IncompleteServerWorkloadSources {
                    reason: "a local krun profile requires its service manager, backend, state view, network root, and ingress report",
                },
            );
        };

        let attachment = backend
            .host_managed_attachment_registration()
            .map_err(LocalNetworkCompositionError::AttachmentRegistration)?;
        let selection = NetworkCapabilitySelection::new(
            attachment.provider_id().clone(),
            ingress.provider_id().clone(),
        );
        let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .capability_requirements()
            .clone();
        let prepared = PreparedLocalKrunServerWorkload {
            network_manager: self.frozen.manager(),
            service_manager,
            backend,
            network_root,
            attachment,
            ingress,
            selection,
            requirements,
            local_node: embedded_local_node_identity(),
        };
        prepared.validate()?;
        Ok(PreparedServerWorkloadProfile::LocalKrun(Box::new(prepared)))
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
    AttachmentRegistration(SandboxAttachmentRegistrationError),
    CapabilitySelection(NetworkCapabilitySelectionError),
    ProviderJournal(ProviderProvisionJournalError),
    ServerWorkload(ServerWorkloadCompositionError),
    IncompleteServerWorkloadSources { reason: &'static str },
    ForwardedServerWorkloadUnavailable,
    ForegroundWorkloadSourcesUnavailable,
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
            Self::AttachmentRegistration(error) => {
                write!(
                    formatter,
                    "local attachment evidence is unavailable: {error}"
                )
            }
            Self::CapabilitySelection(error) => {
                write!(formatter, "local provider selection failed: {error}")
            }
            Self::ProviderJournal(error) => {
                write!(formatter, "local provider journal is unavailable: {error}")
            }
            Self::ServerWorkload(error) => {
                write!(
                    formatter,
                    "local server workload composition failed: {error}"
                )
            }
            Self::IncompleteServerWorkloadSources { reason } => {
                write!(
                    formatter,
                    "incomplete local server workload sources: {reason}"
                )
            }
            Self::ForwardedServerWorkloadUnavailable => formatter.write_str(
                "forwarded workload serving requires an explicit machine-owned provider profile",
            ),
            Self::ForegroundWorkloadSourcesUnavailable => formatter.write_str(
                "foreground workload lifecycle requires a complete managed provider profile",
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
            Self::AttachmentRegistration(error) => Some(error),
            Self::CapabilitySelection(error) => Some(error),
            Self::ProviderJournal(error) => Some(error),
            Self::ServerWorkload(error) => Some(error),
            Self::PreparedAuthorityRootMismatch(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::PreparedContextMismatch(_)
            | Self::IncompleteServerWorkloadSources { .. }
            | Self::ForwardedServerWorkloadUnavailable
            | Self::ForegroundWorkloadSourcesUnavailable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::collections::BTreeMap;
    #[cfg(target_os = "linux")]
    use std::path::Path;
    #[cfg(target_os = "linux")]
    use std::path::PathBuf;
    use std::sync::Arc;

    use serial_test::serial;

    use super::{
        LocalCapabilitySources, LocalNetworkCompositionError, PreparedLocalNetworkComposition,
        PreparedServerWorkloadProfile, StagedLocalNetworkComposition,
    };
    #[cfg(target_os = "linux")]
    use nimbus_engine::Engine;
    #[cfg(target_os = "linux")]
    use nimbus_network::NetworkCapabilitySelection;
    use nimbus_network::{
        LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
        NetworkAttachmentMode, NetworkAttachmentProviderRegistration, NetworkControlPlaneLocality,
        NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
        NetworkIngressProviderRegistration, NetworkIsolationMode, NetworkLifecycleCapabilitySet,
        NetworkLifecycleFeature, NetworkManagementMode, NetworkProviderId,
        NetworkSovereigntyCapabilities,
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

    fn fixture_ingress() -> NetworkIngressProviderRegistration {
        NetworkIngressProviderRegistration::new(
            NetworkProviderId::for_registration_key("nnc6.4.fixture-ingress"),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        )
    }

    #[cfg(target_os = "linux")]
    fn filesystem_snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn visit(root: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            let mut entries = std::fs::read_dir(current)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", current.display()))
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|error| {
                    panic!("failed to enumerate {}: {error}", current.display())
                });
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("snapshot entry should stay below its root")
                    .to_path_buf();
                if path.is_dir() {
                    snapshot.insert(relative, None);
                    visit(root, &path, snapshot);
                } else {
                    snapshot.insert(
                        relative,
                        Some(std::fs::read(&path).unwrap_or_else(|error| {
                            panic!("failed to read snapshot file {}: {error}", path.display())
                        })),
                    );
                }
            }
        }

        let mut snapshot = BTreeMap::new();
        if root.is_dir() {
            visit(root, root, &mut snapshot);
        }
        snapshot
    }

    #[cfg(target_os = "linux")]
    fn prepare_local_krun_server_fixture(root: &Path) -> PreparedLocalNetworkComposition {
        let compose_path = root.join("compose.yaml");
        std::fs::write(
            &compose_path,
            r#"
name: LocalProfile
services:
  api:
    image: busybox:latest
    x_nimbus:
      backend: krun
"#,
        )
        .expect("local Compose fixture should write");
        let selection = ResolvedComposeSelection::explicit(compose_path);
        let node_root =
            LocalNodeNetworkRoot::resolve_for_current_platform(Some(&root.join("node")))
                .expect("node root should resolve");
        PreparedLocalNetworkComposition::prepare(
            StagedLocalNetworkComposition::claim(&node_root)
                .expect("local manager should claim before source collection"),
            Some(&selection),
            &root.join("control"),
            nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            nimbus_server::nimbus_owned_workload_ingress_registration(),
        )
        .expect("real local sources should freeze")
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
    fn server_only_shape_freezes_empty_without_fabricated_workload_sources() {
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
            nimbus_server::nimbus_owned_workload_ingress_registration(),
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
        let profile = server_only
            .prepare_server_workload_profile()
            .expect("server-only composition should prepare the protocol profile");
        assert!(!profile.is_managed());
        match profile {
            PreparedServerWorkloadProfile::ProtocolOnly { network_manager } => {
                assert!(Arc::ptr_eq(&network_manager, &first_manager));
                assert_eq!(
                    network_manager.capability_registry().selections().count(),
                    0
                );
            }
            PreparedServerWorkloadProfile::LocalKrun(_) => {
                panic!("server-only composition must not fabricate local workload providers")
            }
            PreparedServerWorkloadProfile::Forwarded(_) => {
                panic!("server-only composition must not fabricate forwarded workload providers")
            }
        }
        drop(first_manager);
        drop(second_manager);
    }

    #[test]
    #[serial]
    fn finalized_ingress_drift_is_rejected_and_final_drop_permits_reopen() {
        let root_dir = tempfile::tempdir().expect("network root parent");
        let root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(root_dir.path()))
            .expect("absolute explicit root should resolve");
        let ingress = nimbus_server::nimbus_owned_workload_ingress_registration();
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
            local_krun_network_root: None,
            compose_selection: None,
            control_data_dir: root_dir.path().join("control"),
            tenant_isolation_mode: nimbus_tenant::TenantIsolationMode::LocalDevelopment,
            admitted_ingress: Some(ingress),
            forwarded: None,
        };

        let error = prepared
            .validate_start_context(
                None,
                &root_dir.path().join("control"),
                nimbus_tenant::TenantIsolationMode::LocalDevelopment,
                &fixture_ingress(),
            )
            .expect_err("source-report drift must fail before listener effects");
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
        let ingress = nimbus_server::nimbus_owned_workload_ingress_registration();
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
            .select_exact(
                &expected_selection,
                nimbus_sandbox::sandbox_network_plan_requirements(
                    nimbus_sandbox::SandboxBackendKind::Krun,
                )
                .capability_requirements(),
            )
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
        let expected_manager = prepared.manager();
        let expected_service_manager = prepared
            .local_service_manager()
            .expect("the exact local services owner should remain retained");
        let expected_backend = prepared
            .local_krun_backend()
            .expect("the exact local krun owner should remain retained");
        let authority_path = prepared.authority().authority_path().to_path_buf();
        let durable_before = std::fs::read(&authority_path).ok();
        let before_profile = filesystem_snapshot(root.path());
        let profile = prepared
            .prepare_server_workload_profile()
            .expect("complete local sources should prepare a server workload profile");
        assert!(profile.is_managed());
        match &profile {
            PreparedServerWorkloadProfile::LocalKrun(local) => {
                assert!(Arc::ptr_eq(&local.network_manager, &expected_manager));
                assert!(Arc::ptr_eq(
                    &local.service_manager,
                    &expected_service_manager
                ));
                assert!(Arc::ptr_eq(&local.backend, &expected_backend));
                assert_eq!(local.attachment, attachment);
                assert_eq!(local.ingress, ingress);
                assert_eq!(local.selection, expected_selection);
                assert_eq!(
                    local.local_node,
                    nimbus_compute::embedded_local_node_identity()
                );
            }
            PreparedServerWorkloadProfile::ProtocolOnly { .. } => {
                panic!("complete local sources must not collapse to protocol-only")
            }
            PreparedServerWorkloadProfile::Forwarded(_) => {
                panic!("local krun sources must not become forwarded providers")
            }
        }
        assert_eq!(filesystem_snapshot(root.path()), before_profile);

        let engine = Arc::new(
            Engine::new(root.path().join("engine")).expect("caller engine should initialize"),
        );
        let before_completion = filesystem_snapshot(root.path());
        let options = profile
            .complete(engine)
            .expect("validated local providers should complete without an effect");
        assert_eq!(filesystem_snapshot(root.path()), before_completion);
        LocalNetworkManager::bootstrap(node_root.as_path())
            .expect_err("attachment-bearing prepared composition must retain the process claim");
        drop(options);
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
    fn partial_local_server_profiles_fail_before_journal_or_listener_effects() {
        for missing_source in ["service-manager", "krun-backend"] {
            let root = tempfile::tempdir().expect("fixture root");
            let mut prepared = prepare_local_krun_server_fixture(root.path());
            match missing_source {
                "service-manager" => prepared.local_service_manager = None,
                "krun-backend" => prepared.local_krun_backend = None,
                other => panic!("unknown missing source fixture {other}"),
            }
            let before = filesystem_snapshot(root.path());

            let result = prepared.prepare_server_workload_profile();

            assert!(matches!(
                result,
                Err(LocalNetworkCompositionError::IncompleteServerWorkloadSources { .. })
            ));
            assert_eq!(filesystem_snapshot(root.path()), before);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[serial]
    fn crossed_local_server_root_fails_again_before_adapter_construction() {
        let root = tempfile::tempdir().expect("fixture root");
        let prepared = prepare_local_krun_server_fixture(root.path());
        let profile = prepared
            .prepare_server_workload_profile()
            .expect("complete local sources should prepare");
        let PreparedServerWorkloadProfile::LocalKrun(mut local) = profile else {
            panic!("local krun sources must prepare the managed profile")
        };
        local.network_root = root.path().join("crossed-network-root");
        let engine = Arc::new(
            Engine::new(root.path().join("engine")).expect("caller engine should initialize"),
        );
        let before = filesystem_snapshot(root.path());

        let result = PreparedServerWorkloadProfile::LocalKrun(local).complete(engine);

        assert!(matches!(
            result,
            Err(LocalNetworkCompositionError::PreparedAuthorityRootMismatch(
                _
            ))
        ));
        assert_eq!(filesystem_snapshot(root.path()), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[serial]
    fn crossed_local_provider_selection_fails_again_before_adapter_construction() {
        let root = tempfile::tempdir().expect("fixture root");
        let prepared = prepare_local_krun_server_fixture(root.path());
        let profile = prepared
            .prepare_server_workload_profile()
            .expect("complete local sources should prepare");
        let PreparedServerWorkloadProfile::LocalKrun(mut local) = profile else {
            panic!("local krun sources must prepare the managed profile")
        };
        local.selection = NetworkCapabilitySelection::new(
            local.attachment.provider_id().clone(),
            NetworkProviderId::for_registration_key("nnc6.4.crossed-ingress"),
        );
        let engine = Arc::new(
            Engine::new(root.path().join("engine")).expect("caller engine should initialize"),
        );
        let before = filesystem_snapshot(root.path());

        let result = PreparedServerWorkloadProfile::LocalKrun(local).complete(engine);

        assert!(matches!(
            result,
            Err(LocalNetworkCompositionError::PreparedContextMismatch(reason))
                if reason.contains("provider selection")
        ));
        assert_eq!(filesystem_snapshot(root.path()), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[serial]
    fn missing_exact_registry_selection_fails_before_journal_or_listener_effects() {
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
        let node_root =
            LocalNodeNetworkRoot::resolve_for_current_platform(Some(&root.path().join("node")))
                .expect("node root should resolve");
        let mut prepared = PreparedLocalNetworkComposition::prepare_attachment_only(
            StagedLocalNetworkComposition::claim(&node_root)
                .expect("standalone manager should claim"),
            &selection,
            &root.path().join("control"),
        )
        .expect("attachment-only composition should prepare");
        prepared.admitted_ingress =
            Some(nimbus_server::nimbus_owned_workload_ingress_registration());
        let before = filesystem_snapshot(root.path());

        let result = prepared.prepare_server_workload_profile();

        assert!(matches!(
            result,
            Err(LocalNetworkCompositionError::CapabilitySelection(
                nimbus_network::NetworkCapabilitySelectionError::UnregisteredComposition { .. }
            ))
        ));
        assert_eq!(filesystem_snapshot(root.path()), before);
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
