use nimbus_code_index::{ModuleAnalysis, analyze_module};
use serde::{Deserialize, Serialize};

use super::*;
use nimbus_system::{DiskSourcePackageStore, read_module_source_async};

#[derive(Debug, Deserialize)]
pub(crate) struct ModuleSourceParams {
    module: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModuleSourceResponse {
    module: String,
    digest: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_map: Option<String>,
    /// Structural code-navigation index (oxc): exported functions, imports, and
    /// `api.*`/`internal.*` references. Derived on read from the stored source —
    /// no second copy. See FSV7.
    analysis: ModuleAnalysis,
}

/// Serve a deployed module's source from the content-addressed source-package
/// store (hash-verified by the store). Backs the console Source view (FSV4):
/// `GET /api/console/source?module=<path>`.
pub(crate) async fn module_source(
    State(state): State<Arc<AppState>>,
    QueryParams(params): QueryParams<ModuleSourceParams>,
) -> Result<Json<ModuleSourceResponse>, AppError> {
    let store = DiskSourcePackageStore::new(state.engine.data_dir().join("source-packages"));
    let Some(resolved) = read_module_source_async(&state.engine, &store, &params.module).await?
    else {
        return Err(AppError::not_found(format!(
            "no source for module '{}'",
            params.module
        )));
    };
    let analysis = analyze_module(&resolved.source);
    Ok(Json(ModuleSourceResponse {
        module: resolved.path,
        digest: resolved.digest,
        source: resolved.source,
        source_map: resolved.source_map,
        analysis,
    }))
}
