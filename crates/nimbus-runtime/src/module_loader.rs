use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use base64::DecodeError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use crate::backends::v8::embedder::{
    JsErrorBox, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, RequestedModuleType,
    ResolutionKind, SourceCodeCacheInfo, resolve_import, v8,
};
use crate::limits::{RuntimeCompatibilityTarget, RuntimeGuestSemantics};
use crate::node_compat::{
    ResolvedNodeModuleKind, ResolvedNodeTarget, build_package_json_resolver,
    classify_resolved_module_kind, resolve_node_target_with_conditions,
    resolve_node_target_with_user_conditions, translate_commonjs_to_esm,
};
use crate::runtime_capabilities::RuntimePathPolicy;
use deno_node::ops::module_hooks::LoaderHookRegistry;
use twox_hash::XxHash64;

mod code_cache;
mod embedded_builtins;

#[derive(Debug, thiserror::Error, deno_error::JsError)]
enum NodeModuleLoadError {
    #[error("Unknown module format: {mime_type} for URL {url}")]
    #[class(generic)]
    #[property("code" = "ERR_UNKNOWN_MODULE_FORMAT")]
    UnknownModuleFormat { mime_type: String, url: String },

    #[error("Module \"{url}\" needs an import attribute of type \"json\"")]
    #[class(generic)]
    #[property("code" = "ERR_IMPORT_ATTRIBUTE_MISSING")]
    ImportAttributeMissing { url: String },

    #[error("Import attribute \"type\" with value \"json\" is incompatible with module \"{url}\"")]
    #[class(generic)]
    #[property("code" = "ERR_IMPORT_ATTRIBUTE_TYPE_INCOMPATIBLE")]
    ImportAttributeTypeIncompatible { url: String },

    #[error("Import attribute \"type\" with value \"{requested}\" is not supported")]
    #[class(generic)]
    #[property("code" = "ERR_IMPORT_ATTRIBUTE_UNSUPPORTED")]
    ImportAttributeUnsupported { requested: String },

    #[error("No such built-in module: {specifier}")]
    #[class(generic)]
    #[property("code" = "ERR_UNKNOWN_BUILTIN_MODULE")]
    UnknownBuiltinModule { specifier: String },

    #[error(
        "Only URLs with a scheme in: file, data, and node are supported by the default ESM loader. Received protocol '{scheme}:'"
    )]
    #[class(generic)]
    #[property("code" = "ERR_UNSUPPORTED_ESM_URL_SCHEME")]
    UnsupportedEsmUrlScheme { scheme: String },
}

#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[error("Cannot find module '{url}'")]
#[class(generic)]
#[property("code" = "ERR_MODULE_NOT_FOUND")]
#[property("url" = self.url.clone())]
struct NodeModuleNotFoundError {
    url: String,
}

#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[error("Directory import '{url}' is not supported resolving ES modules")]
#[class(generic)]
#[property("code" = "ERR_UNSUPPORTED_DIR_IMPORT")]
#[property("url" = self.url.clone())]
struct NodeUnsupportedDirImportError {
    url: String,
}

pub(crate) use code_cache::BundleModuleCodeCache;
use embedded_builtins::{
    INTERNAL_READLINE_UTILS_SPECIFIER, NIMBUS_INTERNAL_READLINE_UTILS_SPECIFIER,
    NIMBUS_NODE_FS_PROMISES_SPECIFIER, NIMBUS_NODE_FS_SPECIFIER, NIMBUS_NODE_MODULE_SPECIFIER,
    NODE_ASYNC_HOOKS_SPECIFIER, NODE_FS_PROMISES_SPECIFIER, NODE_FS_SPECIFIER,
    NODE_MODULE_SPECIFIER, source_for_supported_node_builtin,
    source_for_supported_web_guest_builtin, supports_extension_backed_node_builtin,
};

#[derive(Clone)]
pub struct RestrictedModuleLoader {
    path_policy: RuntimePathPolicy,
    compatibility_target: RuntimeCompatibilityTarget,
    guest_semantics: RuntimeGuestSemantics,
    node_conditions: Vec<String>,
    code_cache: Arc<BundleModuleCodeCache>,
    loader_hook_registry: Option<LoaderHookRegistry>,
}

impl RestrictedModuleLoader {
    pub fn new(
        path_policy: RuntimePathPolicy,
        compatibility_target: RuntimeCompatibilityTarget,
        guest_semantics: RuntimeGuestSemantics,
        node_conditions: Vec<String>,
        code_cache: Arc<BundleModuleCodeCache>,
        loader_hook_registry: Option<LoaderHookRegistry>,
    ) -> Self {
        let loader = Self {
            path_policy,
            compatibility_target,
            guest_semantics,
            node_conditions,
            code_cache,
            loader_hook_registry,
        };
        if let Some(registry) = loader.loader_hook_registry.clone() {
            let loader_for_default_resolve = loader.clone();
            registry.set_default_resolve(Rc::new(move |specifier, referrer| {
                loader_for_default_resolve
                    .resolve_unhooked(specifier, referrer, ResolutionKind::Import)
                    .map(|specifier| specifier.to_string())
            }));
        }
        loader
    }

    fn unsupported_node_builtin_error(&self, specifier: &str) -> JsErrorBox {
        let reason = match self.compatibility_target {
            RuntimeCompatibilityTarget::WebStandardIsolate => {
                "node: imports are unavailable under RuntimeCompatibilityTarget::WebStandardIsolate"
            }
            RuntimeCompatibilityTarget::BunJsc => {
                "Bun/JSC package resolution is owned by the Bun/JSC backend and is unavailable through the V8 module loader"
            }
            RuntimeCompatibilityTarget::WasmComponent => {
                "WASM components do not use the V8 JavaScript module loader"
            }
            RuntimeCompatibilityTarget::Node20
            | RuntimeCompatibilityTarget::Node22
            | RuntimeCompatibilityTarget::Node24
            | RuntimeCompatibilityTarget::Node26 => match specifier {
                "node:inspector" => {
                    "node:inspector requires a service/microVM route or an explicit local-development inspector grant; production in-process Node profiles do not expose inspector authority"
                }
                "node:repl" => {
                    "node:repl requires an interactive host process and is service/microVM-routed; production in-process Node profiles do not expose REPL authority"
                }
                _ => {
                    return JsErrorBox::from_err(NodeModuleLoadError::UnknownBuiltinModule {
                        specifier: specifier.to_string(),
                    });
                }
            },
        };
        JsErrorBox::generic(format!(
            "unsupported runtime module import {specifier}: {reason}"
        ))
    }

    fn ensure_allowed_specifier(&self, specifier: &ModuleSpecifier) -> Result<(), JsErrorBox> {
        if self
            .supported_node_builtin_source(specifier.as_str())
            .is_some()
        {
            return Ok(());
        }
        if specifier.scheme() == "ext" {
            return Ok(());
        }
        if specifier.scheme() == "data" && self.compatibility_target.is_node() {
            return Ok(());
        }
        if specifier.scheme() != "file" {
            if self.compatibility_target.is_node() {
                return Err(JsErrorBox::from_err(
                    NodeModuleLoadError::UnsupportedEsmUrlScheme {
                        scheme: specifier.scheme().to_string(),
                    },
                ));
            }
            return Err(JsErrorBox::generic(format!(
                "runtime bundle imports must stay within approved runtime roots, unsupported scheme: {}",
                specifier.scheme()
            )));
        }

        let path = specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {specifier}"))
        })?;
        self.path_policy
            .ensure_module_read_path(&path)
            .map(|_| ())
            .map_err(|error| JsErrorBox::generic(error.to_string()))
    }

    async fn load_module_source(
        &self,
        module_specifier: &ModuleSpecifier,
        options: ModuleLoadOptions,
        resolved_hook_format: Option<&str>,
    ) -> Result<ModuleSource, JsErrorBox> {
        if let Some(source) = self.supported_node_builtin_source(module_specifier.as_str()) {
            return Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::Bytes(source.as_bytes().to_vec().into_boxed_slice().into()),
                module_specifier,
                None,
            ));
        }
        if module_specifier.scheme() == "data" && self.compatibility_target.is_node() {
            return load_data_url_module_source(module_specifier, options, resolved_hook_format);
        }
        let path = module_specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {module_specifier}"))
        })?;
        let metadata = std::fs::metadata(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                JsErrorBox::from_err(NodeModuleNotFoundError {
                    url: module_specifier.to_string(),
                })
            } else {
                JsErrorBox::generic(format!(
                    "failed to inspect runtime bundle module {}: {source}",
                    path.display()
                ))
            }
        })?;
        if metadata.is_dir() {
            return Err(JsErrorBox::from_err(NodeUnsupportedDirImportError {
                url: module_specifier.to_string(),
            }));
        }
        let module_type = module_type_from_path(&path, &options, resolved_hook_format)?;
        let mut code = std::fs::read(&path).map_err(|source| {
            JsErrorBox::generic(format!(
                "failed to load runtime bundle module {}: {source}",
                path.display()
            ))
        })?;
        if module_type == ModuleType::JavaScript && self.compatibility_target.is_node() {
            let package_json_resolver = build_package_json_resolver();
            if classify_resolved_module_kind(&path, package_json_resolver.as_ref())?
                == ResolvedNodeModuleKind::CommonJs
            {
                let source = String::from_utf8(code).map_err(|error| {
                    JsErrorBox::generic(format!(
                        "failed to decode runtime CommonJS module {} as utf8: {error}",
                        path.display()
                    ))
                })?;
                code = translate_commonjs_to_esm(
                    &self.path_policy,
                    module_specifier,
                    &source,
                    self.compatibility_target,
                )
                .await?
                .into_bytes();
            }
        }
        let hash = hash_module_source_bytes(&code);
        let code_cache = Some(SourceCodeCacheInfo {
            hash,
            data: self.code_cache.lookup(module_specifier, hash),
        });
        Ok(ModuleSource::new(
            module_type,
            ModuleSourceCode::Bytes(code.into_boxed_slice().into()),
            module_specifier,
            code_cache,
        ))
    }

    fn supported_node_builtin_source(&self, specifier: &str) -> Option<&'static str> {
        if let Some(source) = source_for_supported_web_guest_builtin(
            specifier,
            self.compatibility_target,
            self.guest_semantics,
        ) {
            return Some(source);
        }
        source_for_supported_node_builtin(specifier, self.compatibility_target.is_node())
    }

    fn supports_extension_backed_node_builtin(&self, specifier: &str) -> bool {
        supports_extension_backed_node_builtin(specifier, self.compatibility_target.is_node())
    }

    fn resolve_bare_package_specifier_with_conditions(
        &self,
        specifier: &str,
        referrer: &str,
        conditions: Option<Vec<String>>,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if is_tenant_bundle_operator_only_specifier(specifier) {
            return Err(JsErrorBox::generic(format!(
                "tenant bundle admission rejected operator-only Nimbus transport import {specifier}; use the high-level @nimbus/nimbus SDK with workload identity"
            )));
        }
        let resolved = match conditions {
            Some(conditions) => resolve_node_target_with_conditions(
                &self.path_policy,
                specifier,
                referrer,
                node_resolver::ResolutionMode::Import,
                Some(conditions),
            )?,
            None => resolve_node_target_with_user_conditions(
                &self.path_policy,
                specifier,
                referrer,
                node_resolver::ResolutionMode::Import,
                &self.node_conditions,
            )?,
        };
        match resolved {
            ResolvedNodeTarget::BuiltIn { module_name } => {
                ModuleSpecifier::parse(&format!("node:{module_name}")).map_err(JsErrorBox::from_err)
            }
            ResolvedNodeTarget::Module { path, .. } => {
                let resolved = self
                    .path_policy
                    .ensure_module_read_path(&path)
                    .map_err(|error| JsErrorBox::generic(error.to_string()))?;
                ModuleSpecifier::from_file_path(&resolved).map_err(|_| {
                    JsErrorBox::generic(format!(
                        "resolved runtime package entry is not a valid file URL: {}",
                        resolved.display()
                    ))
                })
            }
        }
    }

    fn resolve_unhooked(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        self.resolve_unhooked_with_conditions(specifier, referrer, kind, None)
    }

    fn resolve_unhooked_with_conditions(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
        conditions: Option<Vec<String>>,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier.starts_with("node:") {
            if specifier == NODE_FS_SPECIFIER {
                return ModuleSpecifier::parse(NIMBUS_NODE_FS_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if specifier == NODE_FS_PROMISES_SPECIFIER {
                return ModuleSpecifier::parse(NIMBUS_NODE_FS_PROMISES_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if specifier == NODE_MODULE_SPECIFIER {
                return ModuleSpecifier::parse(NIMBUS_NODE_MODULE_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if specifier == "node:repl" {
                return Err(self.unsupported_node_builtin_error(specifier));
            }
            if self.supported_node_builtin_source(specifier).is_some()
                || self.supports_extension_backed_node_builtin(specifier)
            {
                return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
            }
            return Err(self.unsupported_node_builtin_error(specifier));
        }
        if specifier == INTERNAL_READLINE_UTILS_SPECIFIER {
            return ModuleSpecifier::parse(NIMBUS_INTERNAL_READLINE_UTILS_SPECIFIER)
                .map_err(JsErrorBox::from_err);
        }
        // Convex default-runtime lanes accept the bare `async_hooks` form the
        // upstream docs use; it aliases the node:async_hooks web builtin.
        if specifier == "async_hooks"
            && source_for_supported_web_guest_builtin(
                NODE_ASYNC_HOOKS_SPECIFIER,
                self.compatibility_target,
                self.guest_semantics,
            )
            .is_some()
        {
            return ModuleSpecifier::parse(NODE_ASYNC_HOOKS_SPECIFIER)
                .map_err(JsErrorBox::from_err);
        }
        if is_bare_package_specifier(specifier) {
            return self
                .resolve_bare_package_specifier_with_conditions(specifier, referrer, conditions);
        }
        let resolved = normalize_file_module_specifier(
            resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?,
        )?;
        match kind {
            ResolutionKind::MainModule | ResolutionKind::Import | ResolutionKind::DynamicImport => {
                self.ensure_allowed_specifier(&resolved)?
            }
        }
        Ok(resolved)
    }

    fn is_commonjs_module(&self, module_specifier: &ModuleSpecifier) -> bool {
        let Ok(path) = module_specifier.to_file_path() else {
            return false;
        };
        classify_resolved_module_kind(&path, build_package_json_resolver().as_ref())
            .is_ok_and(|kind| kind == ResolvedNodeModuleKind::CommonJs)
    }

    fn should_wrap_hook_commonjs_source(
        &self,
        module_specifier: &ModuleSpecifier,
        hook_format: Option<&str>,
    ) -> bool {
        match hook_format {
            Some("commonjs") => true,
            Some(_) => false,
            None => {
                self.compatibility_target.is_node() && self.is_commonjs_module(module_specifier)
            }
        }
    }

    fn is_synthetic_commonjs_wrapper_import(&self, specifier: &str, referrer: &str) -> bool {
        if specifier != NODE_MODULE_SPECIFIER {
            return false;
        }
        ModuleSpecifier::parse(referrer).is_ok_and(|referrer| self.is_commonjs_module(&referrer))
    }
}

impl ModuleLoader for RestrictedModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        self.resolve_unhooked(specifier, referrer, kind)
    }

    fn resolve_with_scope(
        &self,
        scope: &mut v8::PinScope,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
        import_attributes: &HashMap<String, String>,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if !self.is_synthetic_commonjs_wrapper_import(specifier, referrer)
            && let Some(registry) = &self.loader_hook_registry
            && let Some(url) = registry.resolve(scope, specifier, referrer, import_attributes)?
        {
            registry.record_resolved_attributes(&url, import_attributes);
            return ModuleSpecifier::parse(&url).map_err(JsErrorBox::from_err);
        }
        let resolved = self.resolve_unhooked(specifier, referrer, kind)?;
        if let Some(registry) = &self.loader_hook_registry {
            registry.record_resolved_attributes(resolved.as_str(), import_attributes);
        }
        Ok(resolved)
    }

    fn import_meta_resolve(
        &self,
        specifier: &str,
        referrer: &str,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        self.resolve_unhooked(specifier, referrer, ResolutionKind::DynamicImport)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let import_attributes = self
            .loader_hook_registry
            .as_ref()
            .map(|registry| {
                let recorded = registry.take_resolved_attributes(module_specifier.as_str());
                if recorded.is_empty() {
                    import_attributes_from_requested_module_type(&options.requested_module_type)
                } else {
                    recorded
                }
            })
            .unwrap_or_else(|| {
                import_attributes_from_requested_module_type(&options.requested_module_type)
            });
        if let Some(registry) = &self.loader_hook_registry
            && registry.load_active.get()
            && !options.is_synchronous
        {
            let receiver = registry.push_load(module_specifier.to_string(), import_attributes);
            let loader = self.clone();
            let module_specifier = module_specifier.clone();
            return ModuleLoadResponse::Async(Box::pin(async move {
                match receiver.await {
                    Ok(Ok((_, Some(format), _)))
                        if format == "builtin" && module_specifier.scheme() == "node" =>
                    {
                        Ok(ModuleSource::new(
                            ModuleType::JavaScript,
                            ModuleSourceCode::String(String::new().into()),
                            &module_specifier,
                            None,
                        ))
                    }
                    Ok(Ok((Some(source), format, _))) => {
                        let source = if loader
                            .should_wrap_hook_commonjs_source(&module_specifier, format.as_deref())
                        {
                            wrap_hook_commonjs_source(&module_specifier, &source)?
                        } else {
                            source
                        };
                        Ok(ModuleSource::new(
                            module_type_from_hook_format(format.as_deref()),
                            ModuleSourceCode::String(source.into()),
                            &module_specifier,
                            None,
                        ))
                    }
                    Ok(Ok((None, format, effective_url))) => {
                        let module_specifier = effective_url
                            .as_deref()
                            .map(ModuleSpecifier::parse)
                            .transpose()
                            .map_err(JsErrorBox::from_err)?
                            .unwrap_or(module_specifier);
                        loader
                            .load_module_source(&module_specifier, options, format.as_deref())
                            .await
                    }
                    Ok(Err(error)) => Err(JsErrorBox::generic(error)),
                    Err(_) => Err(JsErrorBox::generic("module load hook cancelled")),
                }
            }));
        }
        if let Err(error) = self.ensure_allowed_specifier(module_specifier) {
            return ModuleLoadResponse::Sync(Err(error));
        }
        ModuleLoadResponse::Async(Box::pin({
            let loader = self.clone();
            let module_specifier = module_specifier.clone();
            async move {
                loader
                    .load_module_source(&module_specifier, options, None)
                    .await
            }
        }))
    }

    fn code_cache_ready(
        &self,
        module_specifier: ModuleSpecifier,
        hash: u64,
        code_cache: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = ()>>> {
        self.code_cache.store(module_specifier, hash, code_cache);
        Box::pin(async {})
    }

    fn purge_and_prevent_code_cache(&self, module_specifier: &str) {
        self.code_cache.purge_and_prevent(module_specifier);
    }

    fn should_load_synthetic_esm(&self, specifier: &str) -> bool {
        self.loader_hook_registry
            .as_ref()
            .is_some_and(|registry| registry.load_active.get() && specifier.starts_with("node:"))
    }

    fn pump_event_loop_during_load(&self) -> bool {
        self.loader_hook_registry
            .as_ref()
            .is_some_and(|registry| registry.load_active.get() || registry.resolve_active())
    }
}

fn import_attributes_from_requested_module_type(
    requested_module_type: &RequestedModuleType,
) -> HashMap<String, String> {
    let Some(module_type) = requested_module_type.as_str() else {
        return HashMap::new();
    };
    HashMap::from([("type".to_string(), module_type.to_string())])
}

fn module_type_from_hook_format(format: Option<&str>) -> ModuleType {
    match format {
        Some("json") => ModuleType::Json,
        Some("wasm") => ModuleType::Wasm,
        Some("module") | Some("commonjs") | None => ModuleType::JavaScript,
        Some(other) => ModuleType::Other(Cow::Owned(other.to_string())),
    }
}

fn wrap_hook_commonjs_source(
    module_specifier: &ModuleSpecifier,
    source: &str,
) -> Result<String, JsErrorBox> {
    let file_path = module_specifier.to_file_path().ok();
    let filename = file_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| module_specifier.to_string());
    let dirname = file_path
        .as_deref()
        .and_then(Path::parent)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename_literal = js_string_literal(&filename)?;
    let dirname_literal = js_string_literal(&dirname)?;
    let mut wrapped = format!(
        r#"const __nimbusCjsModuleConstructor =
  globalThis.process?.getBuiltinModule?.("module")?.Module;
if (typeof __nimbusCjsModuleConstructor !== "function") {{
  throw new Error("Nimbus CommonJS hook wrapper requires node:module");
}}
const __nimbusCjsModule = {{ exports: {{}} }};
const __nimbusCjsFilename = {filename_literal};
const __nimbusCjsDirname = {dirname_literal};
const __nimbusCjsRuntimeModule = new __nimbusCjsModuleConstructor(__nimbusCjsFilename);
__nimbusCjsRuntimeModule.filename = __nimbusCjsFilename;
__nimbusCjsRuntimeModule.paths =
  typeof __nimbusCjsModuleConstructor._nodeModulePaths === "function" &&
    __nimbusCjsDirname !== ""
    ? __nimbusCjsModuleConstructor._nodeModulePaths(__nimbusCjsDirname)
    : [];
const __nimbusCjsRequire = function require(specifier) {{
  return __nimbusCjsRuntimeModule.require(specifier);
}};
__nimbusCjsRequire.resolve = function resolve(request, options) {{
  return __nimbusCjsModuleConstructor._resolveFilename(
    request,
    __nimbusCjsRuntimeModule,
    false,
    options == null ? {{}} : {{ __proto__: null, ...options }},
  );
}};
__nimbusCjsRequire.resolve.paths = function paths(request) {{
  return __nimbusCjsModuleConstructor._resolveLookupPaths(
    request,
    __nimbusCjsRuntimeModule,
  );
}};
__nimbusCjsRequire.extensions = __nimbusCjsModuleConstructor._extensions;
__nimbusCjsRequire.cache = __nimbusCjsModuleConstructor._cache;
Object.defineProperty(__nimbusCjsRequire, "main", {{
  configurable: true,
  enumerable: true,
  value: globalThis.process?.mainModule,
  writable: true,
}});
(function (exports, require, module, __filename, __dirname) {{
"#
    );
    wrapped.push_str(source);
    if !source.ends_with('\n') {
        wrapped.push('\n');
    }
    wrapped.push_str(
        r#"}).call(
  __nimbusCjsModule.exports,
  __nimbusCjsModule.exports,
  __nimbusCjsRequire,
  __nimbusCjsModule,
  __nimbusCjsFilename,
  __nimbusCjsDirname
);
const __nimbusCjsDefault = __nimbusCjsModule.exports;
export default __nimbusCjsDefault;
"#,
    );
    for export_name in collect_commonjs_named_exports(source) {
        let export_literal = js_string_literal(&export_name)?;
        let binding_name = commonjs_named_export_binding(&export_name);
        wrapped.push_str(&format!(
            "const {binding_name} = __nimbusCjsDefault[{export_literal}];\n\
             export {{ {binding_name} as {export_name} }};\n"
        ));
    }
    Ok(wrapped)
}

fn js_string_literal(value: &str) -> Result<String, JsErrorBox> {
    serde_json::to_string(value).map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to encode runtime module loader string literal: {error}"
        ))
    })
}

fn collect_commonjs_named_exports(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_property_exports(source, "exports.", &mut names);
    collect_property_exports(source, "module.exports.", &mut names);
    collect_define_property_exports(source, "Object.defineProperty(exports,", &mut names);
    collect_define_property_exports(source, "Object.defineProperty(module.exports,", &mut names);
    names.retain(|name| name != "default" && name != "__esModule");
    names
}

fn collect_property_exports(source: &str, marker: &str, names: &mut BTreeSet<String>) {
    let mut remaining = source;
    while let Some(index) = remaining.find(marker) {
        let after_marker = &remaining[index + marker.len()..];
        let Some(name) = read_js_identifier(after_marker) else {
            remaining = after_marker;
            continue;
        };
        names.insert(name.to_string());
        remaining = &after_marker[name.len()..];
    }
}

fn collect_define_property_exports(source: &str, marker: &str, names: &mut BTreeSet<String>) {
    let mut remaining = source;
    while let Some(index) = remaining.find(marker) {
        let after_marker = remaining[index + marker.len()..].trim_start();
        let Some(after_quote) = after_marker.strip_prefix(['"', '\'']) else {
            remaining = after_marker;
            continue;
        };
        let quote = after_marker.as_bytes()[0] as char;
        let Some(end_index) = after_quote.find(quote) else {
            remaining = after_quote;
            continue;
        };
        let name = &after_quote[..end_index];
        if is_valid_js_identifier(name) {
            names.insert(name.to_string());
        }
        remaining = &after_quote[end_index + 1..];
    }
}

fn read_js_identifier(input: &str) -> Option<&str> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;
    if !is_js_identifier_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, char) in chars {
        if !is_js_identifier_part(char) {
            break;
        }
        end = index + char.len_utf8();
    }
    Some(&input[..end])
}

fn is_valid_js_identifier(name: &str) -> bool {
    read_js_identifier(name).is_some_and(|identifier| identifier.len() == name.len())
}

fn is_js_identifier_start(char: char) -> bool {
    char == '_' || char == '$' || char.is_ascii_alphabetic()
}

fn is_js_identifier_part(char: char) -> bool {
    is_js_identifier_start(char) || char.is_ascii_digit()
}

fn commonjs_named_export_binding(export_name: &str) -> String {
    let mut binding = String::from("__nimbusCjsExport_");
    for char in export_name.chars() {
        if is_js_identifier_part(char) {
            binding.push(char);
        } else {
            binding.push('_');
        }
    }
    binding
}

fn load_data_url_module_source(
    module_specifier: &ModuleSpecifier,
    options: ModuleLoadOptions,
    resolved_hook_format: Option<&str>,
) -> Result<ModuleSource, JsErrorBox> {
    let (module_type, code) =
        data_url_module_source_bytes(module_specifier, &options, resolved_hook_format)?;
    Ok(ModuleSource::new(
        module_type,
        ModuleSourceCode::Bytes(code.into_boxed_slice().into()),
        module_specifier,
        None,
    ))
}

fn data_url_module_source_bytes(
    module_specifier: &ModuleSpecifier,
    options: &ModuleLoadOptions,
    resolved_hook_format: Option<&str>,
) -> Result<(ModuleType, Vec<u8>), JsErrorBox> {
    let specifier = module_specifier.as_str();
    let payload = specifier.strip_prefix("data:").ok_or_else(|| {
        JsErrorBox::generic(format!("invalid data module specifier: {module_specifier}"))
    })?;
    let (media_type, encoded_data) = payload.split_once(',').ok_or_else(|| {
        JsErrorBox::generic(format!("invalid data module specifier: {module_specifier}"))
    })?;
    let module_type = module_type_from_data_url_media_type(
        media_type,
        module_specifier,
        options,
        resolved_hook_format,
    )?;
    let decoded = percent_decode_data_url_bytes(encoded_data, module_specifier)?;
    let bytes = if data_url_media_type_is_base64(media_type) {
        let encoded = std::str::from_utf8(&decoded).map_err(|error| {
            JsErrorBox::generic(format!(
                "invalid base64 data module specifier {module_specifier}: {error}"
            ))
        })?;
        decode_data_url_base64(encoded).map_err(|error| {
            JsErrorBox::generic(format!(
                "invalid base64 data module specifier {module_specifier}: {error}"
            ))
        })?
    } else {
        decoded
    };
    Ok((module_type, bytes))
}

fn module_type_from_data_url_media_type(
    media_type: &str,
    module_specifier: &ModuleSpecifier,
    options: &ModuleLoadOptions,
    resolved_hook_format: Option<&str>,
) -> Result<ModuleType, JsErrorBox> {
    if let Some(format) = resolved_hook_format {
        let module_type = module_type_from_hook_format(Some(format));
        ensure_json_import_attribute(&module_type, Some(module_specifier), options)?;
        return Ok(module_type);
    }

    let mime_type = data_url_mime_type(media_type);
    let module_type = match mime_type.as_str() {
        "text/javascript"
        | "application/javascript"
        | "text/ecmascript"
        | "application/ecmascript" => ModuleType::JavaScript,
        "application/json" => ModuleType::Json,
        "application/wasm" => ModuleType::Wasm,
        _ => {
            return Err(JsErrorBox::from_err(
                NodeModuleLoadError::UnknownModuleFormat {
                    mime_type,
                    url: module_specifier.to_string(),
                },
            ));
        }
    };
    ensure_json_import_attribute(&module_type, Some(module_specifier), options)?;
    Ok(module_type)
}

fn data_url_mime_type(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn data_url_media_type_is_base64(media_type: &str) -> bool {
    media_type
        .split(';')
        .skip(1)
        .any(|parameter| parameter.trim().eq_ignore_ascii_case("base64"))
}

fn decode_data_url_base64(encoded: &str) -> Result<Vec<u8>, DecodeError> {
    let mut normalized = String::with_capacity(encoded.len() + 3);
    for byte in encoded.bytes() {
        match byte {
            b'-' => normalized.push('+'),
            b'_' => normalized.push('/'),
            b'\t' | b'\n' | b'\x0c' | b'\r' | b' ' => {}
            _ => normalized.push(byte as char),
        }
    }
    match normalized.len() % 4 {
        2 => normalized.push_str("=="),
        3 => normalized.push('='),
        _ => {}
    }
    BASE64_STANDARD.decode(normalized)
}

fn percent_decode_data_url_bytes(
    encoded_data: &str,
    module_specifier: &ModuleSpecifier,
) -> Result<Vec<u8>, JsErrorBox> {
    let bytes = encoded_data.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(high) = bytes.get(index + 1).copied() else {
                return Err(invalid_percent_encoded_data_url(module_specifier));
            };
            let Some(low) = bytes.get(index + 2).copied() else {
                return Err(invalid_percent_encoded_data_url(module_specifier));
            };
            let Some(high) = hex_digit_value(high) else {
                return Err(invalid_percent_encoded_data_url(module_specifier));
            };
            let Some(low) = hex_digit_value(low) else {
                return Err(invalid_percent_encoded_data_url(module_specifier));
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

fn invalid_percent_encoded_data_url(module_specifier: &ModuleSpecifier) -> JsErrorBox {
    JsErrorBox::generic(format!(
        "invalid percent-encoded data module specifier: {module_specifier}"
    ))
}

fn hex_digit_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn module_type_from_path(
    path: &Path,
    options: &ModuleLoadOptions,
    resolved_hook_format: Option<&str>,
) -> Result<ModuleType, JsErrorBox> {
    let module_type = if let Some(format) = resolved_hook_format {
        module_type_from_hook_format(Some(format))
    } else if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_ascii_lowercase();
        if ext == "json" {
            ModuleType::Json
        } else if ext == "wasm" {
            ModuleType::Wasm
        } else {
            match &options.requested_module_type {
                RequestedModuleType::Other(ty) => ModuleType::Other(ty.clone()),
                RequestedModuleType::Text => ModuleType::Text,
                RequestedModuleType::Bytes => ModuleType::Bytes,
                _ => ModuleType::JavaScript,
            }
        }
    } else {
        ModuleType::JavaScript
    };

    let module_specifier = ModuleSpecifier::from_file_path(path).ok();
    ensure_json_import_attribute(&module_type, module_specifier.as_ref(), options)?;
    Ok(module_type)
}

fn ensure_json_import_attribute(
    module_type: &ModuleType,
    module_specifier: Option<&ModuleSpecifier>,
    options: &ModuleLoadOptions,
) -> Result<(), JsErrorBox> {
    if let RequestedModuleType::Other(requested) = &options.requested_module_type {
        return Err(JsErrorBox::from_err(
            NodeModuleLoadError::ImportAttributeUnsupported {
                requested: requested.to_string(),
            },
        ));
    }

    let is_json = module_type == &ModuleType::Json;
    let requested_json = options.requested_module_type == RequestedModuleType::Json;
    let url = module_specifier
        .map(|specifier| specifier.to_string())
        .unwrap_or_default();
    if is_json && !requested_json {
        return Err(JsErrorBox::from_err(
            NodeModuleLoadError::ImportAttributeMissing { url },
        ));
    }
    if requested_json && !is_json {
        return Err(JsErrorBox::from_err(
            NodeModuleLoadError::ImportAttributeTypeIncompatible { url },
        ));
    }

    Ok(())
}

fn hash_module_source_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = XxHash64::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn normalize_file_module_specifier(
    module_specifier: ModuleSpecifier,
) -> Result<ModuleSpecifier, JsErrorBox> {
    if module_specifier.scheme() != "file" {
        return Ok(module_specifier);
    }
    let query = module_specifier.query().map(str::to_owned);
    let fragment = module_specifier.fragment().map(str::to_owned);
    let path = module_specifier.to_file_path().map_err(|_| {
        JsErrorBox::generic(format!("invalid file module specifier: {module_specifier}"))
    })?;
    let mut normalized = ModuleSpecifier::from_file_path(&path).map_err(|_| {
        JsErrorBox::generic(format!(
            "file module path cannot be represented as a URL: {}",
            path.display()
        ))
    })?;
    normalized.set_query(query.as_deref());
    normalized.set_fragment(fragment.as_deref());
    Ok(normalized)
}

fn is_bare_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !has_url_like_scheme(specifier)
}

fn is_tenant_bundle_operator_only_specifier(specifier: &str) -> bool {
    specifier == "nimbus/rest"
        || specifier.starts_with("nimbus/rest/")
        || specifier == "nimbus/transports"
        || specifier.starts_with("nimbus/transports/")
        || specifier == "@nimbus/nimbus/transports"
        || specifier.starts_with("@nimbus/nimbus/transports/")
}

fn has_url_like_scheme(specifier: &str) -> bool {
    let Some((scheme, _)) = specifier.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_error::JsErrorClass as _;

    fn assert_error_code(error: &JsErrorBox, expected: &str) {
        let code = error
            .get_additional_properties()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string());
        assert_eq!(code.as_deref(), Some(expected));
    }

    fn assert_error_property(error: &JsErrorBox, property: &str, expected: &str) {
        let value = error
            .get_additional_properties()
            .find(|(key, _)| key == property)
            .map(|(_, value)| value.to_string());
        assert_eq!(value.as_deref(), Some(expected));
    }

    #[test]
    fn bare_package_detection_excludes_url_like_schemes() {
        assert!(!is_bare_package_specifier("ext:core/mod.js"));
        assert!(!is_bare_package_specifier("node:path"));
        assert!(!is_bare_package_specifier("file:///tmp/mod.js"));
        assert!(!is_bare_package_specifier("data:text/javascript,export{}"));
        assert!(is_bare_package_specifier("@scope/pkg/subpath"));
        assert!(is_bare_package_specifier("minimatch"));
    }

    #[test]
    fn unsupported_esm_url_scheme_error_carries_node_code() {
        let error = JsErrorBox::from_err(NodeModuleLoadError::UnsupportedEsmUrlScheme {
            scheme: "http".to_string(),
        });

        assert_error_code(&error, "ERR_UNSUPPORTED_ESM_URL_SCHEME");
    }

    #[test]
    fn node_module_lookup_errors_carry_url_property() {
        let missing_url = "file:///tmp/does-not-exist.mjs";
        let missing_error = JsErrorBox::from_err(NodeModuleNotFoundError {
            url: missing_url.to_string(),
        });
        assert_error_code(&missing_error, "ERR_MODULE_NOT_FOUND");
        assert_error_property(&missing_error, "url", missing_url);

        let directory_url = "file:///tmp/package-dir";
        let directory_error = JsErrorBox::from_err(NodeUnsupportedDirImportError {
            url: directory_url.to_string(),
        });
        assert_error_code(&directory_error, "ERR_UNSUPPORTED_DIR_IMPORT");
        assert_error_property(&directory_error, "url", directory_url);
    }

    #[test]
    fn data_url_module_source_decodes_percent_encoded_javascript() {
        let specifier =
            ModuleSpecifier::parse("data:text/javascript,export%20default%202").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let (module_type, source) =
            data_url_module_source_bytes(&specifier, &options, None).unwrap();

        assert_eq!(module_type, ModuleType::JavaScript);
        assert_eq!(source, b"export default 2");
    }

    #[test]
    fn data_url_module_source_decodes_base64_javascript() {
        let specifier =
            ModuleSpecifier::parse("data:text/javascript;base64,ZXhwb3J0IGRlZmF1bHQgNw==").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let (module_type, source) =
            data_url_module_source_bytes(&specifier, &options, None).unwrap();

        assert_eq!(module_type, ModuleType::JavaScript);
        assert_eq!(source, b"export default 7");
    }

    #[test]
    fn data_url_module_source_decodes_unpadded_base64url_javascript() {
        let specifier =
            ModuleSpecifier::parse("data:text/javascript;base64,ZXhwb3J0IGRlZmF1bHQgNw").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let (module_type, source) =
            data_url_module_source_bytes(&specifier, &options, None).unwrap();

        assert_eq!(module_type, ModuleType::JavaScript);
        assert_eq!(source, b"export default 7");
    }

    #[test]
    fn file_module_specifier_normalization_decodes_percent_encoded_path_segments() {
        let path = std::env::temp_dir().join("test-esm-shebang.mjs");
        let expected = ModuleSpecifier::from_file_path(&path).unwrap();
        let encoded = expected
            .as_str()
            .replace("test-esm-shebang.mjs", "test-esm-shebang%2emjs");
        assert_ne!(encoded, expected.as_str());

        let normalized =
            normalize_file_module_specifier(ModuleSpecifier::parse(&encoded).unwrap()).unwrap();

        assert_eq!(normalized, expected);
    }

    #[test]
    fn file_module_specifier_normalization_preserves_query_and_fragment() {
        let path = std::env::temp_dir().join("test-esm-json.mjs");
        let expected = ModuleSpecifier::from_file_path(&path).unwrap();
        let encoded = format!(
            "{}?cache=1#section",
            expected
                .as_str()
                .replace("test-esm-json.mjs", "test-esm-json%2emjs")
        );

        let normalized =
            normalize_file_module_specifier(ModuleSpecifier::parse(&encoded).unwrap()).unwrap();

        assert_eq!(normalized.path(), expected.path());
        assert_eq!(normalized.query(), Some("cache=1"));
        assert_eq!(normalized.fragment(), Some("section"));
    }

    #[test]
    fn data_url_json_requires_import_attribute() {
        let specifier = ModuleSpecifier::parse("data:application/json,{\"ok\":true}").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let error = data_url_module_source_bytes(&specifier, &options, None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("needs an import attribute of type \"json\"")
        );
    }

    #[test]
    fn data_url_unknown_mime_type_is_rejected() {
        let specifier = ModuleSpecifier::parse("data:text/plain,export default 1").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let error = data_url_module_source_bytes(&specifier, &options, None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unknown module format: text/plain")
        );
    }

    #[test]
    fn tenant_bundle_admission_rejects_operator_only_transport_imports() {
        assert!(is_tenant_bundle_operator_only_specifier("nimbus/rest"));
        assert!(is_tenant_bundle_operator_only_specifier(
            "@nimbus/nimbus/transports/rest"
        ));
        assert!(is_tenant_bundle_operator_only_specifier(
            "@nimbus/nimbus/transports/rest/internal"
        ));
        assert!(is_tenant_bundle_operator_only_specifier(
            "@nimbus/nimbus/transports/host"
        ));
        assert!(is_tenant_bundle_operator_only_specifier(
            "@nimbus/nimbus/transports/grpc"
        ));
        assert!(!is_tenant_bundle_operator_only_specifier("@nimbus/nimbus"));
        assert!(!is_tenant_bundle_operator_only_specifier(
            "@nimbus/nimbus/server"
        ));
    }
}
