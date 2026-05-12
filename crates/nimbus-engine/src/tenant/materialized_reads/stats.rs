use nimbus_core::SequenceNumber;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaterializedReadSurfaceStats {
    pub loaded_table_count: usize,
    pub resident_document_count: usize,
    pub resident_estimated_bytes: usize,
    pub retained_version_count: usize,
    pub retained_estimated_bytes: usize,
    pub table_capacity: usize,
    pub byte_capacity: usize,
    pub version_capacity: usize,
    pub table_load_count: u64,
    pub evaluation_count: u64,
    pub paginated_count: u64,
    pub get_hit_count: u64,
    pub bypass_count: u64,
    pub eviction_count: u64,
    pub in_flight_load_count: u64,
    pub earliest_covered_sequence: Option<SequenceNumber>,
    pub latest_covered_sequence: Option<SequenceNumber>,
    pub earliest_retained_sequence: Option<SequenceNumber>,
    pub latest_retained_sequence: Option<SequenceNumber>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MaterializedServingBackendStats {
    pub(super) loaded_table_count: usize,
    pub(super) resident_document_count: usize,
    pub(super) resident_estimated_bytes: usize,
    pub(super) retained_version_count: usize,
    pub(super) retained_estimated_bytes: usize,
    pub(super) table_capacity: usize,
    pub(super) byte_capacity: usize,
    pub(super) version_capacity: usize,
    pub(super) table_load_count: u64,
    pub(super) bypass_count: u64,
    pub(super) eviction_count: u64,
    pub(super) in_flight_load_count: u64,
    pub(super) earliest_covered_sequence: Option<SequenceNumber>,
    pub(super) latest_covered_sequence: Option<SequenceNumber>,
    pub(super) earliest_retained_sequence: Option<SequenceNumber>,
    pub(super) latest_retained_sequence: Option<SequenceNumber>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializedTablePublicationStats {
    pub generation: u64,
    pub covered_sequence: SequenceNumber,
    pub document_count: usize,
    pub estimated_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ServingSnapshotManagerStats {
    pub retained_snapshot_count: usize,
    pub earliest_retained_sequence: Option<SequenceNumber>,
    pub latest_retained_sequence: Option<SequenceNumber>,
    pub pinned_snapshot_count: usize,
    pub waiter_count: usize,
    pub pruned_snapshot_count: u64,
    pub discarded_out_of_order_count: u64,
}
