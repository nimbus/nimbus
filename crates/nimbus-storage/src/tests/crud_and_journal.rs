use super::*;
use nimbus_core::{PinnedServingSnapshot, ReadVisibility, RequiredSequence, TableState};

#[test]
fn key_helpers_create_prefix_scannable_ranges() {
    let table_id = nimbus_core::TableId::new();
    let id = DocumentId::new();
    let key = document_key(&table_id, &id);
    let prefix = table_prefix(&table_id);
    let end = prefix_end(&prefix).expect("prefix end should exist");

    assert!(key.starts_with(&prefix));
    assert!(key.as_slice() < end.as_slice());
}

#[test]
fn insert_then_get_roundtrip() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Hello");

    let commit = store.insert(&document).expect("insert should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(fetched.fields.get("title"), Some(&json!("Hello")));
}

#[test]
fn redb_table_identity_diagnostics_are_read_only_and_count_documents() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("diagnostic_tasks", "first");
    let second = sample_document("diagnostic_tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let diagnostics = store
        .table_identity_diagnostics()
        .expect("diagnostics should load");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].table_name, first.table);
    assert_eq!(diagnostics[0].state, nimbus_core::TableState::Active);
    assert_eq!(
        diagnostics[0].backend_layout,
        crate::TableBackendLayout::RedbKeyspaceByTableId
    );
    assert_eq!(diagnostics[0].document_count, Some(2));
    assert_eq!(
        diagnostics[0].summary_status,
        crate::TableSummaryStatus::ExactDocumentCount
    );
}

#[test]
fn read_visibility_waits_for_required_sequence() {
    let latest = SequenceNumber(9);
    let required =
        ReadVisibility::AtLeast(RequiredSequence::new(SequenceNumber(7))).required_sequence(latest);
    assert_eq!(required.sequence(), SequenceNumber(7));

    let pinned = ReadVisibility::Pinned(PinnedServingSnapshot::new(SequenceNumber(5)))
        .required_sequence(latest);
    assert_eq!(pinned.sequence(), SequenceNumber(5));
    assert_eq!(
        ReadVisibility::Latest.required_sequence(latest).sequence(),
        latest
    );
}

#[test]
fn uses_shared_table_lifecycle_transition() {
    let next = crate::table_identity::apply_table_lifecycle_transition(
        None,
        crate::table_identity::TableLifecycleTransition::StageHidden,
    )
    .expect("shared lifecycle transition should stage hidden state");
    assert_eq!(next, Some(TableState::Hidden));
}

#[test]
fn native_documents_and_indexes_are_physically_keyed_by_table_id() {
    use redb::ReadableTable;

    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks_physical_identity", "Hello");
    let schema = TableSchema {
        table: document.table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };

    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let commit = store
        .insert_with_indexes(&document, &schema.indexes)
        .expect("insert should succeed");

    let read_txn = store.db.begin_read().expect("read transaction");
    let table_id = {
        let catalog = read_txn
            .open_table(crate::store::TABLE_CATALOG)
            .expect("table catalog should exist");
        let value = catalog
            .get("default\0tasks_physical_identity")
            .expect("catalog read should succeed")
            .expect("catalog row should exist")
            .value()
            .to_string();
        serde_json::from_str::<serde_json::Value>(&value)
            .expect("catalog value should decode")
            .get("table_id")
            .and_then(|value| value.as_str())
            .expect("catalog value should include table_id")
            .to_string()
    };
    let table_id = nimbus_core::TableId::try_from(table_id).expect("table id should parse");
    assert_eq!(
        commit.writes[0].table_id, table_id,
        "commit records should carry the durable table identity used by physical storage"
    );

    let documents = read_txn
        .open_table(crate::store::DOCUMENTS)
        .expect("documents table should exist");
    let table_id_key = document_key(&table_id, &document.id);
    assert!(
        documents
            .get(table_id_key.as_slice())
            .expect("table-id document lookup should succeed")
            .is_some(),
        "native document storage should be keyed by table_id"
    );

    let mut table_name_key = document.table.as_str().as_bytes().to_vec();
    table_name_key.push(0);
    table_name_key.extend_from_slice(document.id.as_str().as_bytes());
    assert!(
        documents
            .get(table_name_key.as_slice())
            .expect("old table-name document lookup should succeed")
            .is_none(),
        "native document storage must not keep using table_name as its physical key"
    );
    drop(documents);

    let indexes = read_txn
        .open_table(crate::store::INDEXES)
        .expect("indexes table should exist");
    let table_id_index_prefix = crate::index::table_index_prefix(&table_id);
    let mut table_name_index_prefix = document.table.as_str().as_bytes().to_vec();
    table_name_index_prefix.push(0);
    let mut saw_table_id_index = false;
    let mut saw_table_name_index = false;
    for item in indexes.iter().expect("index iteration should start") {
        let (key, _) = item.expect("index row should decode");
        saw_table_id_index |= key.value().starts_with(table_id_index_prefix.as_slice());
        saw_table_name_index |= key.value().starts_with(table_name_index_prefix.as_slice());
    }
    assert!(
        saw_table_id_index,
        "index entries should use table_id prefixes"
    );
    assert!(
        !saw_table_name_index,
        "index entries must not use table_name prefixes"
    );

    let fetched = store
        .get(&document.table, &document.id)
        .expect("logical get should succeed")
        .expect("logical document should exist");
    assert_eq!(fetched, document);
    assert_eq!(
        store
            .index_scan_eq(&document.table, "by_title", &json!("Hello"))
            .expect("logical index scan should succeed"),
        vec![document]
    );
}

#[test]
fn native_writes_reject_deleting_table_identity() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_deleting").expect("table should parse");
    let table_id = TableId::new();
    let catalog_key = format!("default\0{}", table.as_str());
    let catalog_value = serde_json::json!({
        "table_id": table_id.as_str(),
        "state": "deleting"
    })
    .to_string();
    let write_txn = store.db.begin_write().expect("write transaction");
    {
        let mut catalog = write_txn
            .open_table(crate::store::TABLE_CATALOG)
            .expect("table catalog should open");
        catalog
            .insert(catalog_key.as_str(), catalog_value.as_str())
            .expect("deleting catalog state should insert");
    }
    write_txn.commit().expect("catalog state should commit");

    let document = Document::new(
        table,
        serde_json::Map::from_iter([("title".to_string(), json!("blocked"))]),
    );
    let error = store
        .insert(&document)
        .expect_err("writes to deleting tables should fail");

    assert!(
        error.to_string().contains("deleting lifecycle state"),
        "deleting table rejection should be explicit: {error:?}"
    );
}

#[test]
fn native_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data() {
    use redb::ReadableTable;

    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_lifecycle").expect("table should parse");
    let schema = TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let old_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let old_commit = store
        .insert_with_indexes(&old_document, &schema.indexes)
        .expect("old document should insert");
    let old_table_id = old_commit.writes[0].table_id.clone();
    let replacement_table_id = TableId::new();

    store
        .stage_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden replacement identity should stage");
    let staged = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(
        staged.iter().any(|identity| {
            identity.namespace
                == crate::table_identity::hidden_table_namespace(&replacement_table_id)
                && identity.table == table
                && identity.table_id == replacement_table_id
                && identity.state == nimbus_core::TableState::Hidden
        }),
        "hidden replacement identity should be visible in catalog snapshots: {staged:?}"
    );

    let retired = store
        .activate_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden identity should activate");
    assert_eq!(
        retired.as_ref(),
        Some(&old_table_id),
        "activating a hidden replacement should retire the previous active identity"
    );
    assert_eq!(
        store.table_id(&table).expect("table id should resolve"),
        Some(replacement_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should resolve against replacement identity")
            .is_none(),
        "old rows must not be reachable through the recreated logical table name"
    );

    let new_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let new_commit = store
        .insert_with_indexes(&new_document, &schema.indexes)
        .expect("new document should insert under replacement identity");
    assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

    {
        let read_txn = store.db.begin_read().expect("read transaction");
        let documents = read_txn
            .open_table(crate::store::DOCUMENTS)
            .expect("documents table should open");
        assert!(
            documents
                .get(document_key(&old_table_id, &old_document.id).as_slice())
                .expect("old physical document lookup should succeed")
                .is_some(),
            "retired table data should remain until hard delete"
        );
    }

    assert!(
        store
            .hard_delete_table_identity(&old_table_id)
            .expect("hard delete should succeed"),
        "hard delete should report that it removed the retiring table"
    );

    let read_txn = store.db.begin_read().expect("read transaction");
    let documents = read_txn
        .open_table(crate::store::DOCUMENTS)
        .expect("documents table should open");
    assert!(
        documents
            .get(document_key(&old_table_id, &old_document.id).as_slice())
            .expect("old physical document lookup should succeed")
            .is_none(),
        "hard delete should remove retired table documents"
    );
    drop(documents);

    let indexes = read_txn
        .open_table(crate::store::INDEXES)
        .expect("indexes table should open");
    let old_index_prefix = crate::index::table_index_prefix(&old_table_id);
    for item in indexes.iter().expect("index iteration should start") {
        let (key, _) = item.expect("index row should decode");
        assert!(
            !key.value().starts_with(old_index_prefix.as_slice()),
            "hard delete should remove retired table index rows"
        );
    }

    let identities = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(
        !identities
            .iter()
            .any(|identity| identity.table_id == old_table_id),
        "hard delete should remove the retired catalog identity: {identities:?}"
    );
    assert_eq!(
        store
            .index_scan_eq(&table, "by_title", &json!("new"))
            .expect("active replacement index scan should succeed"),
        vec![new_document]
    );
}

#[test]
fn redb_durable_replay_retires_recreated_table_identity() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks_replayed_lifecycle").expect("table should parse");
    let old_table_id = TableId::new();
    let new_table_id = TableId::new();
    let old_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let new_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let records = vec![
        DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(1),
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
        DurableMutationRecord::new(
            SequenceNumber(2),
            Timestamp(2),
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

    store
        .apply_durable_records_batch(&records)
        .expect("durable replay should infer table recreation");

    assert_eq!(
        store
            .table_id(&table)
            .expect("active table id should resolve"),
        Some(new_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should use active replacement")
            .is_none(),
        "old-generation rows must not be visible through the recreated table name"
    );
    assert_eq!(
        store
            .scan_table(&table)
            .expect("scan should use active replacement"),
        vec![new_document]
    );
    let identities = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::DEFAULT_TABLE_NAMESPACE
            && identity.table == table
            && identity.table_id == new_table_id
            && identity.state == nimbus_core::TableState::Active
    }));
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::deleting_table_namespace(&old_table_id)
            && identity.table == table
            && identity.table_id == old_table_id
            && identity.state == nimbus_core::TableState::Deleting
    }));
}

#[test]
fn seeded_fault_injector_reproduces_the_same_schedule_for_the_same_seed() {
    let left = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));
    let right = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));

    let left_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| left.check(point).is_err())
    .collect::<Vec<_>>();
    let right_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| right.check(point).is_err())
    .collect::<Vec<_>>();

    assert_eq!(left_results, right_results);
}

#[test]
fn injected_fault_before_visibility_rolls_back_the_write_deterministically() {
    let harness = DeterministicHarness::scripted(
        "storage-before-visibility",
        10,
        Timestamp(10_000),
        [FaultOccurrence {
            point: FaultPoint::StorageCommitBeforeVisibility,
            visit: 1,
        }],
    );
    let store =
        TenantStore::create_in_memory_with_simulation(harness.clock(), harness.fault_injector())
            .expect("store should open with simulation seams");
    let document = sample_document("tasks", "Hello");

    let error = store
        .insert(&document)
        .expect_err("first insert should fail before visibility");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("storage_commit_before_visibility"))
    );
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed after injected failure")
            .is_none()
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should remain unchanged"),
        SequenceNumber(0)
    );

    let commit = store
        .insert(&document)
        .expect("second insert should commit");
    assert_eq!(commit.timestamp, Timestamp(10_000));
    assert_eq!(harness.describe(), "storage-before-visibility (seed 10)");
}

#[test]
fn scheduled_execution_marker_deduplicates_insert_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Hello once");

    let first = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("first insert should succeed");
    let second = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("second insert should succeed");

    assert!(first.is_some(), "first scheduled execution should commit");
    assert!(
        second.is_none(),
        "second scheduled execution should be skipped"
    );
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(1)
    );
    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Hello once")));
}

#[test]
fn commit_log_sequences_increment() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");

    let first_commit = store.insert(&first).expect("first insert should succeed");
    let second_commit = store.insert(&second).expect("second insert should succeed");
    let entries = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log read should succeed");

    assert_eq!(first_commit.sequence, SequenceNumber(1));
    assert_eq!(second_commit.sequence, SequenceNumber(2));
    assert_eq!(entries.len(), 2);
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(2)
    );
}

#[test]
fn durable_journal_serialization_preserves_payload_and_metadata() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
    );
    let mut after = before.clone();
    after.fields.insert("title".to_string(), json!("After"));

    let record = DurableMutationRecord::new(
        SequenceNumber(7),
        Timestamp(42),
        vec![WriteOp {
            table: table.clone(),
            table_id: TableId::new(),
            op_type: WriteOpType::Update,
            doc_id: before.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        Some("scheduled:job-7".to_string()),
    )
    .expect("durable record should build");

    let encoded =
        crate::commit_log::serialize_durable_record(&record).expect("record should serialize");
    let decoded =
        crate::commit_log::deserialize_durable_record(&encoded).expect("record should deserialize");

    assert_eq!(decoded, record);
    assert_eq!(decoded.writes[0].table, table);
    assert_eq!(decoded.writes[0].doc_id, before.id);
    assert_eq!(
        decoded.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("After"))
    );
    assert_eq!(
        decoded.scheduled_execution_id.as_deref(),
        Some("scheduled:job-7")
    );
}

#[test]
fn durable_journal_metadata_supports_dependency_intersection_checks() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("rank".to_string(), json!(3)),
            ("status".to_string(), json!("open")),
        ]),
    );
    let mut after = before.clone();
    after.fields.insert("rank".to_string(), json!(8));

    let table_id = TableId::new();
    let record = DurableMutationRecord::new(
        SequenceNumber(3),
        Timestamp(12),
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Update,
            doc_id: before.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    let mut document_dependency = DependencySet::default();
    document_dependency.record_document(&table, &table_id, before.id.clone());
    assert!(durable_record_intersects_dependency_set(
        &record,
        &document_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut table_dependency = DependencySet::default();
    table_dependency.record_table(&table, &table_id);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &table_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut index_range_dependency = DependencySet::default();
    index_range_dependency.record_index_range(IndexRangeDependency {
        table: table.clone(),
        table_id: table_id.clone(),
        index_id: nimbus_core::IndexId::new(),
        index_name: "by_rank".to_string(),
        field: "rank".to_string(),
        start: Some(json!(5)),
        end: Some(json!(10)),
        start_inclusive: true,
        end_inclusive: true,
    });
    assert!(durable_record_intersects_dependency_set(
        &record,
        &index_range_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut unrelated = DependencySet::default();
    unrelated.record_table(
        &TableName::new("users").expect("table name should be valid"),
        &TableId::new(),
    );
    assert!(!durable_record_intersects_dependency_set(
        &record,
        &unrelated,
        &[],
        |_, _| Ok(None)
    ));
}
