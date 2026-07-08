use std::sync::Arc;

use nimbus_license::LicenseState;
use nimbus_operator::LocalServerSecurityState;

#[derive(Clone)]
pub struct ControlPlaneConfig {
    license_state: LicenseState,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
}

impl ControlPlaneConfig {
    pub fn router_options_default() -> Self {
        Self {
            license_state: LicenseState::community(),
            deploy_admin_token: None,
            local_server_security: None,
        }
    }

    pub fn build_default() -> Self {
        Self {
            license_state: LicenseState::community(),
            deploy_admin_token: std::env::var("NIMBUS_DEPLOY_TOKEN").ok(),
            local_server_security: None,
        }
    }

    pub fn overlay_router_options(&mut self, options: Self) {
        self.license_state = options.license_state;
        if options.deploy_admin_token.is_some() {
            self.deploy_admin_token = options.deploy_admin_token;
        }
        if options.local_server_security.is_some() {
            self.local_server_security = options.local_server_security;
        }
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.license_state = license_state;
        self
    }

    pub fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.deploy_admin_token = Some(token.into());
        self
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn without_deploy_admin_token(mut self) -> Self {
        self.deploy_admin_token = None;
        self
    }

    pub fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.local_server_security = Some(local_server_security);
        self
    }

    pub fn license_state(&self) -> &LicenseState {
        &self.license_state
    }

    pub fn deploy_admin_token(&self) -> Option<&str> {
        self.deploy_admin_token.as_deref()
    }

    pub fn local_server_security(&self) -> Option<Arc<LocalServerSecurityState>> {
        self.local_server_security.clone()
    }
}
