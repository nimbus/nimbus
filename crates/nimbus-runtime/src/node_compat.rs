use std::borrow::Cow;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use deno_core::FastString;
use deno_fs::sync::MaybeArc;
use deno_node::{NodeExtInitServices, NodeRequireLoader};
use deno_permissions::{OpenAccessKind, PermissionsContainer};
use deno_resolver::cache::ParsedSourceCache;
use deno_resolver::cjs::analyzer::{
    DenoAstModuleExportAnalyzer, DenoCjsCodeAnalyzer, NullNodeAnalysisCache,
};
use deno_resolver::cjs::{CjsTracker, IsCjsResolutionMode};
use deno_resolver::npm::{CreateInNpmPkgCheckerOptions, DenoInNpmPackageChecker};
use node_resolver::analyze::{CjsModuleExportAnalyzer, NodeCodeTranslator, NodeCodeTranslatorMode};
use node_resolver::cache::NodeResolutionSys;
use node_resolver::errors::{
    PackageFolderResolveError, PackageFolderResolveErrorKind, PackageNotFoundError,
};
use node_resolver::{
    DenoIsBuiltInNodeModuleChecker, InNpmPackageChecker, NodeResolution, NodeResolutionKind,
    NodeResolver, NodeResolverOptions, NpmPackageFolderResolver, PackageJsonResolver,
    ResolutionMode as NodeResolutionMode, UrlOrPathRef,
};
use sys_traits::impls::RealSys;
use url::Url;

use crate::backends::v8::embedder::{JsErrorBox, ModuleSpecifier};
use crate::runtime_capabilities::RuntimePathPolicy;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScopedInNpmPackageChecker;

impl InNpmPackageChecker for ScopedInNpmPackageChecker {
    fn in_npm_package(&self, specifier: &Url) -> bool {
        specifier
            .to_file_path()
            .ok()
            .is_some_and(|path| path_has_node_modules_segment(&path))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScopedNodeModulesResolver {
    cwd: PathBuf,
    roots: Vec<PathBuf>,
}

impl ScopedNodeModulesResolver {
    pub(crate) fn new(path_policy: &RuntimePathPolicy) -> Self {
        Self {
            cwd: path_policy.cwd().to_path_buf(),
            roots: path_policy.resolution_roots().to_vec(),
        }
    }
}

impl NpmPackageFolderResolver for ScopedNodeModulesResolver {
    fn resolve_package_folder_from_package(
        &self,
        specifier: &str,
        referrer: &UrlOrPathRef,
    ) -> Result<PathBuf, PackageFolderResolveError> {
        let package_name = package_name_from_specifier(specifier);
        let start_dir = referrer
            .path()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .and_then(canonicalize_existing_path)
            .unwrap_or_else(|| self.cwd.clone());
        for search_dir in resolution_search_directories(&start_dir, &self.roots) {
            let package_root = search_dir.join("node_modules").join(package_name);
            if package_root.is_dir() {
                return Ok(package_root);
            }
        }
        Err(PackageFolderResolveError(Box::new(
            PackageFolderResolveErrorKind::PackageNotFound(PackageNotFoundError {
                package_name: specifier.to_string(),
                referrer: referrer.display(),
                referrer_extra: Some(
                    "resolution is restricted to approved runtime roots".to_string(),
                ),
            }),
        )))
    }

    fn resolve_types_package_folder(
        &self,
        _types_package_name: &str,
        _maybe_package_version: Option<&deno_semver::Version>,
        _maybe_referrer: Option<&UrlOrPathRef>,
    ) -> Option<PathBuf> {
        None
    }
}

pub(crate) type LocalPackageJsonResolver = PackageJsonResolver<RealSys>;
pub(crate) type LocalNodeResolver = NodeResolver<
    ScopedInNpmPackageChecker,
    DenoIsBuiltInNodeModuleChecker,
    ScopedNodeModulesResolver,
    RealSys,
>;

type LocalCjsTranslator = NodeCodeTranslator<
    DenoCjsCodeAnalyzer<RealSys>,
    DenoInNpmPackageChecker,
    DenoIsBuiltInNodeModuleChecker,
    ScopedNodeModulesResolver,
    RealSys,
>;

#[derive(Debug, Clone)]
struct ScopedNodeRequireLoader {
    path_policy: RuntimePathPolicy,
    package_json_resolver: Arc<LocalPackageJsonResolver>,
}

impl ScopedNodeRequireLoader {
    fn new(
        path_policy: RuntimePathPolicy,
        package_json_resolver: Arc<LocalPackageJsonResolver>,
    ) -> Self {
        Self {
            path_policy,
            package_json_resolver,
        }
    }
}

impl NodeRequireLoader for ScopedNodeRequireLoader {
    fn ensure_read_permission<'a>(
        &self,
        permissions: &mut PermissionsContainer,
        path: Cow<'a, Path>,
    ) -> Result<Cow<'a, Path>, JsErrorBox> {
        let canonical_path = self
            .path_policy
            .ensure_module_read_path(path.as_ref())
            .map_err(|error| JsErrorBox::generic(error.to_string()))?;
        match permissions.check_open(
            Cow::Owned(canonical_path.clone()),
            OpenAccessKind::ReadNoFollow,
            Some("require()"),
        ) {
            Ok(path) => Ok(Cow::Owned(path.to_path_buf())),
            Err(_) => {
                // The compat harness stages extra modules and child-process
                // scratch files beneath approved runtime roots after the Deno
                // permission snapshot is created. Within those Nimbus-owned
                // roots, the embedder path policy is the intended source of
                // truth for CommonJS reads.
                Ok(Cow::Owned(canonical_path))
            }
        }
    }

    fn load_text_file_lossy(&self, path: &Path) -> Result<FastString, JsErrorBox> {
        let source = std::fs::read(path).map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to read runtime CommonJS module {}: {error}",
                path.display()
            ))
        })?;
        let source = String::from_utf8_lossy(&source);
        Ok(match source {
            Cow::Borrowed(text) => text.to_owned().into(),
            Cow::Owned(text) => text.into(),
        })
    }

    fn is_maybe_cjs(
        &self,
        specifier: &Url,
    ) -> Result<bool, node_resolver::errors::PackageJsonLoadError> {
        let Ok(path) = specifier.to_file_path() else {
            return Ok(false);
        };
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
        Ok(match extension.as_deref() {
            Some("cjs") | Some("cts") => true,
            Some("mjs") | Some("mts") | Some("json") => false,
            Some("js") | Some("jsx") | Some("ts") | Some("tsx") | None => {
                let package_json = self.package_json_resolver.get_closest_package_json(&path)?;
                package_json
                    .as_deref()
                    .map(|package_json| package_json.typ.as_str() != "module")
                    .unwrap_or(true)
            }
            Some(_) => false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedNodeModuleKind {
    EsModule,
    CommonJs,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedNodeTarget {
    BuiltIn {
        module_name: String,
    },
    Module {
        path: PathBuf,
        kind: ResolvedNodeModuleKind,
    },
}

pub(crate) fn build_package_json_resolver() -> Arc<LocalPackageJsonResolver> {
    Arc::new(PackageJsonResolver::new(RealSys, None))
}

pub(crate) fn build_node_resolver(
    path_policy: &RuntimePathPolicy,
    package_json_resolver: Arc<LocalPackageJsonResolver>,
) -> LocalNodeResolver {
    NodeResolver::new(
        ScopedInNpmPackageChecker,
        DenoIsBuiltInNodeModuleChecker,
        ScopedNodeModulesResolver::new(path_policy),
        package_json_resolver,
        NodeResolutionSys::new(RealSys, None),
        NodeResolverOptions::default(),
    )
}

pub(crate) fn build_node_init_services(
    path_policy: &RuntimePathPolicy,
) -> NodeExtInitServices<ScopedInNpmPackageChecker, ScopedNodeModulesResolver, RealSys> {
    let package_json_resolver = build_package_json_resolver();
    let node_resolver = build_node_resolver(path_policy, package_json_resolver.clone());
    NodeExtInitServices {
        node_require_loader: Rc::new(ScopedNodeRequireLoader::new(
            path_policy.clone(),
            package_json_resolver.clone(),
        )),
        node_resolver: MaybeArc::new(node_resolver),
        pkg_json_resolver: package_json_resolver,
        sys: RealSys,
    }
}

pub(crate) fn resolve_node_target(
    path_policy: &RuntimePathPolicy,
    specifier: &str,
    referrer: &str,
    resolution_mode: NodeResolutionMode,
) -> Result<ResolvedNodeTarget, JsErrorBox> {
    let package_json_resolver = build_package_json_resolver();
    let node_resolver = build_node_resolver(path_policy, package_json_resolver.clone());
    let referrer_url = normalize_referrer(referrer)?;
    let resolved = match node_resolver.resolve(
        specifier,
        &referrer_url,
        resolution_mode,
        NodeResolutionKind::Execution,
    ) {
        Ok(resolved) => resolved,
        Err(error) => {
            if let Some(resolved) = try_resolve_package_subpath_without_exports(
                path_policy,
                specifier,
                &referrer_url,
                package_json_resolver.as_ref(),
            )? {
                return Ok(resolved);
            }
            return Err(JsErrorBox::generic(format!(
                "failed to resolve runtime module `{specifier}` from `{referrer}`: {error}"
            )));
        }
    };
    match resolved {
        NodeResolution::BuiltIn(module_name) => Ok(ResolvedNodeTarget::BuiltIn { module_name }),
        NodeResolution::Module(url_or_path) => {
            let path = url_or_path.into_path().map_err(|error| {
                JsErrorBox::generic(format!(
                    "resolved runtime module is not a valid file path: {error}"
                ))
            })?;
            let kind = classify_resolved_module_kind(&path, package_json_resolver.as_ref())?;
            Ok(ResolvedNodeTarget::Module { path, kind })
        }
    }
}

pub(crate) async fn translate_commonjs_to_esm(
    path_policy: &RuntimePathPolicy,
    specifier: &ModuleSpecifier,
    source: &str,
) -> Result<String, JsErrorBox> {
    let package_json_resolver = build_package_json_resolver();
    let in_npm_package_checker = DenoInNpmPackageChecker::new(CreateInNpmPkgCheckerOptions::Byonm);
    let node_resolver = Arc::new(NodeResolver::new(
        in_npm_package_checker.clone(),
        DenoIsBuiltInNodeModuleChecker,
        ScopedNodeModulesResolver::new(path_policy),
        package_json_resolver.clone(),
        NodeResolutionSys::new(RealSys, None),
        NodeResolverOptions::default(),
    ));
    let cjs_tracker = Arc::new(CjsTracker::new(
        in_npm_package_checker.clone(),
        package_json_resolver.clone(),
        IsCjsResolutionMode::ImplicitTypeCommonJs,
        Vec::new(),
    ));
    let parsed_source_cache = Arc::new(ParsedSourceCache::default());
    let module_export_analyzer = Arc::new(DenoAstModuleExportAnalyzer::new(parsed_source_cache));
    let cjs_code_analyzer = DenoCjsCodeAnalyzer::new(
        Arc::new(NullNodeAnalysisCache),
        cjs_tracker,
        module_export_analyzer,
        RealSys,
    );
    let translator = LocalCjsTranslator::new(
        Arc::new(CjsModuleExportAnalyzer::new(
            cjs_code_analyzer,
            in_npm_package_checker,
            node_resolver,
            ScopedNodeModulesResolver::new(path_policy),
            package_json_resolver,
            RealSys,
        )),
        NodeCodeTranslatorMode::ModuleLoader,
    );
    translator
        .translate_cjs_to_esm(specifier, Some(Cow::Borrowed(source)))
        .await
        .map(Cow::into_owned)
        .map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to translate runtime CommonJS module {specifier}: {error}"
            ))
        })
}

fn try_resolve_package_subpath_without_exports(
    path_policy: &RuntimePathPolicy,
    specifier: &str,
    referrer: &Url,
    package_json_resolver: &LocalPackageJsonResolver,
) -> Result<Option<ResolvedNodeTarget>, JsErrorBox> {
    let Some((package_name, package_subpath)) = split_package_specifier(specifier) else {
        return Ok(None);
    };
    let Some(search_start) = referrer
        .to_file_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .and_then(canonicalize_existing_path)
    else {
        return Ok(None);
    };
    for search_dir in resolution_search_directories(&search_start, path_policy.resolution_roots()) {
        let package_root = search_dir.join("node_modules").join(package_name);
        if !package_root.is_dir() {
            continue;
        }

        let package_json = package_json_resolver
            .load_package_json(&package_root.join("package.json"))
            .map_err(|error| {
                JsErrorBox::generic(format!(
                    "failed to load runtime package metadata for {}: {error}",
                    package_root.display()
                ))
            })?;
        if package_json
            .as_deref()
            .and_then(|package_json| package_json.exports.as_ref())
            .is_some()
        {
            return Ok(None);
        }

        let candidate = package_root.join(package_subpath);
        if !candidate.is_file() {
            continue;
        }

        let path = path_policy
            .ensure_module_read_path(&candidate)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?;
        let kind = classify_resolved_module_kind(&path, package_json_resolver)?;
        return Ok(Some(ResolvedNodeTarget::Module { path, kind }));
    }
    Ok(None)
}

pub(crate) fn classify_resolved_module_kind(
    path: &Path,
    package_json_resolver: &LocalPackageJsonResolver,
) -> Result<ResolvedNodeModuleKind, JsErrorBox> {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Ok(ResolvedNodeModuleKind::Json),
        Some("cjs") | Some("cts") => Ok(ResolvedNodeModuleKind::CommonJs),
        Some("mjs") | Some("mts") => Ok(ResolvedNodeModuleKind::EsModule),
        Some("js") | Some("jsx") | Some("ts") | Some("tsx") | None => {
            let package_json = package_json_resolver
                .get_closest_package_json(path)
                .map_err(|error| {
                    JsErrorBox::generic(format!(
                        "failed to load runtime package metadata for {}: {error}",
                        path.display()
                    ))
                })?;
            let package_type = package_json
                .as_deref()
                .map(|package_json| package_json.typ.as_str())
                .unwrap_or("none");
            if package_type == "module" {
                Ok(ResolvedNodeModuleKind::EsModule)
            } else {
                Ok(ResolvedNodeModuleKind::CommonJs)
            }
        }
        Some(other) => Err(JsErrorBox::generic(format!(
            "unsupported runtime module extension `.{other}` for {}",
            path.display()
        ))),
    }
}

fn normalize_referrer(referrer: &str) -> Result<Url, JsErrorBox> {
    if let Ok(url) = Url::parse(referrer) {
        return Ok(url);
    }
    let path = PathBuf::from(referrer);
    Url::from_file_path(&path)
        .map_err(|_| JsErrorBox::generic(format!("invalid runtime referrer `{referrer}`")))
}

pub(crate) fn resolution_search_directories(start_dir: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut current = Some(start_dir);
    while let Some(path) = current {
        if roots.iter().any(|root| path.starts_with(root))
            && directories.iter().all(|existing| existing != path)
        {
            directories.push(path.to_path_buf());
        }
        current = path.parent();
    }
    for root in roots {
        if directories.iter().all(|existing| existing != root) {
            directories.push(root.clone());
        }
    }
    directories
}

pub(crate) fn path_has_node_modules_segment(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(part) if part == "node_modules"))
}

fn canonicalize_existing_path(path: PathBuf) -> Option<PathBuf> {
    std::fs::canonicalize(&path).ok().or(Some(path))
}

fn split_package_specifier(specifier: &str) -> Option<(&str, &str)> {
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with("node:")
    {
        return None;
    }

    if let Some(stripped) = specifier.strip_prefix('@') {
        let mut segments = stripped.splitn(3, '/');
        let scope = segments.next()?;
        let package = segments.next()?;
        let subpath = segments.next()?;
        return Some((&specifier[..scope.len() + package.len() + 2], subpath));
    }

    let (package_name, subpath) = specifier.split_once('/')?;
    Some((package_name, subpath))
}

fn package_name_from_specifier(specifier: &str) -> &str {
    split_package_specifier(specifier)
        .map(|(package_name, _)| package_name)
        .unwrap_or(specifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::RuntimeLimits;
    use crate::runtime::RuntimeBundle;

    #[test]
    fn split_package_specifier_handles_scoped_and_unscoped_subpaths() {
        assert_eq!(
            split_package_specifier("@esbuild/darwin-arm64/bin/esbuild"),
            Some(("@esbuild/darwin-arm64", "bin/esbuild"))
        );
        assert_eq!(
            split_package_specifier("esbuild/lib/main.js"),
            Some(("esbuild", "lib/main.js"))
        );
        assert_eq!(split_package_specifier("esbuild"), None);
        assert_eq!(split_package_specifier("./local.js"), None);
    }

    #[test]
    fn package_name_from_specifier_strips_package_subpaths() {
        assert_eq!(
            package_name_from_specifier("@esbuild/darwin-arm64/bin/esbuild"),
            "@esbuild/darwin-arm64"
        );
        assert_eq!(package_name_from_specifier("es-errors/type"), "es-errors");
        assert_eq!(package_name_from_specifier("@scope/pkg"), "@scope/pkg");
        assert_eq!(package_name_from_specifier("express"), "express");
    }

    #[test]
    fn resolve_node_target_allows_direct_package_subpaths_when_package_has_no_exports() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let functions_root = app_root.join("functions");
        let referrer_dir = functions_root.join("node_modules/esbuild/lib");
        let package_root = functions_root.join("node_modules/@esbuild/darwin-arm64");
        std::fs::create_dir_all(&referrer_dir).expect("referrer dir should build");
        std::fs::create_dir_all(package_root.join("bin")).expect("package bin dir should build");
        std::fs::write(
            package_root.join("package.json"),
            r#"{"name":"@esbuild/darwin-arm64"}"#,
        )
        .expect("package manifest should write");
        std::fs::write(package_root.join("bin/esbuild"), "#!/bin/sh\n")
            .expect("binary should write");

        let bundle_path = app_root.join(".nimbus-codegen-test.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);
        let policy = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::tooling_node22())
            .expect("policy should build");

        let resolved = resolve_node_target(
            &policy,
            "@esbuild/darwin-arm64/bin/esbuild",
            &referrer_dir.join("main.js").display().to_string(),
            node_resolver::ResolutionMode::Require,
        )
        .expect("package subpath should resolve");

        assert_eq!(
            resolved,
            ResolvedNodeTarget::Module {
                path: package_root
                    .join("bin/esbuild")
                    .canonicalize()
                    .expect("resolved binary path should canonicalize"),
                kind: ResolvedNodeModuleKind::CommonJs,
            }
        );
    }
}
