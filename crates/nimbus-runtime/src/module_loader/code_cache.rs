use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::backends::v8::embedder::ModuleSpecifier;

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

    pub(super) fn lookup(
        &self,
        specifier: &ModuleSpecifier,
        hash: u64,
    ) -> Option<Cow<'static, [u8]>> {
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

    pub(super) fn store(&self, specifier: ModuleSpecifier, hash: u64, code_cache: &[u8]) {
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

    pub(super) fn purge_and_prevent(&self, module_specifier: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
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
