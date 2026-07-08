use std::sync::Arc;

use nimbus_auth::ApplicationAuthVerifier;
use nimbus_cloud_functions::CloudFunctionsRegistry;
use nimbus_convex::{ConvexRegistry, ConvexTenancyConfig};
use nimbus_firebase::FirebaseConfig;

use crate::cloudflare_config::CloudflareConfig;

#[derive(Default)]
pub struct DeploymentConfig {
    pub convex_registry: Option<ConvexRegistry>,
    pub system_convex_registry: Option<ConvexRegistry>,
    pub application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub cloud_functions_registry: Option<CloudFunctionsRegistry>,
    pub cloudflare_config: Option<CloudflareConfig>,
    pub firebase_config: Option<FirebaseConfig>,
    pub convex_tenancy: Option<ConvexTenancyConfig>,
}

impl DeploymentConfig {
    pub fn has_system_convex_registry(&self) -> bool {
        self.system_convex_registry.is_some()
    }

    pub fn with_convex(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
        self
    }

    pub fn with_system_convex_registry(mut self, system_convex_registry: ConvexRegistry) -> Self {
        self.system_convex_registry = Some(system_convex_registry);
        self
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn with_application_auth_verifier(
        mut self,
        application_auth_verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Self {
        self.application_auth_verifier = Some(application_auth_verifier);
        self
    }

    pub fn with_cloud_functions(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.cloud_functions_registry = Some(cloud_functions_registry);
        self
    }

    pub fn with_cloudflare(mut self, cloudflare_config: CloudflareConfig) -> Self {
        self.cloudflare_config = Some(cloudflare_config);
        self
    }

    pub fn with_firebase(mut self, firebase_config: FirebaseConfig) -> Self {
        self.firebase_config = Some(firebase_config);
        self
    }

    pub fn with_convex_tenancy(mut self, convex_tenancy: ConvexTenancyConfig) -> Self {
        self.convex_tenancy = Some(convex_tenancy);
        self
    }
}
