use nimbus_core::Error;
use nimbus_sandbox::{SandboxRootSpec, SandboxSpec};
use nimbus_tenant::{
    TenantImageAdmissionSource, TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest, TenantIsolationDecision,
};

use super::ServiceManager;

#[derive(Debug, Default)]
pub(super) struct DefaultTenantImageVerificationProvider;

impl TenantImageVerificationProvider for DefaultTenantImageVerificationProvider {
    fn verify_registry_image(
        &self,
        _request: &TenantImageVerificationRequest,
    ) -> nimbus_core::Result<TenantImageVerificationEvidence> {
        Ok(TenantImageVerificationEvidence::default())
    }
}

impl ServiceManager {
    pub(super) fn admit_sandbox_root(
        &self,
        decision: &TenantIsolationDecision,
        spec: &SandboxSpec,
    ) -> Result<(), Error> {
        let Some(source) = (match &spec.root {
            SandboxRootSpec::Rootfs(_) => None,
            SandboxRootSpec::OciImage(image) => match &image.source {
                nimbus_sandbox::SandboxOciImageSource::Reference(reference) => Some(
                    TenantImageAdmissionSource::registry(reference.reference.as_str()),
                ),
                nimbus_sandbox::SandboxOciImageSource::Build(build) => Some(
                    TenantImageAdmissionSource::local_build(build.image_name.as_str()),
                ),
            },
        }) else {
            return Ok(());
        };
        decision
            .image()
            .admit_image(source, self.image_verification_provider.as_ref())?;
        Ok(())
    }
}
