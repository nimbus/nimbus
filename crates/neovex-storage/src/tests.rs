use std::num::NonZeroU64;
use std::sync::Arc;

use neovex_core::{
    CronJob, CronSchedule, Document, DocumentId, Error, FieldSchema, FieldType, IndexDefinition,
    Mutation, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, SequenceNumber, TableName,
    TableSchema, Timestamp,
};
use serde_json::json;
use tempfile::tempdir;
use time::{Date, Month, PrimitiveDateTime, Time};

use crate::index::encode_index_value;
use crate::keys::{document_key, prefix_end, table_prefix};
use crate::{
    FaultInjector, FaultOccurrence, FaultPoint, ManualClock, SeededFaultInjector, TenantStore,
    UsageStore,
};

fn sample_document(table: &str, title: &str) -> Document {
    Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        },
        created_at: Timestamp(1_000),
    }
}

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
    let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
    let faults = Arc::new(crate::ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::StorageCommitBeforeVisibility,
        visit: 1,
    }]));
    let store = TenantStore::create_in_memory_with_simulation(clock, faults)
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
fn scan_table_is_logically_isolated() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let task = sample_document("tasks", "Task");
    let user = sample_document("users", "User");

    store.insert(&task).expect("task insert should succeed");
    store.insert(&user).expect("user insert should succeed");

    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    let users = store
        .scan_table(&TableName::new("users").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(tasks.len(), 1);
    assert_eq!(users.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Task")));
    assert_eq!(users[0].fields.get("title"), Some(&json!("User")));
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
fn update_applies_patch_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Before");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .update(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(fetched.fields.get("title"), Some(&json!("After")));
}

#[test]
fn delete_removes_document_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Disposable");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert!(fetched.is_none());
}

#[test]
fn store_reopens_from_disk() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let document = sample_document("tasks", "Persisted");

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.insert(&document).expect("insert should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let documents = reopened
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[test]
fn store_get_nonexistent_document_returns_none() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let result = store
        .get(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect("get should succeed");

    assert!(result.is_none());
}

#[test]
fn store_scan_empty_table_returns_empty_vec() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let documents = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert!(documents.is_empty());
}

#[test]
fn store_latest_sequence_on_fresh_store_returns_zero() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should succeed"),
        SequenceNumber(0)
    );
}

#[test]
fn store_update_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .update(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("update should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn store_delete_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .delete(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect_err("delete should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn schema_roundtrip_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table_schema = TableSchema {
        table: TableName::new("users").expect("table name should be valid"),
        fields: vec![
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: Vec::new(),
        access_policy: None,
    };

    store
        .save_table_schema(&table_schema)
        .expect("schema should save");
    let schema = store.load_schema().expect("schema should load");

    assert_eq!(schema.get_table(&table_schema.table), Some(&table_schema));

    store
        .delete_table_schema_entry(&table_schema.table)
        .expect("schema entry should delete");
    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table_schema.table).is_none());
}

#[test]
fn replace_table_schema_rebuilds_indexes_and_persists_schema() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    for email in ["a@test.com", "b@test.com", "a@test.com"] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            field: "email".to_string(),
        }],
        access_policy: None,
    };

    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert_eq!(schema.get_table(&table), Some(&table_schema));

    let docs = store
        .index_scan_eq(&table, "by_email", &json!("a@test.com"))
        .expect("index scan should succeed");
    assert_eq!(docs.len(), 2);
}

#[test]
fn delete_table_schema_clears_schema_and_indexes() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store.insert(&document).expect("insert should succeed");

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            field: "email".to_string(),
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    store
        .delete_table_schema(&table)
        .expect("schema deletion should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table).is_none());
    let docs = store
        .index_scan_eq(&table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn update_with_indexes_validated_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes_validated(
            &table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
            |_existing, updated| {
                assert_eq!(updated.fields.get("email"), Some(&json!("new@test.com")));
                Ok(())
            },
        )
        .expect("validated update should succeed");

    let old_docs = store
        .index_scan_eq(&table, "by_email", &json!("old@test.com"))
        .expect("old index scan should succeed");
    let new_docs = store
        .index_scan_eq(&table, "by_email", &json!("new@test.com"))
        .expect("new index scan should succeed");

    assert!(old_docs.is_empty());
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_key_encoding_preserves_number_sort_order() {
    let mut encoded = [
        encode_index_value(&json!(-1.5)).expect("value should encode"),
        encode_index_value(&json!(0)).expect("value should encode"),
        encode_index_value(&json!(1)).expect("value should encode"),
        encode_index_value(&json!(100)).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!(-1.5)).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!(0)).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!(1)).expect("value should encode")
    );
    assert_eq!(
        encoded[3],
        encode_index_value(&json!(100)).expect("value should encode")
    );
}

#[test]
fn index_key_encoding_preserves_string_sort_order() {
    let mut encoded = [
        encode_index_value(&json!("charlie")).expect("value should encode"),
        encode_index_value(&json!("alpha")).expect("value should encode"),
        encode_index_value(&json!("bravo")).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!("alpha")).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!("bravo")).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!("charlie")).expect("value should encode")
    );
}

#[test]
fn index_insert_and_eq_scan() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    for email in ["a@test.com", "b@test.com", "c@test.com"] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let match_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("b@test.com"),
        )
        .expect("index scan should succeed");
    assert_eq!(match_docs.len(), 1);
    assert_eq!(
        match_docs[0].fields.get("email"),
        Some(&json!("b@test.com"))
    );

    let missing_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("missing@test.com"),
        )
        .expect("index scan should succeed");
    assert!(missing_docs.is_empty());
}

#[test]
fn index_update_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
        )
        .expect("update should succeed");

    let old_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("old@test.com"))
        .expect("index scan should succeed");
    assert!(old_docs.is_empty());

    let new_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("new@test.com"))
        .expect("index scan should succeed");
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_delete_removes_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .delete_with_indexes(&document.table, &document.id, std::slice::from_ref(&index))
        .expect("delete should succeed");

    let docs = store
        .index_scan_eq(&document.table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn index_scan_range_on_numbers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_age".to_string(),
        field: "age".to_string(),
    };
    for age in [20, 30, 40, 50] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(age))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let over_25 = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            None,
            false,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(over_25.len(), 3);

    let between = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            Some(&json!(35)),
            true,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(between.len(), 1);
    assert_eq!(between[0].fields.get("age"), Some(&json!(30)));
}

#[test]
fn scheduled_job_insert_and_claim_due_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "due");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let due = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");

    assert_eq!(due, vec![job.clone()]);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        store
            .claim_due_jobs(Timestamp(1_000))
            .expect("second claim should succeed")
            .is_empty()
    );

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
}

#[test]
fn scheduled_job_future_not_due() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "later");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");

    assert!(
        store
            .claim_due_jobs(Timestamp(4_999))
            .expect("claim should succeed")
            .is_empty()
    );
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .len(),
        1
    );
}

#[test]
fn cancel_scheduled_job_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "cancel me");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");

    assert!(
        store
            .cancel_scheduled_job(&job.id)
            .expect("cancel should succeed")
    );
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        !store
            .cancel_scheduled_job(&job.id)
            .expect("second cancel should succeed")
    );
}

#[test]
fn recover_running_jobs_moves_orphaned_work_back_to_pending() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "recover");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let claimed = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert_eq!(claimed.len(), 1);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("recovery should succeed");

    let pending = store.list_scheduled_jobs().expect("list should succeed");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, job.id);
    assert_eq!(pending[0].run_at, Timestamp(2_000));
}

#[test]
fn cron_job_crud_and_restart_roundtrip() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let cron = CronJob {
        name: "heartbeat".to_string(),
        schedule: CronSchedule::Interval { seconds: 10 },
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
        },
        enabled: true,
        last_run: None,
        next_run: Timestamp(10_000),
        created_at: Timestamp(1_000),
    };

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.save_cron_job(&cron).expect("save should succeed");
        let crons = store.load_cron_jobs().expect("load should succeed");
        assert_eq!(crons, vec![cron.clone()]);
        assert!(store.has_scheduled_work().expect("has work should succeed"));
        store
            .delete_cron_job("heartbeat")
            .expect("delete should succeed");
        assert!(
            store
                .load_cron_jobs()
                .expect("load after delete should succeed")
                .is_empty()
        );
        store.save_cron_job(&cron).expect("re-save should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let crons = reopened.load_cron_jobs().expect("load should succeed");
    assert_eq!(crons, vec![cron]);
}

#[test]
fn has_scheduled_work_detects_pending_or_running_jobs() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert!(!store.has_scheduled_work().expect("has work should succeed"));

    let job = scheduled_insert_job(Timestamp(1_000), "pending");
    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    assert!(store.has_scheduled_work().expect("has work should succeed"));

    let _ = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert!(store.has_scheduled_work().expect("has work should succeed"));

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
    assert!(!store.has_scheduled_work().expect("has work should succeed"));
}

#[test]
fn next_scheduled_work_at_prefers_earliest_pending_or_enabled_cron() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        None
    );

    let future_job = scheduled_insert_job(Timestamp(5_000), "later");
    let earlier_job = scheduled_insert_job(Timestamp(2_000), "earlier");
    store
        .insert_scheduled_job(&future_job)
        .expect("future job insert should succeed");
    store
        .insert_scheduled_job(&earlier_job)
        .expect("earlier job insert should succeed");
    store
        .save_cron_job(&CronJob {
            name: "disabled".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("disabled"))]),
            },
            enabled: false,
            last_run: None,
            next_run: Timestamp(1_000),
            created_at: Timestamp(500),
        })
        .expect("disabled cron should save");
    store
        .save_cron_job(&CronJob {
            name: "heartbeat".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
            },
            enabled: true,
            last_run: None,
            next_run: Timestamp(3_000),
            created_at: Timestamp(500),
        })
        .expect("enabled cron should save");

    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        Some(Timestamp(2_000))
    );

    let claimed = store
        .claim_due_jobs(Timestamp(2_000))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![earlier_job]);

    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        Some(Timestamp(3_000))
    );
}

#[test]
fn scheduled_job_result_roundtrip_and_lookup() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "done");
    let result = ScheduledJobResult {
        id: job.id,
        run_at: job.run_at,
        finished_at: Timestamp(2_000),
        mutation: job.mutation,
        outcome: ScheduledJobOutcome::Failed,
        error: Some("boom".to_string()),
    };

    store
        .record_scheduled_job_result(&result)
        .expect("result record should succeed");
    let loaded = store
        .get_scheduled_job_result(&result.id)
        .expect("result lookup should succeed")
        .expect("result should exist");

    assert_eq!(loaded, result);
}

#[test]
fn claim_due_jobs_includes_u64_max_boundary() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = ScheduledJob {
        id: DocumentId::from_bytes([0; 16]),
        run_at: Timestamp(u64::MAX),
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("edge"))]),
        },
        created_at: Timestamp(1_000),
    };

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let due = store
        .claim_due_jobs(Timestamp(u64::MAX))
        .expect("claim should succeed");

    assert_eq!(due, vec![job]);
}

#[test]
fn usage_store_counts_unique_monthly_active_users_per_month() {
    let store = UsageStore::create_in_memory().expect("usage store should open");
    let march_10 = utc_unix_ms(2026, Month::March, 10);
    let march_20 = utc_unix_ms(2026, Month::March, 20);
    let april_2 = utc_unix_ms(2026, Month::April, 2);

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", march_10)
            .expect("first monthly active user should record"),
    );
    assert!(
        !store
            .record_monthly_active_user("https://issuer.example.com|ada", march_20)
            .expect("same user in same month should dedupe"),
    );
    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|grace", march_20)
            .expect("second user in same month should record"),
    );

    let march = store
        .monthly_active_users_for(march_20)
        .expect("march usage should load");
    assert_eq!(march.month, "2026-03");
    assert_eq!(march.monthly_active_users, 2);
    assert_eq!(march.last_recorded_at_unix_ms, Some(march_20));
    assert_eq!(
        store
            .distinct_identities_for_month(march_20)
            .expect("march identities should load"),
        vec![
            "https://issuer.example.com|ada".to_string(),
            "https://issuer.example.com|grace".to_string()
        ]
    );

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", april_2)
            .expect("same user in next month should count again"),
    );
    let april = store
        .monthly_active_users_for(april_2)
        .expect("april usage should load");
    assert_eq!(april.month, "2026-04");
    assert_eq!(april.monthly_active_users, 1);
}

fn utc_unix_ms(year: i32, month: Month, day: u8) -> u64 {
    let date = Date::from_calendar_date(year, month, day).expect("calendar date should build");
    let datetime = PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_utc();
    u64::try_from(datetime.unix_timestamp_nanos() / 1_000_000)
        .expect("unix milliseconds should fit in u64")
}
