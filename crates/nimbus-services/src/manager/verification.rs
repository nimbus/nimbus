use nimbus_core::Error;
use nimbus_tenant::{
    TenantImageAdmissionSource, TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest, TenantIsolationDecision,
};

use crate::SandboxServiceLaunch;

use super::SandboxServiceManager;

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

impl SandboxServiceManager {
    pub(super) fn admit_launch_image(
        &self,
        decision: &TenantIsolationDecision,
        launch: &SandboxServiceLaunch,
    ) -> Result<(), Error> {
        let source = match launch {
            SandboxServiceLaunch::Image(launch) => {
                TenantImageAdmissionSource::registry(launch.image_reference.as_str())
            }
            SandboxServiceLaunch::Build(launch) => {
                TenantImageAdmissionSource::local_build(launch.image_name.as_str())
            }
        };
        decision
            .image()
            .admit_image(source, self.image_verification_provider.as_ref())?;
        Ok(())
    }
}
