use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn runtime_bundle(&self) -> Option<&RuntimeBundle> {
        self.runtime_bundle.as_ref()
    }

    pub(in crate::adapters::convex) async fn verify_authorization_header(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<InvocationAuth>, AppError> {
        self.auth_verifier
            .verify_authorization_header(headers)
            .await
    }

    pub(in crate::adapters::convex) async fn verify_socket_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, AppError> {
        self.auth_verifier.verify_socket_token(token).await
    }

    pub(in crate::adapters::convex) fn runtime_policy(&self) -> Arc<RuntimePolicy> {
        self.runtime_policy.clone()
    }

    pub(in crate::adapters::convex) fn runtime_executor(&self) -> Arc<RuntimeExecutor> {
        self.runtime_executor.clone()
    }

    pub fn runtime_metrics_snapshot(&self) -> neovex_runtime::RuntimeMetricsSnapshot {
        self.runtime_policy.metrics_snapshot()
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_policy.limits().clone()
    }

    pub(in crate::adapters::convex) fn runtime_subscription_kind(
        &self,
        name: &str,
        required_visibility: ConvexFunctionVisibility,
    ) -> Option<ConvexFunctionKind> {
        let definition = self.functions.get(name)?;
        if self.runtime_bundle.is_none()
            || definition.visibility != required_visibility
            || definition.runtime_handler.is_none()
            || !definition.plan.is_null()
        {
            return None;
        }
        match definition.kind {
            ConvexFunctionKind::Query | ConvexFunctionKind::PaginatedQuery => Some(definition.kind),
            ConvexFunctionKind::Mutation | ConvexFunctionKind::Action => None,
        }
    }
}
