use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use nimbus_core::{
    Document, FieldSchema, FieldType, IndexDefinition, IndexId, IndexState, SequenceNumber,
    TableName, TableSchema,
};
use nimbus_storage::{PointInTimeRestoreTarget, RetentionGcConfig, TenantStore};
use serde_json::{Map, json};

const DEFAULT_COMMITS: usize = 2_048;
const DEFAULT_DOCUMENTS: usize = 256;
const DEFAULT_WINDOW: u64 = 512;
const DEFAULT_MAINTENANCE_STEP: usize = 256;

#[derive(Debug, Default)]
struct RetentionMeasurements {
    runs: usize,
    prepare_elapsed: Duration,
    prepare_elapsed_max: Duration,
    finalize_elapsed: Duration,
    finalize_elapsed_max: Duration,
    journal_records_pruned: u64,
    document_versions_pruned: u64,
    index_versions_pruned: u64,
    database_size_samples: Vec<u64>,
}

impl RetentionMeasurements {
    fn total_records_pruned(&self) -> u64 {
        self.journal_records_pruned
            .saturating_add(self.document_versions_pruned)
            .saturating_add(self.index_versions_pruned)
    }

    fn total_elapsed(&self) -> Duration {
        self.prepare_elapsed.saturating_add(self.finalize_elapsed)
    }
}

#[derive(Debug)]
struct WorkloadResult {
    latest_sequence: SequenceNumber,
    write_elapsed: Duration,
    database_bytes: u64,
    document_versions: u64,
    index_versions: u64,
    checkpoint_sequence: SequenceNumber,
    checkpoint_bytes: usize,
    archive_bytes: usize,
    archive_tail_records: usize,
    export_elapsed: Duration,
    restore_elapsed: Duration,
    retention: RetentionMeasurements,
}

fn main() -> Result<(), Box<dyn Error>> {
    let commits = env_usize("NIMBUS_RETENTION_BENCH_COMMITS", DEFAULT_COMMITS)?;
    let documents = env_usize("NIMBUS_RETENTION_BENCH_DOCUMENTS", DEFAULT_DOCUMENTS)?;
    let window = env_u64("NIMBUS_RETENTION_BENCH_WINDOW", DEFAULT_WINDOW)?;
    let maintenance_step = env_usize(
        "NIMBUS_RETENTION_BENCH_MAINTENANCE_STEP",
        DEFAULT_MAINTENANCE_STEP,
    )?;
    if documents == 0
        || commits < documents
        || window == 0
        || maintenance_step == 0
        || commits as u64 <= window
    {
        return Err(
            "retention qualification requires commits >= documents > 0, commits > window, and maintenance_step > 0"
                .into(),
        );
    }

    let cdc_window = window.saturating_div(2).max(1);
    let bounded_config = RetentionGcConfig::with_windows(window, window, cdc_window, window)?;
    let dir = tempfile::tempdir()?;
    let retain_all = run_workload(
        &dir.path().join("retain-all.redb"),
        &dir.path().join("retain-all-restore.redb"),
        commits,
        documents,
        None,
        maintenance_step,
    )?;
    let bounded = run_workload(
        &dir.path().join("bounded.redb"),
        &dir.path().join("bounded-restore.redb"),
        commits,
        documents,
        Some(bounded_config),
        maintenance_step,
    )?;

    if retain_all.latest_sequence != bounded.latest_sequence {
        return Err("paired retention workloads produced different durable heads".into());
    }
    let expected_checkpoint = SequenceNumber(bounded.latest_sequence.0.saturating_sub(window));
    if bounded.checkpoint_sequence != expected_checkpoint {
        return Err(format!(
            "bounded checkpoint {} did not reach expected floor {}",
            bounded.checkpoint_sequence.0, expected_checkpoint.0
        )
        .into());
    }
    if bounded.archive_tail_records as u64 > window {
        return Err(format!(
            "bounded PITR archive retained {} records for a {window}-sequence window",
            bounded.archive_tail_records
        )
        .into());
    }

    let latest_path_ratio =
        bounded.write_elapsed.as_secs_f64() / retain_all.write_elapsed.as_secs_f64();
    let retained_records_per_second = bounded.retention.total_records_pruned() as f64
        / bounded.retention.total_elapsed().as_secs_f64();
    let first_steady_size = bounded
        .retention
        .database_size_samples
        .first()
        .copied()
        .unwrap_or(bounded.database_bytes);
    let last_steady_size = bounded
        .retention
        .database_size_samples
        .last()
        .copied()
        .unwrap_or(bounded.database_bytes);
    let peak_steady_size = bounded
        .retention
        .database_size_samples
        .iter()
        .copied()
        .max()
        .unwrap_or(bounded.database_bytes);

    println!("Nimbus storage metadata-retention qualification");
    println!(
        "document_commits={commits} latest_sequence={} documents={documents} document_index_pitr_window_sequences={window} cdc_window_sequences={cdc_window} maintenance_step_sequences={maintenance_step}",
        bounded.latest_sequence.0
    );
    println!(
        "retain_all_latest_path_elapsed_ms={:.3} retain_all_writes_per_second={:.2}",
        duration_millis(retain_all.write_elapsed),
        commits as f64 / retain_all.write_elapsed.as_secs_f64()
    );
    println!(
        "bounded_latest_path_elapsed_ms={:.3} bounded_writes_per_second={:.2} latest_path_ratio={latest_path_ratio:.4} latest_path_overhead_percent={:.2}",
        duration_millis(bounded.write_elapsed),
        commits as f64 / bounded.write_elapsed.as_secs_f64(),
        (latest_path_ratio - 1.0) * 100.0
    );
    println!(
        "bounded_maintenance_runs={} prepare_elapsed_ms={:.3} prepare_max_ms={:.3} finalize_elapsed_ms={:.3} finalize_max_ms={:.3}",
        bounded.retention.runs,
        duration_millis(bounded.retention.prepare_elapsed),
        duration_millis(bounded.retention.prepare_elapsed_max),
        duration_millis(bounded.retention.finalize_elapsed),
        duration_millis(bounded.retention.finalize_elapsed_max)
    );
    println!(
        "journal_records_pruned={} document_versions_pruned={} index_versions_pruned={} total_records_pruned={} compaction_records_per_second={retained_records_per_second:.2}",
        bounded.retention.journal_records_pruned,
        bounded.retention.document_versions_pruned,
        bounded.retention.index_versions_pruned,
        bounded.retention.total_records_pruned()
    );
    println!(
        "checkpoint_sequence={} checkpoint_bytes={} retain_all_checkpoint_bytes={}",
        bounded.checkpoint_sequence.0, bounded.checkpoint_bytes, retain_all.checkpoint_bytes
    );
    println!(
        "retain_all_archive_bytes={} retain_all_archive_tail_records={} retain_all_export_ms={:.3} retain_all_restore_ms={:.3}",
        retain_all.archive_bytes,
        retain_all.archive_tail_records,
        duration_millis(retain_all.export_elapsed),
        duration_millis(retain_all.restore_elapsed)
    );
    println!(
        "bounded_archive_bytes={} bounded_archive_tail_records={} bounded_export_ms={:.3} bounded_restore_ms={:.3}",
        bounded.archive_bytes,
        bounded.archive_tail_records,
        duration_millis(bounded.export_elapsed),
        duration_millis(bounded.restore_elapsed)
    );
    println!(
        "retain_all_database_bytes={} bounded_database_bytes={} bounded_first_maintenance_bytes={} bounded_last_maintenance_bytes={} bounded_peak_maintenance_bytes={} bounded_steady_state_growth_bytes={}",
        retain_all.database_bytes,
        bounded.database_bytes,
        first_steady_size,
        last_steady_size,
        peak_steady_size,
        i128::from(last_steady_size) - i128::from(first_steady_size)
    );
    println!(
        "retain_all_document_versions={} bounded_document_versions={} retain_all_index_versions={} bounded_index_versions={}",
        retain_all.document_versions,
        bounded.document_versions,
        retain_all.index_versions,
        bounded.index_versions
    );

    Ok(())
}

fn run_workload(
    database_path: &Path,
    restore_path: &Path,
    commits: usize,
    documents: usize,
    retention_config: Option<RetentionGcConfig>,
    maintenance_step: usize,
) -> Result<WorkloadResult, Box<dyn Error>> {
    let store = TenantStore::open(database_path)?;
    let table = TableName::new("retention_qualification")?;
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
    let mut write_elapsed = Duration::ZERO;
    let mut retention = RetentionMeasurements::default();

    for index in 0..documents {
        let document = Document::new(
            table.clone(),
            Map::from_iter([
                ("key".to_string(), json!(format!("document-{index:08}"))),
                ("revision".to_string(), json!(0_u64)),
                ("payload".to_string(), json!("x".repeat(128))),
            ]),
        );
        let started = Instant::now();
        store.insert(&document)?;
        write_elapsed = write_elapsed.saturating_add(started.elapsed());
        seeded.push(document);
        maybe_run_retention(
            &store,
            database_path,
            retention_config,
            index + 1,
            maintenance_step,
            &mut retention,
        )?;
    }
    for revision in documents..commits {
        let document = &seeded[revision % documents];
        let started = Instant::now();
        store.update(
            &table,
            &document.id,
            &Map::from_iter([("revision".to_string(), json!(revision as u64))]),
        )?;
        write_elapsed = write_elapsed.saturating_add(started.elapsed());
        maybe_run_retention(
            &store,
            database_path,
            retention_config,
            revision + 1,
            maintenance_step,
            &mut retention,
        )?;
    }
    if let Some(config) = retention_config {
        run_retention_if_eligible(&store, database_path, config, &mut retention)?;
    }

    let latest_sequence = store.latest_sequence()?;
    let config = retention_config.unwrap_or_else(RetentionGcConfig::retain_all);
    let state = store.retention_history_state(config)?;
    let checkpoint_bytes = rmp_serde::to_vec_named(&state.checkpoint)?.len();

    let export_started = Instant::now();
    let archive = store.export_point_in_time_restore_archive(
        PointInTimeRestoreTarget::Sequence(latest_sequence),
        config,
    )?;
    let export_elapsed = export_started.elapsed();
    let archive_bytes = rmp_serde::to_vec_named(&archive)?.len();
    let archive_tail_records = archive.journal_tail.len();

    let restored = TenantStore::open(restore_path)?;
    let restore_started = Instant::now();
    restored.import_point_in_time_restore_archive(&archive)?;
    let restore_elapsed = restore_started.elapsed();
    let restored_position = restored
        .export_materialized_journal_snapshot()?
        .materialized_position()?;
    if restored_position != archive.target_position {
        return Err("PITR qualification restore produced a different position".into());
    }

    let diagnostic = store.storage_health_diagnostic()?;
    Ok(WorkloadResult {
        latest_sequence,
        write_elapsed,
        database_bytes: fs::metadata(database_path)?.len(),
        document_versions: diagnostic.document_versions.version_count,
        index_versions: diagnostic.index_versions.version_count,
        checkpoint_sequence: state.confirmed_floor,
        checkpoint_bytes,
        archive_bytes,
        archive_tail_records,
        export_elapsed,
        restore_elapsed,
        retention,
    })
}

fn maybe_run_retention(
    store: &TenantStore,
    database_path: &Path,
    retention_config: Option<RetentionGcConfig>,
    completed_commits: usize,
    maintenance_step: usize,
    measurements: &mut RetentionMeasurements,
) -> Result<(), Box<dyn Error>> {
    if completed_commits.is_multiple_of(maintenance_step)
        && let Some(config) = retention_config
    {
        run_retention_if_eligible(store, database_path, config, measurements)?;
    }
    Ok(())
}

fn run_retention_if_eligible(
    store: &TenantStore,
    database_path: &Path,
    config: RetentionGcConfig,
    measurements: &mut RetentionMeasurements,
) -> Result<(), Box<dyn Error>> {
    let state = store.retention_history_state(config)?;
    if state.desired_floor.0 <= state.confirmed_floor.0 {
        return Ok(());
    }

    let prepare_started = Instant::now();
    let prepared = store.prepare_retained_history(config)?;
    let prepare_elapsed = prepare_started.elapsed();
    let finalize_started = Instant::now();
    let summary = store.finalize_retained_history(prepared)?;
    let finalize_elapsed = finalize_started.elapsed();

    measurements.runs += 1;
    measurements.prepare_elapsed = measurements.prepare_elapsed.saturating_add(prepare_elapsed);
    measurements.prepare_elapsed_max = measurements.prepare_elapsed_max.max(prepare_elapsed);
    measurements.finalize_elapsed = measurements
        .finalize_elapsed
        .saturating_add(finalize_elapsed);
    measurements.finalize_elapsed_max = measurements.finalize_elapsed_max.max(finalize_elapsed);
    measurements.journal_records_pruned = measurements
        .journal_records_pruned
        .saturating_add(summary.journal_records_pruned);
    measurements.document_versions_pruned = measurements
        .document_versions_pruned
        .saturating_add(summary.document_versions_pruned);
    measurements.index_versions_pruned = measurements
        .index_versions_pruned
        .saturating_add(summary.index_versions_pruned);
    measurements
        .database_size_samples
        .push(fs::metadata(database_path)?.len());
    Ok(())
}

fn duration_millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
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
