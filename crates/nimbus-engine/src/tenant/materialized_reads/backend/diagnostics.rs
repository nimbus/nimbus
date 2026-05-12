use std::sync::atomic::Ordering;

#[cfg(test)]
use super::super::pause::MaterializedReadPublishPauseHandle;
use super::super::stats::MaterializedServingBackendStats;
#[cfg(test)]
use super::super::stats::MaterializedTablePublicationStats;
use super::MaterializedServingBackend;
#[cfg(test)]
use nimbus_core::TableName;

impl MaterializedServingBackend {
    pub(crate) fn stats(&self) -> MaterializedServingBackendStats {
        let tables = self
            .tables
            .read()
            .expect("materialized read surface lock should not be poisoned");
        MaterializedServingBackendStats {
            loaded_table_count: tables.len(),
            resident_document_count: tables
                .values()
                .map(|state| state.current.document_count)
                .sum(),
            resident_estimated_bytes: tables
                .values()
                .map(|state| state.current.estimated_bytes)
                .sum(),
            retained_version_count: tables.values().map(|state| state.retained.len()).sum(),
            retained_estimated_bytes: tables.values().map(Self::retained_bytes).sum(),
            table_capacity: self.table_capacity.load(Ordering::Relaxed),
            byte_capacity: self.byte_capacity.load(Ordering::Relaxed),
            version_capacity: self.version_capacity.load(Ordering::Relaxed),
            table_load_count: self.table_load_count.load(Ordering::Relaxed),
            bypass_count: self.bypass_count.load(Ordering::Relaxed),
            eviction_count: self.eviction_count.load(Ordering::Relaxed),
            in_flight_load_count: self.in_flight_load_count.load(Ordering::Relaxed),
            earliest_covered_sequence: tables
                .values()
                .map(|state| state.current.covered_sequence)
                .min_by_key(|sequence| sequence.0),
            latest_covered_sequence: tables
                .values()
                .map(|state| state.current.covered_sequence)
                .max_by_key(|sequence| sequence.0),
            earliest_retained_sequence: tables
                .values()
                .flat_map(|state| {
                    state
                        .retained
                        .iter()
                        .map(|version| version.covered_sequence)
                })
                .min_by_key(|sequence| sequence.0),
            latest_retained_sequence: tables
                .values()
                .flat_map(|state| {
                    state
                        .retained
                        .iter()
                        .map(|version| version.covered_sequence)
                })
                .max_by_key(|sequence| sequence.0),
        }
    }

    #[cfg(test)]
    pub(crate) fn table_publication_stats(
        &self,
        table: &TableName,
    ) -> Option<MaterializedTablePublicationStats> {
        self.tables
            .read()
            .expect("materialized read surface lock should not be poisoned")
            .get(table)
            .map(|state| MaterializedTablePublicationStats {
                generation: state.current.generation,
                covered_sequence: state.current.covered_sequence,
                document_count: state.current.documents.len(),
                estimated_bytes: state.current.estimated_bytes,
            })
    }

    #[cfg(test)]
    pub(crate) fn publish_pause_handle(&self) -> MaterializedReadPublishPauseHandle {
        MaterializedReadPublishPauseHandle {
            state: self.pause_before_publish.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_limits_for_testing(&self, table_capacity: usize, byte_capacity: usize) {
        self.table_capacity
            .store(table_capacity.max(1), Ordering::Relaxed);
        self.byte_capacity
            .store(byte_capacity.max(1), Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn set_version_capacity_for_testing(&self, version_capacity: usize) {
        self.version_capacity
            .store(version_capacity.max(1), Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(super) fn wait_if_publish_pause_armed(&self) {
        self.pause_before_publish.wait_if_armed();
    }
}
