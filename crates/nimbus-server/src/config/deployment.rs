use std::sync::Arc;

use nimbus_auth::ApplicationAuthVerifier;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::cloudflare::CloudflareConfig;
use crate::adapters::convex::{ConvexRegistry, ConvexTenancyConfig};
use crate::adapters::firebase::FirebaseConfig;

#[derive(Default)]
pub(crate) struct DeploymentConfig {
    pub(crate) convex_registry: Option<ConvexRegistry>,
    pub(crate) system_convex_registry: Option<ConvexRegistry>,
    pub(crate) application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub(crate) cloud_functions_registry: Option<CloudFunctionsRegistry>,
    pub(crate) cloudflare_config: Option<CloudflareConfig>,
    pub(crate) firebase_config: Option<FirebaseConfig>,
    pub(crate) convex_tenancy: Option<ConvexTenancyConfig>,
}

impl DeploymentConfig {
    pub(crate) fn has_system_convex_registry(&self) -> bool {
        self.system_convex_registry.is_some()
    }

    pub(crate) fn with_convex(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
        self
    }

    pub(crate) fn with_system_convex_registry(
        mut self,
        system_convex_registry: ConvexRegistry,
    ) -> Self {
        self.system_convex_registry = Some(system_convex_registry);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_application_auth_verifier(
        mut self,
        application_auth_verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Self {
        self.application_auth_verifier = Some(application_auth_verifier);
        self
    }

    pub(crate) fn with_cloud_functions(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.cloud_functions_registry = Some(cloud_functions_registry);
        self
    }

    pub(crate) fn with_cloudflare(mut self, cloudflare_config: CloudflareConfig) -> Self {
        self.cloudflare_config = Some(cloudflare_config);
        self
    }

    pub(crate) fn with_firebase(mut self, firebase_config: FirebaseConfig) -> Self {
        self.firebase_config = Some(firebase_config);
        self
    }

    pub(crate) fn with_convex_tenancy(mut self, convex_tenancy: ConvexTenancyConfig) -> Self {
        self.convex_tenancy = Some(convex_tenancy);
        self
    }
}
