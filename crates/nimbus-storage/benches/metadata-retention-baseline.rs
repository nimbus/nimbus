use std::env;
use std::error::Error;
use std::fs;
use std::time::Instant;

use nimbus_core::{
    Document, FieldSchema, FieldType, IndexDefinition, IndexId, IndexState, TableName, TableSchema,
};
use nimbus_storage::{PointInTimeRestoreTarget, RetentionGcConfig, TenantStore};
use serde_json::{Map, json};

const DEFAULT_COMMITS: usize = 2_048;
const DEFAULT_DOCUMENTS: usize = 256;
const DEFAULT_WINDOW: u64 = 512;

fn main() -> Result<(), Box<dyn Error>> {
    let commits = env_usize("NIMBUS_RETENTION_BENCH_COMMITS", DEFAULT_COMMITS)?;
    let documents = env_usize("NIMBUS_RETENTION_BENCH_DOCUMENTS", DEFAULT_DOCUMENTS)?;
    let window = env_u64("NIMBUS_RETENTION_BENCH_WINDOW", DEFAULT_WINDOW)?;
    if documents == 0 || commits < documents || window == 0 {
        return Err("retention baseline requires commits >= documents > 0 and window > 0".into());
    }

    let dir = tempfile::tempdir()?;
    let database_path = dir.path().join("tenant.redb");
    let store = TenantStore::open(&database_path)?;
    let table = TableName::new("retention_baseline")?;
    store.replace_table_schema(&TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "key".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "revision".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
            FieldSchema {
                name: "payload".to_string(),
                field_type: FieldType::String,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: IndexId::new(),
            name: "by_revision".to_string(),
            fields: vec!["revision".to_string()],
            state: IndexState::Enabled,
        }],
        access_policy: None,
    })?;
    let mut seeded = Vec::with_capacity(documents);

    let write_started = Instant::now();
    for index in 0..documents {
        let document = Document::new(
            table.clone(),
            Map::from_iter([
                ("key".to_string(), json!(format!("document-{index:08}"))),
                ("revision".to_string(), json!(0_u64)),
                ("payload".to_string(), json!("x".repeat(128))),
            ]),
        );
        store.insert(&document)?;
        seeded.push(document);
    }
    for revision in documents..commits {
        let document = &seeded[revision % documents];
        store.update(
            &table,
            &document.id,
            &Map::from_iter([("revision".to_string(), json!(revision as u64))]),
        )?;
    }
    let write_elapsed = write_started.elapsed();

    let before = store.storage_health_diagnostic()?;
    let journal_records = store
        .read_durable_journal_from(nimbus_core::SequenceNumber(1))?
        .len();
    let database_bytes = fs::metadata(&database_path)?.len();
    let latest = store.latest_sequence()?;

    let export_started = Instant::now();
    let archive = store.export_point_in_time_restore_archive(
        PointInTimeRestoreTarget::Sequence(latest),
        RetentionGcConfig::retain_all(),
    )?;
    let export_elapsed = export_started.elapsed();
    let archive_bytes = rmp_serde::to_vec_named(&archive)?.len();

    let compact_started = Instant::now();
    let summary = store.compact_retained_versions(RetentionGcConfig::new(window)?)?;
    let compact_elapsed = compact_started.elapsed();
    let after = store.storage_health_diagnostic()?;

    println!("Nimbus storage metadata-retention baseline");
    println!(
        "document_commits={commits} latest_sequence={} documents={documents} configured_window_sequences={window}",
        latest.0
    );
    println!(
        "write_elapsed_ms={} writes_per_second={:.2}",
        write_elapsed.as_millis(),
        commits as f64 / write_elapsed.as_secs_f64()
    );
    println!(
        "database_bytes={database_bytes} bytes_per_commit={:.2}",
        database_bytes as f64 / commits as f64
    );
    println!(
        "journal_records={journal_records} archive_bytes={archive_bytes} archive_bytes_per_commit={:.2} export_elapsed_ms={}",
        archive_bytes as f64 / commits as f64,
        export_elapsed.as_millis()
    );
    println!(
        "document_versions_before={} document_versions_after={} document_versions_pruned={}",
        before.document_versions.version_count,
        after.document_versions.version_count,
        summary.document_versions_pruned
    );
    println!(
        "index_versions_before={} index_versions_after={} index_versions_pruned={}",
        before.index_versions.version_count,
        after.index_versions.version_count,
        summary.index_versions_pruned
    );
    println!(
        "compaction_elapsed_ms={} journal_records_pruned=0 journal_floor=0",
        compact_elapsed.as_millis()
    );

    Ok(())
}

fn env_usize(name: &str, default: usize) -> Result<usize, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, Box<dyn Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
