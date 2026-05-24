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

    pub(in crate::adapters::convex) fn runtime_bundle_provenance(
        &self,
    ) -> Option<&RuntimeBundleProvenanceConfig> {
        self.runtime_bundle_provenance.as_ref()
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
        self.runtime_lane.policy()
    }

    pub(in crate::adapters::convex) fn runtime_executor(&self) -> Arc<RuntimeExecutor> {
        self.runtime_lane
            .executor()
            .expect("default V8 runtime adapter must be linked")
    }

    fn runtime_lane_policy_for_function(&self, function_name: &str) -> Arc<RuntimePolicy> {
        self.selected_runtime_lane(function_name).policy()
    }

    pub(in crate::adapters::convex) fn runtime_lane_for_function(
        &self,
        function_name: &str,
    ) -> Result<(Arc<RuntimeExecutor>, Arc<RuntimePolicy>), Error> {
        let lane = self.selected_runtime_lane(function_name);
        let Some(executor) = lane.executor() else {
            return Err(Error::InvalidInput(format!(
                "runtime function {function_name} selected the Bun/JSC lane, but the Bun/JSC execution adapter is not linked"
            )));
        };
        Ok((executor, lane.policy()))
    }

    fn selected_runtime_lane(&self, function_name: &str) -> &ConvexRuntimeLane {
        match self
            .functions
            .get(function_name)
            .map(ConvexFunctionDefinition::runtime_selection)
        {
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target: RuntimeCompatibilityTarget::Node20,
                ..
            }) => &self.node20_runtime_lane,
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target: RuntimeCompatibilityTarget::Node22,
                ..
            }) => &self.node22_runtime_lane,
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target: RuntimeCompatibilityTarget::Node24,
                ..
            }) => &self.node24_runtime_lane,
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target: RuntimeCompatibilityTarget::BunJsc,
                ..
            }) => unreachable!("V8/BunJsc target manifests are rejected at registry load"),
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
                ..
            })
            | None => &self.runtime_lane,
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::BunJsc,
                ..
            }) => &self.bun_jsc_runtime_lane,
        }
    }

    pub fn runtime_metrics_snapshot(&self) -> nimbus_runtime::RuntimeMetricsSnapshot {
        self.runtime_lane.policy().metrics_snapshot()
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_lane.limits().clone()
    }

    pub fn runtime_limits_for_function(&self, function_name: &str) -> RuntimeLimits {
        self.runtime_lane_policy_for_function(function_name)
            .limits()
            .clone()
    }

    pub(crate) fn runtime_lane_diagnostics(&self) -> Vec<ConvexRuntimeLaneDiagnostics> {
        vec![
            self.runtime_lane.diagnostics("default", true),
            self.node20_runtime_lane.diagnostics("node20", false),
            self.node22_runtime_lane.diagnostics("node22", false),
            self.node24_runtime_lane.diagnostics("node24", false),
            self.bun_jsc_runtime_lane.diagnostics("bun_jsc", false),
        ]
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
