//! Deploy orchestration (CP3): the compute-owned body behind the server's
//! `POST /deploy` handler. Stages the uploaded Convex/Cloud Functions
//! artifacts, builds the next registries, diffs them against the active
//! deployment, and (unless `dry_run`) activates the new generation and
//! projects it into the `_nimbus` system tables.
//!
//! The transport handler (`nimbus-server`'s `http/deploy.rs`) only extracts the
//! request, authorizes the deploy-admin bearer token, calls [`deploy_app`],
//! and wraps the result in `Json`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use nimbus_auth::ApplicationAuthVerifier;
use nimbus_cloud_functions::{
    CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
    CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE, CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE,
    CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsRegistry, CloudFunctionsRuntimeInvocation,
    CloudFunctionsRuntimeInvoker, CloudFunctionsTriggerExecutor,
};
use nimbus_convex::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistry,
    ConvexRegistryDeploySummary,
};
use nimbus_core::Error;
use nimbus_system::{
    DiskSourcePackageStore, SourcePackageStore, SystemDeploymentFunctionRecordInput,
    SystemDeploymentHttpRouteRecordInput, SystemDeploymentRecordInput, SystemModuleRecordInput,
    SystemSourcePackageRecordInput, parse_source_package, record_deployment_state_async,
    record_source_package_state_async,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::execution::errors::runtime_error_to_core;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_egress_gateway,
};
use crate::state::{ComputeError, ComputeState, DeploymentState};

/// Orchestrates a `deploy_app` request: stage artifacts, diff against the
/// active deployment, and (unless `dry_run`) activate the next generation.
pub async fn deploy_app(
    compute: &ComputeState,
    request: DeployRequest,
) -> Result<DeployResponse, ComputeError> {
    let DeployRequest { dry_run, artifacts } = request;
    // Capture the source package before `artifacts` is moved into staging; it is
    // persisted (content-addressed) and projected only on a real activation.
    let source_package = artifacts.convex.as_ref().and_then(|convex| {
        match (&convex.source_package, &convex.source_package_sha256) {
            (Some(json), Some(digest)) => Some((json.clone(), digest.clone())),
            _ => None,
        }
    });
    let previous_deployment = compute.current_deployment();
    let previous_generation = previous_deployment.generation;
    let previous_registry = previous_deployment.convex_registry();
    let previous_cloud_functions_registry = previous_deployment.cloud_functions_registry();
    let runtime_limits = previous_registry
        .as_ref()
        .map(|registry| registry.runtime_limits())
        .or_else(|| {
            previous_cloud_functions_registry
                .as_ref()
                .map(|registry| registry.runtime_limits())
        })
        .unwrap_or_default();
    let previous_summary = previous_registry
        .as_deref()
        .map(ConvexRegistry::deploy_summary);

    let staged = tokio::task::spawn_blocking(move || stage_deploy_artifacts(&artifacts))
        .await
        .map_err(|error| {
            Error::Internal(format!("deploy artifact staging task failed: {error}"))
        })??;
    let next_registry = staged
        .includes_convex()
        .then(|| {
            ConvexRegistry::from_app_dir(staged.app_dir())
                .map(|registry| registry.with_runtime_limits(runtime_limits.clone()))
        })
        .transpose()?;
    let next_summary = next_registry
        .as_ref()
        .map(ConvexRegistry::deploy_summary)
        .or(previous_summary.clone())
        .unwrap_or_else(DeployDiff::empty_summary);
    let diff = DeployDiff::from_summaries(previous_summary.as_ref(), &next_summary);
    let next_cloud_functions_registry = staged
        .includes_cloud_functions()
        .then(|| {
            CloudFunctionsRegistry::from_app_dir(staged.app_dir())
                .map(|registry| registry.with_runtime_limits(runtime_limits.clone()))
        })
        .transpose()?;

    let generation = if dry_run {
        previous_generation
    } else {
        let next_convex_registry = next_registry
            .map(Arc::new)
            .or_else(|| previous_deployment.convex_registry());
        let next_application_auth_verifier = next_convex_registry
            .as_ref()
            .map(|registry| registry.clone() as Arc<dyn ApplicationAuthVerifier>)
            .or_else(|| previous_deployment.application_auth_verifier());
        let next_cloud_functions_registry = next_cloud_functions_registry
            .map(Arc::new)
            .or_else(|| previous_deployment.cloud_functions_registry());
        let next_deployment = DeploymentState {
            generation: previous_generation.saturating_add(1),
            convex_registry: next_convex_registry,
            application_auth_verifier: next_application_auth_verifier,
            cloud_functions_registry: next_cloud_functions_registry.clone(),
            cloudflare_config: previous_deployment.cloudflare_config(),
            firebase_config: previous_deployment.firebase_config(),
            convex_tenancy: previous_deployment.convex_tenancy(),
        };
        compute.active_deployment.activate(next_deployment);
        if let Some(registry) = next_cloud_functions_registry {
            compute.install_cloud_functions_runtime_hooks(registry)?;
        }
        let generation = compute.current_deployment().generation;
        if let Some(registry) = compute.current_deployment().convex_registry() {
            let summary = registry.deploy_summary();
            let source_ref = format!("deploy:generation:{generation}");
            let input = convex_system_deployment_record_input(&summary, &source_ref);
            record_deployment_state_async(&compute.engine, &input).await?;
            if let Some((source_package_json, expected_digest)) = source_package {
                persist_source_package(compute, &source_package_json, &expected_digest).await?;
            }
        }
        generation
    };

    Ok(DeployResponse {
        dry_run,
        activated: !dry_run,
        generation,
        previous_generation,
        diff,
    })
}

/// The `CloudFunctionsRuntimeInvoker` used to actually execute a bundle.
/// Axum-free: routes through the CP1-relocated blocking-egress entrypoint, so
/// it lives beside the deploy orchestration that wires it into the engine's
/// trigger-invocation executor. `nimbus-server`'s
/// `adapters::cloud_functions::execution` re-exports this under its former
/// name (`ServerCloudFunctionsRuntimeInvoker`) so its integration test module
/// keeps compiling unchanged.
#[derive(Debug, Clone)]
pub struct ComputeCloudFunctionsRuntimeInvoker;

impl CloudFunctionsRuntimeInvoker for ComputeCloudFunctionsRuntimeInvoker {
    fn invoke_runtime_bundle(
        &self,
        invocation: CloudFunctionsRuntimeInvocation,
    ) -> nimbus_core::Result<serde_json::Value> {
        // Route Cloud Functions through the egress-gateway entrypoint (not the
        // coarse no-gateway `_with_host` path) so the handler's `fetch` is bound
        // to the tenant's nimbus-egress PDP. CloudFunctionsHostBridge implements
        // EgressGateway, so the isolate fetch hook inherits the L7 fail-closed
        // for free. (audit M13 — Cloud Functions egress parity.)
        invoke_runtime_bundle_blocking_with_egress_gateway(
            &invocation.runtime_executor,
            invocation.runtime_policy,
            invocation.host_bridge,
            invocation.bundle,
            invocation.request,
            RuntimeBundleInvocationOptions::enforcing_policy_limit(
                &invocation.tenant_id,
                invocation.server_request_id.as_deref(),
                None,
            )
            .with_optional_runtime_bundle_provenance_gate(invocation.provenance_gate.as_ref()),
        )
        .map_err(runtime_error_to_core)
    }
}

impl ComputeState {
    /// Installs the engine's trigger registrations/invocation executor for a
    /// newly activated Cloud Functions registry.
    pub fn install_cloud_functions_runtime_hooks(
        &self,
        registry: Arc<CloudFunctionsRegistry>,
    ) -> Result<(), ComputeError> {
        let deployment_generation = self.current_deployment().generation;
        self.engine
            .install_trigger_registrations(registry.trigger_registrations()?)?;
        self.engine.install_trigger_invocation_executor(Arc::new(
            CloudFunctionsTriggerExecutor::new(
                self.engine.clone(),
                registry,
                deployment_generation,
                self.runtime_service_registry(),
                self.tenant_isolation_mode(),
                Arc::new(ComputeCloudFunctionsRuntimeInvoker),
            ),
        ))?;
        Ok(())
    }
}

/// Projects a Convex registry's deploy summary into the `_nimbus` system
/// deployment record input. Shared by [`deploy_app`] (a real deploy) and the
/// server's startup path (`prepare_system_tenant`, which records the
/// already-loaded registry as generation 0 under a `"startup"` source ref).
pub fn convex_system_deployment_record_input<'a>(
    summary: &'a ConvexRegistryDeploySummary,
    source_ref: &'a str,
) -> SystemDeploymentRecordInput<'a> {
    SystemDeploymentRecordInput {
        source_ref,
        functions: summary
            .functions
            .iter()
            .map(|function| SystemDeploymentFunctionRecordInput {
                name: function.name.as_str(),
                kind: function.kind,
                fingerprint: function.fingerprint.as_str(),
            })
            .collect(),
        http_routes: summary
            .http_routes
            .iter()
            .map(|route| SystemDeploymentHttpRouteRecordInput {
                key: route.key.as_str(),
                fingerprint: route.fingerprint.as_str(),
            })
            .collect(),
        schema_fingerprint: summary.schema_fingerprint.as_deref(),
        index_fingerprint: summary.index_fingerprint.as_deref(),
        runtime_bundle_fingerprint: summary.runtime_bundle_fingerprint.as_deref(),
    }
}

#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    #[serde(default)]
    dry_run: bool,
    artifacts: DeployArtifacts,
}

#[derive(Debug, Deserialize)]
pub struct DeployArtifacts {
    #[serde(default)]
    convex: Option<ConvexDeployArtifacts>,
    #[serde(default)]
    cloud_functions: Option<CloudFunctionsDeployArtifacts>,
}

#[derive(Debug, Deserialize)]
pub struct ConvexDeployArtifacts {
    functions_json: Value,
    #[serde(default)]
    http_routes_json: Option<Value>,
    #[serde(default)]
    schema_json: Option<Value>,
    #[serde(default)]
    auth_config_json: Option<Value>,
    #[serde(default)]
    bundle_mjs: Option<String>,
    #[serde(default)]
    bundle_sha256: Option<String>,
    /// Canonical source-package JSON (original module source + maps) — the
    /// read-artifact behind the console Source view. Optional; paired with its
    /// digest. See the Function Source Visibility plan (FSV3).
    #[serde(default)]
    source_package: Option<String>,
    #[serde(default)]
    source_package_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CloudFunctionsDeployArtifacts {
    artifact_json: Value,
    targets_json: Value,
    bundle_mjs: String,
    bundle_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct DeployResponse {
    dry_run: bool,
    activated: bool,
    generation: u64,
    previous_generation: u64,
    diff: DeployDiff,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployDiff {
    functions: DeployFunctionDiff,
    http_routes: DeployHttpRouteDiff,
    schema_changed: bool,
    indexes_changed: bool,
    runtime_bundle_changed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployFunctionDiff {
    added: Vec<DeployFunctionChange>,
    changed: Vec<DeployFunctionChange>,
    removed: Vec<DeployFunctionChange>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployHttpRouteDiff {
    added: Vec<DeployHttpRouteChange>,
    changed: Vec<DeployHttpRouteChange>,
    removed: Vec<DeployHttpRouteChange>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployFunctionChange {
    name: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeployHttpRouteChange {
    key: String,
}

impl DeployDiff {
    fn empty_summary() -> ConvexRegistryDeploySummary {
        ConvexRegistryDeploySummary {
            functions: Vec::new(),
            http_routes: Vec::new(),
            schema_fingerprint: None,
            index_fingerprint: None,
            runtime_bundle_fingerprint: None,
        }
    }

    fn from_summaries(
        previous: Option<&ConvexRegistryDeploySummary>,
        next: &ConvexRegistryDeploySummary,
    ) -> Self {
        let empty = Self::empty_summary();
        let previous = previous.unwrap_or(&empty);
        Self {
            functions: diff_functions(&previous.functions, &next.functions),
            http_routes: diff_http_routes(&previous.http_routes, &next.http_routes),
            schema_changed: previous.schema_fingerprint != next.schema_fingerprint,
            indexes_changed: previous.index_fingerprint != next.index_fingerprint,
            runtime_bundle_changed: previous.runtime_bundle_fingerprint
                != next.runtime_bundle_fingerprint,
        }
    }
}

fn diff_functions(
    previous: &[ConvexFunctionDeploySummary],
    next: &[ConvexFunctionDeploySummary],
) -> DeployFunctionDiff {
    let previous = previous
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<BTreeMap<_, _>>();
    let next = next
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<BTreeMap<_, _>>();

    DeployFunctionDiff {
        added: next
            .iter()
            .filter(|(name, _)| !previous.contains_key(**name))
            .map(|(_, function)| DeployFunctionChange::from_summary(function))
            .collect(),
        changed: next
            .iter()
            .filter_map(|(name, function)| {
                let previous = previous.get(*name)?;
                (previous.fingerprint != function.fingerprint)
                    .then(|| DeployFunctionChange::from_summary(function))
            })
            .collect(),
        removed: previous
            .iter()
            .filter(|(name, _)| !next.contains_key(**name))
            .map(|(_, function)| DeployFunctionChange::from_summary(function))
            .collect(),
    }
}

fn diff_http_routes(
    previous: &[ConvexHttpRouteDeploySummary],
    next: &[ConvexHttpRouteDeploySummary],
) -> DeployHttpRouteDiff {
    let previous = previous
        .iter()
        .map(|route| (route.key.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let next = next
        .iter()
        .map(|route| (route.key.as_str(), route))
        .collect::<BTreeMap<_, _>>();

    DeployHttpRouteDiff {
        added: next
            .iter()
            .filter(|(key, _)| !previous.contains_key(**key))
            .map(|(_, route)| DeployHttpRouteChange::from_summary(route))
            .collect(),
        changed: next
            .iter()
            .filter_map(|(key, route)| {
                let previous = previous.get(*key)?;
                (previous.fingerprint != route.fingerprint)
                    .then(|| DeployHttpRouteChange::from_summary(route))
            })
            .collect(),
        removed: previous
            .iter()
            .filter(|(key, _)| !next.contains_key(**key))
            .map(|(_, route)| DeployHttpRouteChange::from_summary(route))
            .collect(),
    }
}

impl DeployFunctionChange {
    fn from_summary(summary: &ConvexFunctionDeploySummary) -> Self {
        Self {
            name: summary.name.clone(),
            kind: summary.kind.to_string(),
        }
    }
}

impl DeployHttpRouteChange {
    fn from_summary(summary: &ConvexHttpRouteDeploySummary) -> Self {
        Self {
            key: summary.key.clone(),
        }
    }
}

struct StagedDeployArtifacts {
    app_dir: tempfile::TempDir,
    includes_convex: bool,
    includes_cloud_functions: bool,
}

impl StagedDeployArtifacts {
    fn app_dir(&self) -> &Path {
        self.app_dir.path()
    }

    fn includes_convex(&self) -> bool {
        self.includes_convex
    }

    fn includes_cloud_functions(&self) -> bool {
        self.includes_cloud_functions
    }
}

fn stage_deploy_artifacts(artifacts: &DeployArtifacts) -> Result<StagedDeployArtifacts, Error> {
    validate_deploy_artifacts(artifacts)?;
    let app_dir = tempfile::Builder::new()
        .prefix("nimbus-deploy-")
        .tempdir()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to create deploy staging directory: {error}"
            ))
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(app_dir.path(), std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                Error::InvalidInput(format!(
                    "failed to make deploy staging directory private {}: {error}",
                    app_dir.path().display()
                ))
            },
        )?;
    }
    if let Some(convex) = &artifacts.convex {
        let convex_dir = app_dir.path().join(".nimbus").join("convex");
        std::fs::create_dir_all(&convex_dir).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to create deploy staging directory {}: {error}",
                convex_dir.display()
            ))
        })?;
        write_json_file(&convex_dir.join("functions.json"), &convex.functions_json)?;
        if let Some(value) = &convex.http_routes_json {
            write_json_file(&convex_dir.join("http_routes.json"), value)?;
        }
        if let Some(value) = &convex.schema_json {
            write_json_file(&convex_dir.join("schema.json"), value)?;
        }
        if let Some(value) = &convex.auth_config_json {
            write_json_file(&convex_dir.join("auth.config.json"), value)?;
        }
        if let Some(bundle) = &convex.bundle_mjs {
            std::fs::write(convex_dir.join("bundle.mjs"), bundle).map_err(|error| {
                Error::InvalidInput(format!("failed to stage runtime bundle: {error}"))
            })?;
        }
        if let Some(hash) = &convex.bundle_sha256 {
            std::fs::write(convex_dir.join("bundle.sha256"), hash).map_err(|error| {
                Error::InvalidInput(format!("failed to stage runtime bundle hash: {error}"))
            })?;
        }
    }

    if let Some(cloud_functions) = &artifacts.cloud_functions {
        let cloud_functions_dir = app_dir.path().join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
        std::fs::create_dir_all(&cloud_functions_dir).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to create deploy staging directory {}: {error}",
                cloud_functions_dir.display()
            ))
        })?;
        write_json_file(
            &cloud_functions_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
            &cloud_functions.artifact_json,
        )?;
        write_json_file(
            &cloud_functions_dir.join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
            &cloud_functions.targets_json,
        )?;
        std::fs::write(
            cloud_functions_dir.join(CLOUD_FUNCTIONS_RUNTIME_BUNDLE_FILE),
            &cloud_functions.bundle_mjs,
        )
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to stage cloud functions runtime bundle: {error}"
            ))
        })?;
        std::fs::write(
            cloud_functions_dir.join(CLOUD_FUNCTIONS_RUNTIME_BUNDLE_SHA256_FILE),
            &cloud_functions.bundle_sha256,
        )
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to stage cloud functions runtime bundle hash: {error}"
            ))
        })?;
    }

    Ok(StagedDeployArtifacts {
        app_dir,
        includes_convex: artifacts.convex.is_some(),
        includes_cloud_functions: artifacts.cloud_functions.is_some(),
    })
}

fn validate_deploy_artifacts(artifacts: &DeployArtifacts) -> Result<(), Error> {
    if artifacts.convex.is_none() && artifacts.cloud_functions.is_none() {
        return Err(Error::InvalidInput(
            "deploy request must include convex and/or cloud functions artifacts".to_string(),
        ));
    }
    if let Some(convex) = &artifacts.convex {
        match (&convex.bundle_mjs, &convex.bundle_sha256) {
            (Some(_), Some(_)) | (None, None) => {}
            (Some(_), None) => {
                return Err(Error::InvalidInput(
                    "deploy artifact bundle_mjs requires bundle_sha256".to_string(),
                ));
            }
            (None, Some(_)) => {
                return Err(Error::InvalidInput(
                    "deploy artifact bundle_sha256 requires bundle_mjs".to_string(),
                ));
            }
        }
        match (&convex.source_package, &convex.source_package_sha256) {
            (Some(_), Some(_)) | (None, None) => {}
            (Some(_), None) => {
                return Err(Error::InvalidInput(
                    "deploy artifact source_package requires source_package_sha256".to_string(),
                ));
            }
            (None, Some(_)) => {
                return Err(Error::InvalidInput(
                    "deploy artifact source_package_sha256 requires source_package".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Persist a deploy's source package: store the bytes content-addressed (dedup
/// by digest), verify the client-provided digest, parse the modules, and project
/// the `source_packages` + `modules` rows. See FSV3.
async fn persist_source_package(
    compute: &ComputeState,
    source_package_json: &str,
    expected_digest: &str,
) -> Result<(), ComputeError> {
    let bytes = source_package_json.as_bytes();
    let store = DiskSourcePackageStore::new(compute.engine.data_dir().join("source-packages"));
    let stored = store.put(bytes)?;
    if stored.digest != expected_digest {
        return Err(Error::InvalidInput(format!(
            "source_package_sha256 mismatch: client sent {expected_digest}, server computed {}",
            stored.digest
        ))
        .into());
    }
    let parsed = parse_source_package(bytes)?;
    let modules = parsed
        .modules
        .iter()
        .map(|module| SystemModuleRecordInput {
            path: &module.path,
            sha256: &module.sha256,
        })
        .collect::<Vec<_>>();
    let input = SystemSourcePackageRecordInput {
        digest: &stored.digest,
        storage_key: &stored.storage_key,
        size_bytes: stored.size_bytes,
        unpacked_bytes: parsed.unpacked_bytes,
        modules,
    };
    record_source_package_state_async(&compute.engine, &input).await?;
    Ok(())
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to serialize deploy artifact {}: {error}",
            path.display()
        ))
    })?;
    std::fs::write(path, bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to stage deploy artifact {}: {error}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn stage_deploy_artifacts_uses_private_randomized_directory() {
        let artifacts = DeployArtifacts {
            convex: Some(ConvexDeployArtifacts {
                functions_json: json!([]),
                http_routes_json: None,
                schema_json: None,
                auth_config_json: None,
                bundle_mjs: None,
                bundle_sha256: None,
                source_package: None,
                source_package_sha256: None,
            }),
            cloud_functions: None,
        };

        let staged = stage_deploy_artifacts(&artifacts).expect("deploy artifacts should stage");
        let app_dir = staged.app_dir();
        assert!(
            app_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("nimbus-deploy-")),
            "staging directory should use the Nimbus deploy prefix with a random suffix"
        );
        assert!(app_dir.join(".nimbus/convex/functions.json").is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = std::fs::metadata(app_dir)
                .expect("staging directory metadata should load")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }
}
