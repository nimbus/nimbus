# SEQ4 Versioned Indexes

status: done

## Scope

SEQ4 is complete. This proof records the core oracle,
redb/SQLite/Postgres/MySQL/libSQL physical `index_versions`, and historical
index scan routing for equality, prefix, range, composite range, and
cursor-bound pagination across the embedded and provider read surfaces. The
focused historical-index tests compare indexed results against a full-scan
oracle over historical document versions. Live external-provider conformance
evidence now covers Postgres, MySQL, and libSQL without fixture skip paths.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-core/src/versioned_registry.rs`
- `crates/nimbus-core/src/document_history.rs`
- `crates/nimbus-storage/src/index/keyspace.rs`
- `crates/nimbus-storage/src/index/encoding.rs`
- `crates/nimbus-storage/src/index/scan/read.rs`
- `crates/nimbus-storage/src/store/schema_rewrite.rs`
- `crates/nimbus-storage/src/store/index_versions.rs`
- `crates/nimbus-storage/src/sqlite/index_versions.rs`
- `crates/nimbus-storage/src/index/bounds.rs`
- `crates/nimbus-storage/src/index/keyspace.rs`
- `crates/nimbus-storage/src/store/journal.rs`
- `crates/nimbus-storage/src/sqlite/journal.rs`
- `crates/nimbus-storage/src/postgres/index_versions.rs`
- `crates/nimbus-storage/src/postgres/write.rs`
- `crates/nimbus-storage/src/postgres/backend.rs`
- `crates/nimbus-storage/src/postgres/config.rs`
- `crates/nimbus-storage/src/mysql/index_versions.rs`
- `crates/nimbus-storage/src/mysql/write.rs`
- `crates/nimbus-storage/src/mysql/backend.rs`
- `crates/nimbus-storage/src/libsql/index_versions.rs`
- `crates/nimbus-storage/src/libsql/read.rs`
- `crates/nimbus-storage/src/libsql/write.rs`
- `crates/nimbus-storage/src/libsql/backend.rs`
- `crates/nimbus-storage/src/libsql/remote.rs`

## Preflight Evidence

| Area | Evidence |
| --- | --- |
| Core historical index oracle | `crates/nimbus-core/src/index_history.rs` defines `HistoricalIndexHistory`, `HistoricalIndexVersion`, `HistoricalIndexQuery`, `HistoricalIndexTuple`, and `HistoricalIndexCursor`. |
| Visibility intervals | The oracle builds `visible_from` plus exclusive `visible_until` intervals from `TenantEventRecord` document writes. Updates close the previous tuple and open the new tuple at the same sequence; deletes close the previous tuple without adding a new row. |
| Registry-driven identity | The oracle resolves maintained/queryable index definitions through `VersionedRegistry` and `HistoricalReadShape`, so scans are keyed by stable `TableId`, stable `IndexId`, and the selected historical read sequence. |
| Ordered tuple semantics | `HistoricalIndexScalar` and `HistoricalIndexNumberKey` define a backend-portable scalar ordering for null, boolean, number, and string values. Number ordering follows the same sortable IEEE-754 transform as the existing storage index encoder. |
| Pagination identity | `HistoricalIndexCursor` binds read snapshot, table id, index id, query shape, policy snapshot, storage format generation, last tuple, and last document id. A mismatched cursor fails closed with `HistoricalReadErrorKind::CursorMismatch`. |

## Embedded Physical Storage Evidence

| Area | Evidence |
| --- | --- |
| redb physical index-version rows | `crates/nimbus-storage/src/store/index_versions.rs` adds an `INDEX_VERSIONS` keyspace keyed by the existing ordered index key plus the opening commit sequence. Values store the document id and `visible_from` / exclusive `visible_until` interval. |
| redb atomicity | `store/journal.rs` records index-version rows from the same `WriteOp`s and `SequenceNumber` as document-version rows before appending the tenant event record, keeping document writes, latest index effects, version rows, and commit log inside the same redb write transaction. |
| SQLite physical index-version rows | `SQLITE_INIT_SQL` adds `index_versions(table_id, index_id, encoded_tuple, document_id, visible_from, visible_until)` plus a lookup index. `crates/nimbus-storage/src/sqlite/index_versions.rs` uses the same `encoded_index_tuple_for_document` helper as redb to preserve backend-portable tuple ordering. |
| SQLite atomicity | `sqlite/journal.rs` records SQLite index-version rows from the same `WriteOp`s and `SequenceNumber` as document-version rows before inserting the commit-log row, keeping the version history and latest-row mutation in the same SQLite transaction. |
| Format behavior | `CURRENT_INDEX_VERSION_STORAGE_FORMAT` and `INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY` add a dedicated index-version storage marker. Embedded tests corrupt the marker to an unknown future version and verify fail-closed reads. |
| Durable recovery | redb and SQLite durable-only journal records do not materialize index versions before recovery. Recovery records closed/open visibility intervals while replaying the durable document writes. |

## Embedded Historical Routing Evidence

| Area | Evidence |
| --- | --- |
| Core cursor API | `crates/nimbus-core/src/index_history.rs` exposes `HistoricalIndexTuple::from_document` plus `HistoricalIndexCursor::new`, `validate_context`, `last_tuple`, and `last_document_id`, so physical storage uses the same cursor identity as the oracle. |
| redb historical index scans | `TenantReadSnapshot::historical_index_scan_*_cancellable` in `crates/nimbus-storage/src/store/index_versions.rs` scans visible `index_versions` intervals by ordered index-key bounds, validates the index-version format marker, hydrates documents through `document_versions` at the same read sequence, and returns cursor-bound pages using the SEQ2 `HistoricalReadShape`. |
| SQLite historical index scans | `SqliteReadSnapshot::historical_index_scan_*_cancellable` in `crates/nimbus-storage/src/sqlite/index_versions.rs` scans visible `index_versions` rows by `encoded_tuple` BLOB bounds, validates the index-version format marker, hydrates documents through SQLite `document_versions` at the same read sequence, and returns cursor-bound pages using the SEQ2 `HistoricalReadShape`. |
| Query shapes covered | Embedded tests cover equality, single-field range, composite exact-prefix range, stable page resume, and cursor mismatch fail-closed behavior for redb and SQLite. |

## Provider Physical Storage Evidence

| Area | Evidence |
| --- | --- |
| Postgres physical index-version rows | `crates/nimbus-storage/src/postgres/config.rs` adds `index_versions(table_id, index_id, encoded_tuple, document_id, visible_from, visible_until)` with a primary key over the stable index tuple identity plus opening sequence and a visibility lookup index. |
| Postgres atomicity | `postgres/write.rs` and `postgres/backend.rs` call `record_index_versions_for_events_in_session` / `record_index_versions_for_writes_in_session` in the same SQL transaction as document-version rows, current document/index effects, and tenant-event append. |
| MySQL physical index-version rows | `crates/nimbus-storage/src/mysql/backend.rs` adds `index_versions` with full `encoded_tuple` bytes plus `encoded_tuple_hash BINARY(32)` in the primary key so InnoDB key-length limits do not truncate ordered tuple identity. |
| MySQL atomicity | `mysql/write.rs` and `mysql/backend.rs` call `record_index_versions_for_events_in_session` / `record_index_versions_for_writes_in_session` in the same SQL transaction as document-version rows, current document/index effects, and tenant-event append. |
| libSQL physical index-version rows | `crates/nimbus-storage/src/libsql/index_versions.rs` writes remote `index_versions` rows with the same encoded tuple helper as embedded SQLite and stores the dedicated index-version format marker in remote metadata. |
| libSQL replica cache | `crates/nimbus-storage/src/libsql/remote.rs` copies remote `index_versions` into the local SQLite replica cache during full snapshot materialization, preserving historical index intervals for cache-side verification. |
| Provider durable recovery | Postgres, MySQL, and libSQL provider tests assert durable-only journal records do not materialize index versions before recovery and do materialize closed/open intervals during recovery. |

## Provider Historical Routing Evidence

| Area | Evidence |
| --- | --- |
| Postgres historical index scans | `PostgresTenantStore::historical_index_scan_*_cancellable` in `crates/nimbus-storage/src/postgres/index_versions.rs` validates the index-version format marker, scans visible provider-owned `index_versions` rows by encoded tuple bounds, hydrates each document through Postgres `document_versions` at the same read sequence, and returns SEQ2 cursor-bound pages. |
| MySQL historical index scans | `MySqlTenantStore::historical_index_scan_*_cancellable` in `crates/nimbus-storage/src/mysql/index_versions.rs` uses full `encoded_tuple` range predicates for historical reads while retaining `encoded_tuple_hash` only for primary-key length safety, then hydrates through MySQL `document_versions` at the same read sequence. |
| libSQL historical index scans | `LibsqlReplicaTenantStore::historical_index_scan_*_cancellable` in `crates/nimbus-storage/src/libsql/read.rs` uses the existing current-query freshness barrier and delegates through the refreshed SQLite replica cache, which now includes remote `document_versions` and `index_versions` from snapshot materialization. |
| Provider query shapes covered | Postgres/MySQL/libSQL provider tests cover equality, single-field range, composite exact-prefix range, stable page resume, and cursor mismatch fail-closed behavior. Docker-backed live fixture runs now execute those lanes without skip paths. |

## Full-Scan Oracle Conformance Evidence

| Area | Evidence |
| --- | --- |
| Oracle source | redb, SQLite, Postgres, MySQL, and libSQL historical-index tests derive expected titles by reading each document in the written test corpus through `get_document_version_at(...)` at the same retained sequence as the indexed read. |
| Lifecycle churn | Equality/range tests compare indexed results with the full-scan oracle at insert, update, and delete sequences, including stale-rank and tombstone visibility. |
| Composite ordering | Composite-prefix tests compare indexed prefix and exact-rank results with a full-scan oracle sorted by `(rank, title)`, while pagination still verifies cursor resume and mismatched cursor fail-closed behavior. |
| Backend coverage | Oracle helpers are local to `crud_and_journal.rs`, `sqlite_foundation/journal.rs`, `postgres_provider.rs`, `mysql_provider.rs`, and `libsql_provider.rs`, so every backend read surface carries the same model check. |

## Confirmed Bug Fixed

The first Docker-backed live SEQ4 run found that
`libsql_index_versions_are_materialized_during_durable_recovery` could fail
after `replace_table_schema(...)` because `LibsqlReplicaTenantStore` table
identity diagnostics read the active local replica cache without waiting for
the schema-write freshness barrier. `crates/nimbus-storage/src/libsql/read.rs`
now routes `table_identity_diagnostics()` through `current_query_cache_store()`
so diagnostics use the same freshness guarantee as current query reads.

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-core index_history -- --nocapture` | Passed: `6 passed, 0 failed`. Covers update/delete visibility intervals, prefix scans, range scans, query mismatch, policy snapshot drift, storage format drift, non-queryable index rejection, and zero format-generation fail-closed behavior. |
| `cargo test -p nimbus-storage libsql_index_versions_are_materialized_during_durable_recovery -- --nocapture` | Passed after the libSQL diagnostic cache-freshness fix: `1 passed, 0 failed`, `302 filtered out`. |
| `cargo test -p nimbus-storage index_versions -- --nocapture` | Passed with Docker-backed live external-provider fixtures: `12 passed, 0 failed`, `291 filtered out`. Covers redb, SQLite, Postgres, MySQL, and libSQL direct insert/update/delete visibility intervals, durable recovery materialization, no pre-recovery historical visibility, and embedded fail-closed rejection of unknown future index-version storage format markers. |
| `cargo test -p nimbus-storage historical_index -- --nocapture` | Passed with Docker-backed live external-provider fixtures: `10 passed, 0 failed`, `293 filtered out`. Covers redb, SQLite, Postgres, MySQL, and libSQL historical equality, range, composite prefix-range, pagination, cursor mismatch, and full-scan document-version oracle conformance over physical `index_versions` plus `document_versions`. |
| `NIMBUS_TEST_POSTGRES_URL="host=/private/tmp/nimbus-seq3-pg.YHGBJP port=55432 user=postgres dbname=postgres" cargo test -p nimbus-storage postgres_index_versions -- --nocapture` | Passed against an explicit local Postgres fixture: `2 passed, 0 failed`, `278 filtered out`. Covers provider-owned Postgres `index_versions` rows for direct write history and durable recovery materialization. |
| `NIMBUS_TEST_POSTGRES_URL="host=/private/tmp/nimbus-seq3-pg.YHGBJP port=55432 user=postgres dbname=postgres" cargo test -p nimbus-storage postgres_historical_index -- --nocapture` | Passed against an explicit local Postgres fixture after full-scan oracle assertions landed: `2 passed, 0 failed`, `288 filtered out`. Covers provider-owned Postgres historical equality, range, composite prefix-range, pagination, cursor mismatch, and document-version oracle conformance over physical `index_versions` plus `document_versions`. |
| `cargo check -p nimbus-storage` | Passed after provider index-version modules, write-path hooks, and tests landed. |
