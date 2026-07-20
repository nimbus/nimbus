use super::support::*;
use crate::tests::{
    exercise_applied_sequence_corruption_rejection, exercise_applied_sequence_recovery_replay,
    exercise_durable_update_guard_is_corruption, exercise_pending_prefix_blocks_generic_zero_write,
};

fn durable_insert_record(
    sequence: u64,
    timestamp: u64,
    table_id: TableId,
    document: Document,
) -> TenantEventRecord {
    TenantEventRecord::new(
        SequenceNumber(sequence),
        Timestamp(timestamp),
        vec![WriteOp {
            table: document.table.clone(),
            table_id,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document),
        }],
        None,
    )
    .expect("durable insert record should build")
}

fn assert_post_visibility_fault(error: &Error, visit: u64) {
    assert!(
        matches!(error, Error::Internal(message) if message.contains("storage_commit_after_visibility_before_return") && message.contains(&format!("visit {visit}"))),
        "the provider must return the injected post-visibility fault from visit {visit}: {error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_post_visibility_fault_preserves_one_committed_record_and_replays() {
    let faults = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        visit: 1,
    }]));
    with_test_provider_with_faults(faults, |provider, _config| async move {
        let tenant = TenantId::new("post-visibility-append").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("tasks", "landed");
        let record = durable_insert_record(1, 100, TableId::new(), document.clone());

        let error = opened
            .store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect_err("the acknowledgement-loss seam must return an error");
        assert_post_visibility_fault(&error, 1);
        assert_eq!(
            opened
                .store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("durable journal should remain readable"),
            vec![record],
            "the failed acknowledgement must preserve exactly one durable record"
        );

        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should apply the one durable record exactly once");
        assert_eq!(progress.durable_head, SequenceNumber(1));
        assert_eq!(progress.applied_head, SequenceNumber(1));
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("replayed document should load")
                .as_ref(),
            Some(&document)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_identical_replay_is_idempotent_after_lost_ack() {
    let faults = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        visit: 2,
    }]));
    with_test_provider_with_faults(faults, |provider, _config| async move {
        let tenant =
            TenantId::new("identical-replay-after-ack-loss").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("tasks", "original");
        let record = durable_insert_record(1, 100, TableId::new(), document.clone());

        opened
            .store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("durable append should succeed before apply acknowledgement loss");
        let error = opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect_err("first apply should lose its acknowledgement");
        assert_post_visibility_fault(&error, 2);
        opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("an identical replay must be idempotent");
        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should refresh the local replica");
        assert_eq!(progress.durable_head, SequenceNumber(1));
        assert_eq!(progress.applied_head, SequenceNumber(1));
        assert_eq!(
            opened
                .store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("journal should read"),
            vec![record],
            "idempotent replay must not duplicate the durable record"
        );
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("document should load")
                .as_ref(),
            Some(&document)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_different_content_replay_is_rejected_after_lost_ack() {
    let faults = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
        visit: 2,
    }]));
    with_test_provider_with_faults(faults, |provider, _config| async move {
        let tenant =
            TenantId::new("divergent-replay-after-ack-loss").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table_id = TableId::new();
        let original = crate::tests::sample_document("tasks", "original");
        let divergent = crate::tests::sample_document("tasks", "divergent");
        let record = durable_insert_record(1, 100, table_id.clone(), original.clone());
        let divergent_record = durable_insert_record(1, 200, table_id, divergent.clone());

        opened
            .store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("durable append should succeed before apply acknowledgement loss");
        let error = opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect_err("first apply should lose its acknowledgement");
        assert_post_visibility_fault(&error, 2);
        let replay_error = opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&divergent_record))
            .expect_err("different content must not reuse an applied sequence");
        assert!(
            matches!(
                replay_error,
                Error::Storage {
                    kind: StorageErrorKind::Corruption,
                    ..
                }
            ),
            "different-content replay must be typed corruption: {replay_error}"
        );
        assert_eq!(
            replay_error.retryability(),
            nimbus_core::Retryability::Terminal
        );
        assert_eq!(
            opened
                .store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("durable journal should retain its original record"),
            vec![record],
            "the divergent retry must not replace or duplicate durable content"
        );

        opened
            .store
            .recover_durable_journal()
            .expect("recovery should accept the original durable record");
        assert_eq!(
            opened
                .store
                .get(&original.table, &original.id)
                .expect("original document should load")
                .as_ref(),
            Some(&original)
        );
        assert!(
            opened
                .store
                .get(&divergent.table, &divergent.id)
                .expect("divergent document lookup should succeed")
                .is_none(),
            "the rejected divergent replay must leave no materialized side effect"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_post_visibility_cancellation_does_not_report_safe_retry() {
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    with_test_provider_with_faults(faults.clone(), |provider, _config| async move {
        let tenant = TenantId::new("post-visibility-cancellation").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("tasks", "committed");
        let expected_id = document.id.clone();
        let cancel = Arc::new(tokio::sync::Notify::new());
        let cancel_for_wait = cancel.clone();
        let storage = opened.read_storage.clone();
        let handle = tokio::spawn(async move {
            storage
                .execute_write_cancellable(
                    async move { cancel_for_wait.notified().await },
                    || Ok(()),
                    move |transaction| {
                        transaction.insert_document(&document)?;
                        Ok(document.id)
                    },
                )
                .await
        });

        timeout(Duration::from_secs(5), faults.wait_until_entered())
            .await
            .expect("libSQL write should reach the post-visibility seam");
        cancel.notify_one();
        tokio::time::sleep(Duration::from_millis(25)).await;
        faults.release();

        let outcome = timeout(Duration::from_secs(5), handle)
            .await
            .expect("post-visibility cancellation should resolve")
            .expect("write task should join")
            .expect("write executor should return a committed outcome");
        let committed = match outcome {
            TenantWriteOutcome::Committed(committed) => committed,
            TenantWriteOutcome::CancelledBeforeCommit => {
                panic!("post-visibility cancellation must not advertise a safe retry")
            }
        };
        assert_eq!(committed.value, expected_id);
        assert_eq!(
            committed
                .commit
                .expect("document write should emit a commit entry")
                .sequence,
            SequenceNumber(1)
        );
        assert_eq!(
            opened
                .store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("durable journal should read")
                .len(),
            1,
            "the committed cancellation race must land exactly one record"
        );
        assert!(
            opened
                .store
                .get(
                    &TableName::new("tasks").expect("table should build"),
                    &expected_id
                )
                .expect("committed document should load")
                .is_some()
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_applied_sequence_recovery_replay_is_idempotent_for_all_write_shapes() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("duplicate-replay").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_applied_sequence_recovery_replay(opened.store.as_ref(), "libsql_duplicate_replay");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_applied_sequence_rejects_divergent_content_for_all_write_shapes() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("duplicate-corruption").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_applied_sequence_corruption_rejection(
            opened.store.as_ref(),
            "libsql_duplicate_corruption",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_pending_prefix_blocks_generic_zero_write() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("pending-prefix").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_pending_prefix_blocks_generic_zero_write(
            opened.store.as_ref(),
            "libsql_pending_prefix",
            || {
                opened
                    .store
                    .execute_write(|transaction| {
                        transaction.set_trigger_delivery_cursor(TriggerDeliveryCursor::new(
                            SequenceNumber(1),
                        ))
                    })
                    .map(|_| ())
            },
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_durable_update_guard_reports_corruption() {
    with_test_provider(|provider, _config| async move {
        let missing = provider
            .create_opened_tenant(&TenantId::new("missing-preimage").expect("tenant id"))
            .await
            .expect("tenant should create and open");
        exercise_durable_update_guard_is_corruption(
            missing.store.as_ref(),
            "libsql_missing_preimage",
            false,
        );
        let mismatched = provider
            .create_opened_tenant(&TenantId::new("mismatched-preimage").expect("tenant id"))
            .await
            .expect("tenant should create and open");
        exercise_durable_update_guard_is_corruption(
            mismatched.store.as_ref(),
            "libsql_mismatched_preimage",
            true,
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_direct_writes_refresh_derivative_cache_and_round_trip_journal_progress() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("writes").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("tasks", "First");

        let first_commit = opened
            .store
            .insert_once(&document, Some("exec-1"))
            .expect("first deduplicated insert should succeed")
            .expect("first deduplicated insert should commit");
        assert_eq!(first_commit.sequence, SequenceNumber(1));
        assert!(
            opened
                .store
                .insert_once(&document, Some("exec-1"))
                .expect("duplicate deduplicated insert should succeed")
                .is_none()
        );
        assert_eq!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );

        let second_commit = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("Renamed"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        assert_eq!(second_commit.sequence, SequenceNumber(2));
        let updated = opened
            .store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .expect("updated document should exist");
        assert_eq!(
            updated.fields.get("title").and_then(|value| value.as_str()),
            Some("Renamed")
        );

        let (third_commit, removed) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");
        assert_eq!(third_commit.sequence, SequenceNumber(3));
        assert_eq!(removed.id, document.id);

        timeout(Duration::from_secs(5), async {
            loop {
                if opened
                    .store
                    .journal_progress()
                    .expect("journal progress should load during background refresh")
                    == (crate::store::JournalProgress {
                        durable_head: SequenceNumber(3),
                        applied_head: SequenceNumber(3),
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("background refresh should catch the derivative cache up without a read-triggered refresh");

        let freshness = opened
            .store
            .replica_freshness_stats()
            .expect("freshness stats should load after background refresh");
        assert_eq!(freshness.required_sequence, SequenceNumber(3));
        assert_eq!(freshness.local_applied_sequence, SequenceNumber(3));
        assert_eq!(
            freshness.last_refresh_cause,
            LibsqlReplicaRefreshCause::CommitBarrier
        );
        assert_eq!(
            freshness.last_refresh_path,
            LibsqlReplicaRefreshPath::IncrementalCatchUp
        );
        assert!(
            freshness.incremental_refresh_count >= 1,
            "incremental refresh count should record the commit-barrier catch-up"
        );
        assert_eq!(freshness.refresh_error_count, 0);

        assert!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("deleted lookup should succeed")
                .is_none()
        );
        let after_read = opened
            .store
            .replica_freshness_stats()
            .expect("freshness stats should load after a current-cache read");
        assert_eq!(
            after_read.last_barrier_path,
            LibsqlReplicaBarrierPath::AlreadyCurrentCache
        );
        assert!(
            after_read.barrier_current_count >= 1,
            "a current-cache read should increment the already-current barrier counter"
        );

        let commits = opened
            .store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read");
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].writes[0].op_type, WriteOpType::Insert);
        assert_eq!(commits[1].writes[0].op_type, WriteOpType::Update);
        assert_eq!(commits[2].writes[0].op_type, WriteOpType::Delete);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let first = crate::tests::sample_document("tasks", "First");
        let second = crate::tests::sample_document("tasks", "Second");
        let table_id = TableId::new();
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(1),
                Timestamp(100),
                vec![WriteOp {
                    table: first.table.clone(),
                    table_id: table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: first.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(first.clone()),
                }],
                None,
            )
            .expect("first durable record should build"),
            TenantEventRecord::new(
                SequenceNumber(2),
                Timestamp(200),
                vec![WriteOp {
                    table: second.table.clone(),
                    table_id: table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: second.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(second.clone()),
                }],
                None,
            )
            .expect("second durable record should build"),
        ];

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(0),
            }
        );

        assert_eq!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .as_ref(),
            None
        );

        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should apply pending durable records and refresh the cache");
        assert_eq!(
            progress,
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(2),
            }
        );
        let freshness = opened
            .store
            .replica_freshness_stats()
            .expect("freshness stats should retain recovered progress");
        assert_eq!(freshness.required_sequence, progress.durable_head);
        assert_eq!(
            opened
                .store
                .get(&second.table, &second.id)
                .expect("second lookup should succeed")
                .as_ref(),
            Some(&second)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_tenant_event_journal_replays_mixed_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("tenant-event-mixed").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_tenant_event").expect("table name should build");
        let table_id = TableId::new();
        let schema = TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            }],
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            }],
            access_policy: None,
        };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("title".to_string(), serde_json::json!("evented")),
                ("rank".to_string(), serde_json::json!(7)),
            ]),
        );
        let records = vec![
            TenantEventRecord::from_events(
                SequenceNumber(1),
                Timestamp(100),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::StageHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                    },
                }],
            )
            .expect("stage-hidden event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(2),
                Timestamp(200),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::ActivateHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        replaced_table_id: None,
                    },
                }],
            )
            .expect("activate-hidden event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(3),
                Timestamp(300),
                vec![
                    TenantEventKind::SchemaChange {
                        change: Box::new(SchemaChangeEvent::SetTable {
                            table: table.clone(),
                            table_id: table_id.clone(),
                            previous: None,
                            current: schema.clone(),
                        }),
                    },
                    TenantEventKind::IndexLifecycle {
                        index: nimbus_core::IndexLifecycleEvent {
                            table: table.clone(),
                            table_id: table_id.clone(),
                            index_id: schema.indexes[0].id.clone(),
                            state: schema.indexes[0].state,
                            definition: schema.indexes[0].clone(),
                        },
                    },
                ],
            )
            .expect("schema event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(4),
                Timestamp(400),
                vec![TenantEventKind::DocumentWrite {
                    writes: vec![WriteOp {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        op_type: WriteOpType::Insert,
                        doc_id: document.id.clone(),
                        resource_path_binding: None,
                        trigger_write_origin: None,
                        previous: None,
                        current: Some(document.clone()),
                    }],
                }],
            )
            .expect("document event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(5),
                Timestamp(500),
                vec![TenantEventKind::TriggerDelivery {
                    cursor: TriggerDeliveryCursor::new(SequenceNumber(4)),
                }],
            )
            .expect("trigger cursor event should build"),
        ];

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("mixed tenant event replay should apply");
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("mixed replay should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after replay")
            .expect("tenant should still exist");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(table_id)
        );
        let loaded_schema = opened.store.load_schema().expect("schema should load");
        assert_eq!(loaded_schema.get_table(&table), Some(&schema));
        assert_eq!(
            opened
                .store
                .get(&table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );
        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("trigger cursor should load"),
            TriggerDeliveryCursor::new(SequenceNumber(4))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_durable_replay_retires_recreated_table_identity() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("durable-recreate").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_durable_recreate").expect("table name should build");
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(1),
                Timestamp(100),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: old_table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: old_document.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(old_document.clone()),
                }],
                None,
            )
            .expect("old durable record should build"),
            TenantEventRecord::new(
                SequenceNumber(2),
                Timestamp(200),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: new_table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: new_document.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(new_document.clone()),
                }],
                None,
            )
            .expect("new durable record should build"),
        ];

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("durable replay should infer table recreation");
        provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("replayed table identity should refresh to the local cache");
        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant should reopen after replay")
            .expect("tenant should still exist");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(new_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("old logical lookup should succeed")
                .is_none()
        );
        assert_eq!(
            opened
                .store
                .get(&table, &new_document.id)
                .expect("new logical lookup should succeed")
                .as_ref(),
            Some(&new_document)
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .scan_table_matching_cancellable(&table, &mut check_cancel, |_| Ok(true))
                .expect("active table scan should succeed"),
            vec![new_document]
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after replay");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == new_table_id
                && diagnostic.state == TableState::Active
                && diagnostic.document_count == Some(1)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));
    })
    .await;
}
