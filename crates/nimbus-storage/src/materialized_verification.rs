use std::cmp::Ordering;
use std::mem::size_of;

use nimbus_core::{Error, Result, SequenceNumber};
use sha2::{Digest, Sha256};

pub const MATERIALIZED_VERIFICATION_ROOT_VERSION: u16 = 1;

/// The approved million-leaf resident-memory budget is 192 bytes per logical
/// leaf.
///
/// A node is 144 bytes on supported targets. The IMV2 measurement adds a
/// conservative 16-byte allocator allowance, for a budgeted total of 160
/// bytes. The remaining 32 bytes cover the index and free-list allocation
/// share at that measurement rung without weakening the limit.
pub const VERIFICATION_INDEX_MAX_RESIDENT_BYTES_PER_LEAF: usize = 192;
pub const VERIFICATION_INDEX_NODE_BYTES: usize = size_of::<TreapNode>();
pub const VERIFICATION_INDEX_ALLOCATOR_BYTES_PER_LEAF: usize = 16;
pub const VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF: usize =
    VERIFICATION_INDEX_NODE_BYTES + VERIFICATION_INDEX_ALLOCATOR_BYTES_PER_LEAF;
const _: () = assert!(
    VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF <= VERIFICATION_INDEX_MAX_RESIDENT_BYTES_PER_LEAF
);

const HASH_BYTES: usize = 32;
const KEY_DOMAIN: &[u8] = b"nimbus.materialized-verification.key";
const PRIORITY_DOMAIN: &[u8] = b"nimbus.materialized-verification.priority";
const VALUE_DOMAIN: &[u8] = b"nimbus.materialized-verification.value";
const NODE_DOMAIN: &[u8] = b"nimbus.materialized-verification.node";
const EMPTY_DOMAIN: &[u8] = b"nimbus.materialized-verification.empty";

type Hash = [u8; HASH_BYTES];

/// The format that defines logical keys and Merkle node hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerificationRootVersion(u16);

impl VerificationRootVersion {
    pub const fn current() -> Self {
        Self(MATERIALIZED_VERIFICATION_ROOT_VERSION)
    }

    pub fn new(version: u16) -> Result<Self> {
        if version != MATERIALIZED_VERIFICATION_ROOT_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported materialized verification root version {version}"
            )));
        }
        Ok(Self(version))
    }

    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

/// The state family that owns a canonical materialized leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalLeafKind {
    TableIdentity,
    Schema,
    Document,
    ScheduledExecution,
}

impl LogicalLeafKind {
    const fn tag(self) -> u8 {
        match self {
            Self::TableIdentity => 1,
            Self::Schema => 2,
            Self::Document => 3,
            Self::ScheduledExecution => 4,
        }
    }
}

/// A provider-neutral key for one canonical materialized leaf.
///
/// Callers supply the canonical identity bytes and cannot assemble raw tree
/// keys. The state-family tag prevents equal bytes in different families from
/// sharing one leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalLeafKey(Hash);

impl LogicalLeafKey {
    pub fn new(kind: LogicalLeafKind, canonical_identity: &[u8]) -> Result<Self> {
        if canonical_identity.is_empty() {
            return Err(Error::InvalidInput(
                "materialized verification leaf identity must not be empty".to_string(),
            ));
        }
        Ok(Self(hash_parts(
            VerificationRootVersion::current(),
            KEY_DOMAIN,
            &[&[kind.tag()], canonical_identity],
        )))
    }

    pub const fn as_bytes(&self) -> &Hash {
        &self.0
    }
}

/// The applied state identified by one derived verification root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationPosition {
    version: VerificationRootVersion,
    applied_sequence: SequenceNumber,
    root_hash: Hash,
}

impl VerificationPosition {
    pub fn new(applied_sequence: SequenceNumber, root_hash: Hash) -> Self {
        Self {
            version: VerificationRootVersion::current(),
            applied_sequence,
            root_hash,
        }
    }

    pub fn from_parts(
        version: u16,
        applied_sequence: SequenceNumber,
        root_hash: Hash,
    ) -> Result<Self> {
        Ok(Self {
            version: VerificationRootVersion::new(version)?,
            applied_sequence,
            root_hash,
        })
    }

    pub const fn version(&self) -> VerificationRootVersion {
        self.version
    }

    pub const fn applied_sequence(&self) -> SequenceNumber {
        self.applied_sequence
    }

    pub const fn root_hash(&self) -> &Hash {
        &self.root_hash
    }
}

#[derive(Debug, Clone)]
struct TreapNode {
    key: LogicalLeafKey,
    priority: Hash,
    value_hash: Hash,
    subtree_hash: Hash,
    left: Option<u32>,
    right: Option<u32>,
}

impl TreapNode {
    fn new(version: VerificationRootVersion, key: LogicalLeafKey, value: &[u8]) -> Self {
        let priority = hash_parts(version, PRIORITY_DOMAIN, &[key.as_bytes()]);
        let value_hash = hash_parts(version, VALUE_DOMAIN, &[key.as_bytes(), value]);
        Self {
            key,
            priority,
            value_hash,
            subtree_hash: [0; HASH_BYTES],
            left: None,
            right: None,
        }
    }
}

/// A derived, process-local index over canonical materialized leaves.
#[derive(Debug, Clone)]
pub struct MaterializedVerificationIndex {
    version: VerificationRootVersion,
    nodes: Vec<TreapNode>,
    free_nodes: Vec<u32>,
    root: Option<u32>,
    len: usize,
}

impl Default for MaterializedVerificationIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl MaterializedVerificationIndex {
    pub fn new() -> Self {
        Self::new_with_version(VerificationRootVersion::current())
    }

    fn new_with_version(version: VerificationRootVersion) -> Self {
        Self {
            version,
            nodes: Vec::new(),
            free_nodes: Vec::new(),
            root: None,
            len: 0,
        }
    }

    pub fn from_leaves<I, B>(leaves: I) -> Result<Self>
    where
        I: IntoIterator<Item = (LogicalLeafKey, B)>,
        B: AsRef<[u8]>,
    {
        Self::from_leaves_with_version(VerificationRootVersion::current(), leaves)
    }

    fn from_leaves_with_version<I, B>(version: VerificationRootVersion, leaves: I) -> Result<Self>
    where
        I: IntoIterator<Item = (LogicalLeafKey, B)>,
        B: AsRef<[u8]>,
    {
        let mut nodes = leaves
            .into_iter()
            .map(|(key, value)| TreapNode::new(version, key, value.as_ref()))
            .collect::<Vec<_>>();
        if nodes.len() > u32::MAX as usize {
            return Err(Error::ResourceExhausted(
                "materialized verification index exceeds the u32 node limit".to_string(),
            ));
        }
        nodes.sort_unstable_by_key(|node| node.key);
        if nodes.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(Error::InvalidInput(
                "materialized verification batch contains a duplicate logical leaf key".to_string(),
            ));
        }

        let mut index = Self {
            version,
            len: nodes.len(),
            nodes,
            free_nodes: Vec::new(),
            root: None,
        };
        let mut stack = Vec::<u32>::new();
        for node_index in 0..index.nodes.len() as u32 {
            let mut left = None;
            while let Some(&candidate) = stack.last() {
                if !index.heap_precedes(node_index, candidate) {
                    break;
                }
                left = stack.pop();
            }
            index.nodes[node_index as usize].left = left;
            if let Some(&parent) = stack.last() {
                index.nodes[parent as usize].right = Some(node_index);
            } else {
                index.root = Some(node_index);
            }
            stack.push(node_index);
        }
        if let Some(root) = index.root {
            index.recompute_subtree(root);
        }
        Ok(index)
    }

    pub fn upsert(&mut self, key: LogicalLeafKey, value: &[u8]) -> Result<bool> {
        let value_hash = hash_parts(self.version, VALUE_DOMAIN, &[key.as_bytes(), value]);
        let mut cursor = self.root;
        let mut path = Vec::new();
        while let Some(node_index) = cursor {
            path.push(node_index);
            match key.cmp(&self.nodes[node_index as usize].key) {
                Ordering::Equal => {
                    self.nodes[node_index as usize].value_hash = value_hash;
                    for &path_index in path.iter().rev() {
                        self.recompute_node(path_index);
                    }
                    return Ok(false);
                }
                Ordering::Less => cursor = self.nodes[node_index as usize].left,
                Ordering::Greater => cursor = self.nodes[node_index as usize].right,
            }
        }

        let node = TreapNode::new(self.version, key, value);
        let node_index = self.allocate_node(node)?;
        self.root = Some(self.insert_index(self.root, node_index));
        self.len += 1;
        Ok(true)
    }

    pub fn remove(&mut self, key: &LogicalLeafKey) -> bool {
        let (root, removed) = self.remove_index(self.root, key);
        self.root = root;
        let Some(removed) = removed else {
            return false;
        };
        self.free_nodes.push(removed);
        self.len -= 1;
        true
    }

    pub fn root_hash(&self) -> Hash {
        self.root
            .map(|root| self.nodes[root as usize].subtree_hash)
            .unwrap_or_else(|| empty_hash(self.version))
    }

    pub fn position(&self, applied_sequence: SequenceNumber) -> VerificationPosition {
        VerificationPosition {
            version: self.version,
            applied_sequence,
            root_hash: self.root_hash(),
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn max_depth(&self) -> usize {
        let Some(root) = self.root else {
            return 0;
        };
        let mut maximum = 0;
        let mut stack = vec![(root, 1_usize)];
        while let Some((node_index, depth)) = stack.pop() {
            maximum = maximum.max(depth);
            let node = &self.nodes[node_index as usize];
            if let Some(left) = node.left {
                stack.push((left, depth + 1));
            }
            if let Some(right) = node.right {
                stack.push((right, depth + 1));
            }
        }
        maximum
    }

    pub fn resident_bytes(&self) -> usize {
        size_of::<Self>()
            + self.nodes.capacity() * size_of::<TreapNode>()
            + self.free_nodes.capacity() * size_of::<u32>()
    }

    pub fn resident_bytes_per_leaf(&self) -> usize {
        if self.len == 0 {
            return 0;
        }
        self.resident_bytes().div_ceil(self.len)
    }

    fn allocate_node(&mut self, node: TreapNode) -> Result<u32> {
        if let Some(node_index) = self.free_nodes.pop() {
            self.nodes[node_index as usize] = node;
            return Ok(node_index);
        }
        let node_index = u32::try_from(self.nodes.len()).map_err(|_| {
            Error::ResourceExhausted(
                "materialized verification index exceeds the u32 node limit".to_string(),
            )
        })?;
        self.nodes.push(node);
        Ok(node_index)
    }

    fn insert_index(&mut self, root: Option<u32>, node_index: u32) -> u32 {
        let Some(root) = root else {
            self.recompute_node(node_index);
            return node_index;
        };
        if self.heap_precedes(node_index, root) {
            let key = self.nodes[node_index as usize].key;
            let (left, right) = self.split(Some(root), key);
            self.nodes[node_index as usize].left = left;
            self.nodes[node_index as usize].right = right;
            self.recompute_node(node_index);
            return node_index;
        }

        let key = self.nodes[node_index as usize].key;
        if key < self.nodes[root as usize].key {
            let left = self.insert_index(self.nodes[root as usize].left, node_index);
            self.nodes[root as usize].left = Some(left);
        } else {
            let right = self.insert_index(self.nodes[root as usize].right, node_index);
            self.nodes[root as usize].right = Some(right);
        }
        self.recompute_node(root);
        root
    }

    fn split(&mut self, root: Option<u32>, key: LogicalLeafKey) -> (Option<u32>, Option<u32>) {
        let Some(root) = root else {
            return (None, None);
        };
        if self.nodes[root as usize].key < key {
            let right = self.nodes[root as usize].right;
            let (middle, greater) = self.split(right, key);
            self.nodes[root as usize].right = middle;
            self.recompute_node(root);
            (Some(root), greater)
        } else {
            let left = self.nodes[root as usize].left;
            let (less, middle) = self.split(left, key);
            self.nodes[root as usize].left = middle;
            self.recompute_node(root);
            (less, Some(root))
        }
    }

    fn remove_index(
        &mut self,
        root: Option<u32>,
        key: &LogicalLeafKey,
    ) -> (Option<u32>, Option<u32>) {
        let Some(root) = root else {
            return (None, None);
        };
        match key.cmp(&self.nodes[root as usize].key) {
            Ordering::Equal => {
                let replacement = self.merge(
                    self.nodes[root as usize].left,
                    self.nodes[root as usize].right,
                );
                (replacement, Some(root))
            }
            Ordering::Less => {
                let (left, removed) = self.remove_index(self.nodes[root as usize].left, key);
                self.nodes[root as usize].left = left;
                self.recompute_node(root);
                (Some(root), removed)
            }
            Ordering::Greater => {
                let (right, removed) = self.remove_index(self.nodes[root as usize].right, key);
                self.nodes[root as usize].right = right;
                self.recompute_node(root);
                (Some(root), removed)
            }
        }
    }

    fn merge(&mut self, left: Option<u32>, right: Option<u32>) -> Option<u32> {
        match (left, right) {
            (None, root) | (root, None) => root,
            (Some(left), Some(right)) if self.heap_precedes(left, right) => {
                let merged = self.merge(self.nodes[left as usize].right, Some(right));
                self.nodes[left as usize].right = merged;
                self.recompute_node(left);
                Some(left)
            }
            (Some(left), Some(right)) => {
                let merged = self.merge(Some(left), self.nodes[right as usize].left);
                self.nodes[right as usize].left = merged;
                self.recompute_node(right);
                Some(right)
            }
        }
    }

    fn heap_precedes(&self, left: u32, right: u32) -> bool {
        let left = &self.nodes[left as usize];
        let right = &self.nodes[right as usize];
        (left.priority, left.key) < (right.priority, right.key)
    }

    fn recompute_subtree(&mut self, node_index: u32) -> Hash {
        let left = self.nodes[node_index as usize]
            .left
            .map(|child| self.recompute_subtree(child));
        let right = self.nodes[node_index as usize]
            .right
            .map(|child| self.recompute_subtree(child));
        self.set_subtree_hash(node_index, left, right)
    }

    fn recompute_node(&mut self, node_index: u32) {
        let left = self.nodes[node_index as usize]
            .left
            .map(|child| self.nodes[child as usize].subtree_hash);
        let right = self.nodes[node_index as usize]
            .right
            .map(|child| self.nodes[child as usize].subtree_hash);
        self.set_subtree_hash(node_index, left, right);
    }

    fn set_subtree_hash(
        &mut self,
        node_index: u32,
        left: Option<Hash>,
        right: Option<Hash>,
    ) -> Hash {
        let empty = empty_hash(self.version);
        let node = &self.nodes[node_index as usize];
        let hash = hash_parts(
            self.version,
            NODE_DOMAIN,
            &[
                left.as_ref().unwrap_or(&empty),
                node.key.as_bytes(),
                &node.value_hash,
                right.as_ref().unwrap_or(&empty),
            ],
        );
        self.nodes[node_index as usize].subtree_hash = hash;
        hash
    }
}

fn empty_hash(version: VerificationRootVersion) -> Hash {
    hash_parts(version, EMPTY_DOMAIN, &[])
}

fn hash_parts(version: VerificationRootVersion, domain: &[u8], parts: &[&[u8]]) -> Hash {
    let mut digest = Sha256::new();
    for part in [domain, &version.as_u16().to_be_bytes()]
        .into_iter()
        .chain(parts.iter().copied())
    {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn key(rank: usize) -> LogicalLeafKey {
        LogicalLeafKey::new(LogicalLeafKind::Document, &rank.to_be_bytes())
            .expect("test key should be valid")
    }

    fn leaves(count: usize) -> Vec<(LogicalLeafKey, Vec<u8>)> {
        (0..count)
            .map(|rank| (key(rank), format!("value-{rank}").into_bytes()))
            .collect()
    }

    #[test]
    fn batch_and_incremental_verification_roots_match() {
        let leaves = leaves(256);
        let batch = MaterializedVerificationIndex::from_leaves(leaves.clone())
            .expect("batch root should build");
        let mut incremental = MaterializedVerificationIndex::new();
        for (key, value) in &leaves {
            assert!(
                incremental
                    .upsert(*key, value)
                    .expect("incremental insert should succeed")
            );
        }
        assert_eq!(batch.root_hash(), incremental.root_hash());

        for rank in (0..256).step_by(5) {
            incremental
                .upsert(key(rank), format!("updated-{rank}").as_bytes())
                .expect("incremental update should succeed");
        }
        for rank in (0..256).step_by(7) {
            incremental.remove(&key(rank));
        }
        let rebuilt = MaterializedVerificationIndex::from_leaves(
            (0..256).filter(|rank| rank % 7 != 0).map(|rank| {
                let value = if rank % 5 == 0 {
                    format!("updated-{rank}")
                } else {
                    format!("value-{rank}")
                };
                (key(rank), value.into_bytes())
            }),
        )
        .expect("post-change root should rebuild");
        assert_eq!(rebuilt.root_hash(), incremental.root_hash());
    }

    #[test]
    fn verification_root_is_independent_of_update_order() {
        let leaves = leaves(512);
        let expected = MaterializedVerificationIndex::from_leaves(leaves.clone())
            .expect("reference root should build")
            .root_hash();
        let mut order = (0..leaves.len()).collect::<Vec<_>>();
        let mut seed = 0x71d4_60a5_d9c8_2e13_u64;
        for index in (1..order.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            order.swap(index, seed as usize % (index + 1));
        }
        let mut random = MaterializedVerificationIndex::new();
        for index in order {
            random
                .upsert(leaves[index].0, &leaves[index].1)
                .expect("random insert should succeed");
        }
        let reverse = MaterializedVerificationIndex::from_leaves(leaves.into_iter().rev())
            .expect("reverse root should build");
        assert_eq!(random.root_hash(), expected);
        assert_eq!(reverse.root_hash(), expected);
    }

    #[test]
    fn delete_then_reinsert_restores_root() {
        let mut index =
            MaterializedVerificationIndex::from_leaves(leaves(128)).expect("root should build");
        let before = index.root_hash();
        let removed_key = key(47);
        assert!(index.remove(&removed_key));
        assert_ne!(index.root_hash(), before);
        assert!(
            index
                .upsert(removed_key, b"value-47")
                .expect("reinsert should succeed")
        );
        assert_eq!(index.root_hash(), before);
    }

    #[test]
    fn verification_root_version_separates_formats() {
        let leaves = leaves(32);
        let current = MaterializedVerificationIndex::from_leaves_with_version(
            VerificationRootVersion::current(),
            leaves.clone(),
        )
        .expect("current root should build");
        let future = MaterializedVerificationIndex::from_leaves_with_version(
            VerificationRootVersion(MATERIALIZED_VERIFICATION_ROOT_VERSION + 1),
            leaves,
        )
        .expect("test-only future root should build");
        assert_ne!(current.root_hash(), future.root_hash());
        assert!(
            VerificationPosition::from_parts(
                MATERIALIZED_VERIFICATION_ROOT_VERSION + 1,
                SequenceNumber(1),
                future.root_hash(),
            )
            .is_err()
        );
    }

    #[test]
    fn duplicate_batch_keys_are_rejected() {
        let duplicate = key(1);
        assert!(
            MaterializedVerificationIndex::from_leaves([
                (duplicate, b"first".as_slice()),
                (duplicate, b"second".as_slice()),
            ])
            .is_err()
        );
    }

    #[test]
    fn memory_derivation_stays_inside_the_approved_limit() {
        assert_eq!(VERIFICATION_INDEX_NODE_BYTES, 144);
        assert_eq!(VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF, 160);
    }

    #[test]
    fn generated_operation_histories_match_full_rebuilds() {
        for history_seed in 1..=16_u64 {
            let mut seed = history_seed;
            let mut model = BTreeMap::<usize, Vec<u8>>::new();
            let mut index = MaterializedVerificationIndex::new();
            for step in 0..500 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let rank = (seed as usize) % 256;
                if seed.rotate_left(17) % 4 == 0 {
                    assert_eq!(index.remove(&key(rank)), model.remove(&rank).is_some());
                } else {
                    let value = format!("seed-{history_seed}-step-{step}").into_bytes();
                    let inserted = index
                        .upsert(key(rank), &value)
                        .expect("generated upsert should succeed");
                    assert_eq!(inserted, model.insert(rank, value).is_none());
                }

                if step % 50 == 0 {
                    let rebuilt = MaterializedVerificationIndex::from_leaves(
                        model.iter().map(|(rank, value)| (key(*rank), value)),
                    )
                    .expect("generated model should rebuild");
                    assert_eq!(
                        index.root_hash(),
                        rebuilt.root_hash(),
                        "history seed {history_seed} diverged at step {step}"
                    );
                    assert_eq!(index.len(), model.len());
                }
            }
        }
    }

    #[test]
    fn logical_leaf_families_and_positions_are_opaque() {
        let identity = b"tasks/example";
        let document = LogicalLeafKey::new(LogicalLeafKind::Document, identity)
            .expect("document identity should be valid");
        let schema = LogicalLeafKey::new(LogicalLeafKind::Schema, identity)
            .expect("schema identity should be valid");
        assert_ne!(document, schema);
        assert!(LogicalLeafKey::new(LogicalLeafKind::Document, b"").is_err());

        let mut index = MaterializedVerificationIndex::new();
        index
            .upsert(document, b"canonical value")
            .expect("position leaf should insert");
        let position = index.position(SequenceNumber(41));
        assert_eq!(position.version(), VerificationRootVersion::current());
        assert_eq!(position.applied_sequence(), SequenceNumber(41));
        assert_eq!(position.root_hash(), &index.root_hash());
    }
}
