use super::*;

impl ConvexRegistry {
    pub(in crate::adapters::convex) fn runtime_bundle(&self) -> Option<&RuntimeBundle> {
        self.runtime_bundle.as_ref()
    }

    pub(crate) fn has_runtime_bundle_for_function(&self, function_name: &str) -> bool {
        if matches!(
            self.selected_runtime_lane(function_name)
                .limits()
                .backend_kind,
            nimbus_runtime::RuntimeBackendKind::BunJsc
        ) {
            return self.bun_jsc_runtime_bundle.is_some();
        }
        self.runtime_bundle.is_some()
    }

    pub(in crate::adapters::convex) fn required_runtime_bundle(
        &self,
    ) -> Result<RuntimeBundle, Error> {
        self.runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))
    }

    pub(crate) fn required_runtime_bundle_for_function(
        &self,
        function_name: &str,
    ) -> Result<RuntimeBundle, Error> {
        if matches!(
            self.selected_runtime_lane(function_name)
                .limits()
                .backend_kind,
            nimbus_runtime::RuntimeBackendKind::BunJsc
        ) {
            return self.bun_jsc_runtime_bundle.clone().ok_or_else(|| {
                Error::Internal(
                    "convex Bun/JSC program bundle not loaded for Bun runtime function".to_string(),
                )
            });
        }
        self.required_runtime_bundle()
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

#[cfg(all(test, not(feature = "bun-jsc-linked-adapter")))]
mod tests {
    use super::*;
    use nimbus_runtime::RuntimeBundle;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn bun_jsc_function_fails_closed_when_adapter_is_not_linked() {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:bunProof",
                        "kind": "mutation",
                        "visibility": "public",
                        "runtime_environment": "bun",
                        "runtime_engine": "bun_jsc",
                        "runtime_bundle_content_kind": "javascript",
                        "runtime_javascript_evaluation_format": "program_wrapper",
                        "runtime_compatibility_target": "bun_jsc",
                        "runtime_package_resolution": "bun_self_contained",
                        "runtime_handler": "async () => ({ ok: true })",
                        "plan": null
                    }
                ]
            }))
            .expect("convex manifest json should serialize"),
        )
        .expect("convex manifest should write");
        fs::write(
            convex_dir.join("http_routes.json"),
            serde_json::to_vec_pretty(&json!({ "routes": [] }))
                .expect("convex http route manifest should serialize"),
        )
        .expect("convex http route manifest should write");
        let bun_bundle_path = convex_dir.join("bun_program_bundle.js");
        fs::write(
            &bun_bundle_path,
            "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: \"bun\" }; };",
        )
        .expect("Bun/JSC runtime program bundle should write");
        let bun_hash = RuntimeBundle::compute_sha256_for_path(&bun_bundle_path)
            .expect("Bun/JSC bundle hash should compute");
        fs::write(bun_bundle_path.with_extension("sha256"), bun_hash)
            .expect("Bun/JSC runtime program bundle hash should write");

        let registry = ConvexRegistry::from_app_dir(tempdir.path())
            .expect("convex registry should load Bun/JSC runtime metadata and program bundle");
        let error = registry
            .runtime_lane_for_function("messages:bunProof")
            .expect_err("default build should fail closed before starting a Bun/JSC executor");
        assert!(
            error
                .to_string()
                .contains("Bun/JSC execution adapter is not linked"),
            "unexpected Bun/JSC no-link error: {error}"
        );
    }
}
