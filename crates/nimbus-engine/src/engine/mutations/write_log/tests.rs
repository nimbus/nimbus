use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use nimbus_core::{
    Filter, FilterOp, IndexId, IndexRangeDependency, ManualWallClock, PaginatedWindowDependency,
    PredicateDependency, TableId, WriteOp, WriteOpType,
};
use nimbus_storage::{MemoryTenantStore, NoopFaultInjector};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::json;

use super::*;
use crate::tenant::{JournalFrontierSample, MutationFrontierStats};

fn commit(sequence: u64, body_bytes: usize) -> CommitEntry {
    commit_for_id(sequence, &format!("doc-{sequence}"), body_bytes)
}

fn commit_for_id(sequence: u64, id_key: &str, body_bytes: usize) -> CommitEntry {
    let table = TableName::new("messages").expect("table name should be valid");
    let id = DocumentId::from_key(id_key).expect("document id should be valid");
    let document = Document {
        id: id.clone(),
        table: table.clone(),
        creation_time: Timestamp(sequence),
        update_time: Timestamp(sequence),
        fields: serde_json::Map::from_iter([("body".to_string(), json!("x".repeat(body_bytes)))]),
        typed_fields: Default::default(),
    };
    CommitEntry {
        sequence: SequenceNumber(sequence),
        timestamp: Timestamp(sequence),
        writes: vec![WriteOp {
            table,
            table_id: TableId::new(),
            op_type: WriteOpType::Insert,
            doc_id: id,
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document),
        }],
    }
}

fn test_log(config: WriteLogConfig) -> WriteLog {
    WriteLog::new(config, SequenceNumber(0), SequenceNumber(0))
}

fn frontier_stats(
    log: &WriteLog,
    durable_head: SequenceNumber,
    applied_head: SequenceNumber,
) -> MutationFrontierStats {
    let journal = JournalFrontierSample {
        durable_head,
        applied_head,
    };
    MutationFrontierStats::reconcile(log.frontier_sample(), journal, journal)
}

#[test]
fn pending_stage_entries_staged_to_published_lifecycle() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));
    let staged = log.inspection();
    assert_eq!(staged.pending, vec![SequenceNumber(1)]);
    assert!(staged.published.is_empty());

    log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));
    let published = log.inspection();
    assert!(published.pending.is_empty());
    assert_eq!(published.published, vec![SequenceNumber(1)]);
}

#[test]
fn indexed_document_images_distinguish_published_and_pending_heads() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    let table = TableName::new("messages").expect("table name should be valid");
    let id = DocumentId::from_key("shared").expect("document id should be valid");
    log.stage_pending([commit_for_id(1, "shared", 8)], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));
    log.stage_pending([commit_for_id(2, "shared", 16)], Timestamp(1_001));

    assert!(log.current_prepare_view_available(SequenceNumber(1)));
    let published = log
        .current_document_state(SequenceNumber(1), &table, &id)
        .expect("published image should remain available behind a pending suffix");
    assert_eq!(published.sequence, SequenceNumber(1));
    assert!(matches!(
        log.single_document_change_since(SequenceNumber(1), &table, &id),
        Ok(Some(SingleDocumentWindowChange::Changed { latest }))
            if latest.sequence == SequenceNumber(2)
    ));

    log.discard_unpersisted_suffix(SequenceNumber(2));
    assert!(matches!(
        log.single_document_change_since(SequenceNumber(1), &table, &id),
        Ok(Some(SingleDocumentWindowChange::Unchanged))
    ));
}

#[test]
fn definitive_assignment_rollback_preserves_high_water_and_clears_active_lag() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8), commit(2, 8)], Timestamp(1_000));
    let assigned = frontier_stats(&log, SequenceNumber(0), SequenceNumber(0));
    assert_eq!(assigned.assigned_high_water, SequenceNumber(2));
    assert_eq!(assigned.active_assigned_head, SequenceNumber(2));
    assert_eq!(assigned.assignment_lag, 2);

    log.discard_unpersisted_suffix(SequenceNumber(1));
    let rolled_back = frontier_stats(&log, SequenceNumber(0), SequenceNumber(0));
    assert_eq!(rolled_back.assigned_high_water, SequenceNumber(2));
    assert_eq!(rolled_back.active_assigned_head, SequenceNumber(0));
    assert_eq!(rolled_back.assignment_lag, 0);

    log.stage_pending([commit(1, 8)], Timestamp(1_001));
    let safely_reused = frontier_stats(&log, SequenceNumber(0), SequenceNumber(0));
    assert_eq!(safely_reused.assigned_high_water, SequenceNumber(2));
    assert_eq!(safely_reused.active_assigned_head, SequenceNumber(1));
    assert_eq!(safely_reused.assignment_lag, 1);
}

#[test]
fn publisher_stall_diagnostics_distinguish_assignment_apply_and_publication_lag() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));

    let assignment_stall = frontier_stats(&log, SequenceNumber(0), SequenceNumber(0));
    assert_eq!(
        (
            assignment_stall.assignment_lag,
            assignment_stall.apply_lag,
            assignment_stall.publication_lag,
            assignment_stall.visibility_lag,
        ),
        (1, 0, 0, 0)
    );

    let apply_stall = frontier_stats(&log, SequenceNumber(1), SequenceNumber(0));
    assert_eq!(
        (
            apply_stall.assignment_lag,
            apply_stall.apply_lag,
            apply_stall.publication_lag,
            apply_stall.visibility_lag,
        ),
        (0, 1, 0, 0)
    );

    log.observe_applied_through(SequenceNumber(1), Timestamp(1_001), SequenceNumber(0));
    let publication_stall = frontier_stats(&log, SequenceNumber(1), SequenceNumber(0));
    assert_eq!(
        (
            publication_stall.assignment_lag,
            publication_stall.apply_lag,
            publication_stall.publication_lag,
            publication_stall.visibility_lag,
        ),
        (0, 0, 1, 0)
    );

    log.publish_pending_through(SequenceNumber(1), Timestamp(1_002), SequenceNumber(0));
    let visibility_stall = frontier_stats(&log, SequenceNumber(1), SequenceNumber(0));
    assert_eq!(
        (
            visibility_stall.assignment_lag,
            visibility_stall.apply_lag,
            visibility_stall.publication_lag,
            visibility_stall.visibility_lag,
        ),
        (0, 0, 0, 1)
    );

    let visible = frontier_stats(&log, SequenceNumber(1), SequenceNumber(1));
    assert_eq!(
        (
            visible.assignment_lag,
            visible.apply_lag,
            visible.publication_lag,
            visible.visibility_lag,
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn zero_write_sequence_advances_coverage_without_allocating_an_entry() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.advance_known_zero_write_through(SequenceNumber(1));

    let inspection = log.inspection();
    assert!(inspection.pending.is_empty());
    assert!(inspection.published.is_empty());
    assert!(matches!(
        log.validation_source(SequenceNumber(0), SequenceNumber(1)),
        Ok(ValidationSource::InMemory(_))
    ));
}

#[test]
fn later_zero_write_assignment_cannot_cross_an_earlier_pending_publish() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));

    // A later trigger-delivery cursor can become durable/applied while
    // storage catch-up also applies this already-staged document record.
    // Recording that zero-write assignment must leave publication to the
    // common applied-prefix path so sequence 1 is published before 2.
    log.advance_known_zero_write_through(SequenceNumber(2));
    log.publish_pending_through(SequenceNumber(2), Timestamp(1_001), SequenceNumber(0));

    let inspection = log.inspection();
    assert_eq!(inspection.published, vec![SequenceNumber(1)]);
    assert!(inspection.pending.is_empty());
    assert_eq!(log.assigned_through(), SequenceNumber(2));
    assert!(log.current_prepare_view_available(SequenceNumber(2)));
}

#[test]
fn provider_catch_up_cannot_publish_assigned_suffix() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));
    log.observe_assigned_through_without_coverage(SequenceNumber(2));

    let held_frontier =
        log.observe_applied_through(SequenceNumber(2), Timestamp(1_001), SequenceNumber(0));
    assert_eq!(held_frontier, SequenceNumber(0));
    assert_eq!(log.published_through(), SequenceNumber(0));
    let held_stats = frontier_stats(&log, SequenceNumber(2), SequenceNumber(0));
    assert_eq!(held_stats.storage_applied_head, SequenceNumber(2));
    assert_eq!(held_stats.published_head, SequenceNumber(0));
    assert_eq!(held_stats.publication_lag, 2);
    let held = log.inspection();
    assert!(held.published.is_empty());
    assert_eq!(held.pending, vec![SequenceNumber(1)]);

    let released_frontier =
        log.publish_pending_through(SequenceNumber(1), Timestamp(1_002), SequenceNumber(0));
    assert_eq!(released_frontier, SequenceNumber(2));
    assert_eq!(log.published_through(), SequenceNumber(2));
    let released_stats = frontier_stats(&log, SequenceNumber(2), SequenceNumber(2));
    assert_eq!(released_stats.storage_applied_head, SequenceNumber(2));
    assert_eq!(released_stats.published_head, SequenceNumber(2));
    assert_eq!(released_stats.publication_lag, 0);
    let released = log.inspection();
    assert_eq!(released.published, vec![SequenceNumber(1)]);
    assert!(released.pending.is_empty());
    assert!(matches!(
        log.validation_source(SequenceNumber(1), SequenceNumber(2)),
        Ok(ValidationSource::StorageFallback)
    ));
}

#[test]
fn frontier_diagnostics_remain_ordered_under_concurrent_sampling() {
    const COMMITS: u64 = 256;
    let log = Arc::new(test_log(WriteLogConfig::for_tests(30, 300, usize::MAX)));
    let durable_head = Arc::new(AtomicU64::new(0));
    let applied_head = Arc::new(AtomicU64::new(0));
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);

    let writer = {
        let log = Arc::clone(&log);
        let durable_head = Arc::clone(&durable_head);
        let applied_head = Arc::clone(&applied_head);
        std::thread::spawn(move || {
            for sequence in 1..=COMMITS {
                let sequence = SequenceNumber(sequence);
                log.stage_pending([commit(sequence.0, 8)], Timestamp(sequence.0));
                durable_head.store(sequence.0, Ordering::Release);
                log.observe_applied_through(sequence, Timestamp(sequence.0), SequenceNumber(0));
                log.publish_pending_through(sequence, Timestamp(sequence.0), SequenceNumber(0));
                applied_head.store(sequence.0, Ordering::Release);
            }
            finished_tx
                .send(())
                .expect("frontier sampler should remain connected");
        })
    };

    let mut previous: Option<MutationFrontierStats> = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match finished_rx.try_recv() {
            Ok(()) => break,
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                panic!("frontier writer exited without reporting completion")
            }
        }
        assert!(
            Instant::now() < deadline,
            "frontier writer did not finish within five seconds; last sample: {previous:?}"
        );
        let before = JournalFrontierSample {
            durable_head: SequenceNumber(durable_head.load(Ordering::Acquire)),
            applied_head: SequenceNumber(applied_head.load(Ordering::Acquire)),
        };
        let write_log = log.frontier_sample();
        let after = JournalFrontierSample {
            durable_head: SequenceNumber(durable_head.load(Ordering::Acquire)),
            applied_head: SequenceNumber(applied_head.load(Ordering::Acquire)),
        };
        let stats = MutationFrontierStats::reconcile(write_log, before, after);
        assert!(stats.is_causally_ordered());
        if let Some(previous) = previous {
            assert!(stats.assigned_high_water >= previous.assigned_high_water);
            assert!(stats.active_assigned_head >= previous.active_assigned_head);
            assert!(stats.durable_head >= previous.durable_head);
            assert!(stats.storage_applied_head >= previous.storage_applied_head);
            assert!(stats.published_head >= previous.published_head);
            assert!(stats.applied_head >= previous.applied_head);
        }
        previous = Some(stats);
        std::thread::yield_now();
    }
    writer.join().expect("frontier writer should join");

    let final_stats = frontier_stats(
        &log,
        SequenceNumber(durable_head.load(Ordering::Acquire)),
        SequenceNumber(applied_head.load(Ordering::Acquire)),
    );
    assert_eq!(final_stats.assigned_high_water, SequenceNumber(COMMITS));
    assert_eq!(final_stats.applied_head, SequenceNumber(COMMITS));
    assert!(final_stats.is_causally_ordered());
}

#[test]
fn current_views_refuse_head_beyond_full_image_coverage() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    let staged = commit_for_id(1, "shared", 8);
    let table = staged.writes[0].table.clone();
    let id = staged.writes[0].doc_id.clone();
    log.stage_pending([staged], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));

    log.observe_assigned_through_without_coverage(SequenceNumber(2));
    let published =
        log.observe_applied_through(SequenceNumber(2), Timestamp(1_001), SequenceNumber(0));
    assert_eq!(published, SequenceNumber(2));

    assert_eq!(
        (
            log.current_prepare_view_available(published),
            log.current_document_state(published, &table, &id).is_some(),
        ),
        (false, false),
        "current views must not serve images beyond proven full-image coverage"
    );
}

#[test]
fn lagged_trigger_cursor_zero_write_sequence_preserves_coverage_for_later_commits() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));
    log.stage_pending([commit(2, 8)], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(2), Timestamp(1_000), SequenceNumber(0));
    // The trigger worker for commit 1 lags until commit 2 is already
    // staged, then appends its own zero-write cursor record at sequence 3.
    log.advance_known_zero_write_through(SequenceNumber(3));
    log.stage_pending([commit(4, 8)], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(4), Timestamp(1_000), SequenceNumber(0));

    assert!(matches!(
        log.validation_source(SequenceNumber(0), SequenceNumber(4)),
        Ok(ValidationSource::InMemory(_))
    ));
}

#[test]
fn recovered_empty_window_rebases_without_claiming_history() {
    let log = WriteLog::new(
        WriteLogConfig::for_tests(30, 300, usize::MAX),
        SequenceNumber(2),
        SequenceNumber(3),
    );
    log.rebase_empty_after_recovery(SequenceNumber(3), SequenceNumber(3));

    assert!(matches!(
        log.validation_source(SequenceNumber(2), SequenceNumber(3)),
        Ok(ValidationSource::StorageFallback)
    ));
    assert!(matches!(
        log.validation_source(SequenceNumber(3), SequenceNumber(3)),
        Ok(ValidationSource::InMemory(_))
    ));
}

#[test]
fn skewed_shared_progress_normalizes_without_claiming_history() {
    let log = WriteLog::new(
        WriteLogConfig::for_tests(30, 300, usize::MAX),
        SequenceNumber(5),
        SequenceNumber(4),
    );
    assert!(matches!(
        log.validation_source(SequenceNumber(4), SequenceNumber(5)),
        Ok(ValidationSource::StorageFallback)
    ));

    let recovered = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    recovered.rebase_empty_after_recovery(SequenceNumber(5), SequenceNumber(4));
    assert!(matches!(
        recovered.validation_source(SequenceNumber(4), SequenceNumber(5)),
        Ok(ValidationSource::StorageFallback)
    ));
}

#[test]
fn recovered_unstaged_commits_cannot_be_absorbed_by_zero_write_coverage() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));

    // An ambiguous append durably committed document records 2 and 3 but
    // returned before the live path could stage their images.
    log.observe_assigned_through_without_coverage(SequenceNumber(3));
    // A subsequent schema/trigger record is known zero-write, but must not
    // make the unstaged recovered document span look covered.
    log.advance_known_zero_write_through(SequenceNumber(4));

    assert!(matches!(
        log.validation_source(SequenceNumber(1), SequenceNumber(4)),
        Ok(ValidationSource::StorageFallback)
    ));
}

#[test]
fn ambiguous_append_marks_coverage_unknown_and_forces_storage_fallback() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8)], Timestamp(1_000));
    log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));

    log.mark_coverage_unknown();
    log.advance_known_zero_write_through(SequenceNumber(2));

    assert!(matches!(
        log.validation_source(SequenceNumber(0), SequenceNumber(2)),
        Ok(ValidationSource::StorageFallback)
    ));
}

#[test]
fn pending_stage_validation_sees_pending_entries() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    let pending = commit(1, 8);
    let table = pending.writes[0].table.clone();
    let table_id = pending.writes[0].table_id.clone();
    log.stage_pending([pending], Timestamp(1_000));
    let mut dependencies = DependencySet::default();
    dependencies.record_table(&table, &table_id);

    let ValidationSource::InMemory(view) = log
        .validation_source(SequenceNumber(0), SequenceNumber(0))
        .expect("pending window should cover the validation range")
    else {
        panic!("covered pending entry should not require storage fallback");
    };
    assert_eq!(
        view.first_conflicting_sequence(&dependencies, |_, _| Ok(None)),
        Some(SequenceNumber(1))
    );
}

#[test]
fn out_of_retention_validation_fails_closed() {
    let log = test_log(WriteLogConfig::for_tests(1, 2, usize::MAX));
    log.stage_pending([commit(1, 8), commit(2, 8)], Timestamp(0));
    log.publish_pending_through(SequenceNumber(2), Timestamp(3_000), SequenceNumber(2));
    let inspection = log.inspection();
    assert_eq!(inspection.purged_sequence, SequenceNumber(2));

    let error = log
        .validation_source(SequenceNumber(1), SequenceNumber(2))
        .err()
        .expect("snapshot older than the purge horizon must fail closed");
    assert!(matches!(
        error,
        Error::OutOfRetention {
            ref message,
            minimum_sequence: Some(SequenceNumber(2)),
        } if message.contains("retention horizon")
    ));
}

#[test]
fn stalled_reader_size_cap_trim_respects_min_retention_floor() {
    let probe = WindowEntry::document_commit(commit(1, 1_024), Timestamp(0)).accounted_bytes;
    let log = test_log(WriteLogConfig::for_tests(30, 300, probe + 1));
    log.stage_pending([commit(1, 1_024), commit(2, 1_024)], Timestamp(0));
    log.publish_pending_through(SequenceNumber(2), Timestamp(29_999), SequenceNumber(0));
    let before_min = log.inspection();
    assert_eq!(before_min.published.len(), 2);
    assert!(before_min.accounted_bytes > probe + 1);

    log.publish_pending_through(SequenceNumber(2), Timestamp(30_000), SequenceNumber(0));
    let after_min = log.inspection();
    assert!(after_min.accounted_bytes <= probe + 1);
    assert_eq!(after_min.purged_sequence, SequenceNumber(1));
    assert_eq!(after_min.published, vec![SequenceNumber(2)]);
}

#[test]
#[should_panic(expected = "write-log sequences must append without interior holes")]
fn window_append_asserts_sequence_contiguity() {
    let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
    log.stage_pending([commit(1, 8), commit(3, 8)], Timestamp(0));
}

#[test]
fn bootstrap_fallback_uses_storage_scan() {
    let store = MemoryTenantStore::with_simulation(
        Arc::new(ManualWallClock::new(Timestamp(1_000))),
        Arc::new(NoopFaultInjector),
    );
    let document = document(1, "active", 1);
    let commit = store.insert(&document).expect("seed insert should commit");
    let log = WriteLog::new(
        WriteLogConfig::for_tests(30, 300, usize::MAX),
        commit.sequence,
        commit.sequence,
    );
    let mut dependencies = DependencySet::default();
    dependencies.record_table(&commit.writes[0].table, &commit.writes[0].table_id);

    assert!(matches!(
        log.validation_source(SequenceNumber(0), commit.sequence)
            .expect("bootstrap coverage decision should succeed"),
        ValidationSource::StorageFallback
    ));
    let storage_conflict = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("bootstrap fallback scan should read memory persistence")
        .into_iter()
        .any(|entry| {
            commit_intersects_dependency_set(&entry, &dependencies, &[], |table, id| {
                store.get(table, &id)
            })
        });
    assert!(
        storage_conflict,
        "the mandatory bootstrap fallback must preserve the storage-scan abort decision"
    );
}

#[test]
fn window_vs_storage_scan_differential() {
    const HISTORIES: usize = 20;
    const CASES_PER_HISTORY: usize = 25;
    const CORPUS_SIZE: usize = HISTORIES * CASES_PER_HISTORY;

    let mut rng = StdRng::seed_from_u64(0x5050_5343_3257_4c47);
    let mut checked = 0;
    for history_index in 0..HISTORIES {
        let store = MemoryTenantStore::with_simulation(
            Arc::new(ManualWallClock::new(Timestamp(
                10_000 + u64::try_from(history_index).expect("history index fits u64"),
            ))),
            Arc::new(NoopFaultInjector),
        );
        let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
        let mut active = Vec::<Document>::new();
        let mut history = Vec::<CommitEntry>::new();

        for operation_index in 0..CASES_PER_HISTORY {
            let choose_insert = active.is_empty() || rng.gen_bool(0.45);
            let commit = if choose_insert {
                let document = document(
                    history_index * 1_000 + operation_index,
                    if rng.gen_bool(0.5) {
                        "active"
                    } else {
                        "archived"
                    },
                    rng.gen_range(0..100),
                );
                let commit = store
                    .insert(&document)
                    .expect("generated insert should commit");
                active.push(document);
                commit
            } else {
                let target = rng.gen_range(0..active.len());
                if active.len() > 1 && rng.gen_bool(0.25) {
                    let document = active.swap_remove(target);
                    store
                        .delete_validated_returning_document(&document.table, &document.id, |_| {
                            Ok(())
                        })
                        .expect("generated delete should commit")
                        .0
                } else {
                    let document = &mut active[target];
                    let status = if rng.gen_bool(0.5) {
                        "active"
                    } else {
                        "archived"
                    };
                    let rank = rng.gen_range(0..100);
                    let patch = serde_json::Map::from_iter([
                        ("status".to_string(), json!(status)),
                        ("rank".to_string(), json!(rank)),
                    ]);
                    let commit = store
                        .update_validated(&document.table, &document.id, &patch, |_, _| Ok(()))
                        .expect("generated update should commit");
                    document.fields.extend(patch);
                    document.update_time = commit.timestamp;
                    commit
                }
            };
            log.stage_pending([commit.clone()], Timestamp(1_000));
            log.publish_pending_through(commit.sequence, Timestamp(1_000), SequenceNumber(0));
            history.push(commit);
        }

        let head = history
            .last()
            .expect("generated history should be non-empty")
            .sequence;
        for _ in 0..CASES_PER_HISTORY {
            let snapshot = SequenceNumber(rng.gen_range(0..head.0));
            let target = &history[rng.gen_range(0..history.len())];
            let dependencies = generated_dependencies(&mut rng, target);
            let storage_conflict = store
                .read_commit_log_from(SequenceNumber(snapshot.0.saturating_add(1)))
                .expect("differential storage scan should read")
                .into_iter()
                .find_map(|entry| {
                    commit_intersects_dependency_set(&entry, &dependencies, &[], |table, id| {
                        store.get(table, &id)
                    })
                    .then_some(entry.sequence)
                });
            let ValidationSource::InMemory(view) = log
                .validation_source(snapshot, head)
                .expect("generated in-memory validation should be retained")
            else {
                panic!("fully populated generated history should not fall back");
            };
            let window_conflict =
                view.first_conflicting_sequence(&dependencies, |table, id| store.get(table, &id));
            assert_eq!(
                window_conflict, storage_conflict,
                "differential case {checked} disagreed at snapshot {snapshot}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, CORPUS_SIZE);
}

fn document(slot: usize, status: &str, rank: u64) -> Document {
    let table = TableName::new("differential_messages").expect("table name should be valid");
    Document {
        id: DocumentId::from_key(format!("generated-{slot}"))
            .expect("generated document id should be valid"),
        table,
        creation_time: Timestamp(u64::try_from(slot).unwrap_or(u64::MAX)),
        update_time: Timestamp(u64::try_from(slot).unwrap_or(u64::MAX)),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!(status)),
            ("rank".to_string(), json!(rank)),
        ]),
        typed_fields: Default::default(),
    }
}

fn generated_dependencies(rng: &mut StdRng, commit: &CommitEntry) -> DependencySet {
    let write = &commit.writes[0];
    let mut dependencies = DependencySet::default();
    match rng.gen_range(0..8) {
        0 => dependencies.record_table(&write.table, &write.table_id),
        1 => dependencies.record_document(&write.table, &write.table_id, write.doc_id.clone()),
        2 => dependencies.record_missing_table(&write.table),
        3 => dependencies.record_predicate(PredicateDependency {
            table: write.table.clone(),
            table_id: write.table_id.clone(),
            filters: vec![Filter {
                field: "status".to_string(),
                op: FilterOp::Eq,
                value: json!(if rng.gen_bool(0.5) {
                    "active"
                } else {
                    "archived"
                }),
            }],
        }),
        4 => dependencies.record_paginated_window(PaginatedWindowDependency {
            table: write.table.clone(),
            table_id: write.table_id.clone(),
            filters: Vec::new(),
            order: None,
            start_sort_values: Vec::new(),
            start_doc_id: None,
            end_sort_values: Vec::new(),
            end_doc_id: None,
            result_count: rng.gen_range(0..4),
            page_size: 4,
        }),
        5 => dependencies.record_index_range(IndexRangeDependency {
            table: write.table.clone(),
            table_id: write.table_id.clone(),
            index_id: IndexId::new(),
            index_name: "by_rank".to_string(),
            field: "rank".to_string(),
            start: Some(json!(rng.gen_range(0..50))),
            end: Some(json!(rng.gen_range(50..100))),
            start_inclusive: rng.gen_bool(0.5),
            end_inclusive: rng.gen_bool(0.5),
        }),
        6 => dependencies.record_document(
            &write.table,
            &write.table_id,
            DocumentId::from_key(format!("absent-{}", rng.r#gen::<u64>()))
                .expect("absent id should be valid"),
        ),
        _ => dependencies.record_table(&write.table, &TableId::new()),
    }
    dependencies
}
