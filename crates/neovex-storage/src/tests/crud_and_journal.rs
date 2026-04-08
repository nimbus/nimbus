use super::*;

#[test]
fn key_helpers_create_prefix_scannable_ranges() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let id = DocumentId::new();
    let key = document_key(&table, &id);
    let prefix = table_prefix(&table);
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
            op_type: WriteOpType::Update,
            doc_id: before.id,
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

    let record = DurableMutationRecord::new(
        SequenceNumber(3),
        Timestamp(12),
        vec![WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: before.id,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    let mut document_dependency = DependencySet::default();
    document_dependency.record_document(&table, before.id);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &document_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut table_dependency = DependencySet::default();
    table_dependency.record_table(&table);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &table_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut index_range_dependency = DependencySet::default();
    index_range_dependency.record_index_range(IndexRangeDependency {
        table: table.clone(),
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
    unrelated.record_table(&TableName::new("users").expect("table name should be valid"));
    assert!(!durable_record_intersects_dependency_set(
        &record,
        &unrelated,
        &[],
        |_, _| Ok(None)
    ));
}
