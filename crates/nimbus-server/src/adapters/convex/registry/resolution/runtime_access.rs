use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn runtime_bundle(&self) -> Option<&RuntimeBundle> {
        self.runtime_bundle.as_ref()
    }

    pub(in crate::adapters::convex) fn required_runtime_bundle(
        &self,
    ) -> Result<RuntimeBundle, Error> {
        self.runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))
    }

    pub(crate) async fn verify_bearer_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, AppError> {
        self.auth_verifier.verify_bearer_token(token).await
    }

    pub(in crate::adapters::convex) async fn verify_socket_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, AppError> {
        self.verify_bearer_token(token).await
    }

    pub(in crate::adapters::convex) fn runtime_policy(&self) -> Arc<RuntimePolicy> {
        self.runtime_policy.clone()
    }

    pub(in crate::adapters::convex) fn runtime_executor(&self) -> Arc<RuntimeExecutor> {
        self.runtime_executor.clone()
    }

    pub(in crate::adapters::convex) fn runtime_lane_for_function(
        &self,
        function_name: &str,
    ) -> (Arc<RuntimeExecutor>, Arc<RuntimePolicy>) {
        match self
            .functions
            .get(function_name)
            .and_then(ConvexFunctionDefinition::runtime_compatibility_target)
        {
            Some(RuntimeCompatibilityTarget::Node20) => (
                self.node20_runtime_executor.clone(),
                self.node20_runtime_policy.clone(),
            ),
            Some(RuntimeCompatibilityTarget::Node22) => (
                self.node22_runtime_executor.clone(),
                self.node22_runtime_policy.clone(),
            ),
            Some(RuntimeCompatibilityTarget::Node24) => (
                self.node24_runtime_executor.clone(),
                self.node24_runtime_policy.clone(),
            ),
            Some(RuntimeCompatibilityTarget::WebStandardIsolate) | None => {
                (self.runtime_executor(), self.runtime_policy())
            }
        }
    }

    pub fn runtime_metrics_snapshot(&self) -> nimbus_runtime::RuntimeMetricsSnapshot {
        self.runtime_policy.metrics_snapshot()
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_policy.limits().clone()
    }

    pub fn runtime_limits_for_function(&self, function_name: &str) -> RuntimeLimits {
        let (_, policy) = self.runtime_lane_for_function(function_name);
        policy.limits().clone()
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
