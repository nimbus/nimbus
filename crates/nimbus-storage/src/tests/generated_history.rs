use super::*;

const STORAGE_CONFORMANCE_SEED_ENV: &str = "NIMBUS_STORAGE_CONFORMANCE_SEED";

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
    documents.sort_by_key(|left| left.id.clone());
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
            let document_id = document.id.clone();
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
                "nimbus-storage",
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
            "nimbus-storage",
            test_name,
            "storage final state diverged from the generated model"
        ))
        .unwrap_or_else(|| history.failure_context(
            "storage final state diverged from the generated model",
            None
        ))
    );
}

fn assert_generated_task_mvcc_history_matches_model(
    history: &GeneratedTaskHistory,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
) {
    let table = TableName::new(history.table()).expect("generated task table should be valid");
    let store = TenantStore::create_in_memory().expect("store should open");
    let bootstrap = store
        .export_changefeed_bootstrap()
        .expect("changefeed bootstrap should export before generated replay");
    let mut ids_by_slot = BTreeMap::<u32, DocumentId>::new();
    let mut prefix_sequences = Vec::new();

    for (step_index, step) in history.steps().iter().enumerate() {
        let commit = match step {
            crate::GeneratedTaskHistoryStep::Insert { slot, record } => {
                let document = Document::new(table.clone(), generated_task_fields(record));
                let document_id = document.id.clone();
                let commit = store.insert(&document).unwrap_or_else(|error| {
                    panic!(
                        "{}: {error}",
                        history.failure_context(
                            "generated MVCC insert should commit",
                            Some(step_index)
                        )
                    )
                });
                ids_by_slot.insert(*slot, document_id);
                commit
            }
            crate::GeneratedTaskHistoryStep::Update { slot, record } => {
                let document_id = ids_by_slot.get(slot).unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during generated MVCC update",
                            Some(step_index),
                        )
                    )
                });
                store
                    .update(&table, document_id, &generated_task_fields(record))
                    .unwrap_or_else(|error| {
                        panic!(
                            "{}: {error}",
                            history.failure_context(
                                "generated MVCC update should commit",
                                Some(step_index),
                            )
                        )
                    })
            }
            crate::GeneratedTaskHistoryStep::Delete { slot } => {
                let document_id = ids_by_slot.remove(slot).unwrap_or_else(|| {
                    panic!(
                        "{}",
                        history.failure_context(
                            "missing slot binding during generated MVCC delete",
                            Some(step_index),
                        )
                    )
                });
                store.delete(&table, &document_id).unwrap_or_else(|error| {
                    panic!(
                        "{}: {error}",
                        history.failure_context(
                            "generated MVCC delete should commit",
                            Some(step_index)
                        )
                    )
                })
            }
        };
        prefix_sequences.push(commit.sequence);

        let actual =
            normalize_generated_task_documents(store.scan_table(&table).unwrap_or_else(|error| {
                panic!(
                    "{}: {error}",
                    history.failure_context(
                        "generated MVCC latest scan should succeed",
                        Some(step_index),
                    )
                )
            }));
        let expected = history.model_through(step_index + 1).final_documents();
        assert_eq!(
            actual,
            expected,
            "{}",
            case.map(|case| case.failure_context(
                "nimbus-storage",
                test_name,
                "generated MVCC latest prefix diverged from model"
            ))
            .unwrap_or_else(|| history.failure_context(
                "generated MVCC latest prefix diverged from model",
                Some(step_index),
            ))
        );
    }

    let checkpoints = [
        0,
        prefix_sequences.len() / 2,
        prefix_sequences.len().saturating_sub(1),
    ];
    for checkpoint in checkpoints {
        let Some(sequence) = prefix_sequences.get(checkpoint).copied() else {
            continue;
        };
        let archive = store
            .export_point_in_time_restore_archive(
                crate::PointInTimeRestoreTarget::Sequence(sequence),
                crate::RetentionGcConfig::retain_all(),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{}: {error}",
                    history.failure_context(
                        "generated MVCC PITR archive should export",
                        Some(checkpoint),
                    )
                )
            });
        let restored = TenantStore::create_in_memory().expect("PITR restore store should open");
        restored
            .import_point_in_time_restore_archive(&archive)
            .unwrap_or_else(|error| {
                panic!(
                    "{}: {error}",
                    history.failure_context(
                        "generated MVCC PITR archive should import",
                        Some(checkpoint),
                    )
                )
            });
        let restored_documents = normalize_generated_task_documents(
            restored
                .scan_table(&table)
                .expect("restored generated history scan should succeed"),
        );
        let expected = history.model_through(checkpoint + 1).final_documents();
        assert_eq!(
            restored_documents,
            expected,
            "{}",
            history.failure_context(
                "generated MVCC PITR restored prefix diverged from model",
                Some(checkpoint),
            )
        );
    }

    let mut cursor = bootstrap.cursor;
    let mut cdc_document_sequences = Vec::new();
    loop {
        let page = store
            .stream_changefeed(&cursor, 3)
            .expect("generated MVCC changefeed should stream");
        for event in &page.events {
            if event
                .events
                .iter()
                .any(|event| matches!(event, nimbus_core::TenantEventKind::DocumentWrite { .. }))
            {
                cdc_document_sequences.push(event.sequence);
            }
        }
        cursor = page.next_cursor;
        if !page.has_more && cursor.after.0 >= page.latest_sequence.0 {
            break;
        }
    }
    assert_eq!(
        cdc_document_sequences,
        prefix_sequences,
        "{}",
        case.map(|case| case.failure_context(
            "nimbus-storage",
            test_name,
            "generated MVCC CDC stream missed or duplicated document-write sequences"
        ))
        .unwrap_or_else(|| history.failure_context(
            "generated MVCC CDC stream missed or duplicated document-write sequences",
            None,
        ))
    );
}

fn collect_changefeed_document_sequences<F>(
    mut cursor: crate::ChangefeedCursor,
    mut stream: F,
) -> Vec<SequenceNumber>
where
    F: FnMut(&crate::ChangefeedCursor) -> crate::ChangefeedPage,
{
    let mut sequences = Vec::new();
    loop {
        let page = stream(&cursor);
        for event in &page.events {
            if event
                .events
                .iter()
                .any(|event| matches!(event, nimbus_core::TenantEventKind::DocumentWrite { .. }))
            {
                sequences.push(event.sequence);
            }
        }
        cursor = page.next_cursor;
        if !page.has_more && cursor.after.0 >= page.latest_sequence.0 {
            break;
        }
    }
    sequences
}

fn build_generated_task_durable_record(
    store: &TenantStore,
    history: &GeneratedTaskHistory,
    step_index: usize,
    table_id: &TableId,
    documents_by_slot: &mut BTreeMap<u32, Document>,
) -> TenantEventRecord {
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
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
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
                table_id: table_id.clone(),
                op_type: WriteOpType::Update,
                doc_id: current.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
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
                table_id: table_id.clone(),
                op_type: WriteOpType::Delete,
                doc_id: previous.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(previous),
                current: None,
            }]
        }
    };

    TenantEventRecord::new(
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
                    let document_id = live_ids[index].clone();
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
fn datadriven_generated_task_history_drives_mvcc_pitr_and_cdc_conformance() {
    let history = GeneratedTaskHistory::datadriven(
        "datadriven-mvcc",
        r#"
        insert 0 todo 1 first
        insert 1 done 2 second
        update 0 done 3 first_done
        insert 2 in_progress 4 third
        delete 1
        update 2 done 5 third_done
        "#,
    )
    .expect("datadriven history should parse");
    assert_generated_task_mvcc_history_matches_model(
        &history,
        None,
        "datadriven_generated_task_history_drives_mvcc_pitr_and_cdc_conformance",
    );
}

#[test]
fn generated_mvcc_history_required_seed_corpus_matches_pitr_and_cdc_models() {
    let history = GeneratedTaskHistory::seeded("generated-mvcc-required", 41, 18);
    assert_generated_task_mvcc_history_matches_model(
        &history,
        None,
        "generated_mvcc_history_required_seed_corpus_matches_pitr_and_cdc_models",
    );
}

#[test]
fn generated_retained_checkpoint_restores_every_available_target() {
    let history = GeneratedTaskHistory::seeded("retained-checkpoint", 97, 18);
    let table = TableName::new(history.table()).expect("generated task table should be valid");
    let store = TenantStore::create_in_memory().expect("store should open");
    replay_generated_task_history(
        &history,
        |_slot, record| {
            let document = Document::new(table.clone(), generated_task_fields(record));
            let document_id = document.id.clone();
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
    .expect("generated retained history should replay");

    let latest = store
        .latest_sequence()
        .expect("latest sequence should read");
    let config = crate::RetentionGcConfig::new(6).expect("bounded retention should build");
    let summary = store
        .compact_retained_history(config)
        .expect("generated retained history should compact");
    let checkpoint = summary.after.confirmed_floor;
    assert_eq!(checkpoint, SequenceNumber(latest.0 - 6));

    for target in checkpoint.0..=latest.0 {
        let archive = store
            .export_point_in_time_restore_archive(
                crate::PointInTimeRestoreTarget::Sequence(SequenceNumber(target)),
                config,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{}: {error}",
                    history.failure_context(
                        "retained checkpoint archive should export",
                        Some(target as usize - 1),
                    )
                )
            });
        let restored = TenantStore::create_in_memory().expect("restore target should open");
        restored
            .import_point_in_time_restore_archive(&archive)
            .expect("retained checkpoint archive should restore");
        assert_eq!(
            restored
                .export_materialized_journal_snapshot()
                .expect("restored snapshot should export")
                .materialized_position()
                .expect("restored position should compute"),
            archive.target_position,
            "target sequence {target}"
        );
        assert_eq!(
            normalize_generated_task_documents(
                restored
                    .scan_table(&table)
                    .expect("restored generated table should scan")
            ),
            history.model_through(target as usize).final_documents(),
            "target sequence {target}"
        );
    }

    let expired = store
        .export_point_in_time_restore_archive(
            crate::PointInTimeRestoreTarget::Sequence(SequenceNumber(checkpoint.0 - 1)),
            config,
        )
        .expect_err("target before retained checkpoint should expire");
    assert!(matches!(
        expired,
        Error::HistoricalRead {
            kind: nimbus_core::HistoricalReadErrorKind::RetentionExpired,
            ..
        }
    ));
}

#[test]
fn canonical_digest_generated_history_matches_redb_sqlite_pitr_cdc_and_rebuild_paths() {
    let clock = Arc::new(ManualWallClock::new(Timestamp(90_000)));
    let redb = TenantStore::create_in_memory_with_simulation(
        clock.clone(),
        Arc::new(crate::NoopFaultInjector),
    )
    .expect("redb store should open");
    let sqlite_dir = tempdir().expect("sqlite tempdir should create");
    let sqlite = SqliteTenantStore::open_with_simulation(
        sqlite_dir.path().join("tenant.sqlite3"),
        clock.clone(),
        Arc::new(crate::NoopFaultInjector),
    )
    .expect("sqlite store should open");
    let table = TableName::new("tasks").expect("table should parse");
    let table_id = TableId::new();
    redb.stage_hidden_table_identity(&table, &table_id)
        .expect("redb hidden table should stage");
    sqlite
        .stage_hidden_table_identity(&table, &table_id)
        .expect("sqlite hidden table should stage");
    redb.activate_hidden_table_identity(&table, &table_id)
        .expect("redb hidden table should activate");
    sqlite
        .activate_hidden_table_identity(&table, &table_id)
        .expect("sqlite hidden table should activate");

    let redb_bootstrap = redb
        .export_changefeed_bootstrap()
        .expect("redb changefeed bootstrap should export");
    let sqlite_bootstrap = sqlite
        .export_changefeed_bootstrap()
        .expect("sqlite changefeed bootstrap should export");
    assert_eq!(redb_bootstrap.cursor.after, sqlite_bootstrap.cursor.after);

    let history = GeneratedTaskHistory::seeded("parity-digest", 83, 16);
    let mut ids_by_slot = BTreeMap::<u32, DocumentId>::new();
    let mut prefix_sequences = Vec::new();
    for (step_index, step) in history.steps().iter().enumerate() {
        clock.set(Timestamp(91_000 + step_index as u64));
        let (redb_commit, sqlite_commit) = match step {
            crate::GeneratedTaskHistoryStep::Insert { slot, record } => {
                let document = Document::new(table.clone(), generated_task_fields(record));
                ids_by_slot.insert(*slot, document.id.clone());
                (
                    redb.insert(&document).expect("redb insert should commit"),
                    sqlite
                        .insert(&document)
                        .expect("sqlite insert should commit"),
                )
            }
            crate::GeneratedTaskHistoryStep::Update { slot, record } => {
                let document_id = ids_by_slot
                    .get(slot)
                    .expect("generated parity update slot should exist");
                (
                    redb.update(&table, document_id, &generated_task_fields(record))
                        .expect("redb update should commit"),
                    sqlite
                        .update(&table, document_id, &generated_task_fields(record))
                        .expect("sqlite update should commit"),
                )
            }
            crate::GeneratedTaskHistoryStep::Delete { slot } => {
                let document_id = ids_by_slot
                    .remove(slot)
                    .expect("generated parity delete slot should exist");
                (
                    redb.delete(&table, &document_id)
                        .expect("redb delete should commit"),
                    sqlite
                        .delete(&table, &document_id)
                        .expect("sqlite delete should commit"),
                )
            }
        };
        assert_eq!(redb_commit.sequence, sqlite_commit.sequence);
        prefix_sequences.push(redb_commit.sequence);
    }

    let redb_snapshot = redb
        .export_materialized_journal_snapshot()
        .expect("redb latest snapshot should export");
    let sqlite_snapshot = sqlite
        .export_materialized_journal_snapshot()
        .expect("sqlite latest snapshot should export");
    let redb_latest = redb_snapshot
        .materialized_position()
        .expect("redb latest digest should compute");
    let sqlite_latest = sqlite_snapshot
        .materialized_position()
        .expect("sqlite latest digest should compute");
    assert_eq!(redb_latest, sqlite_latest);

    for sequence in [
        prefix_sequences[prefix_sequences.len() / 2],
        *prefix_sequences
            .last()
            .expect("generated parity sequence should exist"),
    ] {
        let redb_archive = redb
            .export_point_in_time_restore_archive(
                crate::PointInTimeRestoreTarget::Sequence(sequence),
                crate::RetentionGcConfig::retain_all(),
            )
            .expect("redb PITR archive should export");
        let sqlite_archive = sqlite
            .export_point_in_time_restore_archive(
                crate::PointInTimeRestoreTarget::Sequence(sequence),
                crate::RetentionGcConfig::retain_all(),
            )
            .expect("sqlite PITR archive should export");
        assert_eq!(redb_archive.target_position, sqlite_archive.target_position);

        let restored = TenantStore::create_in_memory_with_simulation(
            clock.clone(),
            Arc::new(crate::NoopFaultInjector),
        )
        .expect("restored parity store should open");
        restored
            .import_point_in_time_restore_archive(&redb_archive)
            .expect("redb PITR archive should restore through replay");
        assert_eq!(
            restored
                .export_materialized_journal_snapshot()
                .expect("restored parity snapshot should export")
                .materialized_position()
                .expect("restored parity digest should compute"),
            redb_archive.target_position
        );
    }

    let redb_sequences = collect_changefeed_document_sequences(redb_bootstrap.cursor, |cursor| {
        redb.stream_changefeed(cursor, 4)
            .expect("redb changefeed should stream")
    });
    let sqlite_sequences =
        collect_changefeed_document_sequences(sqlite_bootstrap.cursor, |cursor| {
            sqlite
                .stream_changefeed(cursor, 4)
                .expect("sqlite changefeed should stream")
        });
    assert_eq!(redb_sequences, prefix_sequences);
    assert_eq!(sqlite_sequences, prefix_sequences);
}

#[test]
fn storage_conformance_required_seed_corpus_matches_model() {
    let seed = std::env::var(STORAGE_CONFORMANCE_SEED_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(67);
    let history = GeneratedTaskHistory::seeded("storage-conformance", seed, 18);
    assert_generated_task_history_matches_model_on_storage_surface(
        &history,
        None,
        "storage_conformance_required_seed_corpus_matches_model",
    );
}

#[test]
fn schema_index_lifecycle_transition_and_retention_pin_set_the_diagnostic_retention_floor() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_conformance").expect("table should parse");
    let schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
            state: nimbus_core::IndexState::Enabled,
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("schema/index transition should commit");
    let hidden_id = TableId::new();
    store
        .stage_hidden_table_identity(&table, &hidden_id)
        .expect("lifecycle stage should commit");
    store
        .activate_hidden_table_identity(&table, &hidden_id)
        .expect("lifecycle activation should commit");
    let _pin = store.pin_retention_participant(
        RetentionParticipant::ShadowMaterializer,
        store.latest_sequence().expect("sequence should load"),
        Some(hidden_id),
        "generated conformance retention pin",
    );
    assert_eq!(
        store
            .storage_health_diagnostic()
            .expect("health diagnostic should load")
            .retention_floor,
        Some(store.latest_sequence().expect("sequence should load"))
    );
}

#[test]
fn crash_replay_diagnostic_and_retention_snapshot_diagnostic_are_seed_replayable() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let table_id = TableId::new();
    let document = sample_document("tasks_crash_replay", "pending");
    let record = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(100),
        vec![WriteOp {
            table: document.table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("record should build");
    {
        let store = TenantStore::open(&path).expect("store should open");
        store
            .append_durable_records_batch(&[record])
            .expect("append should commit before simulated crash");
        assert_eq!(
            store
                .storage_health_diagnostic()
                .expect("diagnostic should load")
                .last_recovery_status,
            "pending_replay"
        );
    }

    let recovered = TenantStore::open(&path).expect("store should reopen");
    recovered
        .recover_durable_journal()
        .expect("recovery should replay pending record");
    let health = recovered
        .storage_health_diagnostic()
        .expect("diagnostic should load after replay");
    assert_eq!(health.last_recovery_status, "caught_up");

    let floor = RetentionFloor::new();
    let _pin = floor.pin(
        RetentionParticipant::EmbeddedReplica,
        health.applied_head,
        Some(table_id.clone()),
        "replica snapshot diagnostic",
    );
    let restored = RetentionFloor::restore_from_snapshot(floor.snapshot());
    assert!(matches!(
        restored.hard_delete_decision(&table_id, health.applied_head),
        HardDeleteDecision::Denied { .. }
    ));
}

#[test]
#[ignore = "verification harness required corpus runs in dedicated harness lanes"]
fn verification_harness_required_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Required)
        .expect("required corpus should resolve")
    {
        let history = case.history("storage-history");
        assert_generated_task_history_matches_model_on_storage_surface(
            &history,
            Some(case),
            "verification_harness_required_generated_history_seed_corpus_matches_model",
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
    let table_id = TableId::new();
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
            &table_id,
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
