use super::support::*;
use crate::tests::{
    exercise_durable_update_guard_is_corruption, exercise_pending_prefix_blocks_generic_zero_write,
    exercise_ppsc_different_content_applied_sequence_reuse_rejection,
    exercise_ppsc_identical_applied_sequence_replay,
};
use nimbus_core::{Error, Result};

struct CancelBeforeCommitVisibility;

impl FaultInjector for CancelBeforeCommitVisibility {
    fn check(&self, point: FaultPoint) -> Result<()> {
        if point == FaultPoint::StorageCommitBeforeVisibility {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

fn mysql_pipeline_barriers(count: u64) -> Vec<TenantEventRecord> {
    (1..=count)
        .map(|sequence| {
            TenantEventRecord::barrier(
                SequenceNumber(sequence),
                Timestamp(sequence.saturating_mul(100)),
                format!("mysql-pipeline-{sequence}"),
            )
            .expect("barrier record should build")
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_batch_journal_insert_uses_one_provider_statement() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("batch-journal-statement").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_batch_journal_insert_uses_one_provider_statement(
            opened.store.as_ref(),
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_packet_bounded_journal_chunks_commit_atomically() {
    with_test_provider(|_cleanup_provider, mut config| async move {
        assert!(
            !config.connection_string.contains("max_allowed_packet="),
            "fixture URL should not preconfigure a client packet ceiling"
        );
        let separator = if config.connection_string.contains('?') {
            '&'
        } else {
            '?'
        };
        config.connection_string = format!(
            "{}{separator}max_allowed_packet=1024",
            config.connection_string
        );
        let provider = MySqlProvider::connect(config)
            .await
            .expect("provider should connect with a 1 KiB client packet ceiling");
        let tenant = TenantId::new("packet-bounded-journal").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let records = (1..=4)
            .map(|sequence| {
                TenantEventRecord::barrier(
                    SequenceNumber(sequence),
                    Timestamp(sequence.saturating_mul(100)),
                    "x".repeat(400),
                )
                .expect("barrier record should build")
            })
            .collect::<Vec<_>>();

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("packet-bounded batch append should succeed");

        let diagnostic = opened.store.write_pipeline_diagnostic();
        assert_eq!(diagnostic.batch_attempt_count, 1);
        assert_eq!(diagnostic.journal_record_count, 4);
        assert!(diagnostic.journal_statement_count > 1);
        assert_eq!(
            diagnostic.provider_operation_count,
            diagnostic.journal_statement_count
        );
        assert_eq!(diagnostic.max_observed_in_flight, 1);
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(4),
                applied_head: SequenceNumber(0),
            }
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_pipeline_lease_cas_precedes_all_statements() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("pipeline-lease-first").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let lease = opened
            .store
            .acquire_committer_lease("lease-owner", std::time::Duration::from_secs(30))
            .expect("lease should be acquired");
        let records = mysql_pipeline_barriers(2);

        let error = opened
            .store
            .fenced_append_and_apply_durable_records_batch(
                &lease.owner_id,
                lease.epoch.saturating_add(1),
                SequenceNumber(0),
                &records,
            )
            .expect_err("wrong epoch must fence before pipeline work");
        assert!(matches!(error, crate::CommitterLeaseError::Fenced { .. }));
        let diagnostic = opened.store.write_pipeline_diagnostic();
        assert_eq!(diagnostic.batch_attempt_count, 0);
        assert_eq!(diagnostic.journal_statement_count, 0);
        assert_eq!(diagnostic.provider_operation_count, 0);
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(0),
                applied_head: SequenceNumber(0),
            }
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_pre_admission_cancellation_is_not_a_pipeline_failure() {
    with_test_provider(|provider, _config| async move {
        let tenant =
            TenantId::new("pipeline-pre-admission-cancel").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let lease = opened
            .store
            .acquire_committer_lease(
                "pre-admission-cancel-owner",
                std::time::Duration::from_secs(30),
            )
            .expect("lease should be acquired");
        let records = mysql_pipeline_barriers(1);

        let error = opened
            .store
            .fenced_append_and_apply_durable_records_batch_cancellable(
                &lease.owner_id,
                lease.epoch,
                SequenceNumber(0),
                &records,
                || Err(Error::Cancelled),
            )
            .expect_err("pre-admission cancellation should abort the write");
        assert!(matches!(
            error,
            crate::CommitterLeaseError::Storage(Error::Cancelled)
        ));
        let diagnostic = opened.store.write_pipeline_diagnostic();
        assert_eq!(diagnostic.batch_attempt_count, 0);
        assert_eq!(diagnostic.journal_statement_count, 0);
        assert_eq!(diagnostic.provider_operation_count, 0);
        assert_eq!(diagnostic.cancellation_count, 0);
        assert_eq!(diagnostic.error_count, 0);
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(0),
                applied_head: SequenceNumber(0),
            }
        );
        assert_eq!(
            opened
                .store
                .read_committer_lease()
                .expect("lease should read")
                .expect("lease should exist")
                .durable_sequence,
            SequenceNumber(0)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_sql_pipeline_cancellation_rolls_back() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("pipeline-cancel").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_sql_pipeline_cancellation_rolls_back(
            opened.store.as_ref(),
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_sql_pipeline_post_runner_cancellation_is_counted_once() {
    with_test_provider_and_fault_injector(
        std::sync::Arc::new(CancelBeforeCommitVisibility),
        |provider, _config| async move {
            let tenant =
                TenantId::new("pipeline-post-runner-cancel").expect("tenant id should build");
            let opened = provider
                .create_opened_tenant(&tenant)
                .await
                .expect("tenant should create and open");
            let lease = opened
                .store
                .acquire_committer_lease(
                    "post-runner-cancel-owner",
                    std::time::Duration::from_secs(30),
                )
                .expect("lease should be acquired");
            let records = mysql_pipeline_barriers(1);

            let error = opened
                .store
                .fenced_append_and_apply_durable_records_batch(
                    &lease.owner_id,
                    lease.epoch,
                    SequenceNumber(0),
                    &records,
                )
                .expect_err("pre-visibility cancellation should abort after batch apply");
            assert!(matches!(
                error,
                crate::CommitterLeaseError::Storage(Error::Cancelled)
            ));
            let diagnostic = opened.store.write_pipeline_diagnostic();
            assert_eq!(diagnostic.batch_attempt_count, 1);
            assert_eq!(diagnostic.provider_operation_count, 1);
            assert_eq!(diagnostic.cancellation_count, 1);
            assert_eq!(diagnostic.error_count, 1);
            assert_eq!(
                opened
                    .store
                    .journal_progress()
                    .expect("progress should read"),
                crate::store::JournalProgress {
                    durable_head: SequenceNumber(0),
                    applied_head: SequenceNumber(0),
                }
            );
            assert_eq!(
                opened
                    .store
                    .read_committer_lease()
                    .expect("lease should read")
                    .expect("lease should exist")
                    .durable_sequence,
                SequenceNumber(0)
            );
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_ppsc_identical_replay_is_idempotent_for_all_write_shapes() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("duplicate-replay").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_ppsc_identical_applied_sequence_replay(
            opened.store.as_ref(),
            "mysql_duplicate_replay",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("duplicate-corruption").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_ppsc_different_content_applied_sequence_reuse_rejection(
            opened.store.as_ref(),
            "mysql_duplicate_corruption",
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_pending_prefix_blocks_generic_zero_write() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("pending-prefix").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        exercise_pending_prefix_blocks_generic_zero_write(
            opened.store.as_ref(),
            "mysql_pending_prefix",
            || {
                opened
                    .store
                    .set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(1)))
            },
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_durable_update_guard_reports_corruption() {
    with_test_provider(|provider, _config| async move {
        let missing = provider
            .create_opened_tenant(&TenantId::new("missing-preimage").expect("tenant id"))
            .await
            .expect("tenant should create and open");
        exercise_durable_update_guard_is_corruption(
            missing.store.as_ref(),
            "mysql_missing_preimage",
            false,
        );
        let mismatched = provider
            .create_opened_tenant(&TenantId::new("mismatched-preimage").expect("tenant id"))
            .await
            .expect("tenant should create and open");
        exercise_durable_update_guard_is_corruption(
            mismatched.store.as_ref(),
            "mysql_mismatched_preimage",
            true,
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_direct_writes_dedupe_and_journal_progress_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("writes").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_direct_writes_dedupe_and_journal_progress_round_trip(opened.store.as_ref());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_durable_journal_recovery_applies_pending_records() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_durable_journal_recovery_applies_pending_records(
            opened.store.as_ref(),
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_tenant_event_journal_replays_mixed_history() {
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
            indexes: vec![nimbus_core::IndexDefinition {
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
async fn mysql_durable_replay_retires_recreated_table_identity() {
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

#[tokio::test(flavor = "multi_thread")]
async fn mysql_materialized_position_matches_the_provider_independent_reference() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("position-parity").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        assert_eq!(
            crate::tests::contract_scenarios::exercise_materialized_position_is_provider_independent(
                opened.store.as_ref()
            ),
            crate::tests::contract_scenarios::reference_materialized_position(),
            "MySQL must reach the same materialized position as every other provider"
        );
    })
    .await;
}
