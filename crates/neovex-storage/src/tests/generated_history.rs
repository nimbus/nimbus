use super::*;

fn next_seeded_u64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    *seed
}

fn generated_task_fields(
    record: &GeneratedTaskRecord,
) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([
        ("title".to_string(), json!(record.title)),
        ("status".to_string(), json!(record.status)),
        ("rank".to_string(), json!(record.rank)),
    ])
}

fn normalize_generated_task_documents(mut documents: Vec<Document>) -> Vec<GeneratedTaskRecord> {
    documents.sort_by_key(|left| left.id);
    let mut records = documents
        .into_iter()
        .map(|document| GeneratedTaskRecord {
            title: document
                .get_field("title")
                .and_then(serde_json::Value::as_str)
                .expect("generated task title should be present")
                .to_string(),
            status: document
                .get_field("status")
                .and_then(serde_json::Value::as_str)
                .expect("generated task status should be present")
                .to_string(),
            rank: document
                .get_field("rank")
                .and_then(serde_json::Value::as_i64)
                .expect("generated task rank should be present"),
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|left| left.title.clone());
    records
}

fn assert_generated_task_history_matches_model_on_storage_surface(
    history: &GeneratedTaskHistory,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
) {
    let table = TableName::new(history.table()).expect("generated task table should be valid");
    let store = TenantStore::create_in_memory().expect("store should open");

    replay_generated_task_history(
        history,
        |_slot, record| {
            let document = Document::new(table.clone(), generated_task_fields(record));
            let document_id = document.id;
            store.insert(&document)?;
            Ok::<DocumentId, Error>(document_id)
        },
        |_slot, document_id, record| {
            store.update(&table, document_id, &generated_task_fields(record))?;
            Ok::<(), Error>(())
        },
        |_slot, document_id| {
            store.delete(&table, document_id)?;
            Ok::<(), Error>(())
        },
    )
    .unwrap_or_else(|error| {
        panic!(
            "{}: {error}",
            case.map(|case| case.failure_context(
                "neovex-storage",
                test_name,
                "storage replay failed"
            ))
            .unwrap_or_else(|| history.failure_context("storage replay failed", None))
        )
    });

    let actual = normalize_generated_task_documents(
        store
            .scan_table(&table)
            .expect("storage scan should succeed after generated replay"),
    );
    let expected = history.model().final_documents();
    assert_eq!(
        actual,
        expected,
        "{}",
        case.map(|case| case.failure_context(
            "neovex-storage",
            test_name,
            "storage final state diverged from the generated model"
        ))
        .unwrap_or_else(|| history.failure_context(
            "storage final state diverged from the generated model",
            None
        ))
    );
}

fn build_generated_task_durable_record(
    store: &TenantStore,
    history: &GeneratedTaskHistory,
    step_index: usize,
    documents_by_slot: &mut BTreeMap<u32, Document>,
) -> DurableMutationRecord {
    let sequence = SequenceNumber(
        store
            .latest_sequence()
            .expect("latest sequence should read")
            .0
            .saturating_add(1),
    );
    let step = history
        .steps()
        .get(step_index)
        .expect("generated recovery step should exist");
    let writes = match step {
        crate::GeneratedTaskHistoryStep::Insert { slot, record } => {
            let document = Document::new(
                TableName::new(history.table()).expect("generated task table should be valid"),
                record.fields(),
            );
            documents_by_slot.insert(*slot, document.clone());
            vec![WriteOp {
                table: document.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: document.id,
                previous: None,
                current: Some(document),
            }]
        }
        crate::GeneratedTaskHistoryStep::Update { slot, record } => {
            let previous = documents_by_slot.get(slot).cloned().unwrap_or_else(|| {
                panic!(
                    "{}",
                    history.failure_context(
                        "missing generated task slot while building durable update record",
                        Some(step_index),
                    )
                )
            });
            let mut current = previous.clone();
            current.fields = record.fields();
            documents_by_slot.insert(*slot, current.clone());
            vec![WriteOp {
                table: current.table.clone(),
                op_type: WriteOpType::Update,
                doc_id: current.id,
                previous: Some(previous),
                current: Some(current),
            }]
        }
        crate::GeneratedTaskHistoryStep::Delete { slot } => {
            let previous = documents_by_slot.remove(slot).unwrap_or_else(|| {
                panic!(
                    "{}",
                    history.failure_context(
                        "missing generated task slot while building durable delete record",
                        Some(step_index),
                    )
                )
            });
            vec![WriteOp {
                table: previous.table.clone(),
                op_type: WriteOpType::Delete,
                doc_id: previous.id,
                previous: Some(previous),
                current: None,
            }]
        }
    };

    DurableMutationRecord::new(
        sequence,
        Timestamp(80_000_u64.saturating_add(step_index as u64)),
        writes,
        None,
    )
    .unwrap_or_else(|error| {
        panic!(
            "{}: {error}",
            history.failure_context(
                "generated durable recovery record should build",
                Some(step_index),
            )
        )
    })
}

#[test]
fn shadow_materializer_seeded_rebuild_matches_live_state_across_operation_sequences() {
    let table = TableName::new("tasks").expect("table name should be valid");

    for initial_seed in [1_u64, 7, 13, 42] {
        let live = TenantStore::create_in_memory().expect("store should open");
        let mut seed = initial_seed;
        let mut live_ids = Vec::new();
        let snapshot_step = (next_seeded_u64(&mut seed) % 12 + 4) as usize;
        let mut checkpoint = live
            .export_materialized_journal_snapshot()
            .expect("initial checkpoint should export");

        for step in 0..24 {
            let draw = next_seeded_u64(&mut seed);
            let choice = if live_ids.is_empty() { 0 } else { draw % 3 };
            match choice {
                0 => {
                    let document = Document::new(
                        table.clone(),
                        serde_json::Map::from_iter([
                            (
                                "title".to_string(),
                                json!(format!("seed-{initial_seed}-insert-{step}")),
                            ),
                            ("rank".to_string(), json!((draw % 100) as i64)),
                        ]),
                    );
                    live.insert(&document).expect("insert should succeed");
                    live_ids.push(document.id);
                }
                1 => {
                    let index = (draw as usize) % live_ids.len();
                    let document_id = live_ids[index];
                    live.update(
                        &table,
                        &document_id,
                        &serde_json::Map::from_iter([
                            (
                                "title".to_string(),
                                json!(format!("seed-{initial_seed}-update-{step}")),
                            ),
                            ("rank".to_string(), json!(((draw >> 8) % 100) as i64)),
                        ]),
                    )
                    .expect("update should succeed");
                }
                _ => {
                    let index = (draw as usize) % live_ids.len();
                    let document_id = live_ids.swap_remove(index);
                    live.delete(&table, &document_id)
                        .expect("delete should succeed");
                }
            }

            if step == snapshot_step {
                checkpoint = live
                    .export_materialized_journal_snapshot()
                    .expect("mid-run checkpoint should export");
            }
        }

        let journal_tail = live
            .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
            .expect("journal tail should read");
        let config = ShadowMaterializerConfig {
            compaction_threshold_records: ((initial_seed % 4) + 2) as usize,
        };

        let left = ShadowMaterializer::from_checkpoint_and_journal(
            checkpoint.clone(),
            journal_tail.clone(),
            config,
        )
        .expect("left shadow materializer should rebuild");
        let right =
            ShadowMaterializer::from_checkpoint_and_journal(checkpoint, journal_tail, config)
                .expect("right shadow materializer should rebuild");
        let live_snapshot = live
            .export_materialized_journal_snapshot()
            .expect("live snapshot should export");

        assert_eq!(
            left.current_snapshot(),
            live_snapshot,
            "seed {initial_seed}"
        );
        assert_eq!(
            left.current_snapshot(),
            right.current_snapshot(),
            "rebuild should be deterministic for seed {initial_seed}"
        );
        assert_eq!(
            left.manifest(),
            right.manifest(),
            "manifest should be deterministic for seed {initial_seed}"
        );
    }
}

#[test]
fn generated_task_history_matches_model_on_storage_surface() {
    let history = GeneratedTaskHistory::seeded("storage-history", 31, 24);
    assert_generated_task_history_matches_model_on_storage_surface(
        &history,
        None,
        "generated_task_history_matches_model_on_storage_surface",
    );
}

#[test]
#[ignore = "verification harness PR corpus runs in dedicated harness lanes"]
fn verification_harness_pr_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let history = case.history("storage-history");
        assert_generated_task_history_matches_model_on_storage_surface(
            &history,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
        );
    }
}

#[test]
#[ignore = "verification harness nightly corpus runs in dedicated harness lanes"]
fn verification_harness_nightly_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let history = case.history("storage-history");
        assert_generated_task_history_matches_model_on_storage_surface(
            &history,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        );
    }
}

#[test]
fn generated_recovery_campaign_replays_durable_journal_across_repeated_restarts_and_rebuilds_shadow_state()
 {
    let history = GeneratedTaskHistory::seeded("storage-recovery-history", 53, 18);
    let restart_schedule = ScriptedRestartSchedule::seeded(
        "storage-recovery-restarts",
        53,
        history.steps().len(),
        3,
        &[RestartBoundary::DurableAppendBeforeApply],
    );
    assert!(
        restart_schedule.restart_points().len() >= 2,
        "recovery campaign should exercise repeated restarts: {}",
        restart_schedule.describe()
    );

    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let table = TableName::new(history.table()).expect("generated task table should be valid");
    let mut durable_documents_by_slot = BTreeMap::new();
    let mut recovered_prefix_len = 0_usize;

    for step_index in 0..history.steps().len() {
        let store = TenantStore::open(&path).expect("store should open");
        let visible_before_append = normalize_generated_task_documents(
            store
                .scan_table(&table)
                .expect("authoritative scan should succeed before restart"),
        );
        let expected_before_recovery = history
            .model_through(recovered_prefix_len)
            .final_documents();
        assert_eq!(
            visible_before_append,
            expected_before_recovery,
            "{}",
            restart_schedule.failure_context(
                "visible state before recovery should match the last recovered prefix",
                Some(step_index),
            )
        );

        let record = build_generated_task_durable_record(
            &store,
            &history,
            step_index,
            &mut durable_documents_by_slot,
        );
        store
            .append_durable_records_batch(&[record])
            .unwrap_or_else(|error| {
                panic!(
                    "{}: {error}",
                    restart_schedule.failure_context(
                        "durable append should succeed during recovery campaign",
                        Some(step_index),
                    )
                )
            });

        let visible_before_recovery = normalize_generated_task_documents(
            store
                .scan_table(&table)
                .expect("authoritative scan should stay on the last applied prefix"),
        );
        assert_eq!(
            visible_before_recovery,
            expected_before_recovery,
            "{}",
            restart_schedule.failure_context(
                "durable-but-unapplied records must stay invisible before recovery",
                Some(step_index),
            )
        );
        drop(store);

        let should_restart = restart_schedule
            .restart_point_after_step(step_index)
            .is_some()
            || step_index + 1 == history.steps().len();
        if !should_restart {
            continue;
        }

        let reopened = TenantStore::open(&path).expect("store should reopen");
        let progress = reopened
            .recover_durable_journal()
            .expect("recovery should apply all pending durable records");
        assert_eq!(
            progress.durable_head,
            progress.applied_head,
            "{}",
            restart_schedule.failure_context(
                "recovery should converge durable and applied heads",
                Some(step_index),
            )
        );

        recovered_prefix_len = step_index + 1;
        let expected_after_recovery = history
            .model_through(recovered_prefix_len)
            .final_documents();
        let actual_after_recovery = normalize_generated_task_documents(
            reopened
                .scan_table(&table)
                .expect("authoritative scan should succeed after recovery"),
        );
        assert_eq!(
            actual_after_recovery,
            expected_after_recovery,
            "{}",
            restart_schedule.failure_context(
                "recovered authoritative state should match the generated prefix model",
                Some(step_index),
            )
        );

        let checkpoint = reopened
            .export_materialized_journal_snapshot()
            .expect("checkpoint should export after recovery");
        let journal_tail = reopened
            .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
            .expect("journal tail should read after recovery");
        let shadow = ShadowMaterializer::from_checkpoint_and_journal(
            checkpoint,
            journal_tail,
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .expect("shadow materializer should rebuild after recovery");
        let shadow_documents =
            normalize_generated_task_documents(shadow.current_snapshot().documents.clone());
        assert_eq!(
            shadow_documents,
            expected_after_recovery,
            "{}",
            restart_schedule.failure_context(
                "shadow rebuild should match the recovered authoritative state",
                Some(step_index),
            )
        );
    }
}
