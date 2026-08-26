//! Owns the complete process-local materialized-verification concept: versioned
//! Merkle structure, canonical journal-delta decoder, session tracker, and its
//! contract tests. Keeping these parts together makes one format and one
//! fail-closed transition boundary reviewable as a unit.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;

use nimbus_core::{
    Error, Result, SchemaChangeEvent, SequenceNumber, TableId, TenantEventKind, TenantEventRecord,
    WriteOp,
};
use sha2::{Digest, Sha256};

use crate::materialized_position::{
    canonical_document_identity, canonical_document_value, canonical_scheduled_execution_identity,
    canonical_scheduled_execution_value, canonical_schema_identity,
    canonical_schema_identity_for_name, canonical_schema_value, canonical_table_identity_identity,
    canonical_table_identity_value,
};
use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry};

pub const MATERIALIZED_VERIFICATION_ROOT_VERSION: u16 = 1;
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
/// Construction stays with the applied-record decoder below. Callers cannot
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

fn deltas_for_validated_record(
    record: &TenantEventRecord,
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
) -> Result<Vec<MaterializedStateDelta>> {
    let mut deltas = Vec::new();
    if record.events.is_empty() {
        append_document_deltas(&record.writes, table_identities, &mut deltas)?;
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            append_scheduled_execution_delta(execution_id, &mut deltas)?;
        }
        return Ok(deltas);
    }
    for event in &record.events {
        match event {
            TenantEventKind::DocumentWrite { writes } => {
                append_document_deltas(writes, table_identities, &mut deltas)?;
            }
            TenantEventKind::SchemaChange { change } => match change.as_ref() {
                SchemaChangeEvent::SetTable {
                    table_id, current, ..
                } => {
                    append_default_table_identity_deltas(
                        &current.table,
                        table_id,
                        table_identities,
                        true,
                        &mut deltas,
                    )?;
                    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
                        LogicalLeafKind::Schema,
                        canonical_schema_identity(current)?,
                        canonical_schema_value(current)?,
                    )?));
                }
                SchemaChangeEvent::DeleteTable {
                    table, previous, ..
                } => {
                    let identity = if let Some(previous) = previous {
                        canonical_schema_identity(previous)?
                    } else {
                        canonical_schema_identity_for_name(table.as_str())?
                    };
                    deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
                        LogicalLeafKind::Schema,
                        &identity,
                    )?));
                }
            },
            // A lifecycle event can delete an unbounded document family, and
            // the event does not carry those document IDs. Rebuild instead of
            // pretending that a partial delta is exact.
            TenantEventKind::TableLifecycle { .. } => {
                deltas.push(MaterializedStateDelta::Invalidate)
            }
            TenantEventKind::ScheduledExecution { execution_id } => {
                append_scheduled_execution_delta(execution_id, &mut deltas)?;
            }
            TenantEventKind::IndexLifecycle { .. }
            | TenantEventKind::TriggerDelivery { .. }
            | TenantEventKind::Barrier { .. } => {}
        }
    }
    Ok(deltas)
}

fn append_document_deltas(
    writes: &[WriteOp],
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    for write in writes {
        let allow_create = write.previous.is_none() && write.current.is_some();
        append_default_table_identity_deltas(
            &write.table,
            &write.table_id,
            table_identities,
            allow_create,
            deltas,
        )?;
        if let Some(current) = &write.current {
            deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
                LogicalLeafKind::Document,
                canonical_document_identity(current)?,
                canonical_document_value(current)?,
            )?));
        } else if let Some(previous) = &write.previous {
            deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
                LogicalLeafKind::Document,
                &canonical_document_identity(previous)?,
            )?));
        } else {
            return Err(Error::Internal(format!(
                "materialized document write {} has neither a previous nor current image",
                write.doc_id
            )));
        }
    }
    Ok(())
}

fn append_default_table_identity_deltas(
    table: &nimbus_core::TableName,
    table_id: &TableId,
    table_identities: &mut HashMap<TableId, TableIdentitySnapshotEntry>,
    allow_create: bool,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    if let Some(identity) = table_identities.get(table_id) {
        if identity.table != *table {
            return Err(Error::Internal(format!(
                "materialized write table {} disagrees with table id {} owned by {}",
                table, table_id, identity.table
            )));
        }
        if identity.is_active() {
            append_table_identity_upsert(identity, deltas)?;
            return Ok(());
        }
        if identity.state == nimbus_core::TableState::Deleting {
            return Err(Error::Internal(format!(
                "materialized write references deleting table identity {}",
                table_id
            )));
        }
    }
    if !table_identities.contains_key(table_id) && !allow_create {
        return Err(Error::Internal(format!(
            "materialized write for unknown table identity {} cannot be exact",
            table_id
        )));
    }

    if let Some(previous_active) = table_identities
        .values()
        .find(|identity| identity.table == *table && identity.is_active())
        .cloned()
    {
        deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
            LogicalLeafKind::TableIdentity,
            &canonical_table_identity_identity(&previous_active)?,
        )?));
        let deleting = TableIdentitySnapshotEntry {
            namespace: crate::table_identity::deleting_table_namespace(&previous_active.table_id),
            table: previous_active.table,
            table_id: previous_active.table_id.clone(),
            state: nimbus_core::TableState::Deleting,
        };
        append_table_identity_upsert(&deleting, deltas)?;
        table_identities.insert(deleting.table_id.clone(), deleting);
    }

    if let Some(staged_hidden) = table_identities.get(table_id).cloned() {
        deltas.push(MaterializedStateDelta::Remove(LogicalLeafKey::new(
            LogicalLeafKind::TableIdentity,
            &canonical_table_identity_identity(&staged_hidden)?,
        )?));
    }
    let identity = TableIdentitySnapshotEntry::default_namespace(table.clone(), table_id.clone());
    append_table_identity_upsert(&identity, deltas)?;
    table_identities.insert(table_id.clone(), identity);
    Ok(())
}

fn append_table_identity_upsert(
    identity: &TableIdentitySnapshotEntry,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
        LogicalLeafKind::TableIdentity,
        canonical_table_identity_identity(identity)?,
        canonical_table_identity_value(identity)?,
    )?));
    Ok(())
}

fn append_scheduled_execution_delta(
    execution_id: &str,
    deltas: &mut Vec<MaterializedStateDelta>,
) -> Result<()> {
    deltas.push(MaterializedStateDelta::Upsert(MaterializedStateLeaf::new(
        LogicalLeafKind::ScheduledExecution,
        canonical_scheduled_execution_identity(execution_id)?,
        canonical_scheduled_execution_value(execution_id)?,
    )?));
    Ok(())
}

fn canonical_snapshot_seed(
    snapshot: &MaterializedJournalSnapshot,
) -> Result<MaterializedVerificationSeed> {
    let state = snapshot.canonical_state()?;
    let mut leaves = Vec::with_capacity(
        state.table_identities().len()
            + state.schema_tables().len()
            + state.documents().len()
            + state.scheduled_execution_ids().len(),
    );
    let mut table_identities = HashMap::with_capacity(state.table_identities().len());
    for identity in state.table_identities() {
        table_identities.insert(identity.table_id.clone(), identity.clone());
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::TableIdentity,
                &canonical_table_identity_identity(identity)?,
            )?,
            canonical_table_identity_value(identity)?,
        ));
    }
    for table in state.schema_tables() {
        leaves.push((
            LogicalLeafKey::new(LogicalLeafKind::Schema, &canonical_schema_identity(table)?)?,
            canonical_schema_value(table)?,
        ));
    }
    for document in state.documents() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::Document,
                &canonical_document_identity(document)?,
            )?,
            canonical_document_value(document)?,
        ));
    }
    for execution_id in state.scheduled_execution_ids() {
        leaves.push((
            LogicalLeafKey::new(
                LogicalLeafKind::ScheduledExecution,
                &canonical_scheduled_execution_identity(execution_id)?,
            )?,
            canonical_scheduled_execution_value(execution_id)?,
        ));
    }
    Ok(MaterializedVerificationSeed {
        leaves,
        table_identities,
    })
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
    use std::str::FromStr;

    use nimbus_core::{
        Document, DocumentId, Schema, TableId, TableLifecycleEvent, TableName, TableSchema,
        Timestamp, WriteOp, WriteOpType,
    };
    use serde_json::json;

    use super::*;
    use crate::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION;

    fn key(rank: usize) -> LogicalLeafKey {
        LogicalLeafKey::new(LogicalLeafKind::Document, &rank.to_be_bytes())
            .expect("test key should be valid")
    }

    fn leaves(count: usize) -> Vec<(LogicalLeafKey, Vec<u8>)> {
        (0..count)
            .map(|rank| (key(rank), format!("value-{rank}").into_bytes()))
            .collect()
    }

    fn empty_snapshot() -> MaterializedJournalSnapshot {
        MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: SequenceNumber(0),
            durable_head: SequenceNumber(0),
            table_identities: Vec::new(),
            schema: Schema::default(),
            documents: Vec::new(),
            resource_path_bindings: Vec::new(),
            scheduled_execution_ids: Vec::new(),
            trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
        }
    }

    fn inserted_document_record(sequence: u64) -> (TenantEventRecord, TableId, Document) {
        let table = TableName::new("tasks").expect("table should be valid");
        let table_id = TableId::from_str("tasks-table").expect("table id should be valid");
        let document = Document::with_id_at(
            DocumentId::from_key("task-1").expect("document id should be valid"),
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("one"))]),
            Timestamp(10),
        );
        let record = TenantEventRecord::new(
            SequenceNumber(sequence),
            Timestamp(11),
            vec![WriteOp {
                table,
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(document.clone()),
            }],
            None,
        )
        .expect("record should be valid");
        (record, table_id, document)
    }

    fn snapshot_after_insert(
        record: &TenantEventRecord,
        table_id: TableId,
        document: Document,
    ) -> MaterializedJournalSnapshot {
        MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: record.sequence,
            durable_head: record.sequence,
            table_identities: vec![TableIdentitySnapshotEntry::default_namespace(
                document.table.clone(),
                table_id,
            )],
            schema: Schema::default(),
            documents: vec![document],
            resource_path_bindings: Vec::new(),
            scheduled_execution_ids: Vec::new(),
            trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
        }
    }

    fn assert_local_provider_applied_delta(
        export: impl Fn() -> Result<MaterializedJournalSnapshot>,
        append: impl Fn(&[TenantEventRecord]) -> Result<()>,
        apply: impl Fn(&[TenantEventRecord]) -> Result<()>,
    ) {
        let (record, _, _) = inserted_document_record(1);
        let mut tracker = MaterializedVerificationTracker::from_snapshot(
            &export().expect("baseline snapshot should export"),
        )
        .expect("baseline tracker should build");
        append(std::slice::from_ref(&record)).expect("durable append should succeed");
        apply(std::slice::from_ref(&record)).expect("materialized apply should succeed");
        let MaterializedDeltaApplyOutcome::Advanced(incremental) =
            tracker.apply_applied_record(&record)
        else {
            panic!("post-apply delta should advance the tracker");
        };
        let rebuilt = MaterializedVerificationTracker::from_snapshot(
            &export().expect("post-apply snapshot should export"),
        )
        .expect("post-apply tracker should rebuild")
        .position()
        .expect("post-apply tracker should be valid");
        assert_eq!(incremental, rebuilt);
    }

    #[test]
    fn local_provider_apply_paths_publish_only_post_apply_deltas() {
        let redb = crate::TenantStore::create_in_memory().expect("redb store should open");
        assert_local_provider_applied_delta(
            || redb.export_materialized_journal_snapshot(),
            |records| redb.append_durable_records_batch(records),
            |records| redb.apply_durable_records_batch(records),
        );

        let memory = crate::MemoryTenantStore::new();
        assert_local_provider_applied_delta(
            || memory.export_materialized_journal_snapshot(),
            |records| memory.append_durable_records_batch(records),
            |records| memory.apply_durable_records_batch(records),
        );

        let directory = tempfile::tempdir().expect("sqlite tempdir should create");
        let sqlite = crate::SqliteTenantStore::open(directory.path().join("tenant.sqlite3"))
            .expect("sqlite store should open");
        assert_local_provider_applied_delta(
            || sqlite.export_materialized_journal_snapshot(),
            |records| sqlite.append_durable_records_batch(records),
            |records| sqlite.apply_durable_records_batch(records),
        );
    }

    #[test]
    fn snapshot_restore_invalidates_local_verification_generations() {
        let snapshot = empty_snapshot();

        let redb = crate::TenantStore::create_in_memory().expect("redb store should open");
        let redb_generation = redb.materialized_verification_generation();
        redb.restore_materialized_journal_from_snapshot(&snapshot)
            .expect("redb snapshot should restore");
        assert!(!redb.materialized_verification_generation_is_current(redb_generation));

        let memory = crate::MemoryTenantStore::new();
        let memory_generation = memory.materialized_verification_generation();
        memory
            .restore_materialized_journal_from_snapshot(&snapshot)
            .expect("memory snapshot should restore");
        assert!(!memory.materialized_verification_generation_is_current(memory_generation));

        let directory = tempfile::tempdir().expect("sqlite tempdir should create");
        let sqlite = crate::SqliteTenantStore::open(directory.path().join("tenant.sqlite3"))
            .expect("sqlite store should open");
        let sqlite_generation = sqlite.materialized_verification_generation();
        sqlite
            .restore_materialized_journal_from_snapshot(&snapshot)
            .expect("sqlite snapshot should restore");
        assert!(!sqlite.materialized_verification_generation_is_current(sqlite_generation));
    }

    #[test]
    fn overlapping_replacements_share_one_non_current_generation() {
        let invalidator = MaterializedVerificationInvalidator::default();
        let before = invalidator.generation();
        assert!(invalidator.is_current(before));

        let update = invalidator
            .begin_update()
            .expect("first replacement should begin");
        let during = invalidator.generation();
        assert!(!invalidator.is_current(before));
        assert!(!invalidator.is_current(during));
        let overlapping = invalidator
            .begin_update()
            .expect("an overlapping replacement should join the invalidation epoch");

        drop(update);
        assert_eq!(invalidator.generation(), during);
        assert!(!invalidator.is_current(during));

        drop(overlapping);
        let after = invalidator.generation();
        assert!(invalidator.is_current(after));
        assert!(!invalidator.is_current(before));
        assert!(!invalidator.is_current(during));
    }

    #[test]
    fn schema_scheduler_and_lifecycle_records_have_safe_verification_effects() {
        let store = crate::TenantStore::create_in_memory().expect("store should open");
        let mut tracker = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("baseline snapshot should export"),
        )
        .expect("baseline tracker should build");
        let table = TableName::new("tasks").expect("table should be valid");
        let table_id = TableId::from_str("tasks-table").expect("table id should be valid");
        let schema = TableSchema {
            table: table.clone(),
            fields: Vec::new(),
            indexes: Vec::new(),
            access_policy: None,
        };
        let deleted_schema = schema.clone();
        let schema_record = TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(1),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema,
            },
        )
        .expect("schema record should build");
        let scheduled_record = TenantEventRecord::from_events(
            SequenceNumber(2),
            Timestamp(2),
            vec![TenantEventKind::ScheduledExecution {
                execution_id: "execution-1".to_string(),
            }],
        )
        .expect("scheduled record should build");
        let schema_delete_record = TenantEventRecord::schema_change(
            SequenceNumber(3),
            Timestamp(3),
            SchemaChangeEvent::DeleteTable {
                table: table.clone(),
                table_id: Some(table_id),
                previous: Some(deleted_schema),
            },
        )
        .expect("schema delete record should build");

        for record in [&schema_record, &scheduled_record, &schema_delete_record] {
            store
                .append_durable_records_batch(std::slice::from_ref(record))
                .expect("record should append");
            store
                .apply_durable_records_batch(std::slice::from_ref(record))
                .expect("record should apply");
            let MaterializedDeltaApplyOutcome::Advanced(incremental) =
                tracker.apply_applied_record(record)
            else {
                panic!("exact record should advance");
            };
            let rebuilt = MaterializedVerificationTracker::from_snapshot(
                &store
                    .export_materialized_journal_snapshot()
                    .expect("snapshot should export"),
            )
            .expect("snapshot tracker should rebuild")
            .position()
            .expect("rebuilt tracker should be valid");
            assert_eq!(incremental, rebuilt);
        }

        let lifecycle_record = TenantEventRecord::table_lifecycle(
            SequenceNumber(4),
            Timestamp(4),
            TableLifecycleEvent::StageHidden {
                table: TableName::new("replacement").expect("table should be valid"),
                table_id: TableId::from_str("replacement-table").expect("table id should be valid"),
            },
        )
        .expect("lifecycle record should build");
        store
            .append_durable_records_batch(std::slice::from_ref(&lifecycle_record))
            .expect("lifecycle record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&lifecycle_record))
            .expect("lifecycle record should apply");
        assert_eq!(
            tracker.apply_applied_record(&lifecycle_record),
            MaterializedDeltaApplyOutcome::Invalidated
        );
        assert!(!tracker.is_valid());
    }

    #[test]
    fn document_insert_update_delete_deltas_match_full_rebuilds() {
        let store = crate::TenantStore::create_in_memory().expect("store should open");
        let mut tracker = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("baseline snapshot should export"),
        )
        .expect("baseline tracker should build");
        let (insert, table_id, original) = inserted_document_record(1);
        let mut updated = original.clone();
        updated.update_time = Timestamp(20);
        updated.fields.insert("title".to_string(), json!("two"));
        let update = TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(21),
            vec![WriteOp {
                table: original.table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Update,
                doc_id: original.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(original),
                current: Some(updated.clone()),
            }],
            None,
        )
        .expect("update record should build");
        let delete = TenantEventRecord::new(
            SequenceNumber(3),
            Timestamp(22),
            vec![WriteOp {
                table: updated.table.clone(),
                table_id,
                op_type: WriteOpType::Delete,
                doc_id: updated.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(updated),
                current: None,
            }],
            None,
        )
        .expect("delete record should build");

        for record in [insert, update, delete] {
            store
                .append_durable_records_batch(std::slice::from_ref(&record))
                .expect("record should append");
            store
                .apply_durable_records_batch(std::slice::from_ref(&record))
                .expect("record should apply");
            assert!(matches!(
                tracker.apply_applied_record(&record),
                MaterializedDeltaApplyOutcome::Advanced(_)
            ));
            let rebuilt = MaterializedVerificationTracker::from_snapshot(
                &store
                    .export_materialized_journal_snapshot()
                    .expect("snapshot should export"),
            )
            .expect("snapshot tracker should rebuild");
            assert_eq!(tracker.position(), rebuilt.position());
        }
    }

    #[test]
    fn hidden_lineage_document_write_matches_provider_activation() {
        let store = crate::TenantStore::create_in_memory().expect("store should open");
        let (initial_record, initial_table_id, _) = inserted_document_record(1);
        store
            .append_durable_records_batch(std::slice::from_ref(&initial_record))
            .expect("initial write should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&initial_record))
            .expect("initial write should apply");
        let table = TableName::new("tasks").expect("table should be valid");
        let hidden_id = TableId::from_str("hidden-table").expect("table id should be valid");
        store
            .stage_hidden_table_identity(&table, &hidden_id)
            .expect("hidden identity should stage");
        let checkpoint = store
            .export_materialized_journal_snapshot()
            .expect("checkpoint should export");
        let hidden_document = Document::with_id_at(
            DocumentId::from_key("hidden-task").expect("document id should be valid"),
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("hidden"))]),
            Timestamp(10),
        );
        let record = TenantEventRecord::new(
            SequenceNumber(checkpoint.applied_sequence.0 + 1),
            Timestamp(11),
            vec![WriteOp {
                table,
                table_id: hidden_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: hidden_document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(hidden_document),
            }],
            None,
        )
        .expect("hidden write record should build");
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("hidden write should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("hidden write should apply");
        let expected = store
            .export_materialized_journal_snapshot()
            .expect("applied snapshot should export");
        assert!(
            expected
                .table_identities
                .iter()
                .any(|identity| { identity.table_id == hidden_id && identity.is_active() })
        );
        assert!(expected.table_identities.iter().any(|identity| {
            identity.table_id == initial_table_id
                && identity.state == nimbus_core::TableState::Deleting
        }));
        assert!(!expected.table_identities.iter().any(|identity| {
            identity.table_id == hidden_id && identity.state == nimbus_core::TableState::Hidden
        }));
        assert_eq!(expected.documents.len(), 2);

        let mut tracker = MaterializedVerificationTracker::from_snapshot(&checkpoint)
            .expect("checkpoint tracker should build");
        assert!(matches!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Advanced(_)
        ));
        let rebuilt = MaterializedVerificationTracker::from_snapshot(&expected)
            .expect("expected tracker should build");
        assert_eq!(tracker.position(), rebuilt.position());
    }

    #[test]
    fn root_advances_with_applied_sequence() {
        let (record, table_id, document) = inserted_document_record(1);
        let expected = MaterializedVerificationTracker::from_snapshot(&snapshot_after_insert(
            &record, table_id, document,
        ))
        .expect("post-apply snapshot should build")
        .position()
        .expect("post-apply tracker should be valid");
        let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
            .expect("empty tracker should build");

        let MaterializedDeltaApplyOutcome::Advanced(actual) = tracker.apply_applied_record(&record)
        else {
            panic!("contiguous applied record should advance the tracker");
        };
        assert_eq!(actual.applied_sequence(), SequenceNumber(1));
        assert_eq!(actual.root_hash(), expected.root_hash());
    }

    #[test]
    fn failed_apply_does_not_advance_root() {
        let (record, _, _) = inserted_document_record(2);
        let store = crate::TenantStore::create_in_memory().expect("store should open");
        let tracker = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("snapshot should export"),
        )
        .expect("tracker should build");
        let before = tracker.position().expect("tracker should be valid");

        assert!(store.apply_durable_records_batch(&[record]).is_err());
        assert_eq!(tracker.position(), Some(before));
    }

    #[test]
    fn replay_duplicate_keeps_root() {
        let (record, _, _) = inserted_document_record(1);
        let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
            .expect("tracker should build");
        let MaterializedDeltaApplyOutcome::Advanced(first) = tracker.apply_applied_record(&record)
        else {
            panic!("first record should advance");
        };
        assert_eq!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Duplicate(first)
        );
        assert_eq!(tracker.position(), Some(first));
    }

    #[test]
    fn corrupt_index_never_reports_success() {
        let (record, _, _) = inserted_document_record(1);
        let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
            .expect("tracker should build");
        assert!(matches!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Advanced(_)
        ));
        let mut corrupt = record;
        corrupt.integrity_sha256[0] ^= 0xff;

        assert_eq!(
            tracker.apply_applied_record(&corrupt),
            MaterializedDeltaApplyOutcome::Invalidated
        );
        assert!(!tracker.is_valid());
        assert_eq!(tracker.position(), None);
    }

    #[test]
    fn full_scrub_detects_state_tamper_at_same_sequence() {
        let (record, _, document) = inserted_document_record(1);
        let store = crate::MemoryTenantStore::new();
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should apply");

        let expected_snapshot = store
            .export_materialized_journal_snapshot()
            .expect("expected snapshot should export");
        let expected = MaterializedVerificationTracker::from_snapshot(&expected_snapshot)
            .expect("expected tracker should build")
            .position()
            .expect("expected tracker should be valid");

        let mut tampered = document;
        tampered
            .fields
            .insert("title".to_string(), serde_json::json!("tampered"));
        store
            .tamper_document_for_testing(tampered)
            .expect("test state should tamper");

        let actual_snapshot = store
            .export_materialized_journal_snapshot()
            .expect("tampered snapshot should export");
        let actual = MaterializedVerificationTracker::from_snapshot(&actual_snapshot)
            .expect("tampered tracker should build")
            .position()
            .expect("tampered tracker should be valid");

        assert_eq!(actual.applied_sequence(), expected.applied_sequence());
        assert_ne!(actual.root_hash(), expected.root_hash());
    }

    #[test]
    fn apply_gap_invalidates_verification_index() {
        let (record, _, _) = inserted_document_record(2);
        let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
            .expect("tracker should build");

        assert_eq!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Invalidated
        );
        assert!(!tracker.is_valid());
        assert_eq!(tracker.position(), None);
    }

    #[test]
    fn durable_head_ahead_of_apply_does_not_advance_verification_root() {
        let (record, _, _) = inserted_document_record(1);
        let store = crate::TenantStore::create_in_memory().expect("store should open");
        let tracker = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("snapshot should export"),
        )
        .expect("tracker should build");
        let before = tracker.position().expect("tracker should be valid");

        store
            .append_durable_records_batch(&[record])
            .expect("durable append should succeed");
        let progress = store.journal_progress().expect("progress should load");
        assert_eq!(progress.durable_head, SequenceNumber(1));
        assert_eq!(progress.applied_head, SequenceNumber(0));
        assert_eq!(tracker.position(), Some(before));
    }

    fn adversarial_chain_keys(required: usize) -> Vec<LogicalLeafKey> {
        let candidates = (0..20_000_u64)
            .map(|rank| {
                let mut raw = [0; HASH_BYTES];
                raw[HASH_BYTES - 8..].copy_from_slice(&rank.to_be_bytes());
                LogicalLeafKey(raw)
            })
            .collect::<Vec<_>>();
        let priorities = candidates
            .iter()
            .map(|key| {
                hash_parts(
                    VerificationRootVersion::current(),
                    PRIORITY_DOMAIN,
                    &[key.as_bytes()],
                )
            })
            .collect::<Vec<_>>();
        let mut tails = Vec::<Hash>::new();
        let mut tail_indices = Vec::<usize>::new();
        let mut previous = vec![None; candidates.len()];
        for (index, priority) in priorities.iter().copied().enumerate() {
            let position = tails.partition_point(|tail| tail < &priority);
            if position > 0 {
                previous[index] = Some(tail_indices[position - 1]);
            }
            if position == tails.len() {
                tails.push(priority);
                tail_indices.push(index);
            } else {
                tails[position] = priority;
                tail_indices[position] = index;
            }
        }
        assert!(
            tails.len() >= required,
            "candidate corpus must contain a chain"
        );
        let mut cursor = Some(tail_indices[required - 1]);
        let mut selected = Vec::with_capacity(required);
        while let Some(index) = cursor {
            selected.push(candidates[index]);
            cursor = previous[index];
        }
        selected.reverse();
        selected
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
        assert_eq!(VERIFICATION_INDEX_NODE_BYTES, 148);
        assert_eq!(VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF, 164);
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

    #[test]
    fn adversarial_depth_is_rejected_and_incremental_insert_rolls_back() {
        let keys = adversarial_chain_keys(VERIFICATION_INDEX_MAX_DEPTH + 1);
        assert!(matches!(
            MaterializedVerificationIndex::from_leaves(
                keys.iter().copied().map(|key| (key, b"value"))
            ),
            Err(Error::ResourceExhausted(_))
        ));

        let mut index = MaterializedVerificationIndex::new();
        for key in keys.iter().take(VERIFICATION_INDEX_MAX_DEPTH) {
            index
                .upsert(*key, b"value")
                .expect("a tree at the safety limit should build");
        }
        assert_eq!(index.max_depth(), VERIFICATION_INDEX_MAX_DEPTH);
        let root_before = index.root_hash();
        let len_before = index.len();
        assert!(matches!(
            index.upsert(keys[VERIFICATION_INDEX_MAX_DEPTH], b"value"),
            Err(Error::ResourceExhausted(_))
        ));
        assert_eq!(index.root_hash(), root_before);
        assert_eq!(index.len(), len_before);
        assert_eq!(index.max_depth(), VERIFICATION_INDEX_MAX_DEPTH);
    }
}
