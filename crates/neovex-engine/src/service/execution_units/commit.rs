use neovex_core::{
    CommitEntry, DependencySet, Error, Result, SequenceNumber, TableName,
    commit_intersects_dependency_set,
};
use neovex_storage::ResolvedScheduleOp;

use super::MutationExecutionUnit;
use super::state::ExecutionUnitLifecycle;

struct FinalizationGuard<'a> {
    unit: &'a MutationExecutionUnit,
}

impl Drop for FinalizationGuard<'_> {
    fn drop(&mut self) {
        self.unit.finish_finalization();
    }
}

impl MutationExecutionUnit {
    pub fn commit(&self) -> Result<Option<CommitEntry>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let finalization_guard = FinalizationGuard { unit: self };
        let (writes, schedule_ops, conflict_dependencies) = {
            let mut state = self.active_state()?;
            state.lifecycle = ExecutionUnitLifecycle::Finalizing;
            let writes = self.build_resolved_writes(&state);
            let schedule_ops = self.build_resolved_schedule_ops(&state);
            let mut conflict_dependencies = state.read_dependencies.clone();
            conflict_dependencies.extend(&state.write_dependencies);
            (writes, schedule_ops, conflict_dependencies)
        };
        if writes.is_empty() && schedule_ops.is_empty() {
            return Ok(None);
        }

        let result = (|| -> Result<Option<CommitEntry>> {
            self.ensure_schema_unchanged(&conflict_dependencies)?;
            self.ensure_no_conflicts(&conflict_dependencies)?;

            let commit = {
                let _sequence_guard = self.runtime.lock_mutation_sequence();
                self.runtime
                    .store
                    .apply_execution_unit_batch(&writes, &schedule_ops)?
            };
            Ok(commit)
        })();
        drop(finalization_guard);
        let commit = result?;

        if let Some(commit) = &commit {
            self.runtime.mark_durable_head(commit.sequence);
            self.runtime.invalidate_document_cache_for_commit(commit);
            self.runtime.mark_applied_head(commit.sequence);
            self.service.process_commit(self.runtime.clone(), commit);
        }
        if schedule_ops
            .iter()
            .any(|operation| matches!(operation, ResolvedScheduleOp::Insert { .. }))
        {
            self.service.wake_scheduler();
        }
        Ok(commit)
    }

    fn ensure_schema_unchanged(&self, dependencies: &DependencySet) -> Result<()> {
        let current_schema = self.runtime.schema();
        for table in touched_tables(dependencies) {
            if current_schema.get_table(&table) != self.schema_snapshot.get_table(&table) {
                return Err(Error::Conflict(format!(
                    "table schema changed during transaction: {}",
                    table
                )));
            }
        }
        Ok(())
    }

    fn ensure_no_conflicts(&self, dependencies: &DependencySet) -> Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }

        let commits = self
            .runtime
            .store
            .read_commit_log_from(SequenceNumber(self.snapshot_sequence.0.saturating_add(1)))?;
        for commit in commits {
            if commit_intersects_dependency_set(&commit, dependencies, &[], |table, document_id| {
                self.runtime.store.get(table, &document_id)
            }) {
                return Err(Error::Conflict(
                    "transaction conflict detected; retry the mutation".to_string(),
                ));
            }
        }
        Ok(())
    }
}

fn touched_tables(dependencies: &DependencySet) -> Vec<TableName> {
    let mut tables = dependencies.tables.iter().cloned().collect::<Vec<_>>();
    for (table, _) in &dependencies.documents {
        if !tables.iter().any(|candidate| candidate == table) {
            tables.push(table.clone());
        }
    }
    for dependency in &dependencies.index_ranges {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    for dependency in &dependencies.predicates {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    for dependency in &dependencies.paginated_windows {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    tables
}
