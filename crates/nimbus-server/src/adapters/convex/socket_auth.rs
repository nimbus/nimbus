use std::sync::Arc;

use nimbus_auth::ApplicationAuthError;
use nimbus_convex::{ConvexRegistry, ConvexSiloAuthAuthority};
use nimbus_core::InvocationAuth;
use nimbus_tenant::TenantIsolationContext;

/// Complete trust and execution context admitted for a Convex WebSocket.
///
/// Keeping these values together prevents the subscription layer from
/// independently rediscovering deployment, tenant, or authentication state.
pub(in crate::adapters::convex) struct ConvexSocketAdmission {
    pub(in crate::adapters::convex) registry: Arc<ConvexRegistry>,
    pub(in crate::adapters::convex) initial_auth: Option<InvocationAuth>,
    pub(in crate::adapters::convex) tenant_context: TenantIsolationContext,
    pub(in crate::adapters::convex) auth_authority: ConvexSocketAuthAuthority,
}

impl ConvexSocketAdmission {
    pub(in crate::adapters::convex) fn tenant_id(&self) -> &nimbus_core::TenantId {
        self.tenant_context.tenant_id()
    }
}

/// Authentication authority fixed when a Convex WebSocket is admitted.
///
/// Application sockets retain the verifier selected by their URL silo from
/// the active deployment snapshot. System sockets retain their operator-owned
/// registry behavior. Message handling cannot rediscover either authority from
/// mutable process-global state.
#[derive(Clone)]
pub(in crate::adapters::convex) enum ConvexSocketAuthAuthority {
    Application(ConvexSiloAuthAuthority),
    System(Arc<ConvexRegistry>),
}

impl ConvexSocketAuthAuthority {
    pub(in crate::adapters::convex) fn application(authority: ConvexSiloAuthAuthority) -> Self {
        Self::Application(authority)
    }

    pub(in crate::adapters::convex) fn system(registry: Arc<ConvexRegistry>) -> Self {
        Self::System(registry)
    }

    pub(in crate::adapters::convex) async fn verify_bearer_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, ApplicationAuthError> {
        match self {
            Self::Application(authority) => authority.verify_bearer_token(token).await,
            Self::System(registry) => registry.verify_socket_token(token).await,
        }
    }
}
