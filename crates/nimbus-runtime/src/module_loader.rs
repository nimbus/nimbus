use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use crate::backends::v8::embedder::{
    JsErrorBox, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, RequestedModuleType,
    ResolutionKind, SourceCodeCacheInfo, resolve_import,
};
use crate::limits::RuntimeCompatibilityTarget;
use crate::node_compat::{
    ResolvedNodeModuleKind, ResolvedNodeTarget, build_package_json_resolver,
    classify_resolved_module_kind, resolve_node_target, translate_commonjs_to_esm,
};
use crate::runtime_capabilities::RuntimePathPolicy;
use twox_hash::XxHash64;

mod code_cache;
mod embedded_builtins;

pub(crate) use code_cache::BundleModuleCodeCache;
use embedded_builtins::{
    INTERNAL_READLINE_UTILS_SPECIFIER, NIMBUS_INTERNAL_READLINE_UTILS_SPECIFIER,
    NIMBUS_NODE_FS_PROMISES_SPECIFIER, NIMBUS_NODE_FS_SPECIFIER, NIMBUS_NODE_MODULE_SPECIFIER,
    NODE_FS_PROMISES_SPECIFIER, NODE_FS_SPECIFIER, NODE_MODULE_SPECIFIER,
    source_for_supported_node_builtin, supports_extension_backed_node_builtin,
};

#[derive(Debug, Clone)]
pub struct RestrictedModuleLoader {
    path_policy: RuntimePathPolicy,
    compatibility_target: RuntimeCompatibilityTarget,
    code_cache: Arc<BundleModuleCodeCache>,
}

impl RestrictedModuleLoader {
    pub fn new(
        path_policy: RuntimePathPolicy,
        compatibility_target: RuntimeCompatibilityTarget,
        code_cache: Arc<BundleModuleCodeCache>,
    ) -> Self {
        Self {
            path_policy,
            compatibility_target,
            code_cache,
        }
    }

    fn unsupported_node_builtin_error(&self, specifier: &str) -> JsErrorBox {
        let reason = match self.compatibility_target {
            RuntimeCompatibilityTarget::WebStandardIsolate => {
                "node: imports are unavailable under RuntimeCompatibilityTarget::WebStandardIsolate"
            }
            RuntimeCompatibilityTarget::Node20
            | RuntimeCompatibilityTarget::Node22
            | RuntimeCompatibilityTarget::Node24 => {
                "unsupported node: builtin for the current Node-compatible surface; the verified extension-backed lane currently includes core semantics builtins (node:assert/strict, node:buffer, node:console, node:events, node:path including posix/win32, node:punycode, node:querystring, node:string_decoder, node:url), process/timing builtins (node:process, node:timers, node:timers/promises, node:util, node:diagnostics_channel, node:perf_hooks), selected host/runtime builtins (node:fs, node:fs/promises, node:os, node:tty, node:stream including consumers/promises/web, node:child_process, node:crypto, node:worker_threads), and the in-progress networking family (node:dns, node:net, node:dgram, node:tls, node:http, node:https, node:http2), plus minimal Node globals"
            }
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
        if specifier.scheme() != "file" {
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
    ) -> Result<ModuleSource, JsErrorBox> {
        if let Some(source) = self.supported_node_builtin_source(module_specifier.as_str()) {
            return Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::Bytes(source.as_bytes().to_vec().into_boxed_slice().into()),
                module_specifier,
                None,
            ));
        }
        let path = module_specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {module_specifier}"))
        })?;
        let module_type = module_type_from_path(&path, &options)?;
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
                code = translate_commonjs_to_esm(&self.path_policy, module_specifier, &source)
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
        source_for_supported_node_builtin(specifier, self.compatibility_target.is_node())
    }

    fn supports_extension_backed_node_builtin(&self, specifier: &str) -> bool {
        supports_extension_backed_node_builtin(specifier, self.compatibility_target.is_node())
    }

    fn resolve_bare_package_specifier(
        &self,
        specifier: &str,
        referrer: &str,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        match resolve_node_target(
            &self.path_policy,
            specifier,
            referrer,
            node_resolver::ResolutionMode::Import,
        )? {
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
}

impl ModuleLoader for RestrictedModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
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
        if is_bare_package_specifier(specifier) {
            return self.resolve_bare_package_specifier(specifier, referrer);
        }
        let resolved = resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)?;
        match kind {
            ResolutionKind::MainModule | ResolutionKind::Import | ResolutionKind::DynamicImport => {
                self.ensure_allowed_specifier(&resolved)?
            }
        }
        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        if let Err(error) = self.ensure_allowed_specifier(module_specifier) {
            return ModuleLoadResponse::Sync(Err(error));
        }
        ModuleLoadResponse::Async(Box::pin({
            let loader = self.clone();
            let module_specifier = module_specifier.clone();
            async move { loader.load_module_source(&module_specifier, options).await }
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
}

fn module_type_from_path(
    path: &Path,
    options: &ModuleLoadOptions,
) -> Result<ModuleType, JsErrorBox> {
    let module_type = if let Some(extension) = path.extension() {
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

    if module_type == ModuleType::Json && options.requested_module_type != RequestedModuleType::Json
    {
        return Err(JsErrorBox::generic(
            "Attempted to load JSON module without specifying \"type\": \"json\" attribute in the import statement.",
        ));
    }

    Ok(module_type)
}

fn hash_module_source_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = XxHash64::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn is_bare_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !has_url_like_scheme(specifier)
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

    #[test]
    fn bare_package_detection_excludes_url_like_schemes() {
        assert!(!is_bare_package_specifier("ext:core/mod.js"));
        assert!(!is_bare_package_specifier("node:path"));
        assert!(!is_bare_package_specifier("file:///tmp/mod.js"));
        assert!(!is_bare_package_specifier("data:text/javascript,export{}"));
        assert!(is_bare_package_specifier("@scope/pkg/subpath"));
        assert!(is_bare_package_specifier("minimatch"));
    }
}
