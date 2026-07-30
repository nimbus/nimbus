use super::*;

impl OciAttachmentAdapter<'_> {
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
