use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, RequestedModuleType, ResolutionKind,
    SourceCodeCacheInfo, resolve_import,
};
use deno_error::JsErrorBox;
use twox_hash::XxHash64;

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
pub struct SandboxedModuleLoader {
    allowed_root: PathBuf,
    code_cache: Arc<BundleModuleCodeCache>,
}

impl SandboxedModuleLoader {
    pub fn new(allowed_root: PathBuf, code_cache: Arc<BundleModuleCodeCache>) -> Self {
        Self {
            allowed_root,
            code_cache,
        }
    }

    fn ensure_allowed_specifier(&self, specifier: &ModuleSpecifier) -> Result<(), JsErrorBox> {
        if specifier.scheme() != "file" {
            return Err(JsErrorBox::generic(format!(
                "runtime bundle imports must stay within the bundle root, unsupported scheme: {}",
                specifier.scheme()
            )));
        }

        let path = specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {specifier}"))
        })?;
        let candidate = canonicalize_for_sandbox(&path).map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to resolve runtime bundle import {}: {error}",
                path.display()
            ))
        })?;
        if !candidate.starts_with(&self.allowed_root) {
            return Err(JsErrorBox::generic(format!(
                "runtime bundle import is outside the bundle root: {}",
                candidate.display()
            )));
        }
        Ok(())
    }

    fn load_module_source(
        &self,
        module_specifier: &ModuleSpecifier,
        options: ModuleLoadOptions,
    ) -> Result<ModuleSource, JsErrorBox> {
        let path = module_specifier.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("invalid file module specifier: {module_specifier}"))
        })?;
        let module_type = module_type_from_path(&path, &options)?;
        let code = std::fs::read(&path).map_err(|source| {
            JsErrorBox::generic(format!(
                "failed to load runtime bundle module {}: {source}",
                path.display()
            ))
        })?;
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
}

impl ModuleLoader for SandboxedModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
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
            async move { loader.load_module_source(&module_specifier, options) }
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

fn canonicalize_for_sandbox(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "module path does not have a parent directory",
                )
            })?;
            let parent = parent.canonicalize()?;
            let file_name = path.file_name().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "module path does not have a file name",
                )
            })?;
            Ok(parent.join(file_name))
        }
        Err(error) => Err(error),
    }
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
