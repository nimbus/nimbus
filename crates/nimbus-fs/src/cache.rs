use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct ChunkCache<K, V> {
    capacity: usize,
    entries: HashMap<K, V>,
    order: VecDeque<K>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLookup {
    Hit,
    Miss,
}

impl<K, V> ChunkCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get_or_insert_with(&mut self, key: K, fetch: impl FnOnce(&K) -> V) -> (V, CacheLookup) {
        if let Some(value) = self.entries.get(&key).cloned() {
            self.touch(&key);
            return (value, CacheLookup::Hit); // cache hit avoids a re-fetch
        }
        let value = fetch(&key);
        self.insert(key, value.clone());
        (value, CacheLookup::Miss)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    fn insert(&mut self, key: K, value: V) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), value);
            self.touch(&key);
            return;
        }
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        while self.entries.len() > self.capacity {
            self.evict_oldest();
        }
    }

    fn touch(&mut self, key: &K) {
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.clone());
    }

    fn evict_oldest(&mut self) {
        if let Some(evicted) = self.order.pop_front() {
            self.entries.remove(&evicted);
        }
    }
}
