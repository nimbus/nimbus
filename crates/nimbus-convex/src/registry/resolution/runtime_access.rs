use super::*;

impl ConvexRegistry {
    pub fn runtime_bundle(&self) -> Option<&RuntimeBundle> {
        self.runtime_bundle.as_ref()
    }

    pub fn has_runtime_bundle_for_function(&self, function_name: &str) -> bool {
        let Some(definition) = self.functions.get(function_name) else {
            return false;
        };
        if definition.runtime_handler.is_none() || !definition.plan.is_null() {
            return false;
        }
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

    pub fn required_runtime_bundle(&self) -> Result<RuntimeBundle, Error> {
        self.runtime_bundle()
            .cloned()
            .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))
    }

    pub fn required_runtime_bundle_for_function(
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

    pub fn runtime_bundle_provenance(&self) -> Option<&RuntimeBundleProvenanceConfig> {
        self.runtime_bundle_provenance.as_ref()
    }

    pub async fn verify_bearer_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, ApplicationAuthError> {
        self.auth_verifier.verify_bearer_token(token).await
    }

    pub async fn verify_socket_token(
        &self,
        token: &str,
    ) -> Result<InvocationAuth, ApplicationAuthError> {
        self.verify_bearer_token(token).await
    }

    /// Authoritative runtime lane (`"default" | "node" | "bun"`) for the nested
    /// `ctx.run*` dispatcher's local-vs-host decision, in the same vocabulary the
    /// isolate freezes into `globalThis.__nimbusRuntimeEnvironmentLane`. Returns
    /// `None` for callees the local bundle cannot invoke — unknown functions and
    /// plan-backed/non-runtime functions — so those resolve to host dispatch,
    /// which owns their execution. This is the single source of truth for the
    /// decision: no guest-reachable JavaScript state participates.
    pub fn runtime_environment_for_function(&self, function_name: &str) -> Option<&'static str> {
        let definition = self.functions.get(function_name)?;
        if definition.runtime_handler.is_none() || !definition.plan.is_null() {
            return None;
        }
        Some(
            match self
                .selected_runtime_lane(function_name)
                .limits()
                .compatibility_target
            {
                RuntimeCompatibilityTarget::Node20
                | RuntimeCompatibilityTarget::Node22
                | RuntimeCompatibilityTarget::Node24
                | RuntimeCompatibilityTarget::Node26 => "node",
                RuntimeCompatibilityTarget::BunJsc => "bun",
                RuntimeCompatibilityTarget::WebStandardIsolate
                | RuntimeCompatibilityTarget::WasmComponent => "default",
            },
        )
    }

    pub fn required_runtime_limits_for_function(
        &self,
        function_name: &str,
    ) -> Result<RuntimeLimits, Error> {
        let lane = self.selected_runtime_lane(function_name);
        if lane.execution_adapter_state == RuntimeExecutionAdapterState::NotLinked {
            return Err(Error::InvalidInput(format!(
                "runtime function {function_name} selected the Bun/JSC lane, but the Bun/JSC execution adapter is not linked"
            )));
        }
        Ok(lane.limits().clone())
    }

    fn selected_runtime_lane(&self, function_name: &str) -> &RuntimeExecutionRequirements {
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
                compatibility_target: RuntimeCompatibilityTarget::Node26,
                ..
            }) => &self.node26_runtime_lane,
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::V8,
                compatibility_target:
                    RuntimeCompatibilityTarget::BunJsc | RuntimeCompatibilityTarget::WasmComponent,
                ..
            }) => unreachable!("V8 non-V8-target manifests are rejected at registry load"),
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
            Some(ConvexRuntimeSelection {
                engine: nimbus_runtime::RuntimeBackendKind::Wasmtime,
                ..
            }) => unreachable!("Wasmtime Convex manifests are rejected at registry load"),
        }
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_lane.limits().clone()
    }

    pub fn runtime_limits_for_function(&self, function_name: &str) -> RuntimeLimits {
        self.selected_runtime_lane(function_name).limits().clone()
    }

    pub fn runtime_lane_diagnostics(&self) -> Vec<RuntimeExecutionRequirementsDiagnostics> {
        vec![
            self.runtime_lane.diagnostics("default", true),
            self.node20_runtime_lane.diagnostics("node20", false),
            self.node22_runtime_lane.diagnostics("node22", false),
            self.node24_runtime_lane.diagnostics("node24", false),
            self.node26_runtime_lane.diagnostics("node26", false),
            self.bun_jsc_runtime_lane.diagnostics("bun_jsc", false),
        ]
    }

    pub fn runtime_subscription_kind(
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
    use nimbus_runtime::{
        RuntimeBundle, RuntimeCompatibilityTarget, RuntimeLimits, RuntimeNodeSupportPhase,
    };
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn convex_registry_routes_only_runtime_only_functions_to_runtime_bundle() {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:compiledPlan",
                        "kind": "query",
                        "visibility": "public",
                        "plan": {
                            "table": "messages",
                            "filters": [],
                            "order": null,
                            "limit": 20
                        },
                        "runtime_handler": null
                    },
                    {
                        "name": "messages:runtimeOnly",
                        "kind": "query",
                        "visibility": "public",
                        "plan": null,
                        "runtime_handler": "async () => []"
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
        let bundle_path = convex_dir.join("bundle.mjs");
        fs::write(
            &bundle_path,
            "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: [] }; }; export {};",
        )
        .expect("convex runtime bundle should write");
        let bundle_hash = RuntimeBundle::compute_sha256_for_path(&bundle_path)
            .expect("convex runtime bundle hash should compute");
        fs::write(bundle_path.with_extension("sha256"), bundle_hash)
            .expect("convex runtime bundle hash should write");

        let registry =
            ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");

        assert!(
            !registry.has_runtime_bundle_for_function("messages:compiledPlan"),
            "compiled plan functions should stay on the compiled operation path"
        );
        assert!(
            registry.has_runtime_bundle_for_function("messages:runtimeOnly"),
            "runtime-only functions should use the runtime bundle"
        );
    }

    #[test]
    fn convex_default_lane_carries_convex_guest_semantics_and_node_lanes_stay_host() {
        let registry = ConvexRegistry::empty();
        assert_eq!(
            registry.runtime_limits().guest_semantics,
            nimbus_runtime::RuntimeGuestSemantics::ConvexDefault,
            "the default Convex lane must opt into the Convex default-runtime guest semantics"
        );
        for (lane_label, lane) in [
            ("node20", &registry.node20_runtime_lane),
            ("node22", &registry.node22_runtime_lane),
            ("node24", &registry.node24_runtime_lane),
            ("node26", &registry.node26_runtime_lane),
        ] {
            assert_eq!(
                lane.limits().guest_semantics,
                nimbus_runtime::RuntimeGuestSemantics::Host,
                "{lane_label} lane must stay on Host semantics (the upstream Node runtime is \
                 exempt from the default-runtime determinism contract)"
            );
        }
        // A server-supplied V8 base-limits override must not strip the
        // semantics opt-in.
        let overridden = ConvexRegistry::empty().with_runtime_limits(RuntimeLimits::default());
        assert_eq!(
            overridden.runtime_limits().guest_semantics,
            nimbus_runtime::RuntimeGuestSemantics::ConvexDefault,
            "base-limits overrides must not lose the ConvexDefault opt-in"
        );
    }

    #[test]
    fn convex_node_runtime_lanes_follow_lts_registry_targets() {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:legacyNode20",
                        "kind": "action",
                        "runtime_environment": "node",
                        "node_runtime_target": "20",
                        "plan": null
                    },
                    {
                        "name": "messages:defaultNode",
                        "kind": "action",
                        "runtime_environment": "node",
                        "plan": null
                    },
                    {
                        "name": "messages:activeNode24",
                        "kind": "action",
                        "runtime_environment": "node",
                        "runtime_compatibility_target": "24",
                        "plan": null
                    },
                    {
                        "name": "messages:currentNode26",
                        "kind": "action",
                        "runtime_environment": "node",
                        "runtime_compatibility_target": "26",
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

        let registry =
            ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
        for (function_name, expected_target, expected_phase, product_default) in [
            (
                "messages:legacyNode20",
                RuntimeCompatibilityTarget::Node20,
                RuntimeNodeSupportPhase::EolLegacy,
                false,
            ),
            (
                "messages:defaultNode",
                RuntimeCompatibilityTarget::product_default_node_lts_target(),
                RuntimeNodeSupportPhase::ActiveLts,
                true,
            ),
            (
                "messages:activeNode24",
                RuntimeCompatibilityTarget::Node24,
                RuntimeNodeSupportPhase::ActiveLts,
                true,
            ),
            (
                "messages:currentNode26",
                RuntimeCompatibilityTarget::Node26,
                RuntimeNodeSupportPhase::CurrentNonLts,
                false,
            ),
        ] {
            let limits = registry.runtime_limits_for_function(function_name);
            let metadata = limits
                .compatibility_target
                .node_lts_metadata()
                .expect("Convex Node lane should map to registry metadata");
            assert_eq!(limits.compatibility_target, expected_target);
            assert_eq!(
                limits.compatibility_target.node_support_phase(),
                Some(expected_phase)
            );
            assert_eq!(metadata.product_default, product_default);
            assert!(limits.grants.net_connect.is_empty());
            assert!(limits.grants.net_listen.is_empty());
            assert!(limits.grants.worker.is_empty());
            assert!(limits.grants.run.is_empty());
            assert!(limits.grants.ffi.is_empty());
            assert!(!limits.grants.sys.contains(&"inspector".to_string()));
            assert!(
                !limits
                    .grants
                    .env_read
                    .contains(&"NODE_TLS_REJECT_UNAUTHORIZED".to_string()),
                "Convex Node lanes should use production in-process grants by default"
            );
        }
    }

    #[test]
    fn convex_node_runtime_same_target_override_preserves_policy_grants() {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:localNode22",
                        "kind": "action",
                        "runtime_environment": "node",
                        "runtime_compatibility_target": "22",
                        "plan": null
                    },
                    {
                        "name": "messages:prodNode24",
                        "kind": "action",
                        "runtime_environment": "node",
                        "runtime_compatibility_target": "24",
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

        let registry = ConvexRegistry::from_app_dir(tempdir.path())
            .expect("convex registry should load")
            .with_runtime_limits(RuntimeLimits::application_node22_local_development());
        let local_node22 = registry.runtime_limits_for_function("messages:localNode22");
        let prod_node24 = registry.runtime_limits_for_function("messages:prodNode24");

        assert_eq!(
            local_node22.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        );
        assert!(
            local_node22
                .grants
                .net_connect
                .contains(&"localhost".to_string()),
            "same-target Node override should preserve explicit network grants"
        );
        assert!(
            local_node22.grants.worker.contains(&"thread".to_string()),
            "same-target Node override should preserve worker grants"
        );
        assert_eq!(
            prod_node24.compatibility_target,
            RuntimeCompatibilityTarget::Node24
        );
        assert!(
            prod_node24.grants.net_connect.is_empty(),
            "other Node targets should keep production in-process grants"
        );
    }

    fn assert_convex_use_node_action_package_canary(
        target: RuntimeCompatibilityTarget,
        target_manifest_value: &str,
    ) {
        let tempdir = tempdir().expect("convex manifest tempdir should build");
        let convex_dir = tempdir.path().join(".nimbus").join("convex");
        let staged_package_dir = convex_dir.join("node_modules").join("left-pad");
        fs::create_dir_all(&staged_package_dir).expect("staged package directory should build");
        fs::write(
            staged_package_dir.join("package.json"),
            r#"{"name":"left-pad","version":"1.3.0","main":"index.js"}"#,
        )
        .expect("staged package metadata should write");
        fs::write(
            convex_dir.join("functions.json"),
            serde_json::to_vec_pretty(&json!({
                "functions": [
                    {
                        "name": "messages:nodePackageAction",
                        "kind": "action",
                        "visibility": "public",
                        "runtime_environment": "node",
                        "runtime_compatibility_target": target_manifest_value,
                        "runtime_package_resolution": "node_external_packages",
                        "runtime_handler": "() => null",
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
        fs::write(
            convex_dir.join("node_external_packages.json"),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "mode": "explicit",
                "configuredExternalPackages": ["left-pad"],
                "stagingRoot": ".nimbus/convex/node_modules",
                "packages": [
                    {
                        "packageName": "left-pad",
                        "packageRoot": "node_modules/left-pad",
                        "stagedPackageRoot": ".nimbus/convex/node_modules/left-pad",
                        "sizeBytes": 128,
                        "resolvedSpecifiers": ["left-pad"],
                        "importers": [
                            {
                                "file": "messages.ts",
                                "kind": "import",
                                "specifier": "left-pad"
                            }
                        ]
                    }
                ]
            }))
            .expect("node external package manifest should serialize"),
        )
        .expect("node external package manifest should write");

        let registry =
            ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
        let definition = registry
            .function_definition("messages:nodePackageAction")
            .expect("canary action should load");
        let selection = definition.runtime_selection();
        assert_eq!(selection.compatibility_target, target);
        assert_eq!(
            selection.package_resolution,
            ConvexRuntimePackageResolution::NodeExternalPackages
        );
        assert_eq!(
            registry
                .runtime_limits_for_function("messages:nodePackageAction")
                .compatibility_target,
            target
        );
    }

    #[test]
    #[ignore = "Convex use-node action package canary: executed by node-compat canary registry"]
    fn convex_use_node_action_package_canary_node22() {
        assert_convex_use_node_action_package_canary(RuntimeCompatibilityTarget::Node22, "22");
    }

    #[test]
    #[ignore = "Convex use-node action package canary: executed by node-compat canary registry"]
    fn convex_use_node_action_package_canary_node24() {
        assert_convex_use_node_action_package_canary(RuntimeCompatibilityTarget::Node24, "24");
    }

    #[test]
    #[ignore = "Convex use-node action package canary: executed by node-compat canary registry"]
    fn convex_use_node_action_package_canary_node26_current() {
        assert_convex_use_node_action_package_canary(RuntimeCompatibilityTarget::Node26, "26");
    }

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
            .required_runtime_limits_for_function("messages:bunProof")
            .expect_err("default build should fail closed before admitting a Bun/JSC lane");
        assert!(
            error
                .to_string()
                .contains("Bun/JSC execution adapter is not linked"),
            "unexpected Bun/JSC no-link error: {error}"
        );
    }
}
