//! Owns the complete process-local materialized-verification concept: versioned
//! Merkle structure, canonical journal-delta decoder (`delta_decoder`), session
//! tracker, and its contract tests (`tests`). Keeping these parts in one module
//! tree makes one format and one fail-closed transition boundary reviewable as
//! a unit.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;

use nimbus_core::{Error, Result, SequenceNumber, TableId, TenantEventRecord};
use sha2::{Digest, Sha256};

use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry};

mod delta_decoder;

use delta_decoder::{canonical_snapshot_seed, deltas_for_validated_record};

pub const MATERIALIZED_VERIFICATION_ROOT_VERSION: u16 = 2;
pub const VERIFICATION_INDEX_MAX_DEPTH: usize = 128;

/// The approved million-leaf resident-memory budget is 192 bytes per logical
/// leaf.
///
/// A node is 148 bytes on supported targets. The IMV2 measurement assigns a
/// conservative 16-byte allocator allowance, for a budgeted total of 164
/// bytes. The remaining 28 bytes cover the index and free-list allocation
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
    ResourcePathBinding,
    TriggerDeliveryCursor,
}

impl LogicalLeafKind {
    const fn tag(self) -> u8 {
        match self {
            Self::TableIdentity => 1,
            Self::Schema => 2,
            Self::Document => 3,
            Self::ScheduledExecution => 4,
            Self::ResourcePathBinding => 5,
            Self::TriggerDeliveryCursor => 6,
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
    subtree_depth: u32,
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
            subtree_depth: 1,
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
        index.recompute_all();
        if index.max_depth() > VERIFICATION_INDEX_MAX_DEPTH {
            return Err(Error::ResourceExhausted(format!(
                "materialized verification index depth {} exceeds the safety limit {}",
                index.max_depth(),
                VERIFICATION_INDEX_MAX_DEPTH
            )));
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
        let (node_index, reused) = self.allocate_node(node)?;
        self.root = Some(self.insert_index(self.root, node_index));
        self.len += 1;
        if self.max_depth() > VERIFICATION_INDEX_MAX_DEPTH {
            let (root, removed) = self.remove_index(self.root, &key);
            self.root = root;
            self.len -= 1;
            debug_assert_eq!(removed, Some(node_index));
            if reused {
                self.free_nodes.push(node_index);
            } else {
                debug_assert_eq!(node_index as usize + 1, self.nodes.len());
                self.nodes.pop();
            }
            return Err(Error::ResourceExhausted(format!(
                "materialized verification index depth would exceed the safety limit {}",
                VERIFICATION_INDEX_MAX_DEPTH
            )));
        }
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
        self.root
            .map(|root| self.nodes[root as usize].subtree_depth as usize)
            .unwrap_or(0)
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

    fn allocate_node(&mut self, node: TreapNode) -> Result<(u32, bool)> {
        if let Some(node_index) = self.free_nodes.pop() {
            self.nodes[node_index as usize] = node;
            return Ok((node_index, true));
        }
        let node_index = u32::try_from(self.nodes.len()).map_err(|_| {
            Error::ResourceExhausted(
                "materialized verification index exceeds the u32 node limit".to_string(),
            )
        })?;
        self.nodes.push(node);
        Ok((node_index, false))
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

    fn recompute_all(&mut self) {
        let Some(root) = self.root else {
            return;
        };
        let mut stack = vec![(root, false)];
        while let Some((node_index, visited)) = stack.pop() {
            if visited {
                self.recompute_node(node_index);
                continue;
            }
            stack.push((node_index, true));
            let node = &self.nodes[node_index as usize];
            if let Some(right) = node.right {
                stack.push((right, false));
            }
            if let Some(left) = node.left {
                stack.push((left, false));
            }
        }
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
        let subtree_depth = 1 + node
            .left
            .map(|child| self.nodes[child as usize].subtree_depth)
            .unwrap_or(0)
            .max(
                node.right
                    .map(|child| self.nodes[child as usize].subtree_depth)
                    .unwrap_or(0),
            );
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
        self.nodes[node_index as usize].subtree_depth = subtree_depth;
        hash
    }
}

/// One exact change to the canonical materialized-state leaf set.
///
/// Construction stays with the applied-record decoder in `delta_decoder`. Callers cannot
/// assemble raw keys or values that use a second canonicalization path.
#[derive(Debug, Clone)]
enum MaterializedStateDelta {
    Upsert(MaterializedStateLeaf),
    Remove(LogicalLeafKey),
    /// The record changed state whose exact leaf set is not present in the
    /// journal event. A bounded session must rebuild from materialized state.
    Invalidate,
}

/// A validated canonical leaf carried by an exact materialized-state delta.
#[derive(Debug, Clone)]
struct MaterializedStateLeaf {
    key: LogicalLeafKey,
    value: Vec<u8>,
}

impl MaterializedStateLeaf {
    fn new(kind: LogicalLeafKind, identity: Vec<u8>, value: Vec<u8>) -> Result<Self> {
        Ok(Self {
            key: LogicalLeafKey::new(kind, &identity)?,
            value,
        })
    }
}

/// Result of offering one successfully applied record to a verification
/// session's process-local tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializedDeltaApplyOutcome {
    Advanced(VerificationPosition),
    Duplicate(VerificationPosition),
    Invalidated,
}

/// Session-owned root state over one contiguous applied journal prefix.
///
/// This type is deliberately not installed in a provider or in Nimbus's
/// materialized serving cache. IMV5 retains it only inside bounded verification
/// sessions. Any unrepresentable event, sequence gap, invalid record, or tree
/// update error drops the derived index without affecting normal storage work.
#[derive(Debug, Clone)]
pub struct MaterializedVerificationTracker {
    active: Option<ActiveVerificationIndex>,
}

/// A process-local generation captured by a bounded verification session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializedVerificationGeneration(u64);

/// Shared invalidation signal for state replacement paths that cannot publish
/// an exact journal delta, such as a libSQL replica-cache swap.
#[derive(Debug, Clone, Default)]
pub struct MaterializedVerificationInvalidator {
    state: Arc<parking_lot::Mutex<MaterializedVerificationInvalidationState>>,
}

#[derive(Debug, Default)]
struct MaterializedVerificationInvalidationState {
    generation: u64,
    active_updates: u64,
}

/// Keeps a replacement generation non-current for its complete mutation
/// window. Overlapping replacement work shares one non-current epoch. Derived
/// verification state must not turn valid storage concurrency into a write
/// failure.
pub(crate) struct MaterializedVerificationUpdateGuard {
    invalidator: MaterializedVerificationInvalidator,
}

impl MaterializedVerificationInvalidator {
    pub fn generation(&self) -> MaterializedVerificationGeneration {
        MaterializedVerificationGeneration(self.state.lock().generation)
    }

    pub(crate) fn begin_update(&self) -> Result<MaterializedVerificationUpdateGuard> {
        let mut state = self.state.lock();
        if state.active_updates == 0 {
            debug_assert_eq!(state.generation & 1, 0);
            state.generation = state.generation.wrapping_add(1);
        }
        state.active_updates = state.active_updates.checked_add(1).ok_or_else(|| {
            Error::ResourceExhausted(
                "materialized verification replacement count overflow".to_string(),
            )
        })?;
        drop(state);
        Ok(MaterializedVerificationUpdateGuard {
            invalidator: self.clone(),
        })
    }

    pub fn is_current(&self, generation: MaterializedVerificationGeneration) -> bool {
        generation.0 & 1 == 0 && self.generation() == generation
    }
}

impl Drop for MaterializedVerificationUpdateGuard {
    fn drop(&mut self) {
        let mut state = self.invalidator.state.lock();
        debug_assert!(state.active_updates > 0);
        state.active_updates = state.active_updates.saturating_sub(1);
        if state.active_updates == 0 {
            debug_assert_eq!(state.generation & 1, 1);
            state.generation = state.generation.wrapping_add(1);
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveVerificationIndex {
    index: MaterializedVerificationIndex,
    applied_sequence: SequenceNumber,
    table_identities: HashMap<TableId, TableIdentitySnapshotEntry>,
}

struct MaterializedVerificationSeed {
    leaves: Vec<(LogicalLeafKey, Vec<u8>)>,
    table_identities: HashMap<TableId, TableIdentitySnapshotEntry>,
}

impl MaterializedVerificationTracker {
    pub fn from_snapshot(snapshot: &MaterializedJournalSnapshot) -> Result<Self> {
        let seed = canonical_snapshot_seed(snapshot)?;
        Ok(Self {
            active: Some(ActiveVerificationIndex {
                index: MaterializedVerificationIndex::from_leaves(seed.leaves)?,
                applied_sequence: snapshot.applied_sequence,
                table_identities: seed.table_identities,
            }),
        })
    }

    pub fn position(&self) -> Option<VerificationPosition> {
        self.active
            .as_ref()
            .map(|active| active.index.position(active.applied_sequence))
    }

    pub fn is_valid(&self) -> bool {
        self.active.is_some()
    }

    /// Returns the logical leaf count retained by this session tracker.
    pub fn leaf_count(&self) -> usize {
        self.active.as_ref().map_or(0, |active| active.index.len())
    }

    /// Returns the storage-owned resident-byte estimate for this tracker.
    pub fn resident_bytes(&self) -> usize {
        self.active
            .as_ref()
            .map_or(0, |active| active.index.resident_bytes())
    }

    /// Applies a record only after its storage effects are known to be visible.
    ///
    /// A caller must never invoke this method for durable append alone. The
    /// tracker publishes the new sequence only after every exact delta has
    /// updated the tree.
    pub fn apply_applied_record(
        &mut self,
        record: &TenantEventRecord,
    ) -> MaterializedDeltaApplyOutcome {
        if record.validate_integrity().is_err() {
            self.invalidate();
            return MaterializedDeltaApplyOutcome::Invalidated;
        }
        let Some(active) = self.active.as_mut() else {
            return MaterializedDeltaApplyOutcome::Invalidated;
        };
        if record.sequence.0 <= active.applied_sequence.0 {
            return MaterializedDeltaApplyOutcome::Duplicate(
                active.index.position(active.applied_sequence),
            );
        }
        if record.sequence.0 != active.applied_sequence.0.saturating_add(1) {
            self.invalidate();
            return MaterializedDeltaApplyOutcome::Invalidated;
        }
        let deltas = match deltas_for_validated_record(record, &mut active.table_identities) {
            Ok(deltas) => deltas,
            Err(_) => {
                self.invalidate();
                return MaterializedDeltaApplyOutcome::Invalidated;
            }
        };
        for delta in deltas {
            let result = match delta {
                MaterializedStateDelta::Upsert(leaf) => {
                    active.index.upsert(leaf.key, &leaf.value).map(|_| ())
                }
                MaterializedStateDelta::Remove(key) => {
                    active.index.remove(&key);
                    Ok(())
                }
                MaterializedStateDelta::Invalidate => {
                    self.invalidate();
                    return MaterializedDeltaApplyOutcome::Invalidated;
                }
            };
            if result.is_err() {
                self.invalidate();
                return MaterializedDeltaApplyOutcome::Invalidated;
            }
        }
        let Some(active) = self.active.as_mut() else {
            return MaterializedDeltaApplyOutcome::Invalidated;
        };
        active.applied_sequence = record.sequence;
        MaterializedDeltaApplyOutcome::Advanced(active.index.position(record.sequence))
    }

    pub fn invalidate(&mut self) {
        self.active = None;
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
mod tests;
