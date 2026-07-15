use nimbus_core::{
    DependencySet, SequenceNumber, TableId, TableName, commit_intersects_dependency_set,
};
use tracing::warn;

use crate::tenant::TenantRuntime;

use super::phase_metrics::env_positive_usize;
use super::prepared::PreparedCommit;

/// Upper bound on how many recent commits one shadow observation may scan.
///
/// The observation window opens at the request's enqueue-time snapshot, so
/// under sustained load the un-clamped window grows with queue depth — and
/// because the scan runs under the sequence gate, an unbounded scan feeds
/// back into longer gate holds and deeper queues (measured as a collapse
/// from ~16.6k to ~0.6k mut/s at N=256 before this bound existed). The
/// clamp keeps the per-observation cost constant; conflicts older than the
/// window are not counted and the truncation is recorded instead, so the
/// metric stays honest about what it skipped.
const DEFAULT_SHADOW_CONFLICT_WINDOW_MAX: usize = 64;

/// Observe only every N-th eligible batch/mutation. Even a bounded scan is
/// a storage read of full commit entries under the sequence gate; at
/// saturation the observation *frequency* — one scan per batch — is itself
/// a material tax (measured ~95% of under-gate time at N=256 with
/// per-request unsampled observation). Shadow metrics exist to
/// characterize workloads, so a deterministic sample is sufficient; the
/// first eligible observation is always taken.
const DEFAULT_SHADOW_CONFLICT_SAMPLE_EVERY: usize = 16;

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

/// Computes where a bounded shadow scan starts and whether the bound
/// truncated the requested window.
///
/// The scan wants `(snapshot, durable_head]`; the bound keeps at most
/// `window_max` trailing commits of that range. Pure so the clamp math is
/// unit-testable without storage.
fn shadow_scan_start(
    snapshot_sequence: SequenceNumber,
    durable_head: SequenceNumber,
    window_max: usize,
) -> (SequenceNumber, bool) {
    let requested_start = snapshot_sequence.0.saturating_add(1);
    let bounded_start = durable_head
        .0
        .saturating_add(1)
        .saturating_sub(window_max as u64);
    if bounded_start > requested_start {
        (SequenceNumber(bounded_start), true)
    } else {
        (SequenceNumber(requested_start), false)
    }
}

/// Counts conflicts for one batch of prepared mutations against durable
/// commits newer than the batch's earliest planning snapshot.
///
/// One observation per batch (paths A) or per mutation (path B), sampled
/// every `NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY`-th eligible observation
/// (default 16, first always taken) and scanning at most
/// `NIMBUS_SHADOW_CONFLICT_WINDOW_MAX` trailing commits (default 64).
/// `shadow_checks_sampled` records how many observations actually ran, so
/// conflict totals read as a sampled rate, not an absolute count. Errors
/// are deliberately swallowed after a warning: shadow observation must
/// never reject, retry, or otherwise change a mutation.
pub(super) fn observe_shadow_conflicts(
    runtime: &TenantRuntime,
    snapshot_sequence: SequenceNumber,
    dependency_sets: &[DependencySet],
) {
    if dependency_sets.iter().all(DependencySet::is_empty) {
        return;
    }
    let sample_every = env_positive_usize(
        "NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY",
        DEFAULT_SHADOW_CONFLICT_SAMPLE_EVERY,
    );
    if !runtime
        .commit_phase_metrics()
        .shadow_sample_tick(sample_every)
    {
        return;
    }

    let window_max = env_positive_usize(
        "NIMBUS_SHADOW_CONFLICT_WINDOW_MAX",
        DEFAULT_SHADOW_CONFLICT_WINDOW_MAX,
    );
    let (scan_start, truncated) =
        shadow_scan_start(snapshot_sequence, runtime.durable_head(), window_max);

    let commits = match runtime.store.read_commit_log_from(scan_start) {
        Ok(commits) => commits,
        Err(error) => {
            warn!(
                tenant = %runtime.tenant_id(),
                error = %error,
                "shadow conflict commit-window read failed"
            );
            runtime
                .commit_phase_metrics()
                .record_shadow_check(0, false, truncated);
            return;
        }
    };
    let window_size = commits.len();
    let conflicting = commits.iter().any(|commit| {
        dependency_sets.iter().any(|dependencies| {
            !dependencies.is_empty()
                && commit_intersects_dependency_set(commit, dependencies, &[], {
                    |table, document_id| runtime.store.get(table, &document_id)
                })
        })
    });
    runtime
        .commit_phase_metrics()
        .record_shadow_check(window_size, conflicting, truncated);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_scan_start_is_unclamped_when_the_window_covers_the_snapshot() {
        let (start, truncated) = shadow_scan_start(SequenceNumber(10), SequenceNumber(20), 256);
        assert_eq!(start, SequenceNumber(11));
        assert!(!truncated);
    }

    #[test]
    fn shadow_scan_start_clamps_to_the_trailing_window_and_reports_truncation() {
        // Snapshot far behind the head: only the trailing `window_max`
        // commits are scanned.
        let (start, truncated) = shadow_scan_start(SequenceNumber(10), SequenceNumber(5_000), 256);
        assert_eq!(start, SequenceNumber(5_000 + 1 - 256));
        assert!(truncated);
    }

    #[test]
    fn shadow_scan_start_saturates_near_the_origin() {
        let (start, truncated) = shadow_scan_start(SequenceNumber(0), SequenceNumber(3), 256);
        assert_eq!(start, SequenceNumber(1));
        assert!(!truncated);
    }

    #[test]
    fn shadow_scan_start_exact_boundary_is_not_truncated() {
        // durable_head+1-window == snapshot+1 → the window exactly covers
        // the requested range; no truncation.
        let (start, truncated) = shadow_scan_start(SequenceNumber(100), SequenceNumber(356), 256);
        assert_eq!(start, SequenceNumber(101));
        assert!(!truncated);
    }
}
