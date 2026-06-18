use nimbus_code_index::{ModuleAnalysis, analyze_module};
use serde::{Deserialize, Serialize};

use super::*;
use nimbus_system::{
    DiskSourcePackageStore, read_module_source_async, read_source_package_modules_async,
};

#[derive(Debug, Deserialize)]
pub(crate) struct ModuleSourceParams {
    module: String,
}

/// A reverse call edge: a function in this module (`target`) is called from
/// `caller` (a `module:export` path elsewhere in the deployment).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct CalledByEdge {
    target: String,
    caller: String,
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
    /// Cross-module reverse call edges: which functions elsewhere call into this
    /// module's functions ("called by"). Built from the whole package (FSV7).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    called_by: Vec<CalledByEdge>,
    /// Per-identifier type hints captured at deploy by the TS compiler (FSV8),
    /// carried in the source package. Absent when the deploy had no toolchain.
    #[serde(skip_serializing_if = "Option::is_none")]
    type_info: Option<serde_json::Value>,
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
    let package_modules =
        read_source_package_modules_async(&state.engine, &store, &params.module).await?;
    let called_by = compute_called_by(&params.module, &package_modules);
    Ok(Json(ModuleSourceResponse {
        module: resolved.path,
        digest: resolved.digest,
        source: resolved.source,
        source_map: resolved.source_map,
        analysis,
        called_by,
        type_info: resolved.type_info,
    }))
}

/// Build the reverse call edges into `requested` by analyzing every module in
/// the package: each `api.*`/`internal.*` reference is attributed to the
/// enclosing exported function (the latest export declared at or above the
/// reference's line), giving a `caller` of `callerModule:export`.
fn compute_called_by(requested: &str, modules: &[(String, String)]) -> Vec<CalledByEdge> {
    let mut edges = Vec::new();
    for (caller_module, source) in modules {
        let analysis = analyze_module(source);
        let mut exports = analysis.exports;
        exports.sort_by_key(|export| export.line);
        for reference in &analysis.references {
            let Some((target_module, target_fn)) = reference.target.split_once(':') else {
                continue;
            };
            if target_module != requested {
                continue;
            }
            let Some(caller_fn) = exports
                .iter()
                .rev()
                .find(|export| export.line <= reference.line)
                .map(|export| export.name.as_str())
            else {
                continue;
            };
            edges.push(CalledByEdge {
                target: target_fn.to_owned(),
                caller: format!("{caller_module}:{caller_fn}"),
            });
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn called_by_attributes_references_to_enclosing_export() {
        let notifications = (
            "notifications".to_owned(),
            r#"import { api, internal } from "./_generated/api";
import { mutation } from "./_generated/server";

export const announce = mutation({
  handler: async (ctx) => {
    await ctx.runMutation(internal.users.touch, {});
    await ctx.runQuery(api.messages.list, {});
  },
});
"#
            .to_owned(),
        );
        let edges = compute_called_by("messages", std::slice::from_ref(&notifications));
        assert!(
            edges
                .iter()
                .any(|e| e.target == "list" && e.caller == "notifications:announce"),
            "expected messages:list called by notifications:announce, got {edges:?}"
        );
        // A reference to a different module is not attributed to `messages`.
        assert!(edges.iter().all(|e| e.caller == "notifications:announce"));
    }

    #[test]
    fn called_by_is_empty_when_nothing_references_the_module() {
        let other = ("other".to_owned(), "export const noop = 1;\n".to_owned());
        assert!(compute_called_by("messages", std::slice::from_ref(&other)).is_empty());
    }
}
