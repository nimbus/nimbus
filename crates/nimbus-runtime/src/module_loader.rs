use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

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
            RuntimeCompatibilityTarget::BunJsc => {
                "Bun/JSC package resolution is owned by the Bun/JSC backend and is unavailable through the V8 module loader"
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
                    "unsupported node: builtin for the current Node-compatible surface; the verified extension-backed lane currently includes core semantics builtins (node:assert/strict, node:buffer, node:console, node:events, node:path including posix/win32, node:punycode, node:querystring, node:string_decoder, node:url), process/timing builtins (node:process, node:timers, node:timers/promises, node:util, node:diagnostics_channel, node:perf_hooks), selected host/runtime builtins (node:fs, node:fs/promises, node:os, node:tty, node:stream including consumers/promises/web, node:child_process, node:crypto, node:worker_threads), and the in-progress networking family (node:dns, node:net, node:dgram, node:tls, node:http, node:https, node:http2), plus minimal Node globals"
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
        if module_specifier.scheme() == "data" && self.compatibility_target.is_node() {
            return load_data_url_module_source(module_specifier, options);
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

fn load_data_url_module_source(
    module_specifier: &ModuleSpecifier,
    options: ModuleLoadOptions,
) -> Result<ModuleSource, JsErrorBox> {
    let (module_type, code) = data_url_module_source_bytes(module_specifier, &options)?;
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
) -> Result<(ModuleType, Vec<u8>), JsErrorBox> {
    let specifier = module_specifier.as_str();
    let payload = specifier.strip_prefix("data:").ok_or_else(|| {
        JsErrorBox::generic(format!("invalid data module specifier: {module_specifier}"))
    })?;
    let (media_type, encoded_data) = payload.split_once(',').ok_or_else(|| {
        JsErrorBox::generic(format!("invalid data module specifier: {module_specifier}"))
    })?;
    let module_type = module_type_from_data_url_media_type(media_type, module_specifier, options)?;
    let decoded = percent_decode_data_url_bytes(encoded_data, module_specifier)?;
    let bytes = if data_url_media_type_is_base64(media_type) {
        let encoded = std::str::from_utf8(&decoded).map_err(|error| {
            JsErrorBox::generic(format!(
                "invalid base64 data module specifier {module_specifier}: {error}"
            ))
        })?;
        BASE64_STANDARD.decode(encoded).map_err(|error| {
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
) -> Result<ModuleType, JsErrorBox> {
    let mime_type = data_url_mime_type(media_type);
    let module_type = match mime_type.as_str() {
        "text/javascript"
        | "application/javascript"
        | "text/ecmascript"
        | "application/ecmascript" => ModuleType::JavaScript,
        "application/json" => ModuleType::Json,
        "application/wasm" => ModuleType::Wasm,
        _ => {
            return Err(JsErrorBox::generic(format!(
                "Unknown module format: {mime_type} for URL {module_specifier}"
            )));
        }
    };
    ensure_json_import_attribute(&module_type, options)?;
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

    ensure_json_import_attribute(&module_type, options)?;
    Ok(module_type)
}

fn ensure_json_import_attribute(
    module_type: &ModuleType,
    options: &ModuleLoadOptions,
) -> Result<(), JsErrorBox> {
    if module_type == &ModuleType::Json
        && options.requested_module_type != RequestedModuleType::Json
    {
        return Err(JsErrorBox::generic(
            "Attempted to load JSON module without specifying \"type\": \"json\" attribute in the import statement.",
        ));
    }

    Ok(())
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

    #[test]
    fn data_url_module_source_decodes_percent_encoded_javascript() {
        let specifier =
            ModuleSpecifier::parse("data:text/javascript,export%20default%202").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let (module_type, source) = data_url_module_source_bytes(&specifier, &options).unwrap();

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

        let (module_type, source) = data_url_module_source_bytes(&specifier, &options).unwrap();

        assert_eq!(module_type, ModuleType::JavaScript);
        assert_eq!(source, b"export default 7");
    }

    #[test]
    fn data_url_json_requires_import_attribute() {
        let specifier = ModuleSpecifier::parse("data:application/json,{\"ok\":true}").unwrap();
        let options = ModuleLoadOptions {
            requested_module_type: RequestedModuleType::None,
            is_dynamic_import: false,
            is_synchronous: false,
        };

        let error = data_url_module_source_bytes(&specifier, &options).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Attempted to load JSON module without specifying")
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

        let error = data_url_module_source_bytes(&specifier, &options).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unknown module format: text/plain")
        );
    }
}
