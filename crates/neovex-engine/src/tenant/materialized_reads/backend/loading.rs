use std::collections::HashMap;
use std::sync::atomic::Ordering;

use neovex_core::{Result, SequenceNumber, TableName};

use super::publication::apply_write_to_materialized_documents;
use super::state::estimate_document_bytes;
use super::{MaterializedServingBackend, ServingSnapshot, ServingSnapshotManager};
use crate::persistence::TenantPersistence;
use crate::tenant::materialized_reads::warm_load::{
    MaterializedWarmLoadDecision, MaterializedWarmLoadPermit,
};

impl MaterializedServingBackend {
    pub(crate) fn serving_snapshot_for_table_with_mode(
        &self,
        snapshots: &ServingSnapshotManager,
        table: &TableName,
        required_sequence: SequenceNumber,
        count_bypass: bool,
    ) -> Option<ServingSnapshot> {
        let mut access = self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned");
        let mut tables = self
            .tables
            .write()
            .expect("materialized read surface lock should not be poisoned");
        let table_state = tables.get_mut(table)?;
        if table_state.current.covered_sequence.0 < required_sequence.0 {
            if count_bypass {
                self.bypass_count.fetch_add(1, Ordering::Relaxed);
            }
            return None;
        }
        Self::touch_locked(&mut access, table, table_state);
        Self::compact_access_order_locked(&mut access, &tables);
        snapshots.snapshot_covering_table(table, required_sequence)
    }

    pub(crate) fn load_serving_snapshot_cancellable(
        &self,
        snapshots: &ServingSnapshotManager,
        store: &TenantPersistence,
        table: &TableName,
        required_sequence: SequenceNumber,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<ServingSnapshot> {
        loop {
            if let Some(snapshot) =
                self.serving_snapshot_for_table_with_mode(snapshots, table, required_sequence, true)
            {
                return Ok(snapshot);
            }

            match self.warm_loads.begin_or_wait_for_warm_load(table) {
                MaterializedWarmLoadDecision::Wait(wait_state) => {
                    wait_state.wait_cancellable(check_cancel)?;
                    continue;
                }
                MaterializedWarmLoadDecision::Load(_owner) => {
                    let _warm_load = MaterializedWarmLoadPermit::new(&self.in_flight_load_count);
                    if let Some(snapshot) = self.serving_snapshot_for_table_with_mode(
                        snapshots,
                        table,
                        required_sequence,
                        false,
                    ) {
                        return Ok(snapshot);
                    }

                    let generation = self.next_generation();
                    check_cancel()?;
                    let starting_sequence = store.applied_sequence()?;
                    let mut materialized_documents = store.scan_table_matching_cancellable(
                        table,
                        check_cancel,
                        |_document| Ok(true),
                    )?;
                    let mut materialized_by_id = materialized_documents
                        .drain(..)
                        .map(|document| (document.id.clone(), document))
                        .collect::<HashMap<_, _>>();
                    let mut document_count = materialized_by_id.len();
                    let mut estimated_bytes = materialized_by_id
                        .values()
                        .map(estimate_document_bytes)
                        .sum::<usize>();
                    let mut replayed_sequence = starting_sequence;

                    loop {
                        check_cancel()?;
                        let target_sequence = store.applied_sequence()?;
                        if replayed_sequence.0 >= target_sequence.0 {
                            #[cfg(test)]
                            self.wait_if_publish_pause_armed();
                            check_cancel()?;
                            let publish_target_sequence = store.applied_sequence()?;
                            if replayed_sequence.0 >= publish_target_sequence.0 {
                                break;
                            }
                            continue;
                        }

                        let commits = store.read_commit_log_from(SequenceNumber(
                            replayed_sequence.0.saturating_add(1),
                        ))?;
                        let commits = commits
                            .into_iter()
                            .take_while(|commit| commit.sequence.0 <= target_sequence.0)
                            .collect::<Vec<_>>();
                        let Some(last_commit) = commits.last() else {
                            return Err(neovex_core::Error::Internal(format!(
                                "materialized read surface for table {} made no progress while catching up from sequence {} to {}",
                                table, replayed_sequence.0, target_sequence.0
                            )));
                        };
                        for commit in &commits {
                            for write in &commit.writes {
                                if &write.table == table {
                                    apply_write_to_materialized_documents(
                                        &mut materialized_by_id,
                                        &mut document_count,
                                        &mut estimated_bytes,
                                        write,
                                    );
                                }
                            }
                        }
                        replayed_sequence = last_commit.sequence;
                    }

                    self.publish_table_snapshot(
                        snapshots,
                        table.clone(),
                        generation,
                        replayed_sequence,
                        materialized_by_id,
                    );
                    return self
                        .serving_snapshot_for_table_with_mode(
                            snapshots,
                            table,
                            required_sequence,
                            true,
                        )
                        .ok_or_else(|| {
                            neovex_core::Error::Internal(format!(
                                "materialized serving snapshot for sequence {} should be available after loading table {}",
                                required_sequence.0, table
                            ))
                        });
                }
            }
        }
    }

    pub(crate) fn clear_publications(&self) {
        self.tables
            .write()
            .expect("materialized read surface lock should not be poisoned")
            .clear();
        for wait_state in self.warm_loads.clear() {
            wait_state.mark_completed();
        }
        *self
            .access
            .lock()
            .expect("materialized read surface access lock should not be poisoned") =
            super::state::MaterializedReadAccessState::default();
    }
}
