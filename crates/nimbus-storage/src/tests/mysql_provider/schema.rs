use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_index_reads_round_trip_after_schema_write() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("indexed-reads").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_index_reads_round_trip_after_schema_write(
            opened.store.as_ref(),
        );
    })
    .await;
}

fn tasks_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

fn task(table: &TableName, team: &str, status: Option<&str>, rank: u64) -> Document {
    let mut fields = serde_json::Map::from_iter([
        ("team".to_string(), serde_json::json!(team)),
        ("rank".to_string(), serde_json::json!(rank)),
    ]);
    if let Some(status) = status {
        fields.insert("status".to_string(), serde_json::json!(status));
    }
    Document::new(table.clone(), fields)
}

/// A schema write fills the `index_entries` keyspace for the documents that
/// already exist and a schema delete empties it. Neither touches the
/// `documents` layout: no generated column and no per-index InnoDB key.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_schema_write_populates_and_clears_index_entries_without_ddl() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("schema").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks").expect("table name should build");
        let table_schema = tasks_schema(&table);
        let complete = task(&table, "ops", Some("open"), 1);
        let partial = task(&table, "ops", None, 2);
        for document in [&complete, &partial] {
            opened
                .store
                .insert(document)
                .expect("document should insert before the schema exists");
        }

        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");
        let table_id = opened
            .store
            .table_id(&table)
            .expect("table id should load")
            .expect("table id should exist");
        let rows = index_entry_rows(&config.connection_string, opened.store.database_name()).await;
        assert_eq!(
            rows.iter()
                .map(|(row_table, row_index, row_document, _)| (
                    row_table.as_str(),
                    row_index.as_str(),
                    row_document.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![(
                table_id.as_str(),
                table_schema.indexes[0].id.as_str(),
                complete.id.to_string().as_str()
            )],
            "only the document with every indexed field carries a tuple: {rows:?}"
        );
        assert_eq!(
            document_index_counts(&config.connection_string, opened.store.database_name()).await,
            (0, 0),
            "a schema write must not add generated columns or keys to documents"
        );

        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_prefix_cancellable(
                    &table,
                    "by_team_status_rank",
                    &[serde_json::json!("ops")],
                    &mut check_cancel,
                )
                .expect("index scan should succeed"),
            vec![complete.clone()]
        );

        opened
            .store
            .delete_table_schema(&table)
            .expect("schema delete should succeed");
        assert!(
            index_entry_rows(&config.connection_string, opened.store.database_name())
                .await
                .is_empty(),
            "schema delete should purge the table's keyspace rows"
        );
        assert_eq!(
            document_index_counts(&config.connection_string, opened.store.database_name()).await,
            (0, 0)
        );
        assert_eq!(
            opened.store.load_schema().expect("schema should load"),
            Schema::default()
        );
        assert_eq!(
            opened
                .store
                .get(&table, &complete.id)
                .expect("document should still read")
                .as_ref(),
            Some(&complete)
        );
    })
    .await;
}

/// A schema change backfills the indexes it adds from the documents that
/// already exist, keeps the entries of an index it leaves unchanged, and
/// purges the entries of an index it removes.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_schema_apply_backfills_indexes_added_after_documents_exist() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("schema-backfill").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks").expect("table name should build");
        let by_team = nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_team".to_string(),
            fields: vec!["team".to_string()],
        };
        let by_rank = nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        };
        let by_status = nimbus_core::IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        };
        let mut first = tasks_schema(&table);
        first.indexes = vec![by_team.clone(), by_rank.clone()];
        opened
            .store
            .replace_table_schema(&first)
            .expect("first schema should apply");

        let documents = [
            task(&table, "ops", Some("open"), 3),
            task(&table, "ops", None, 1),
            task(&table, "dev", Some("done"), 2),
        ];
        for document in &documents {
            opened
                .store
                .insert(document)
                .expect("document should insert under the first schema");
        }
        let rows_by_index = |rows: &[IndexEntryRow], index: &nimbus_core::IndexDefinition| {
            rows.iter()
                .filter(|(_, row_index, _, _)| row_index == index.id.as_str())
                .map(|(_, _, row_document, tuple)| (row_document.clone(), tuple.clone()))
                .collect::<Vec<_>>()
        };
        let before =
            index_entry_rows(&config.connection_string, opened.store.database_name()).await;
        assert_eq!(rows_by_index(&before, &by_team).len(), 3);
        assert_eq!(rows_by_index(&before, &by_rank).len(), 3);
        let rank_rows_before = rows_by_index(&before, &by_rank);

        // Add by_status, remove by_team, keep by_rank as it is.
        let mut second = first.clone();
        second.indexes = vec![by_rank.clone(), by_status.clone()];
        opened
            .store
            .replace_table_schema(&second)
            .expect("second schema should apply");
        let applied = opened
            .store
            .load_schema()
            .expect("schema should load")
            .get_table(&table)
            .cloned()
            .expect("table schema should exist");
        assert_eq!(
            applied
                .indexes
                .iter()
                .map(|index| index.id.clone())
                .collect::<Vec<_>>(),
            vec![by_rank.id.clone(), by_status.id.clone()],
            "an unchanged index keeps its id across the apply"
        );

        let after = index_entry_rows(&config.connection_string, opened.store.database_name()).await;
        assert!(
            rows_by_index(&after, &by_team).is_empty(),
            "a removed index is purged: {after:?}"
        );
        assert_eq!(
            rows_by_index(&after, &by_rank),
            rank_rows_before,
            "an unchanged index keeps its entries"
        );
        assert_eq!(
            rows_by_index(&after, &by_status)
                .into_iter()
                .map(|(document_id, _)| document_id)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                documents[0].id.to_string(),
                documents[2].id.to_string(),
            ]),
            "an added index is backfilled only for documents that carry the field"
        );

        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_prefix_cancellable(
                    &table,
                    "by_status",
                    &[serde_json::json!("done")],
                    &mut check_cancel,
                )
                .expect("backfilled index should read"),
            vec![documents[2].clone()]
        );
        let low = serde_json::json!(2);
        assert_eq!(
            opened
                .store
                .index_scan_range_cancellable(
                    &table,
                    "by_rank",
                    std::ops::Bound::Included(&low),
                    std::ops::Bound::Unbounded,
                    &mut check_cancel,
                )
                .expect("kept index should read")
                .into_iter()
                .map(|document| document.id)
                .collect::<Vec<_>>(),
            vec![documents[2].id.clone(), documents[0].id.clone()],
            "range reads come back in encoded tuple order"
        );
    })
    .await;
}

/// Fires once at `StorageCommitBeforeVisibility` after it is armed.
struct ArmedCommitBeforeVisibilityFault {
    armed: std::sync::atomic::AtomicBool,
    fired: std::sync::atomic::AtomicBool,
}

impl ArmedCommitBeforeVisibilityFault {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            armed: std::sync::atomic::AtomicBool::new(false),
            fired: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::Acquire)
    }
}

impl FaultInjector for ArmedCommitBeforeVisibilityFault {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::StorageCommitBeforeVisibility
            && self.armed.load(Ordering::Acquire)
            && !self.fired.swap(true, Ordering::AcqRel)
        {
            return Err(nimbus_core::Error::Internal(
                "injected mysql commit-before-visibility fault".to_string(),
            ));
        }
        Ok(())
    }
}

/// Finding F2 of the index keyspace plan: a schema apply that fails at the
/// commit boundary leaves nothing behind. The apply is DML only, so the
/// schema row, the keyspace rows, the journal entry, and the committer
/// lease advance all roll back together instead of committing implicitly
/// under DDL. A retry of the same apply then lands whole.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_schema_apply_fault_before_commit_leaves_no_partial_state() {
    let fault = ArmedCommitBeforeVisibilityFault::new();
    let injector = fault.clone();
    with_test_provider_and_fault_injector(injector, |provider, config| async move {
        let tenant = TenantId::new("schema-fault").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks").expect("table name should build");
        let table_schema = tasks_schema(&table);
        let document = task(&table, "ops", Some("open"), 1);
        opened
            .store
            .insert(&document)
            .expect("document should insert before the schema exists");
        let lease = opened
            .store
            .acquire_committer_lease("schema-fault-owner", std::time::Duration::from_secs(30))
            .expect("lease should be acquired");
        let progress_before = opened
            .store
            .journal_progress()
            .expect("progress should read");

        fault.arm();
        let error = opened
            .store
            .fenced_replace_table_schema(
                &lease.owner_id,
                lease.epoch,
                progress_before.durable_head,
                &table_schema,
            )
            .expect_err("the armed fault should abort the schema apply");
        assert!(fault.fired(), "the fault point should have fired");
        assert!(
            matches!(
                &error,
                crate::CommitterLeaseError::Storage(nimbus_core::Error::Internal(message))
                    if message.contains("injected mysql commit-before-visibility fault")
            ),
            "unexpected error: {error:?}"
        );

        assert_eq!(
            opened.store.load_schema().expect("schema should load"),
            Schema::default(),
            "the schema row must roll back with the transaction"
        );
        assert!(
            index_entry_rows(&config.connection_string, opened.store.database_name())
                .await
                .is_empty(),
            "the keyspace rows must roll back with the transaction"
        );
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("progress should read"),
            progress_before,
            "the journal must not advance"
        );
        assert_eq!(
            opened
                .store
                .read_committer_lease()
                .expect("lease should read")
                .expect("lease should exist")
                .durable_sequence,
            progress_before.durable_head,
            "the lease durable sequence must not run ahead of the storage head"
        );

        opened
            .store
            .fenced_replace_table_schema(
                &lease.owner_id,
                lease.epoch,
                progress_before.durable_head,
                &table_schema,
            )
            .expect("the retried schema apply should land");
        let progress_after = opened
            .store
            .journal_progress()
            .expect("progress should read");
        assert_eq!(
            progress_after.durable_head,
            SequenceNumber(progress_before.durable_head.0 + 1)
        );
        assert_eq!(
            opened
                .store
                .read_committer_lease()
                .expect("lease should read")
                .expect("lease should exist")
                .durable_sequence,
            progress_after.durable_head
        );
        let rows = index_entry_rows(&config.connection_string, opened.store.database_name()).await;
        assert_eq!(
            rows.len(),
            1,
            "the retry backfills the existing document: {rows:?}"
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_prefix_cancellable(
                    &table,
                    "by_team_status_rank",
                    &[serde_json::json!("ops")],
                    &mut check_cancel,
                )
                .expect("index scan should succeed after the retry"),
            vec![document.clone()]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("table-lifecycle").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_lifecycle").expect("table name should build");
        let schema = TableSchema {
            table: table.clone(),
            fields: Vec::new(),
            indexes: vec![nimbus_core::IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_title".to_string(),
                fields: vec!["title".to_string()],
            }],
            access_policy: None,
        };
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema write should succeed");

        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let old_commit = opened
            .store
            .insert(&old_document)
            .expect("old document should insert");
        let old_table_id = old_commit.writes[0].table_id.clone();
        let replacement_table_id = TableId::new();

        opened
            .store
            .stage_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden replacement identity should stage");
        let staged = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after staging");
        assert!(staged.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == replacement_table_id
                && diagnostic.state == TableState::Hidden
                && diagnostic.backend_layout == crate::TableBackendLayout::SharedDocumentsByTableId
                && diagnostic.summary_status == crate::TableSummaryStatus::Unsupported
                && diagnostic.document_count.is_none()
        }));

        let retired = opened
            .store
            .activate_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden identity should activate");
        assert_eq!(retired.as_ref(), Some(&old_table_id));
        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(replacement_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("logical get should use active replacement")
                .is_none()
        );

        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let new_commit = opened
            .store
            .insert(&new_document)
            .expect("new document should insert under replacement identity");
        assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after activation");
        let active = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.table_name == table && diagnostic.table_id == replacement_table_id
            })
            .expect("active replacement diagnostic should exist");
        assert_eq!(active.state, TableState::Active);
        assert_eq!(active.document_count, Some(1));
        assert_eq!(
            active.summary_status,
            crate::TableSummaryStatus::ExactDocumentCount
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));

        assert!(
            opened
                .store
                .hard_delete_table_identity(&old_table_id)
                .expect("hard delete should succeed")
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after hard delete");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.table_id != old_table_id),
            "hard delete should remove retired catalog identity: {diagnostics:?}"
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_prefix_cancellable(
                    &table,
                    "by_title",
                    &[serde_json::json!("new")],
                    &mut check_cancel,
                )
                .expect("active replacement index scan should succeed"),
            vec![new_document]
        );
    })
    .await;
}

/// Regression for the InnoDB 64-key cap: a tenant schema with more than 64
/// maintained indexes must apply and serve index reads on MySQL.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_schema_with_more_than_sixty_four_indexes_applies_and_reads() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("many-indexes").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("wide").expect("table name should build");
        let index_count = 70usize;
        let fields = (0..index_count)
            .map(|position| format!("f{position}"))
            .collect::<Vec<_>>();
        let table_schema = TableSchema {
            table: table.clone(),
            fields: fields
                .iter()
                .map(|name| FieldSchema {
                    name: name.clone(),
                    field_type: FieldType::Number,
                    required: false,
                })
                .collect(),
            indexes: fields
                .iter()
                .map(|name| nimbus_core::IndexDefinition {
                    id: nimbus_core::IndexId::new(),
                    state: nimbus_core::IndexState::Enabled,
                    name: format!("by_{name}"),
                    fields: vec![name.clone()],
                })
                .collect(),
            access_policy: None,
        };
        let documents = (0..2u64)
            .map(|seed| {
                Document::new(
                    table.clone(),
                    serde_json::Map::from_iter(
                        fields
                            .iter()
                            .map(|name| (name.clone(), serde_json::json!(seed))),
                    ),
                )
            })
            .collect::<Vec<_>>();
        for document in &documents {
            opened
                .store
                .insert(document)
                .expect("document should insert before the schema exists");
        }

        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("a schema with more than 64 indexes should apply on MySQL");

        let mut check_cancel = || Ok(());
        for index_name in [
            format!("by_{}", fields[0]),
            format!("by_{}", fields[index_count - 1]),
        ] {
            let found = opened
                .store
                .index_scan_prefix_cancellable(
                    &table,
                    &index_name,
                    &[serde_json::json!(1)],
                    &mut check_cancel,
                )
                .expect("index scan should succeed");
            assert_eq!(found, vec![documents[1].clone()], "{index_name}");
        }
    })
    .await;
}
