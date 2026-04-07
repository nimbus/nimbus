use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use neovex_core::{Document, SequenceNumber, TableName};

use super::{MaterializedServingBackend, MaterializedTableDocuments, ServingSnapshot};

pub(super) struct PublishedMaterializedTable {
    pub(super) generation: u64,
    pub(super) covered_sequence: SequenceNumber,
    pub(super) document_count: usize,
    pub(super) estimated_bytes: usize,
    pub(super) documents: Arc<MaterializedTableDocuments>,
}

pub(super) struct RetainedMaterializedTable {
    pub(super) access_stamp: u64,
    pub(super) current: PublishedMaterializedTable,
    pub(super) retained: VecDeque<PublishedMaterializedTable>,
}

#[derive(Default)]
pub(super) struct MaterializedReadAccessState {
    pub(super) access_order: VecDeque<(TableName, u64)>,
    pub(super) next_access_stamp: u64,
}

impl MaterializedServingBackend {
    pub(crate) fn new() -> Self {
        Self {
            tables: std::sync::RwLock::new(HashMap::new()),
            access: std::sync::Mutex::new(MaterializedReadAccessState::default()),
            warm_loads: super::MaterializedWarmLoadCoordinator::default(),
            next_generation: std::sync::atomic::AtomicU64::new(0),
            table_capacity: std::sync::atomic::AtomicUsize::new(
                super::DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY,
            ),
            byte_capacity: std::sync::atomic::AtomicUsize::new(
                super::DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY,
            ),
            version_capacity: std::sync::atomic::AtomicUsize::new(
                super::DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY,
            ),
            table_load_count: std::sync::atomic::AtomicU64::new(0),
            bypass_count: std::sync::atomic::AtomicU64::new(0),
            eviction_count: std::sync::atomic::AtomicU64::new(0),
            in_flight_load_count: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            pause_before_publish: Arc::new(super::MaterializedReadPublishPauseState::default()),
        }
    }

    pub(super) fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(super) fn current_limits(&self) -> (usize, usize) {
        (
            self.table_capacity.load(Ordering::Relaxed).max(1),
            self.byte_capacity.load(Ordering::Relaxed).max(1),
        )
    }

    pub(super) fn current_version_capacity(&self) -> usize {
        self.version_capacity.load(Ordering::Relaxed).max(1)
    }

    pub(super) fn touch_locked(
        access: &mut MaterializedReadAccessState,
        table: &TableName,
        table_state: &mut RetainedMaterializedTable,
    ) {
        access.next_access_stamp = access.next_access_stamp.wrapping_add(1);
        if access.next_access_stamp == 0 {
            access.next_access_stamp = 1;
        }
        table_state.access_stamp = access.next_access_stamp;
        access
            .access_order
            .push_back((table.clone(), table_state.access_stamp));
    }

    pub(super) fn compact_access_order_locked(
        access: &mut MaterializedReadAccessState,
        tables: &HashMap<TableName, RetainedMaterializedTable>,
    ) {
        let threshold = tables.len().max(1).saturating_mul(8).max(64);
        if access.access_order.len() <= threshold {
            return;
        }

        access.access_order.retain(|(table, stamp)| {
            tables
                .get(table)
                .is_some_and(|state| state.access_stamp == *stamp)
        });
    }

    pub(super) fn retained_bytes(table_state: &RetainedMaterializedTable) -> usize {
        table_state
            .retained
            .iter()
            .map(|version| version.estimated_bytes)
            .sum::<usize>()
    }

    fn total_version_bytes(tables: &HashMap<TableName, RetainedMaterializedTable>) -> usize {
        tables
            .values()
            .map(|state| state.current.estimated_bytes + Self::retained_bytes(state))
            .sum()
    }

    pub(super) fn prune_retained_versions_locked(
        &self,
        tables: &mut HashMap<TableName, RetainedMaterializedTable>,
    ) {
        let version_capacity = self.current_version_capacity();
        for table_state in tables.values_mut() {
            while table_state.retained.len().saturating_add(1) > version_capacity {
                table_state.retained.pop_front();
            }
        }

        let (_, byte_capacity) = self.current_limits();
        while Self::total_version_bytes(tables) > byte_capacity {
            let mut oldest_table = None;
            let mut oldest_sequence: Option<SequenceNumber> = None;
            for (table, state) in tables.iter() {
                let Some(candidate) = state.retained.front() else {
                    continue;
                };
                let candidate_sequence = candidate.covered_sequence;
                if oldest_sequence
                    .map(|sequence| candidate_sequence.0 < sequence.0)
                    .unwrap_or(true)
                {
                    oldest_sequence = Some(candidate_sequence);
                    oldest_table = Some(table.clone());
                }
            }
            let Some(oldest_table) = oldest_table else {
                break;
            };
            if let Some(state) = tables.get_mut(&oldest_table) {
                state.retained.pop_front();
            }
        }
    }

    pub(super) fn evict_if_needed_locked(
        &self,
        tables: &mut HashMap<TableName, RetainedMaterializedTable>,
        access: &mut MaterializedReadAccessState,
    ) {
        self.prune_retained_versions_locked(tables);
        let (table_capacity, byte_capacity) = self.current_limits();
        loop {
            let resident_bytes = Self::total_version_bytes(tables);
            let over_tables = tables.len() > table_capacity;
            let over_bytes = resident_bytes > byte_capacity && tables.len() > 1;
            if !over_tables && !over_bytes {
                break;
            }

            let Some((table, stamp)) = access.access_order.pop_front() else {
                break;
            };
            let should_evict = tables
                .get(&table)
                .is_some_and(|state| state.access_stamp == stamp);
            if !should_evict {
                continue;
            }
            tables.remove(&table);
            self.eviction_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn current_serving_snapshot_from_locked_tables(
        tables: &HashMap<TableName, RetainedMaterializedTable>,
    ) -> Option<ServingSnapshot> {
        let covered_sequence = tables
            .values()
            .map(|state| state.current.covered_sequence)
            .min_by_key(|sequence| sequence.0)?;
        let mut snapshot_tables = HashMap::new();
        for (table, table_state) in tables {
            snapshot_tables.insert(table.clone(), table_state.current.documents.clone());
        }
        Some(ServingSnapshot::from_tables(
            covered_sequence,
            snapshot_tables,
        ))
    }
}

pub(super) fn estimate_document_bytes(document: &Document) -> usize {
    document
        .to_msgpack()
        .map(|bytes| bytes.len())
        // Sizing is advisory; fall back to a coarse JSON estimate instead of
        // poisoning the materialized-read locks on an unexpected serialization
        // failure.
        .unwrap_or_else(|_| document.to_json().to_string().len())
}
