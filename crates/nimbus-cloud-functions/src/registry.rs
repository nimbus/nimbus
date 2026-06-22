use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_artifacts::{ArtifactVerificationPolicy, ArtifactVerifierBackend};
use nimbus_core::{DocumentTriggerPattern, Error, Result};
use nimbus_engine::TriggerRegistration;
use nimbus_provenance::RuntimeBundleProvenanceConfig;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, NominalRuntimeHostPressureSource,
    RuntimeAdaptiveControllerSettings, RuntimeBundle, RuntimeExecutor, RuntimeHostPressureSource,
    RuntimeHostResourceBudget, RuntimeLimits, RuntimePolicy, RuntimeScalingPlanSet,
};

use crate::{
    CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
    CloudFunctionsArtifactManifest, CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition,
    CloudFunctionsTargetsManifest,
};
#[derive(Debug)]
pub struct CloudFunctionsRegistry {
    artifact_dir: PathBuf,
    targets: HashMap<String, CloudFunctionsTargetDefinition>,
    runtime_bundle: RuntimeBundle,
    runtime_policy: Arc<RuntimePolicy>,
    runtime_executor: Arc<RuntimeExecutor>,
    runtime_bundle_provenance: Option<RuntimeBundleProvenanceConfig>,
}

impl CloudFunctionsRegistry {
    pub fn from_app_dir(app_dir: impl AsRef<Path>) -> Result<Self> {
        Self::from_artifact_dir(app_dir.as_ref().join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR))
    }

    pub fn from_artifact_dir(artifact_dir: impl AsRef<Path>) -> Result<Self> {
        let artifact_dir = artifact_dir.as_ref().to_path_buf();
        let manifest = read_json_file::<CloudFunctionsArtifactManifest>(
            &artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
        )?;
        manifest.validate()?;

        let targets_manifest = read_json_file::<CloudFunctionsTargetsManifest>(
            &artifact_dir.join(&manifest.targets_manifest),
        )?;
        targets_manifest.validate()?;

        let runtime_bundle = load_runtime_bundle(
            &artifact_dir.join(&manifest.runtime_bundle.entry_file),
            &artifact_dir.join(&manifest.runtime_bundle.sha256_file),
        )?;
        let targets = targets_manifest
            .targets
            .into_iter()
            .map(|target| (target.name.clone(), target))
            .collect();

        let runtime_policy = Arc::new(RuntimePolicy::default());
        let runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy.clone()));
        Ok(Self {
            artifact_dir,
            targets,
            runtime_bundle,
            runtime_policy,
            runtime_executor,
            runtime_bundle_provenance: None,
        })
    }

    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        let runtime_policy = Arc::new(RuntimePolicy::new(limits));
        self.runtime_policy = runtime_policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy));
        self
    }

    pub fn with_runtime_host_resource_budget(self, budget: RuntimeHostResourceBudget) -> Self {
        self.with_runtime_host_governor(
            budget,
            Arc::new(NominalRuntimeHostPressureSource),
            RuntimeAdaptiveControllerSettings::default(),
        )
    }

    pub fn with_runtime_host_governor(
        mut self,
        budget: RuntimeHostResourceBudget,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
        adaptive_settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        let runtime_policy = Arc::new(
            RuntimePolicy::with_host_resource_governor(
                self.runtime_policy.limits().clone(),
                budget,
                pressure_source,
            )
            .with_adaptive_controller_settings(adaptive_settings),
        );
        self.runtime_policy = runtime_policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy));
        self
    }

    pub fn with_effective_runtime_scaling_plan(self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.with_effective_runtime_scaling_plans(RuntimeScalingPlanSet::single(plan))
    }

    pub fn with_effective_runtime_scaling_plans(mut self, plans: RuntimeScalingPlanSet) -> Self {
        let runtime_policy = Arc::new(
            self.runtime_policy
                .clone_with_effective_scaling_plans(plans),
        );
        self.runtime_policy = runtime_policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy));
        self
    }

    pub fn with_runtime_bundle_provenance_verifier(
        mut self,
        policy: ArtifactVerificationPolicy,
        verifier: impl ArtifactVerifierBackend + 'static,
    ) -> Self {
        self.runtime_bundle_provenance = Some(RuntimeBundleProvenanceConfig::new(
            policy,
            Arc::new(verifier),
            "cloud functions runtime bundle",
        ));
        self
    }

    pub fn with_runtime_bundle_provenance_verifier_arc(
        mut self,
        policy: ArtifactVerificationPolicy,
        verifier: Arc<dyn ArtifactVerifierBackend>,
    ) -> Self {
        self.runtime_bundle_provenance = Some(RuntimeBundleProvenanceConfig::new(
            policy,
            verifier,
            "cloud functions runtime bundle",
        ));
        self
    }

    pub fn artifact_dir(&self) -> &Path {
        &self.artifact_dir
    }

    pub fn runtime_bundle(&self) -> RuntimeBundle {
        self.runtime_bundle.clone()
    }

    pub fn runtime_policy(&self) -> Arc<RuntimePolicy> {
        self.runtime_policy.clone()
    }

    pub fn runtime_executor(&self) -> Arc<RuntimeExecutor> {
        self.runtime_executor.clone()
    }

    pub fn runtime_bundle_provenance(&self) -> Option<&RuntimeBundleProvenanceConfig> {
        self.runtime_bundle_provenance.as_ref()
    }

    pub fn runtime_limits(&self) -> RuntimeLimits {
        self.runtime_policy.limits().clone()
    }

    pub fn required_firestore_trigger_target(
        &self,
        registration_id: &str,
    ) -> Result<&CloudFunctionsTargetDefinition> {
        let target = self.targets.get(registration_id).ok_or_else(|| {
            Error::InvalidInput(format!(
                "cloud functions trigger target `{registration_id}` is not present in targets manifest"
            ))
        })?;
        if !matches!(
            target.binding,
            CloudFunctionsTargetBinding::FirestoreDocument { .. }
        ) {
            return Err(Error::InvalidInput(format!(
                "cloud functions target `{registration_id}` is not a Firestore document trigger"
            )));
        }
        Ok(target)
    }

    pub fn resolve_https_target(
        &self,
        request_path: &str,
    ) -> Option<&CloudFunctionsTargetDefinition> {
        self.targets.values().find(|target| {
            matches!(
                &target.binding,
                CloudFunctionsTargetBinding::Https { path, .. } if path == request_path
            )
        })
    }

    pub fn trigger_registrations(&self) -> Result<Vec<TriggerRegistration>> {
        let mut targets = self
            .targets
            .values()
            .filter_map(|target| match &target.binding {
                CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type,
                    document,
                    ..
                } => Some((target.name.clone(), *event_type, document.clone())),
                CloudFunctionsTargetBinding::Https { .. } => None,
            })
            .collect::<Vec<_>>();
        targets.sort_by(|left, right| left.0.cmp(&right.0));
        targets
            .into_iter()
            .map(|(name, event_type, document)| {
                TriggerRegistration::new(
                    &name,
                    event_type,
                    DocumentTriggerPattern::from_segments(document.split('/')).map_err(|error| {
                        Error::InvalidInput(format!(
                            "cloud functions trigger target `{name}` has invalid document pattern `{document}`: {error}"
                        ))
                    })?,
                )
            })
            .collect()
    }
}

fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let contents = std::fs::read_to_string(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read cloud functions artifact file {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse cloud functions artifact file {}: {error}",
            path.display()
        ))
    })
}

fn load_runtime_bundle(bundle_path: &Path, hash_path: &Path) -> Result<RuntimeBundle> {
    let expected_sha256 = std::fs::read_to_string(hash_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read cloud functions runtime bundle hash {}: {error}",
            hash_path.display()
        ))
    })?;
    RuntimeBundle::with_expected_sha256(bundle_path, expected_sha256).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to load cloud functions runtime bundle {}: {error}",
            bundle_path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use nimbus_core::FirestoreCloudEventType;
    use nimbus_runtime::{
        EffectiveRuntimeScalingPlan, RuntimeBundle, RuntimeHostPressureLevel,
        RuntimeHostPressureSample, RuntimeHostPressureSource, RuntimeHostResourceBudget,
        RuntimeMemoryPressureDecision, RuntimeMemoryPressureLevel,
        RuntimeMemoryPressureSourceStatus,
    };
    use tempfile::tempdir;

    use super::CloudFunctionsRegistry;
    use crate::{
        CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
        CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
        CloudFunctionsAuthoringSurface, CloudFunctionsExecutionPrincipal,
        CloudFunctionsSignatureType, CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition,
        CloudFunctionsTargetsManifest,
    };

    #[derive(Debug)]
    struct FixedRuntimeHostPressureSource(RuntimeHostPressureSample);

    impl RuntimeHostPressureSource for FixedRuntimeHostPressureSource {
        fn sample(&self) -> RuntimeHostPressureSample {
            self.0
        }
    }

    fn two_seat_runtime_host_budget() -> RuntimeHostResourceBudget {
        RuntimeHostResourceBudget {
            host_millicpus: 2000,
            system_reserved_millicpus: 0,
            nimbus_control_plane_reserved_millicpus: 0,
            runtime_hard_ceiling_millicpus: None,
            runtime_seat_millicpus: std::num::NonZeroU32::new(1000)
                .expect("one CPU seat is nonzero"),
        }
    }

    fn critical_runtime_host_pressure_source() -> Arc<dyn RuntimeHostPressureSource> {
        Arc::new(FixedRuntimeHostPressureSource(
            RuntimeHostPressureSample::observed(
                RuntimeHostPressureLevel::Critical,
                RuntimeMemoryPressureDecision::for_level(
                    RuntimeMemoryPressureLevel::Nominal,
                    RuntimeMemoryPressureSourceStatus::Observed,
                ),
                false,
            ),
        ))
    }

    #[test]
    fn cloud_functions_registry_loads_bundle_and_trigger_targets() {
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "syncUser".to_string(),
                entrypoint: "exports.syncUser".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionPrincipal::ServiceAccount,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function () {
  return "ok";
};

export {};
"#,
        );

        let registry =
            CloudFunctionsRegistry::from_app_dir(app_dir.path()).expect("registry should load");
        let target = registry
            .required_firestore_trigger_target("syncUser")
            .expect("trigger target should resolve");

        assert_eq!(target.entrypoint, "exports.syncUser");
        assert_eq!(
            registry.artifact_dir(),
            app_dir.path().join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR)
        );
    }

    #[test]
    fn cloud_functions_registry_applies_runtime_host_resource_budget_to_runtime_policy() {
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(app_dir.path(), &[], "export {};");
        let budget = two_seat_runtime_host_budget();
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("registry should load")
            .with_runtime_host_governor(
                budget,
                critical_runtime_host_pressure_source(),
                nimbus_runtime::RuntimeAdaptiveControllerSettings::shadow(
                    nimbus_runtime::RuntimeControllerReplayConfig::default(),
                ),
            );

        assert_eq!(registry.runtime_policy().host_resource_budget(), budget);
        assert_eq!(
            registry
                .runtime_policy()
                .adaptive_controller_settings()
                .mode(),
            nimbus_runtime::RuntimeAdaptiveControllerMode::Shadow
        );
        assert_eq!(
            registry
                .runtime_policy()
                .host_resource_decision()
                .host_pressure_level,
            RuntimeHostPressureLevel::Critical
        );
        assert_eq!(
            registry
                .runtime_policy()
                .host_resource_decision()
                .effective_dispatch_seats,
            0
        );
    }

    #[test]
    fn cloud_functions_registry_applies_effective_runtime_scaling_plan_to_runtime_policy() {
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(app_dir.path(), &[], "export {};");
        let plan = EffectiveRuntimeScalingPlan::baked_standard("syncUser", 5);
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("registry should load")
            .with_effective_runtime_scaling_plan(plan.clone());

        assert_eq!(registry.runtime_policy().effective_scaling_plan(), &plan);
        assert_eq!(
            registry
                .runtime_policy()
                .effective_scaling_plan()
                .effective
                .max_warm,
            5
        );
    }

    #[test]
    fn cloud_functions_registry_rejects_invalid_artifact_manifest() {
        let app_dir = tempdir().expect("app tempdir should build");
        let artifact_dir = app_dir.path().join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
        fs::create_dir_all(&artifact_dir).expect("artifact dir should create");
        let mut manifest = CloudFunctionsArtifactManifest::v1();
        manifest.import_resolution.covered_specifiers = vec!["firebase-functions/v2".to_string()];
        fs::write(
            artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).expect("manifest should encode"),
        )
        .expect("manifest should write");

        let error = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect_err("registry should reject invalid manifest");
        assert!(
            error
                .to_string()
                .contains("covered import specifiers do not match")
        );
    }

    fn write_cloud_functions_artifact(
        app_dir: &Path,
        targets: &[CloudFunctionsTargetDefinition],
        bundle: &str,
    ) {
        let artifact_dir = app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
        fs::create_dir_all(&artifact_dir).expect("artifact dir should create");
        fs::write(
            artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
            serde_json::to_vec_pretty(&CloudFunctionsArtifactManifest::v1())
                .expect("manifest should encode"),
        )
        .expect("manifest should write");
        fs::write(
            artifact_dir.join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
            serde_json::to_vec_pretty(
                &CloudFunctionsTargetsManifest::v1(targets.to_vec())
                    .expect("targets should validate"),
            )
            .expect("targets should encode"),
        )
        .expect("targets should write");

        let bundle_path = artifact_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("bundle should write");
        let bundle_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("bundle sha should write");
    }
}
