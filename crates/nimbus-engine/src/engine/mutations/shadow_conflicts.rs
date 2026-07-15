use nimbus_core::{
    DependencySet, SequenceNumber, TableId, TableName, commit_intersects_dependency_set,
};
use tracing::warn;

use crate::tenant::TenantRuntime;

use super::prepared::PreparedCommit;

/// Derives observational document dependencies without changing the real OCC
/// read set. Paths A and B remain serialized committers; these dependencies are
/// used only by `observe_shadow_conflicts`.
pub(super) fn prepared_document_dependencies(
    prepared: &PreparedCommit,
    mut resolve_table_id: impl FnMut(&TableName) -> Option<TableId>,
) -> DependencySet {
    let mut dependencies = DependencySet::default();
    for write in &prepared.write_set {
        let table_id = write
            .table_id
            .clone()
            .or_else(|| resolve_table_id(&write.table));
        if let Some(table_id) = table_id {
            dependencies.record_document(&write.table, &table_id, write.doc_id.clone());
        } else {
            dependencies.record_missing_table(&write.table);
        }
    }
    dependencies
}

/// Counts conflicts against durable commits newer than the observed planning
/// snapshot. Errors are deliberately swallowed after a warning: shadow
/// observation must never reject, retry, or otherwise change a mutation.
pub(super) fn observe_shadow_conflicts(
    runtime: &TenantRuntime,
    snapshot_sequence: SequenceNumber,
    dependencies: &DependencySet,
) {
    if dependencies.is_empty() {
        runtime.commit_phase_metrics().record_shadow_check(0, false);
        return;
    }

    let commits = match runtime
        .store
        .read_commit_log_from(SequenceNumber(snapshot_sequence.0.saturating_add(1)))
    {
        Ok(commits) => commits,
        Err(error) => {
            warn!(
                tenant = %runtime.tenant_id(),
                error = %error,
                "shadow conflict commit-window read failed"
            );
            runtime.commit_phase_metrics().record_shadow_check(0, false);
            return;
        }
    };
    let window_size = commits.len();
    let conflicting = commits.iter().any(|commit| {
        commit_intersects_dependency_set(commit, dependencies, &[], |table, document_id| {
            runtime.store.get(table, &document_id)
        })
    });
    runtime
        .commit_phase_metrics()
        .record_shadow_check(window_size, conflicting);
}
