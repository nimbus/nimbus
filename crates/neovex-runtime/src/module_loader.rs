use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};

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

const NODE_FS_PROMISES_SPECIFIER: &str = "node:fs/promises";
const NEOVEX_NODE_FS_PROMISES_SPECIFIER: &str = "neovex:node/fs/promises";

const NODE_FS_PROMISES_MODULE_SOURCE: &str = r#"
function mapFsHostError(error, operation) {
  const hostError = error?.neovexHostError;
  if (!hostError || typeof hostError !== "object") {
    const message = typeof error?.message === "string" ? error.message : "";
    const code = typeof error?.code === "string" && error.code.length > 0
      ? error.code
      : message.match(/"code":"([A-Z0-9_]+)"/)?.[1] ?? null;
    if (code === null) {
      throw error;
    }
    const mappedError = new Error(message.length > 0 ? message : `${operation} failed`);
    mappedError.code = code;
    throw mappedError;
  }
  const message =
    typeof hostError.message === "string" && hostError.message.length > 0
      ? hostError.message
      : String(error?.message ?? `${operation} failed`);
  const mappedError = new Error(message);
  mappedError.code = hostError.code ?? null;
  mappedError.neovexHostError = hostError;
  throw mappedError;
}

function toStats(value) {
  const isFile = value?.isFile === true;
  const isDirectory = value?.isDirectory === true;
  const isSymlink = value?.isSymlink === true;
  const size = Number(value?.size ?? 0);
  const mtimeMs = value?.mtimeMs ?? null;
  const atimeMs = value?.atimeMs ?? null;
  const birthtimeMs = value?.birthtimeMs ?? null;
  const ctimeMs = value?.ctimeMs ?? null;
  return {
    isFile() {
      return isFile;
    },
    isDirectory() {
      return isDirectory;
    },
    isSymbolicLink() {
      return isSymlink;
    },
    isBlockDevice() {
      return false;
    },
    isCharacterDevice() {
      return false;
    },
    isFIFO() {
      return false;
    },
    isSocket() {
      return false;
    },
    size,
    mtimeMs,
    atimeMs,
    birthtimeMs,
    ctimeMs,
    mtime: mtimeMs == null ? null : new Date(mtimeMs),
    atime: atimeMs == null ? null : new Date(atimeMs),
    birthtime: birthtimeMs == null ? null : new Date(birthtimeMs),
    ctime: ctimeMs == null ? null : new Date(ctimeMs),
    mode: value?.mode ?? null,
  };
}

function toDirent(value) {
  const isFile = value?.isFile === true;
  const isDirectory = value?.isDirectory === true;
  const isSymlink = value?.isSymlink === true;
  return {
    name: String(value?.name ?? ""),
    isFile() {
      return isFile;
    },
    isDirectory() {
      return isDirectory;
    },
    isSymbolicLink() {
      return isSymlink;
    },
  };
}

function normalizeReadFileEncoding(options) {
  if (options === undefined || options === null) {
    return null;
  }
  if (typeof options === "string") {
    return options.toLowerCase();
  }
  if (typeof options === "object" && typeof options.encoding === "string") {
    return options.encoding.toLowerCase();
  }
  return null;
}

function normalizeMkdirOptions(options) {
  if (options === undefined || options === null) {
    return { recursive: false, mode: null };
  }
  if (typeof options === "boolean") {
    return { recursive: options, mode: null };
  }
  if (typeof options === "object") {
    return {
      recursive: options.recursive === true,
      mode: typeof options.mode === "number" ? options.mode : null,
    };
  }
  return { recursive: false, mode: null };
}

async function readFile(path, options) {
  const normalizedEncoding =
    normalizeReadFileEncoding(options);
  let result;
  try {
    result = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_fs_read_file", {
      path: String(path),
      encoding: normalizedEncoding,
    });
  } catch (error) {
    mapFsHostError(error, "readFile");
  }
  if (result?.kind === "text") {
    return result.value;
  }
  return Uint8Array.from(result?.value ?? []);
}

async function writeFile(path, data) {
  if (typeof data === "string") {
    try {
      await globalThis.__neovexAsyncHostValue("op_neovex_runtime_fs_write_file", {
        path: String(path),
        text: data,
      });
    } catch (error) {
      mapFsHostError(error, "writeFile");
    }
    return;
  }
  if (data instanceof ArrayBuffer) {
    try {
      await globalThis.__neovexAsyncHostValue("op_neovex_runtime_fs_write_file", {
        path: String(path),
        bytes: Array.from(new Uint8Array(data)),
      });
    } catch (error) {
      mapFsHostError(error, "writeFile");
    }
    return;
  }
  if (ArrayBuffer.isView(data)) {
    try {
      await globalThis.__neovexAsyncHostValue("op_neovex_runtime_fs_write_file", {
        path: String(path),
        bytes: Array.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength)),
      });
    } catch (error) {
      mapFsHostError(error, "writeFile");
    }
    return;
  }
  throw new TypeError("node:fs/promises writeFile currently supports string, ArrayBuffer, or typed-array input");
}

async function stat(path) {
  try {
    const value = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_stat", {
      path: String(path),
      follow_symlink: true,
    });
    return toStats(value);
  } catch (error) {
    mapFsHostError(error, "stat");
  }
}

async function lstat(path) {
  try {
    const value = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_stat", {
      path: String(path),
      follow_symlink: false,
    });
    return toStats(value);
  } catch (error) {
    mapFsHostError(error, "lstat");
  }
}

async function mkdir(path, options) {
  const normalizedOptions = normalizeMkdirOptions(options);
  try {
    await globalThis.__neovexAsyncHostValue("op_neovex_runtime_mkdir", {
      path: String(path),
      recursive: normalizedOptions.recursive,
      mode: normalizedOptions.mode,
    });
  } catch (error) {
    mapFsHostError(error, "mkdir");
  }
}

async function readdir(path, options) {
  const withFileTypes =
    typeof options === "object" && options !== null && options.withFileTypes === true;
  let entries;
  try {
    entries = await globalThis.__neovexAsyncHostValue("op_neovex_runtime_read_dir", {
      path: String(path),
    });
  } catch (error) {
    mapFsHostError(error, "readdir");
  }
  const normalizedEntries = Array.isArray(entries) ? entries : [];
  if (withFileTypes) {
    return normalizedEntries.map(toDirent);
  }
  return normalizedEntries.map((entry) => String(entry?.name ?? ""));
}

export { lstat, mkdir, readFile, readdir, stat, writeFile };
export default { lstat, mkdir, readFile, readdir, stat, writeFile };
"#;

#[allow(dead_code)]
const NODE_MODULE_MODULE_SOURCE: &str = r#"
const __neovexCommonJsCache = globalThis.__neovexCommonJsCache ??= new Map();

function __neovexRuntimeDirname(filename) {
  const slashIndex = Math.max(filename.lastIndexOf("/"), filename.lastIndexOf("\\"));
  if (slashIndex === -1) {
    return ".";
  }
  if (slashIndex === 0) {
    return filename.slice(0, 1);
  }
  return filename.slice(0, slashIndex);
}

function __neovexRuntimeResolveRequire(specifier, referrer) {
  return globalThis.__neovexSyncHostValue("op_neovex_runtime_require_resolve", {
    specifier: String(specifier),
    referrer: referrer === null || referrer === undefined ? null : String(referrer),
  });
}

function __neovexRuntimeReadRequireFile(path) {
  return globalThis.__neovexSyncHostValue("op_neovex_runtime_require_read_file", {
    path: String(path),
  });
}

function __neovexRuntimeLoadResolvedRequire(resolved, referrer) {
  switch (resolved?.kind) {
    case "builtin":
      if (resolved.module_name === "module") {
        return __neovexNodeModuleNamespace;
      }
      throw new Error(`require() does not support builtin module ${resolved.module_name} in Neovex yet`);
    case "json": {
      const cacheKey = resolved.path;
      if (__neovexCommonJsCache.has(cacheKey)) {
        return __neovexCommonJsCache.get(cacheKey).exports;
      }
      const module = {
        id: cacheKey,
        filename: cacheKey,
        exports: JSON.parse(__neovexRuntimeReadRequireFile(cacheKey)),
        loaded: true,
        children: [],
        paths: [],
      };
      __neovexCommonJsCache.set(cacheKey, module);
      return module.exports;
    }
    case "common_js":
      return __neovexRuntimeInstantiateCommonJs(resolved.path);
    case "es_module":
      throw new Error(`require() does not support loading ES modules in Neovex yet: ${resolved.path}`);
    default:
      throw new Error(`unsupported require() resolution result for ${String(referrer ?? "runtime")}`);
  }
}

function __neovexRuntimeInstantiateCommonJs(filename) {
  if (__neovexCommonJsCache.has(filename)) {
    return __neovexCommonJsCache.get(filename).exports;
  }

  const module = {
    id: filename,
    filename,
    exports: {},
    loaded: false,
    children: [],
    paths: [],
  };
  __neovexCommonJsCache.set(filename, module);

  try {
    const source = __neovexRuntimeReadRequireFile(filename);
    const require = createRequire(filename);
    require.cache = __neovexCommonJsCache;
    const compiled = new Function(
      "exports",
      "require",
      "module",
      "__filename",
      "__dirname",
      `${source}\n//# sourceURL=${encodeURI(filename)}`,
    );
    compiled(
      module.exports,
      require,
      module,
      filename,
      __neovexRuntimeDirname(filename),
    );
    module.loaded = true;
    return module.exports;
  } catch (error) {
    __neovexCommonJsCache.delete(filename);
    throw error;
  }
}

function createRequire(referrer) {
  const normalizedReferrer =
    referrer === null || referrer === undefined ? null : String(referrer);
  const require = function require(specifier) {
    return __neovexRuntimeLoadResolvedRequire(
      __neovexRuntimeResolveRequire(specifier, normalizedReferrer),
      normalizedReferrer,
    );
  };
  require.resolve = function resolve(specifier) {
    const resolved = __neovexRuntimeResolveRequire(specifier, normalizedReferrer);
    return resolved?.path ?? `node:${resolved?.module_name ?? specifier}`;
  };
  require.cache = __neovexCommonJsCache;
  return require;
}

const Module = Object.freeze({
  _cache: __neovexCommonJsCache,
  _load(specifier, parent) {
    const parentReferrer =
      parent && typeof parent.filename === "string" ? parent.filename : null;
    return __neovexRuntimeLoadResolvedRequire(
      __neovexRuntimeResolveRequire(specifier, parentReferrer),
      parentReferrer,
    );
  },
});

const __neovexNodeModuleNamespace = Object.freeze({ createRequire, Module });

export { createRequire, Module };
export default __neovexNodeModuleNamespace;
"#;

#[derive(Debug, Clone)]
struct BundleModuleCodeCacheEntry {
    hash: u64,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct BundleModuleCodeCacheState {
    entries: HashMap<String, BundleModuleCodeCacheEntry>,
    latest_hashes: HashMap<String, u64>,
    prevented_hashes: HashMap<String, u64>,
    writes: usize,
}

#[derive(Debug, Default)]
pub struct BundleModuleCodeCache {
    state: Mutex<BundleModuleCodeCacheState>,
}

impl BundleModuleCodeCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn lookup(&self, specifier: &ModuleSpecifier, hash: u64) -> Option<Cow<'static, [u8]>> {
        let key = specifier.to_string();
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        state.latest_hashes.insert(key.clone(), hash);
        match state.prevented_hashes.get(&key).copied() {
            Some(prevented_hash) if prevented_hash == hash => return None,
            Some(_) => {
                state.prevented_hashes.remove(&key);
            }
            None => {}
        }
        match state.entries.get(&key) {
            Some(entry) if entry.hash == hash => Some(Cow::Owned(entry.data.clone())),
            Some(_) => {
                state.entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn store(&self, specifier: ModuleSpecifier, hash: u64, code_cache: &[u8]) {
        let key = specifier.to_string();
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        state.latest_hashes.insert(key.clone(), hash);
        if state.prevented_hashes.get(&key).copied() == Some(hash) {
            return;
        }
        state.entries.insert(
            key,
            BundleModuleCodeCacheEntry {
                hash,
                data: code_cache.to_vec(),
            },
        );
        state.writes = state.writes.saturating_add(1);
    }

    fn purge_and_prevent(&self, module_specifier: &str) {
        let mut state = self
            .state
            .lock()
            .expect("bundle code cache lock should not be poisoned");
        let removed = state.entries.remove(module_specifier);
        if let Some(hash) = state
            .latest_hashes
            .get(module_specifier)
            .copied()
            .or_else(|| removed.map(|entry| entry.hash))
        {
            state
                .prevented_hashes
                .insert(module_specifier.to_string(), hash);
        }
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.state
            .lock()
            .expect("bundle code cache lock should not be poisoned")
            .entries
            .len()
    }

    #[cfg(test)]
    pub(crate) fn write_count(&self) -> usize {
        self.state
            .lock()
            .expect("bundle code cache lock should not be poisoned")
            .writes
    }
}

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
            RuntimeCompatibilityTarget::Node22 => {
                "unsupported node: builtin for the current Node22 surface; the verified extension-backed lane currently includes node:path, node:url, node:os, node:tty, node:child_process, node:crypto, node:worker_threads, plus node:fs/promises and minimal Node globals"
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
        if module_type == ModuleType::JavaScript
            && matches!(
                self.compatibility_target,
                RuntimeCompatibilityTarget::Node22
            )
        {
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
        if !matches!(
            self.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        ) {
            return None;
        }
        match specifier {
            NODE_FS_PROMISES_SPECIFIER | NEOVEX_NODE_FS_PROMISES_SPECIFIER => {
                Some(NODE_FS_PROMISES_MODULE_SOURCE)
            }
            _ => None,
        }
    }

    fn supports_extension_backed_node_builtin(&self, specifier: &str) -> bool {
        if !matches!(
            self.compatibility_target,
            RuntimeCompatibilityTarget::Node22
        ) {
            return false;
        }
        matches!(
            specifier,
            "node:module"
                | "node:path"
                | "node:url"
                | "node:os"
                | "node:tty"
                | "node:child_process"
                | "node:crypto"
                | "node:worker_threads"
        )
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
            if specifier == NODE_FS_PROMISES_SPECIFIER {
                return ModuleSpecifier::parse(NEOVEX_NODE_FS_PROMISES_SPECIFIER)
                    .map_err(JsErrorBox::from_err);
            }
            if self.supported_node_builtin_source(specifier).is_some()
                || self.supports_extension_backed_node_builtin(specifier)
            {
                return ModuleSpecifier::parse(specifier).map_err(JsErrorBox::from_err);
            }
            return Err(self.unsupported_node_builtin_error(specifier));
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
        && !specifier.starts_with("file:")
        && !specifier.starts_with("http:")
        && !specifier.starts_with("https:")
        && !specifier.starts_with("data:")
}

#[cfg(test)]
mod tests {
    use super::BundleModuleCodeCache;
    use deno_core::ModuleSpecifier;

    #[test]
    fn bundle_code_cache_prevents_same_hash_after_purge() {
        let cache = BundleModuleCodeCache::new();
        let specifier =
            ModuleSpecifier::parse("file:///bundle/mod.js").expect("module specifier should parse");

        cache.store(specifier.clone(), 11, b"compiled");
        assert!(cache.lookup(&specifier, 11).is_some());

        cache.purge_and_prevent(specifier.as_str());
        assert!(cache.lookup(&specifier, 11).is_none());

        cache.store(specifier.clone(), 11, b"compiled-again");
        assert!(cache.lookup(&specifier, 11).is_none());

        cache.store(specifier.clone(), 12, b"compiled-new");
        let cached = cache
            .lookup(&specifier, 12)
            .expect("new hash should be allowed");
        assert_eq!(cached.as_ref(), b"compiled-new");
    }
}
