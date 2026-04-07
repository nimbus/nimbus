use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Mutex, RwLock};

use neovex_core::TableName;

#[cfg(test)]
use super::pause::MaterializedReadPublishPauseState;
use super::snapshot::{MaterializedTableDocuments, ServingSnapshot, ServingSnapshotManager};
use super::warm_load::MaterializedWarmLoadCoordinator;
use super::{
    DEFAULT_MATERIALIZED_SURFACE_BYTE_CAPACITY, DEFAULT_MATERIALIZED_SURFACE_TABLE_CAPACITY,
    DEFAULT_MATERIALIZED_SURFACE_VERSION_CAPACITY,
};
#[cfg(test)]
use std::sync::Arc;

mod diagnostics;
mod loading;
mod publication;
mod state;

use self::state::{MaterializedReadAccessState, RetainedMaterializedTable};

// Lock ordering for multi-lock materialized-read operations is
// `backend.access -> backend.tables -> snapshots.state`. Keep that order when
// touching more than one of these locks in the same path.
pub(super) struct MaterializedServingBackend {
    tables: RwLock<HashMap<TableName, RetainedMaterializedTable>>,
    access: Mutex<MaterializedReadAccessState>,
    warm_loads: MaterializedWarmLoadCoordinator,
    next_generation: AtomicU64,
    table_capacity: AtomicUsize,
    byte_capacity: AtomicUsize,
    version_capacity: AtomicUsize,
    table_load_count: AtomicU64,
    bypass_count: AtomicU64,
    eviction_count: AtomicU64,
    in_flight_load_count: AtomicU64,
    #[cfg(test)]
    pause_before_publish: Arc<MaterializedReadPublishPauseState>,
}
