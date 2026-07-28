//! Layered SQLite write-overhead benchmark.
//!
//! This benchmark replays one batch distribution captured from the saturated
//! `concurrent-write-throughput` CRUD workload through progressively fuller
//! layers:
//!
//! 1. one raw SQLite row mutation per logical mutation;
//! 2. the Nimbus durable data shape, with reusable statements and invariants
//!    hoisted out of the per-record loop;
//! 3. the production `SqliteTenantStore` append-then-apply path.
//!
//! All lanes use the workspace's bundled SQLCipher SQLite build, WAL,
//! `synchronous=FULL`, the same 768 CRUD records and payload bytes, and the
//! same six-batch distribution. Setup and connection initialization happen
//! outside timed write intervals. The report keeps logical mutations, row
//! changes, transactions, and sync-bearing commits separate so unlike work is
//! not presented as an apples-to-apples "writes per second" comparison.
//!
//! Environment:
//!
//! - `NIMBUS_SWO_ROUNDS` (default 12; supported range 2–31)
//! - `NIMBUS_SWO_REPETITIONS_PER_SAMPLE` (default 60)
//! - `NIMBUS_SWO_OUT` (optional Markdown output path)

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nimbus_core::{
    Document, DocumentId, SequenceNumber, TableId, TableName, TenantEventRecord, Timestamp,
    WriteOp, WriteOpType,
};
use nimbus_storage::{SqliteTenantStore, commit_log, sqlite_init_sql};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use tempfile::TempDir;

const DOCUMENTS: usize = 256;
const MUTATIONS: usize = DOCUMENTS * 3;
const BATCH_SIZES: [usize; 6] = [5, 251, 90, 256, 20, 146];
const NEXT_SEQUENCE_KEY: &str = "next_sequence";
const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
const DOCUMENT_VERSION_FORMAT_KEY: &str = "document_versions.storage_format";

#[derive(Clone)]
struct Fixture {
    table: TableName,
    table_id: TableId,
    records: Vec<TenantEventRecord>,
    payloads: Vec<Vec<u8>>,
}

#[derive(Clone, Copy)]
struct IoSnapshot {
    db_bytes: u64,
    wal_bytes: u64,
    page_size: u64,
    wal_frames: u64,
    checkpointed_frames: u64,
    auto_checkpoint_pages: u64,
}

impl IoSnapshot {
    fn fieldwise_max(self, other: Self) -> Self {
        Self {
            db_bytes: self.db_bytes.max(other.db_bytes),
            wal_bytes: self.wal_bytes.max(other.wal_bytes),
            page_size: self.page_size.max(other.page_size),
            wal_frames: self.wal_frames.max(other.wal_frames),
            checkpointed_frames: self.checkpointed_frames.max(other.checkpointed_frames),
            auto_checkpoint_pages: self.auto_checkpoint_pages.max(other.auto_checkpoint_pages),
        }
    }
}

#[derive(Clone, Copy)]
struct Sample {
    seconds: f64,
    io: IoSnapshot,
}

#[derive(Clone, Copy)]
struct LaneModel {
    logical_mutations: u64,
    sql_statements: u64,
    row_changes: u64,
    transactions: u64,
    sync_commits: u64,
}

struct Summary {
    mean_mutations_per_second: f64,
    median_mutations_per_second: f64,
    ci_low: f64,
    ci_high: f64,
    cv_percent: f64,
    mean_seconds: f64,
    io: IoSnapshot,
    throughputs: Vec<f64>,
}

struct VersionProbeRow {
    table_id: String,
    document_id: String,
    sequence: u64,
    timestamp: u64,
    tombstone: u64,
    data_json: Option<String>,
    typed_fields_json: Option<String>,
    creation_time: Option<u64>,
    update_time: Option<u64>,
}

fn main() {
    let rounds = env_usize("NIMBUS_SWO_ROUNDS", 12);
    let repetitions = env_usize("NIMBUS_SWO_REPETITIONS_PER_SAMPLE", 60);
    assert!(
        (2..=31).contains(&rounds),
        "NIMBUS_SWO_ROUNDS must be between 2 and 31 so the reported 95% Student-t interval uses a tabulated critical value"
    );
    assert!(
        BATCH_SIZES.iter().sum::<usize>() == MUTATIONS,
        "captured batch distribution must cover the fixture"
    );
    let fixture = build_fixture();
    validate_storage_fixture(&fixture);

    let raw = run_samples(rounds, repetitions, || run_raw_lane(&fixture));
    let resident_current = run_samples(rounds, repetitions, || {
        run_resident_current_sql_lane(&fixture)
    });
    let guarded_prepared = run_samples(rounds, repetitions, || {
        run_guarded_prepared_sql_lane(&fixture)
    });
    let shaped = run_samples(rounds, repetitions, || run_shaped_lane(&fixture));
    let storage = run_samples(rounds, repetitions, || run_storage_lane(&fixture));
    let serialization = measure_serialization(&fixture, rounds);
    let connection = measure_connection_costs(rounds);
    let sqlite_identity = sqlite_build_identity();

    let raw_model = LaneModel {
        logical_mutations: MUTATIONS as u64,
        sql_statements: MUTATIONS as u64 + (BATCH_SIZES.len() as u64 * 2),
        row_changes: MUTATIONS as u64,
        transactions: BATCH_SIZES.len() as u64,
        sync_commits: BATCH_SIZES.len() as u64,
    };
    let shaped_model = LaneModel {
        logical_mutations: MUTATIONS as u64,
        // DML plus two metadata reads, two metadata writes, and four
        // BEGIN/COMMIT statements per batch.
        sql_statements: (MUTATIONS as u64 * 3) + (BATCH_SIZES.len() as u64 * 8),
        row_changes: (MUTATIONS as u64 * 3) + (BATCH_SIZES.len() as u64 * 2),
        transactions: BATCH_SIZES.len() as u64 * 2,
        sync_commits: BATCH_SIZES.len() as u64 * 2,
    };
    let resident_current_model = LaneModel {
        logical_mutations: MUTATIONS as u64,
        // Current per-record validation query shape, on one already
        // initialized resident connection. Fixture format/table invariants are
        // seeded before timing.
        sql_statements: 6_449,
        row_changes: shaped_model.row_changes,
        transactions: shaped_model.transactions,
        sync_commits: shaped_model.sync_commits,
    };
    let guarded_prepared_model = LaneModel {
        logical_mutations: MUTATIONS as u64,
        // Same replay-preimage and resource-binding guards as the current
        // loop, with statements cached and batch-invariant reads hoisted.
        sql_statements: 3_401,
        row_changes: shaped_model.row_changes,
        transactions: shaped_model.transactions,
        sync_commits: shaped_model.sync_commits,
    };
    let storage_model = LaneModel {
        logical_mutations: MUTATIONS as u64,
        // Source-counted for this fixture: 6,452 workload statements plus 19
        // initialization statements on each of 12 fresh writer connections.
        // This excludes the busy-handler C API and SQLite-internal statements.
        sql_statements: 6_680,
        // First-use document-format metadata and table identity are created
        // inside the production measured interval.
        row_changes: shaped_model.row_changes + 2,
        transactions: shaped_model.transactions,
        sync_commits: shaped_model.sync_commits,
    };

    let mut report = String::new();
    writeln!(report, "# Layered SQLite write overhead\n").unwrap();
    writeln!(
        report,
        "workload: 256 phased CRUD units / {MUTATIONS} logical mutations; batch distribution: `{BATCH_SIZES:?}`; rounds: {rounds}; repetitions/sample: {repetitions}; WAL + `synchronous=FULL`; bundled SQLCipher SQLite\n"
    )
    .unwrap();
    writeln!(
        report,
        "I/O evidence reports the fieldwise maximum observed across every measured repetition and round.\n"
    )
    .unwrap();
    writeln!(
        report,
        "SQLite runtime: `{}`; SQLCipher: `{}`; source id: `{}`\n",
        sqlite_identity.0, sqlite_identity.1, sqlite_identity.2
    )
    .unwrap();
    writeln!(
        report,
        "| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |"
    )
    .unwrap();
    writeln!(report, "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|").unwrap();
    render_lane(&mut report, "raw row mutation", &raw, raw_model);
    render_lane(
        &mut report,
        "current-loop SQL, resident connection",
        &resident_current,
        resident_current_model,
    );
    render_lane(
        &mut report,
        "guarded prepared/hoisted SQL",
        &guarded_prepared,
        guarded_prepared_model,
    );
    render_lane(
        &mut report,
        "Nimbus-shaped SQL lower bound",
        &shaped,
        shaped_model,
    );
    render_lane(
        &mut report,
        "production storage append+apply",
        &storage,
        storage_model,
    );
    writeln!(report, "## Bytes and checkpoint state\n").unwrap();
    writeln!(
        report,
        "| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |"
    )
    .unwrap();
    writeln!(report, "|---|---:|---:|---:|---:|---:|---:|").unwrap();
    render_io(&mut report, "raw row mutation", raw.io);
    render_io(
        &mut report,
        "current-loop SQL, resident connection",
        resident_current.io,
    );
    render_io(
        &mut report,
        "guarded prepared/hoisted SQL",
        guarded_prepared.io,
    );
    render_io(&mut report, "Nimbus-shaped SQL lower bound", shaped.io);
    render_io(&mut report, "production storage append+apply", storage.io);

    writeln!(report, "\n## Raw measured-round samples\n").unwrap();
    render_samples(&mut report, "raw row mutation", &raw);
    render_samples(
        &mut report,
        "current-loop SQL, resident connection",
        &resident_current,
    );
    render_samples(
        &mut report,
        "guarded prepared/hoisted SQL",
        &guarded_prepared,
    );
    render_samples(&mut report, "Nimbus-shaped SQL lower bound", &shaped);
    render_samples(&mut report, "production storage append+apply", &storage);

    writeln!(report, "\n## CPU-only serialization\n").unwrap();
    writeln!(
        report,
        "Production record MessagePack plus the current document JSON/typed-field encoding work: **{:.0} logical mutations/s** ({:.3} ms for one {MUTATIONS}-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.\n",
        MUTATIONS as f64 / serialization,
        serialization * 1_000.0
    )
    .unwrap();

    writeln!(report, "## Connection and initialization cost\n").unwrap();
    writeln!(
        report,
        "| operation | mean µs/op |\n|---|---:|\n| `Connection::open` only | {:.1} |\n| production-equivalent connection init on initialized DB | {:.1} |\n| `SqliteTenantStore::open` + schema load | {:.1} |\n",
        connection.0, connection.1, connection.2
    )
    .unwrap();

    print!("{report}");
    if let Ok(path) = std::env::var("NIMBUS_SWO_OUT") {
        fs::write(&path, report).expect("write layered SQLite report");
        eprintln!("[sqlite-write-overhead] report written to {path}");
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn build_fixture() -> Fixture {
    let table = TableName::new("tasks").expect("fixture table");
    let table_id = TableId::try_from("01J00000000000000000000000".to_string())
        .expect("fixed fixture table id");
    let mut inserted = Vec::with_capacity(DOCUMENTS);
    let mut updated = Vec::with_capacity(DOCUMENTS);
    for unit in 0..DOCUMENTS {
        let id = DocumentId::from_key(format!("task-{unit:05}")).expect("fixture id");
        let initial = Document::with_id_at(
            id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("open")),
                ("rank".to_string(), json!(unit)),
                ("title".to_string(), json!(format!("task-{unit:05}"))),
            ]),
            Timestamp(1_000 + unit as u64),
        );
        let mut revised = initial.clone();
        revised.update_time = Timestamp(2_000 + unit as u64);
        revised.set_field("rank", json!(unit + 300));
        inserted.push(initial);
        updated.push(revised);
    }

    let mut records = Vec::with_capacity(MUTATIONS);
    for document in &inserted {
        records.push(record(
            records.len() + 1,
            &table_id,
            WriteOpType::Insert,
            None,
            Some(document.clone()),
        ));
    }
    for (before, after) in inserted.iter().zip(&updated) {
        records.push(record(
            records.len() + 1,
            &table_id,
            WriteOpType::Update,
            Some(before.clone()),
            Some(after.clone()),
        ));
    }
    for document in &updated {
        records.push(record(
            records.len() + 1,
            &table_id,
            WriteOpType::Delete,
            Some(document.clone()),
            None,
        ));
    }
    let payloads = records
        .iter()
        .map(|record| commit_log::serialize_tenant_event_record(record).expect("serialize fixture"))
        .collect();
    Fixture {
        table,
        table_id,
        records,
        payloads,
    }
}

fn record(
    sequence: usize,
    table_id: &TableId,
    op_type: WriteOpType,
    previous: Option<Document>,
    current: Option<Document>,
) -> TenantEventRecord {
    let document = current.as_ref().or(previous.as_ref()).expect("document");
    TenantEventRecord::new(
        SequenceNumber(sequence as u64),
        Timestamp(10_000 + sequence as u64),
        vec![WriteOp {
            table: document.table.clone(),
            table_id: table_id.clone(),
            op_type,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("fixture record")
}

fn configure_connection(conn: &Connection) {
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("busy timeout");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;",
    )
    .expect("configure SQLite");
}

fn run_samples<F>(rounds: usize, repetitions: usize, mut run: F) -> Summary
where
    F: FnMut() -> Sample,
{
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let mut seconds = 0.0;
        let mut io_max: Option<IoSnapshot> = None;
        for _ in 0..repetitions {
            let sample = run();
            seconds += sample.seconds;
            io_max = Some(io_max.map_or(sample.io, |previous| previous.fieldwise_max(sample.io)));
        }
        samples.push(Sample {
            seconds: seconds / repetitions as f64,
            io: io_max.expect("at least one repetition"),
        });
    }
    summarize(&samples)
}

fn run_raw_lane(fixture: &Fixture) -> Sample {
    let dir = TempDir::new().expect("raw tempdir");
    let path = dir.path().join("raw.sqlite3");
    let conn = Connection::open(&path).expect("open raw database");
    configure_connection(&conn);
    conn.execute_batch(
        "CREATE TABLE raw_documents (
            id TEXT NOT NULL PRIMARY KEY,
            payload BLOB NOT NULL
        );",
    )
    .expect("create raw table");

    let started = Instant::now();
    for_each_batch(|start, end| {
        conn.execute_batch("BEGIN IMMEDIATE").expect("raw begin");
        for (index, record) in fixture.records[start..end].iter().enumerate() {
            let absolute = start + index;
            let write = &record.writes[0];
            match write.op_type {
                WriteOpType::Insert => {
                    conn.prepare_cached("INSERT INTO raw_documents (id, payload) VALUES (?1, ?2)")
                        .expect("prepare raw insert")
                        .execute(params![write.doc_id.as_str(), &fixture.payloads[absolute]])
                        .expect("raw insert");
                }
                WriteOpType::Update => {
                    conn.prepare_cached("UPDATE raw_documents SET payload = ?2 WHERE id = ?1")
                        .expect("prepare raw update")
                        .execute(params![write.doc_id.as_str(), &fixture.payloads[absolute]])
                        .expect("raw update");
                }
                WriteOpType::Delete => {
                    conn.prepare_cached("DELETE FROM raw_documents WHERE id = ?1")
                        .expect("prepare raw delete")
                        .execute(params![write.doc_id.as_str()])
                        .expect("raw delete");
                }
            }
        }
        conn.execute_batch("COMMIT").expect("raw commit");
    });
    let seconds = started.elapsed().as_secs_f64();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM raw_documents", [], |row| {
            row.get::<_, u64>(0)
        })
        .expect("raw count"),
        0,
        "phased raw CRUD must leave no live rows"
    );
    let io = inspect_io(&path, &conn);
    Sample { seconds, io }
}

fn run_resident_current_sql_lane(fixture: &Fixture) -> Sample {
    let dir = TempDir::new().expect("resident current tempdir");
    let path = dir.path().join("resident-current.sqlite3");
    let conn = initialized_shaped_connection(&path, fixture);

    let started = Instant::now();
    for_each_batch(|start, end| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("resident append begin");
        let next = conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [NEXT_SEQUENCE_KEY],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .expect("resident next sequence");
        if next.is_none() {
            black_box(
                conn.query_row("SELECT MAX(sequence) FROM commit_log", [], |row| {
                    row.get::<_, Option<u64>>(0)
                })
                .expect("resident latest sequence"),
            );
        }
        for (offset, record) in fixture.records[start..end].iter().enumerate() {
            conn.execute(
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                params![record.sequence.0, &fixture.payloads[start + offset]],
            )
            .expect("resident journal insert");
        }
        conn.execute(
            "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
            params![NEXT_SEQUENCE_KEY, (end as u64 + 1).to_be_bytes()],
        )
        .expect("resident next sequence");
        conn.execute_batch("COMMIT")
            .expect("resident append commit");

        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("resident apply begin");
        black_box(
            conn.query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [APPLIED_SEQUENCE_KEY],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .expect("resident applied sequence"),
        );
        for record in &fixture.records[start..end] {
            current_loop_apply_record(&conn, fixture, record);
        }
        conn.execute(
            "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
            params![APPLIED_SEQUENCE_KEY, (end as u64).to_be_bytes()],
        )
        .expect("resident applied sequence");
        conn.execute_batch("COMMIT").expect("resident apply commit");
    });
    let seconds = started.elapsed().as_secs_f64();
    assert_shaped_state(&conn);
    let io = inspect_io(&path, &conn);
    Sample { seconds, io }
}

fn run_guarded_prepared_sql_lane(fixture: &Fixture) -> Sample {
    let dir = TempDir::new().expect("guarded prepared tempdir");
    let path = dir.path().join("guarded-prepared.sqlite3");
    let conn = initialized_shaped_connection(&path, fixture);
    let hidden_namespace = format!("hidden:{}", fixture.table_id.as_str());

    let started = Instant::now();
    for_each_batch(|start, end| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("guarded append begin");
        let next = conn
            .prepare_cached("SELECT value_blob FROM metadata WHERE key = ?1")
            .expect("prepare guarded next sequence")
            .query_row([NEXT_SEQUENCE_KEY], |row| row.get::<_, Vec<u8>>(0))
            .optional()
            .expect("guarded next sequence");
        if next.is_none() {
            black_box(
                conn.prepare_cached("SELECT MAX(sequence) FROM commit_log")
                    .expect("prepare guarded latest sequence")
                    .query_row([], |row| row.get::<_, Option<u64>>(0))
                    .expect("guarded latest sequence"),
            );
        }
        for (offset, record) in fixture.records[start..end].iter().enumerate() {
            conn.prepare_cached("INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)")
                .expect("prepare guarded journal")
                .execute(params![
                    record.sequence.0,
                    &fixture.payloads[start + offset]
                ])
                .expect("guarded journal insert");
        }
        put_metadata(&conn, NEXT_SEQUENCE_KEY, (end as u64 + 1).to_be_bytes());
        conn.execute_batch("COMMIT").expect("guarded append commit");

        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("guarded apply begin");
        black_box(
            conn.prepare_cached("SELECT value_blob FROM metadata WHERE key = ?1")
                .expect("prepare guarded applied sequence")
                .query_row([APPLIED_SEQUENCE_KEY], |row| row.get::<_, Vec<u8>>(0))
                .optional()
                .expect("guarded applied sequence"),
        );
        // These four reads are invariant for this one-table batch and preserve
        // the current format, schema, and table-identity checks.
        black_box(
            conn.prepare_cached("SELECT value_blob FROM metadata WHERE key = ?1")
                .expect("prepare guarded format")
                .query_row([DOCUMENT_VERSION_FORMAT_KEY], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .optional()
                .expect("guarded document format"),
        );
        black_box(
            conn.prepare_cached("SELECT schema_json FROM schemas WHERE table_name = ?1")
                .expect("prepare guarded schema")
                .query_row([fixture.table.as_str()], |row| row.get::<_, String>(0))
                .optional()
                .expect("guarded schema"),
        );
        black_box(
            conn.prepare_cached(
                "SELECT table_id, state FROM table_catalog
                 WHERE namespace = ?1 AND table_name = ?2",
            )
            .expect("prepare guarded hidden identity")
            .query_row(params![hidden_namespace, fixture.table.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()
            .expect("guarded hidden identity"),
        );
        black_box(
            conn.prepare_cached(
                "SELECT table_id, state FROM table_catalog
                 WHERE namespace = 'default' AND table_name = ?1",
            )
            .expect("prepare guarded active identity")
            .query_row([fixture.table.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("guarded active identity"),
        );
        for record in &fixture.records[start..end] {
            guarded_prepared_apply_record(&conn, record);
        }
        put_metadata(&conn, APPLIED_SEQUENCE_KEY, (end as u64).to_be_bytes());
        conn.execute_batch("COMMIT").expect("guarded apply commit");
    });
    let seconds = started.elapsed().as_secs_f64();
    assert_shaped_state(&conn);
    let io = inspect_io(&path, &conn);
    Sample { seconds, io }
}

fn initialized_shaped_connection(path: &Path, fixture: &Fixture) -> Connection {
    let conn = Connection::open(path).expect("open shaped database");
    configure_connection(&conn);
    conn.execute_batch(sqlite_init_sql())
        .expect("initialize Nimbus schema");
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id, state)
         VALUES ('default', ?1, ?2, 'active')",
        params![fixture.table.as_str(), fixture.table_id.as_str()],
    )
    .expect("seed table identity");
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)",
        params![DOCUMENT_VERSION_FORMAT_KEY, 1_u64.to_be_bytes()],
    )
    .expect("seed document version format");
    conn
}

fn current_loop_apply_record(conn: &Connection, fixture: &Fixture, record: &TenantEventRecord) {
    let write = &record.writes[0];
    black_box(
        conn.query_row(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            [DOCUMENT_VERSION_FORMAT_KEY],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("resident document format"),
    );
    current_loop_write_version(conn, record);
    black_box(
        conn.query_row(
            "SELECT schema_json FROM schemas WHERE table_name = ?1",
            [fixture.table.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("resident schema"),
    );
    black_box(
        conn.query_row(
            "SELECT table_id, state FROM table_catalog
             WHERE namespace = ?1 AND table_name = ?2",
            params![
                format!("hidden:{}", fixture.table_id.as_str()),
                fixture.table.as_str()
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .expect("resident hidden identity"),
    );
    black_box(
        conn.query_row(
            "SELECT table_id, state FROM table_catalog
             WHERE namespace = 'default' AND table_name = ?1",
            [fixture.table.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("resident active identity"),
    );
    black_box(
        conn.query_row(
            "SELECT creation_time, update_time, data_json, typed_fields_json
             FROM documents WHERE table_id = ?1 AND id = ?2",
            params![write.table_id.as_str(), write.doc_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .expect("resident preimage"),
    );
    current_loop_write_document(conn, record);
    if write.op_type == WriteOpType::Delete {
        conn.execute(
            "DELETE FROM resource_path_bindings WHERE locator_key = ?1",
            [write.doc_id.as_str().as_bytes()],
        )
        .expect("resident resource binding delete");
    }
}

fn current_loop_write_version(conn: &Connection, record: &TenantEventRecord) {
    let write = &record.writes[0];
    match &write.current {
        Some(current) => {
            conn.execute(
                "INSERT INTO document_versions (
                    table_id, id, commit_sequence, commit_time, tombstone,
                    data_json, typed_fields_json, creation_time, update_time
                 ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
                params![
                    write.table_id.as_str(),
                    write.doc_id.as_str(),
                    record.sequence.0,
                    record.timestamp.0,
                    serde_json::to_string(&current.fields).expect("serialize fields"),
                    serde_json::to_string(&current.typed_fields).expect("serialize typed"),
                    current.creation_time.0,
                    current.update_time.0,
                ],
            )
            .expect("resident live version");
        }
        None => {
            conn.execute(
                "INSERT INTO document_versions (
                    table_id, id, commit_sequence, commit_time, tombstone
                 ) VALUES (?1, ?2, ?3, ?4, 1)",
                params![
                    write.table_id.as_str(),
                    write.doc_id.as_str(),
                    record.sequence.0,
                    record.timestamp.0,
                ],
            )
            .expect("resident tombstone");
        }
    }
}

fn current_loop_write_document(conn: &Connection, record: &TenantEventRecord) {
    let write = &record.writes[0];
    match write.op_type {
        WriteOpType::Insert => {
            let current = write.current.as_ref().expect("insert current");
            conn.execute(
                "INSERT INTO documents (
                    table_id, id, data_json, typed_fields_json, creation_time, update_time
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    write.table_id.as_str(),
                    write.doc_id.as_str(),
                    serde_json::to_string(&current.fields).expect("serialize fields"),
                    serde_json::to_string(&current.typed_fields).expect("serialize typed"),
                    current.creation_time.0,
                    current.update_time.0,
                ],
            )
            .expect("resident document insert");
        }
        WriteOpType::Update => {
            let current = write.current.as_ref().expect("update current");
            conn.execute(
                "UPDATE documents SET
                    data_json = ?3, typed_fields_json = ?4,
                    creation_time = ?5, update_time = ?6
                 WHERE table_id = ?1 AND id = ?2",
                params![
                    write.table_id.as_str(),
                    write.doc_id.as_str(),
                    serde_json::to_string(&current.fields).expect("serialize fields"),
                    serde_json::to_string(&current.typed_fields).expect("serialize typed"),
                    current.creation_time.0,
                    current.update_time.0,
                ],
            )
            .expect("resident document update");
        }
        WriteOpType::Delete => {
            conn.execute(
                "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                params![write.table_id.as_str(), write.doc_id.as_str()],
            )
            .expect("resident document delete");
        }
    }
}

fn guarded_prepared_apply_record(conn: &Connection, record: &TenantEventRecord) {
    let write = &record.writes[0];
    black_box(
        conn.prepare_cached(
            "SELECT creation_time, update_time, data_json, typed_fields_json
             FROM documents WHERE table_id = ?1 AND id = ?2",
        )
        .expect("prepare guarded preimage")
        .query_row(
            params![write.table_id.as_str(), write.doc_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .expect("guarded preimage"),
    );
    shaped_apply_record(conn, record);
    if write.op_type == WriteOpType::Delete {
        conn.prepare_cached("DELETE FROM resource_path_bindings WHERE locator_key = ?1")
            .expect("prepare guarded resource binding delete")
            .execute([write.doc_id.as_str().as_bytes()])
            .expect("guarded resource binding delete");
    }
}

fn run_shaped_lane(fixture: &Fixture) -> Sample {
    let dir = TempDir::new().expect("shaped tempdir");
    let path = dir.path().join("shaped.sqlite3");
    let conn = initialized_shaped_connection(&path, fixture);

    let started = Instant::now();
    for_each_batch(|start, end| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("shaped append begin");
        let _: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [NEXT_SEQUENCE_KEY],
                |row| row.get(0),
            )
            .optional()
            .expect("shaped next sequence");
        for (offset, record) in fixture.records[start..end].iter().enumerate() {
            let absolute = start + offset;
            conn.prepare_cached("INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)")
                .expect("prepare shaped journal")
                .execute(params![record.sequence.0, &fixture.payloads[absolute]])
                .expect("shaped journal insert");
        }
        put_metadata(&conn, NEXT_SEQUENCE_KEY, (end as u64 + 1).to_be_bytes());
        conn.execute_batch("COMMIT").expect("shaped append commit");

        conn.execute_batch("BEGIN IMMEDIATE")
            .expect("shaped apply begin");
        let _: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                [APPLIED_SEQUENCE_KEY],
                |row| row.get(0),
            )
            .optional()
            .expect("shaped applied sequence");
        for record in &fixture.records[start..end] {
            shaped_apply_record(&conn, record);
        }
        put_metadata(&conn, APPLIED_SEQUENCE_KEY, (end as u64).to_be_bytes());
        conn.execute_batch("COMMIT").expect("shaped apply commit");
    });
    let seconds = started.elapsed().as_secs_f64();
    assert_shaped_state(&conn);
    let io = inspect_io(&path, &conn);
    Sample { seconds, io }
}

fn shaped_apply_record(conn: &Connection, record: &TenantEventRecord) {
    let write = &record.writes[0];
    match &write.current {
        Some(current) => {
            let fields = serde_json::to_string(&current.fields).expect("serialize fields");
            let typed = serde_json::to_string(&current.typed_fields).expect("serialize typed");
            conn.prepare_cached(
                "INSERT INTO document_versions (
                    table_id, id, commit_sequence, commit_time, tombstone,
                    data_json, typed_fields_json, creation_time, update_time
                 ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
            )
            .expect("prepare shaped version")
            .execute(params![
                write.table_id.as_str(),
                write.doc_id.as_str(),
                record.sequence.0,
                record.timestamp.0,
                fields,
                typed,
                current.creation_time.0,
                current.update_time.0,
            ])
            .expect("shaped live version");
            match write.op_type {
                WriteOpType::Insert => {
                    conn.prepare_cached(
                        "INSERT INTO documents (
                            table_id, id, data_json, typed_fields_json,
                            creation_time, update_time
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )
                    .expect("prepare shaped document insert")
                    .execute(params![
                        write.table_id.as_str(),
                        write.doc_id.as_str(),
                        serde_json::to_string(&current.fields).expect("serialize fields"),
                        serde_json::to_string(&current.typed_fields).expect("serialize typed"),
                        current.creation_time.0,
                        current.update_time.0,
                    ])
                    .expect("shaped document insert");
                }
                WriteOpType::Update => {
                    conn.prepare_cached(
                        "UPDATE documents
                         SET data_json = ?3, typed_fields_json = ?4,
                             creation_time = ?5, update_time = ?6
                         WHERE table_id = ?1 AND id = ?2",
                    )
                    .expect("prepare shaped document update")
                    .execute(params![
                        write.table_id.as_str(),
                        write.doc_id.as_str(),
                        serde_json::to_string(&current.fields).expect("serialize fields"),
                        serde_json::to_string(&current.typed_fields).expect("serialize typed"),
                        current.creation_time.0,
                        current.update_time.0,
                    ])
                    .expect("shaped document update");
                }
                WriteOpType::Delete => unreachable!("delete has no current document"),
            }
        }
        None => {
            conn.prepare_cached(
                "INSERT INTO document_versions (
                    table_id, id, commit_sequence, commit_time, tombstone
                 ) VALUES (?1, ?2, ?3, ?4, 1)",
            )
            .expect("prepare shaped tombstone")
            .execute(params![
                write.table_id.as_str(),
                write.doc_id.as_str(),
                record.sequence.0,
                record.timestamp.0,
            ])
            .expect("shaped tombstone");
            conn.prepare_cached("DELETE FROM documents WHERE table_id = ?1 AND id = ?2")
                .expect("prepare shaped document delete")
                .execute(params![write.table_id.as_str(), write.doc_id.as_str()])
                .expect("shaped document delete");
        }
    }
}

fn run_storage_lane(fixture: &Fixture) -> Sample {
    let dir = TempDir::new().expect("storage tempdir");
    let path = dir.path().join("storage.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("open production storage");

    let started = Instant::now();
    for_each_batch(|start, end| {
        let records = &fixture.records[start..end];
        store
            .append_durable_records_batch(records)
            .expect("production append");
        store
            .apply_durable_records_batch(records)
            .expect("production apply");
    });
    let seconds = started.elapsed().as_secs_f64();
    let progress = store
        .journal_progress()
        .expect("production journal progress");
    assert_eq!(progress.durable_head, SequenceNumber(MUTATIONS as u64));
    assert_eq!(progress.applied_head, SequenceNumber(MUTATIONS as u64));
    assert_storage_live_state(&store, fixture, MUTATIONS);
    let probe = Connection::open(&path).expect("open production probe");
    configure_connection(&probe);
    assert_storage_database_state(&probe, fixture);
    let io = inspect_io(&path, &probe);
    black_box(store);
    Sample { seconds, io }
}

fn validate_storage_fixture(fixture: &Fixture) {
    let dir = TempDir::new().expect("production validation tempdir");
    let path = dir.path().join("production-validation.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("open production validation storage");
    let mut applied = 0;
    for size in BATCH_SIZES {
        let end = applied + size;
        let records = &fixture.records[applied..end];
        store
            .append_durable_records_batch(records)
            .expect("validation append");
        store
            .apply_durable_records_batch(records)
            .expect("validation apply");
        assert_storage_live_state(&store, fixture, end);
        applied = end;
    }
    let probe = Connection::open(&path).expect("open production validation probe");
    configure_connection(&probe);
    assert_storage_database_state(&probe, fixture);
}

fn assert_storage_live_state(
    store: &SqliteTenantStore,
    fixture: &Fixture,
    applied_mutations: usize,
) {
    let documents = store
        .scan_table(&fixture.table)
        .expect("scan production validation table");
    let document_count = documents.len();
    let mut actual = documents
        .into_iter()
        .map(|document| (document.id.clone(), document))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        actual.len(),
        document_count,
        "production table scan must not contain duplicate document ids"
    );

    for index in 0..DOCUMENTS {
        let insert_sequence = index + 1;
        let update_sequence = DOCUMENTS + index + 1;
        let delete_sequence = (DOCUMENTS * 2) + index + 1;
        let expected =
            if applied_mutations < insert_sequence || applied_mutations >= delete_sequence {
                None
            } else if applied_mutations >= update_sequence {
                fixture.records[DOCUMENTS + index].writes[0]
                    .current
                    .as_ref()
            } else {
                fixture.records[index].writes[0].current.as_ref()
            };
        let id = &fixture.records[index].writes[0].doc_id;
        match expected {
            Some(expected) => assert_eq!(
                actual.remove(id).as_ref(),
                Some(expected),
                "production live document differs after mutation prefix {applied_mutations}: {id}"
            ),
            None => assert!(
                actual.remove(id).is_none(),
                "production live document should be absent after mutation prefix {applied_mutations}: {id}"
            ),
        }
    }
    assert!(
        actual.is_empty(),
        "production table contains documents outside the fixed fixture"
    );
}

fn assert_storage_database_state(conn: &Connection, fixture: &Fixture) {
    let mut journal = conn
        .prepare("SELECT sequence, record_blob FROM commit_log ORDER BY sequence")
        .expect("prepare production journal audit");
    let journal_rows = journal
        .query_map([], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .expect("query production journal audit")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect production journal audit");
    assert_eq!(journal_rows.len(), MUTATIONS);
    for (index, (sequence, payload)) in journal_rows.iter().enumerate() {
        assert_eq!(*sequence, (index + 1) as u64);
        assert_eq!(
            payload, &fixture.payloads[index],
            "production journal payload differs at sequence {sequence}"
        );
    }

    let mut versions = conn
        .prepare(
            "SELECT table_id, id, commit_sequence, commit_time, tombstone,
                    data_json, typed_fields_json, creation_time, update_time
             FROM document_versions
             ORDER BY commit_sequence",
        )
        .expect("prepare production version audit");
    let version_rows = versions
        .query_map([], |row| {
            Ok(VersionProbeRow {
                table_id: row.get(0)?,
                document_id: row.get(1)?,
                sequence: row.get(2)?,
                timestamp: row.get(3)?,
                tombstone: row.get(4)?,
                data_json: row.get(5)?,
                typed_fields_json: row.get(6)?,
                creation_time: row.get(7)?,
                update_time: row.get(8)?,
            })
        })
        .expect("query production version audit")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect production version audit");
    assert_eq!(version_rows.len(), MUTATIONS);
    for (index, (row, record)) in version_rows.iter().zip(&fixture.records).enumerate() {
        let write = &record.writes[0];
        assert_eq!(row.table_id, fixture.table_id.as_str());
        assert_eq!(row.document_id, write.doc_id.as_str());
        assert_eq!(row.sequence, (index + 1) as u64);
        assert_eq!(row.timestamp, record.timestamp.0);
        match &write.current {
            Some(current) => {
                assert_eq!(row.tombstone, 0);
                assert_eq!(
                    row.data_json.as_deref(),
                    Some(
                        serde_json::to_string(&current.fields)
                            .expect("serialize expected production fields")
                            .as_str()
                    )
                );
                assert_eq!(
                    row.typed_fields_json.as_deref(),
                    Some(
                        serde_json::to_string(&current.typed_fields)
                            .expect("serialize expected production typed fields")
                            .as_str()
                    )
                );
                assert_eq!(row.creation_time, Some(current.creation_time.0));
                assert_eq!(row.update_time, Some(current.update_time.0));
            }
            None => {
                assert_eq!(row.tombstone, 1);
                assert_eq!(row.data_json, None);
                assert_eq!(row.typed_fields_json, None);
                assert_eq!(row.creation_time, None);
                assert_eq!(row.update_time, None);
            }
        }
    }

    let mut catalog_query = conn
        .prepare(
            "SELECT namespace, table_name, table_id, state
             FROM table_catalog
             ORDER BY namespace, table_name",
        )
        .expect("prepare production table catalog audit");
    let catalog = catalog_query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("query production table catalog audit")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect production table catalog audit");
    assert_eq!(
        catalog,
        vec![(
            "default".to_string(),
            fixture.table.as_str().to_string(),
            fixture.table_id.as_str().to_string(),
            "active".to_string(),
        )]
    );

    let metadata_rows = conn
        .query_row("SELECT COUNT(*) FROM metadata", [], |row| {
            row.get::<_, u64>(0)
        })
        .expect("count production metadata");
    assert_eq!(metadata_rows, 3);
    for (key, expected) in [
        (NEXT_SEQUENCE_KEY, (MUTATIONS as u64 + 1).to_be_bytes()),
        (APPLIED_SEQUENCE_KEY, (MUTATIONS as u64).to_be_bytes()),
        (DOCUMENT_VERSION_FORMAT_KEY, 1_u64.to_be_bytes()),
    ] {
        let actual = conn
            .query_row(
                "SELECT value_blob FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap_or_else(|error| panic!("read production metadata {key}: {error}"));
        assert_eq!(actual, expected, "production metadata differs for {key}");
    }

    for table in ["documents", "index_versions", "resource_path_bindings"] {
        let statement = format!("SELECT COUNT(*) FROM {table}");
        let rows = conn
            .query_row(&statement, [], |row| row.get::<_, u64>(0))
            .unwrap_or_else(|error| panic!("count production {table}: {error}"));
        assert_eq!(rows, 0, "production {table} must be empty");
    }
}

fn for_each_batch(mut visit: impl FnMut(usize, usize)) {
    let mut start = 0;
    for size in BATCH_SIZES {
        let end = start + size;
        visit(start, end);
        start = end;
    }
}

fn put_metadata(conn: &Connection, key: &str, value: [u8; 8]) {
    conn.prepare_cached(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
    )
    .expect("prepare metadata")
    .execute(params![key, value])
    .expect("write metadata");
}

fn assert_shaped_state(conn: &Connection) {
    let journal_rows = conn
        .query_row("SELECT COUNT(*) FROM commit_log", [], |row| {
            row.get::<_, u64>(0)
        })
        .expect("count shaped journal rows");
    let version_rows = conn
        .query_row("SELECT COUNT(*) FROM document_versions", [], |row| {
            row.get::<_, u64>(0)
        })
        .expect("count shaped version rows");
    let live_rows = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |row| {
            row.get::<_, u64>(0)
        })
        .expect("count shaped live rows");
    assert_eq!(journal_rows, MUTATIONS as u64);
    assert_eq!(version_rows, MUTATIONS as u64);
    assert_eq!(live_rows, 0, "phased shaped CRUD must leave no live rows");
}

fn inspect_io(path: &Path, conn: &Connection) -> IoSnapshot {
    let db_bytes = file_len(path);
    let wal_bytes = file_len(&wal_path(path));
    let page_size = conn
        .query_row("PRAGMA page_size", [], |row| row.get::<_, u64>(0))
        .expect("page size");
    let auto_checkpoint_pages = conn
        .query_row("PRAGMA wal_autocheckpoint", [], |row| row.get::<_, u64>(0))
        .expect("autocheckpoint");
    let (_busy, wal_frames, checkpointed_frames) = conn
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .expect("passive checkpoint");
    IoSnapshot {
        db_bytes,
        wal_bytes,
        page_size,
        wal_frames,
        checkpointed_frames,
        auto_checkpoint_pages,
    }
}

fn wal_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", path.display()))
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn measure_serialization(fixture: &Fixture, rounds: usize) -> f64 {
    let iterations = rounds.max(10) * 20;
    let started = Instant::now();
    for _ in 0..iterations {
        for record in &fixture.records {
            black_box(commit_log::serialize_tenant_event_record(record).expect("serialize record"));
            let write = &record.writes[0];
            if let Some(current) = &write.current {
                black_box(serde_json::to_string(&current.fields).expect("serialize fields"));
                black_box(
                    serde_json::to_string(&current.typed_fields).expect("serialize typed fields"),
                );
                black_box(serde_json::to_string(&current.fields).expect("serialize fields twice"));
                black_box(
                    serde_json::to_string(&current.typed_fields)
                        .expect("serialize typed fields twice"),
                );
            }
        }
    }
    started.elapsed().as_secs_f64() / iterations as f64
}

fn measure_connection_costs(rounds: usize) -> (f64, f64, f64) {
    let iterations = rounds.max(10) * 10;
    let dir = TempDir::new().expect("connection tempdir");
    let path = dir.path().join("connection.sqlite3");
    let initialized = Connection::open(&path).expect("open initialized database");
    configure_connection(&initialized);
    initialized
        .execute_batch(sqlite_init_sql())
        .expect("initialize connection database");
    drop(initialized);

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(Connection::open(&path).expect("plain connection open"));
    }
    let open_micros = started.elapsed().as_secs_f64() * 1_000_000.0 / iterations as f64;

    let started = Instant::now();
    for _ in 0..iterations {
        let conn = Connection::open(&path).expect("initialized connection open");
        configure_connection(&conn);
        conn.execute_batch(sqlite_init_sql())
            .expect("production-equivalent initialize");
        black_box(conn);
    }
    let initialized_micros = started.elapsed().as_secs_f64() * 1_000_000.0 / iterations as f64;

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(SqliteTenantStore::open(&path).expect("production store open"));
    }
    let store_micros = started.elapsed().as_secs_f64() * 1_000_000.0 / iterations as f64;
    (open_micros, initialized_micros, store_micros)
}

fn sqlite_build_identity() -> (String, String, String) {
    let conn = Connection::open_in_memory().expect("open SQLite identity connection");
    let source_id = conn
        .query_row("SELECT sqlite_source_id()", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("SQLite source id");
    let cipher_version = conn
        .query_row("PRAGMA cipher_version", [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "not reported".to_string());
    (rusqlite::version().to_string(), cipher_version, source_id)
}

fn summarize(samples: &[Sample]) -> Summary {
    let throughputs = samples
        .iter()
        .map(|sample| MUTATIONS as f64 / sample.seconds)
        .collect::<Vec<_>>();
    let mean = throughputs.iter().sum::<f64>() / throughputs.len() as f64;
    let mut sorted = throughputs.clone();
    sorted.sort_by(f64::total_cmp);
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };
    let variance = if throughputs.len() > 1 {
        throughputs
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (throughputs.len() - 1) as f64
    } else {
        0.0
    };
    let std_dev = variance.sqrt();
    let t_critical = student_t_critical_95(throughputs.len().saturating_sub(1));
    let margin = t_critical * std_dev / (throughputs.len() as f64).sqrt();
    Summary {
        mean_mutations_per_second: mean,
        median_mutations_per_second: median,
        ci_low: mean - margin,
        ci_high: mean + margin,
        cv_percent: if mean == 0.0 {
            0.0
        } else {
            std_dev / mean * 100.0
        },
        mean_seconds: samples.iter().map(|sample| sample.seconds).sum::<f64>()
            / samples.len() as f64,
        io: samples
            .iter()
            .map(|sample| sample.io)
            .reduce(IoSnapshot::fieldwise_max)
            .expect("at least one sample"),
        throughputs,
    }
}

fn student_t_critical_95(degrees_of_freedom: usize) -> f64 {
    const TWO_SIDED_95_PERCENT: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    assert!(
        (1..=TWO_SIDED_95_PERCENT.len()).contains(&degrees_of_freedom),
        "unsupported Student-t degrees of freedom: {degrees_of_freedom}"
    );
    TWO_SIDED_95_PERCENT[degrees_of_freedom - 1]
}

fn render_lane(report: &mut String, name: &str, summary: &Summary, model: LaneModel) {
    let rate = summary.mean_mutations_per_second / model.logical_mutations as f64;
    let statements = if model.sql_statements == 0 {
        "—".to_string()
    } else {
        format!("{:.0}", model.sql_statements as f64 * rate)
    };
    writeln!(
        report,
        "| {name} | {:.0} | [{:.0}, {:.0}] | {:.0} | {:.1} | {statements} | {:.0} | {:.1} | {:.1} | {:.3} ms |",
        summary.mean_mutations_per_second,
        summary.ci_low,
        summary.ci_high,
        summary.median_mutations_per_second,
        summary.cv_percent,
        model.row_changes as f64 * rate,
        model.transactions as f64 * rate,
        model.sync_commits as f64 * rate,
        summary.mean_seconds * 1_000.0,
    )
    .unwrap();
}

fn render_io(report: &mut String, name: &str, io: IoSnapshot) {
    writeln!(
        report,
        "| {name} | {} | {} | {} | {} | {} | {} |",
        io.db_bytes,
        io.wal_bytes,
        io.page_size,
        io.wal_frames,
        io.checkpointed_frames,
        io.auto_checkpoint_pages,
    )
    .unwrap();
}

fn render_samples(report: &mut String, name: &str, summary: &Summary) {
    let samples = summary
        .throughputs
        .iter()
        .map(|sample| format!("{sample:.3}"))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(report, "- **{name}:** {samples}").unwrap();
}
