use super::*;
use crate::backends::oci::network::netavark::{
    PreparedNetavarkSetup, PreparedNetavarkTeardown,
    execute_prepared_container_network_setup_for_test,
    execute_prepared_container_network_teardown_for_test, prepare_container_network_setup,
    prepare_container_network_teardown,
};

struct DeterministicAttachmentHostEffects;

impl AttachmentHostEffects for DeterministicAttachmentHostEffects {
    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        if let Some(parent) = context.layout.netns_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create deterministic namespace parent {}: {error}",
                    parent.display()
                ),
            })?;
        }
        std::fs::write(&context.layout.netns_path, b"deterministic test namespace").map_err(
            |error| SandboxError::OperationFailed {
                message: format!(
                    "failed to write deterministic namespace {}: {error}",
                    context.layout.netns_path.display()
                ),
            },
        )
    }

    fn prepare_provider_setup(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup> {
        prepare_container_network_setup(ipam, &context.operation())
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<Ipv4Addr>> {
        execute_prepared_container_network_setup_for_test(ipam, &context.operation(), prepared)
    }

    fn prepare_provider_teardown(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkTeardown> {
        prepare_container_network_teardown(ipam, &context.operation())
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkTeardown,
    ) -> Result<()> {
        execute_prepared_container_network_teardown_for_test(ipam, context.layout, prepared)
    }

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        match std::fs::remove_file(&context.layout.netns_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to remove deterministic namespace {}: {error}",
                    context.layout.netns_path.display()
                ),
            }),
        }
    }
}

impl OciAttachmentAdapter<'_> {
    /// Exercise the production attachment algorithm with deterministic
    /// namespace and Netavark effects on any test host.
    pub(crate) fn attach_with_test_host(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        authority: AttachmentAttachAuthority<'_>,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        lifecycle.attach_with(
            &self.context,
            authority,
            &DeterministicAttachmentHostEffects,
            &mut NoopAttachmentPhaseObserver,
            after_provider_setup,
        )
    }

    pub(super) fn detach_machine_forwarded_with<T>(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        before_provider_detach: impl FnOnce() -> Result<T>,
        after_provider_detach: impl FnOnce(T) -> Result<()>,
    ) -> AttachmentDetachResult {
        lifecycle.detach_machine_forwarded_with(
            &self.context,
            mode,
            host,
            before_provider_detach,
            after_provider_detach,
        )
    }

    pub(super) fn attach_with(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        authority: AttachmentAttachAuthority<'_>,
        host: &impl AttachmentHostEffects,
        observer: &mut impl AttachmentPhaseObserver,
        after_provider_setup: impl FnOnce(&[Ipv4Addr]) -> Result<()>,
    ) -> Result<Vec<Ipv4Addr>> {
        lifecycle.attach_with(
            &self.context,
            authority,
            host,
            observer,
            after_provider_setup,
        )
    }

    pub(super) fn detach_host_managed_with(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        mode: AttachmentTeardownMode,
        host: &impl AttachmentHostEffects,
        before_provider_detach: impl FnOnce(AttachmentAuxiliaryDisposition) -> Result<()>,
    ) -> AttachmentDetachResult {
        lifecycle.detach_host_managed_with(&self.context, mode, host, before_provider_detach)
    }

    pub(crate) fn complete_injected_setup(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        setup: Result<Vec<Ipv4Addr>>,
    ) -> Result<Vec<Ipv4Addr>> {
        lifecycle.complete_injected_setup(&self.context, setup)
    }

    pub(crate) fn compensate_injected_host_setup_failure(
        &self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        batch: OciPortBindLifetimeBatch,
        primary: SandboxError,
    ) -> SandboxError {
        lifecycle.compensate_injected_host_setup_failure(&self.context, batch, primary)
    }
}

impl OciAttachmentLifecycle<'_> {
    /// Route an injected provider result through the canonical compensation
    /// seam without manufacturing a live Netavark lifetime batch.
    fn complete_injected_setup(
        &self,
        context: &OciAttachmentContext<'_>,
        setup: Result<Vec<Ipv4Addr>>,
    ) -> Result<Vec<Ipv4Addr>> {
        setup.map_err(|primary| self.compensate_setup_failure(context, None, primary))
    }

    /// Preserve fault-fixture access while keeping the executable compensation
    /// algorithm in the lifecycle owner.
    fn compensate_injected_host_setup_failure(
        &self,
        context: &OciAttachmentContext<'_>,
        batch: OciPortBindLifetimeBatch,
        primary: SandboxError,
    ) -> SandboxError {
        self.compensate_setup_failure(context, Some(batch), primary)
    }

    fn compensate_setup_failure(
        &self,
        context: &OciAttachmentContext<'_>,
        batch: Option<OciPortBindLifetimeBatch>,
        primary: SandboxError,
    ) -> SandboxError {
        self.compensate_setup_failure_with(context, &RealAttachmentHostEffects, batch, primary)
    }
}
