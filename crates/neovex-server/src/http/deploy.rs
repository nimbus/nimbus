use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::http::header::AUTHORIZATION;
use neovex_core::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;
use crate::ConvexRegistry;
use crate::adapters::convex::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistryDeploySummary,
};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) async fn deploy_app(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<DeployRequest>,
) -> Result<Json<DeployResponse>, AppError> {
    authorize_deploy(&state, &headers)?;
    let (previous_generation, previous_registry) = state.convex_registry.snapshot();
    let runtime_limits = previous_registry
        .as_ref()
        .map(|registry| registry.runtime_limits())
        .unwrap_or_default();
    let previous_summary = previous_registry
        .as_deref()
        .map(ConvexRegistry::deploy_summary);

    let staged = stage_deploy_artifacts(&request.artifacts)?;
    let next_registry =
        ConvexRegistry::from_app_dir(staged.app_dir())?.with_runtime_limits(runtime_limits);
    let next_summary = next_registry.deploy_summary();
    let diff = DeployDiff::from_summaries(previous_summary.as_ref(), &next_summary);

    let generation = if request.dry_run {
        previous_generation
    } else {
        state.convex_registry.activate(next_registry).0
    };

    Ok(Json(DeployResponse {
        dry_run: request.dry_run,
        activated: !request.dry_run,
        generation,
        previous_generation,
        diff,
    }))
}

fn authorize_deploy(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(expected) = state.deploy_admin_token.as_deref() else {
        return Err(AppError::unauthorized(
            "deploy admin API is disabled; set NEOVEX_DEPLOY_TOKEN before starting the server",
        ));
    };
    let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(AppError::unauthorized(
            "deploy admin API requires Authorization: Bearer <token>",
        ));
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(AppError::unauthorized(
            "deploy admin API requires Authorization: Bearer <token>",
        ));
    };
    if token != expected {
        return Err(AppError::unauthorized("invalid deploy admin token"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployRequest {
    #[serde(default)]
    dry_run: bool,
    artifacts: DeployArtifacts,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployArtifacts {
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
}

#[derive(Debug, Serialize)]
pub(crate) struct DeployResponse {
    dry_run: bool,
    activated: bool,
    generation: u64,
    previous_generation: u64,
    diff: DeployDiff,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DeployDiff {
    functions: DeployFunctionDiff,
    http_routes: DeployHttpRouteDiff,
    schema_changed: bool,
    indexes_changed: bool,
    runtime_bundle_changed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DeployFunctionDiff {
    added: Vec<DeployFunctionChange>,
    changed: Vec<DeployFunctionChange>,
    removed: Vec<DeployFunctionChange>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DeployHttpRouteDiff {
    added: Vec<DeployHttpRouteChange>,
    changed: Vec<DeployHttpRouteChange>,
    removed: Vec<DeployHttpRouteChange>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DeployFunctionChange {
    name: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DeployHttpRouteChange {
    key: String,
}

impl DeployDiff {
    fn from_summaries(
        previous: Option<&ConvexRegistryDeploySummary>,
        next: &ConvexRegistryDeploySummary,
    ) -> Self {
        let empty = ConvexRegistryDeploySummary {
            functions: Vec::new(),
            http_routes: Vec::new(),
            schema_fingerprint: None,
            index_fingerprint: None,
            runtime_bundle_fingerprint: None,
        };
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
    app_dir: PathBuf,
}

impl StagedDeployArtifacts {
    fn app_dir(&self) -> &Path {
        &self.app_dir
    }
}

impl Drop for StagedDeployArtifacts {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.app_dir);
    }
}

fn stage_deploy_artifacts(artifacts: &DeployArtifacts) -> Result<StagedDeployArtifacts, Error> {
    validate_bundle_pair(artifacts)?;
    let app_dir = std::env::temp_dir().join(format!(
        "neovex-deploy-{}-{}",
        std::process::id(),
        STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let convex_dir = app_dir.join(".neovex").join("convex");
    std::fs::create_dir_all(&convex_dir).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to create deploy staging directory {}: {error}",
            convex_dir.display()
        ))
    })?;

    write_json_file(
        &convex_dir.join("functions.json"),
        &artifacts.functions_json,
    )?;
    if let Some(value) = &artifacts.http_routes_json {
        write_json_file(&convex_dir.join("http_routes.json"), value)?;
    }
    if let Some(value) = &artifacts.schema_json {
        write_json_file(&convex_dir.join("schema.json"), value)?;
    }
    if let Some(value) = &artifacts.auth_config_json {
        write_json_file(&convex_dir.join("auth.config.json"), value)?;
    }
    if let Some(bundle) = &artifacts.bundle_mjs {
        std::fs::write(convex_dir.join("bundle.mjs"), bundle).map_err(|error| {
            Error::InvalidInput(format!("failed to stage runtime bundle: {error}"))
        })?;
    }
    if let Some(hash) = &artifacts.bundle_sha256 {
        std::fs::write(convex_dir.join("bundle.sha256"), hash).map_err(|error| {
            Error::InvalidInput(format!("failed to stage runtime bundle hash: {error}"))
        })?;
    }

    Ok(StagedDeployArtifacts { app_dir })
}

fn validate_bundle_pair(artifacts: &DeployArtifacts) -> Result<(), Error> {
    match (&artifacts.bundle_mjs, &artifacts.bundle_sha256) {
        (Some(_), Some(_)) | (None, None) => Ok(()),
        (Some(_), None) => Err(Error::InvalidInput(
            "deploy artifact bundle_mjs requires bundle_sha256".to_string(),
        )),
        (None, Some(_)) => Err(Error::InvalidInput(
            "deploy artifact bundle_sha256 requires bundle_mjs".to_string(),
        )),
    }
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
