use nimbus_core::Error;
use nimbus_tenant::{
    TenantImageAdmissionSource, TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest, TenantIsolationDecision,
};

use crate::SandboxBackedServiceImplementation;

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
    pub(super) fn admit_launch_image(
        &self,
        decision: &TenantIsolationDecision,
        launch: &SandboxBackedServiceImplementation,
    ) -> Result<(), Error> {
        let source = match launch {
            SandboxBackedServiceImplementation::Image(launch) => {
                TenantImageAdmissionSource::registry(launch.image_reference.as_str())
            }
            SandboxBackedServiceImplementation::Build(launch) => {
                TenantImageAdmissionSource::local_build(launch.image_name.as_str())
            }
        };
        decision
            .image()
            .admit_image(source, self.image_verification_provider.as_ref())?;
        Ok(())
    }
}
