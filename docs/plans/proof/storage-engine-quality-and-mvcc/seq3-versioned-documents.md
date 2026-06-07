---
status: done
phase: SEQ3
plan: docs/plans/storage-engine-quality-and-mvcc-plan.md
updated: 2026-06-07
---

# SEQ3 Versioned Documents

SEQ3 designs and implements versioned document storage beside the current-row
cache. Historical document point reads must use the SEQ1 resolved read snapshot
and the SEQ2 read-shape bundle, while latest reads stay on the current fast
path.

## Scope

SEQ3 must add:

- version records for document insert, update, and delete histories
- efficient latest-at-or-before lookup by stable `TableId`, `DocumentId`, and
  `CommitSequence`
- parity between current-row reads and the latest visible version
- storage format gates for document-version layout changes
- diagnostics evidence for document version counts and retained range
- tests proving historical point reads across create/update/delete histories
  and table identity replacement

SEQ3 does not implement historical index scans or pagination. SEQ4 owns
versioned index storage after document point-read semantics are proven.

## Read-Before-Edit Checklist

Before editing document storage, read:

- `crates/nimbus-storage/src/store/journal.rs`
- `crates/nimbus-storage/src/sqlite/journal.rs`
- `crates/nimbus-storage/src/postgres/write.rs`
- `crates/nimbus-storage/src/mysql/write.rs`
- `crates/nimbus-storage/src/libsql/write.rs`
- `crates/nimbus-storage/src/store/read.rs`
- `crates/nimbus-storage/src/sqlite/read.rs`
- `crates/nimbus-storage/src/postgres/read.rs`
- `crates/nimbus-storage/src/mysql/read.rs`
- `crates/nimbus-storage/src/tests/crud_and_journal.rs`
- generated-history and provider parity tests that already cover durable
  journal replay and table identity

## Starting Decisions From SEQ2

| Topic | Decision |
| --- | --- |
| Read coordinate | Historical reads use `HistoricalReadSnapshot` rather than a caller timestamp alone. |
| Metadata shape | Historical reads first resolve `HistoricalReadShape` from `VersionedRegistry`; document history lookup is keyed by that shape's stable `TableId`. |
| Latest path | Current-row reads remain on the existing document store path unless a phase proves a replacement is faster and equally correct. |
| Deletions | Historical delete tombstones must distinguish "not yet created", "visible document", and "deleted at or before read snapshot". |
| Format behavior | Unknown old/future document-version storage format generations fail closed. |

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Canonical document-version oracle | `crates/nimbus-core/src/document_history.rs` defines `DocumentVersionHistory` and `DocumentVersion`, keyed by stable `TableId`, `DocumentId`, and `CommitSequence`. |
| Historical point reads | `DocumentVersionHistory::get_at` consumes a SEQ2 `HistoricalReadShape` and returns the latest version at or before the shape's resolved read snapshot. |
| Tombstones | Delete writes are represented as tombstone versions and return `None` at or after the delete sequence. |
| Table identity isolation | The history key includes stable `TableId`, so a replacement table with the same logical name and document id does not see old table history. |
| Format behavior | Document history format generation `0` fails closed with `HistoricalReadErrorKind::FormatMismatch`. |

## Embedded Physical Storage Evidence

| Area | Evidence |
| --- | --- |
| redb physical document-version rows | `crates/nimbus-storage/src/store/document_versions.rs` adds `document_versions` rows keyed by stable `TableId`, `DocumentId`, and `SequenceNumber`. Direct writes record live payloads or tombstones in the same redb write transaction as current rows and commit-log append. |
| redb durable recovery | `apply_durable_record_in_write_txn` records document versions before replaying current-row mutations. `redb_document_versions_are_materialized_during_durable_recovery` proves durable-only records do not expose history before recovery and do expose insert/update/delete history after recovery. |
| SQLite physical document-version rows | `crates/nimbus-storage/src/sqlite/document_versions.rs` and `SQLITE_INIT_SQL` add a `document_versions` table keyed by `(table_id, id, commit_sequence)`. The table intentionally has no `table_catalog` foreign key because table lifecycle hard-delete and history retention/GC are separate responsibilities. |
| SQLite durable recovery | `append_tenant_event_record` records direct-write history in the same SQLite transaction as current rows and the commit log. `apply_durable_record_in_conn` records versions before replaying durable current-row mutations. `sqlite_document_versions_are_materialized_during_durable_recovery` proves durable-only rows do not materialize historical versions before recovery. |
| Postgres physical document-version rows | `crates/nimbus-storage/src/postgres/document_versions.rs` and tenant schema initialization add provider-owned `document_versions` rows keyed by `(table_id, id, commit_sequence)`. Direct commits record versions before commit-log append, and durable recovery records versions before replaying current-row mutations. Fixture-gated tests: `postgres_document_versions_track_direct_write_history` and `postgres_document_versions_are_materialized_during_durable_recovery`. A live explicit local Postgres fixture caught and then verified the generated DDL tokenization fix for the `document_versions` live/tombstone check constraint. |
| MySQL physical document-version rows | `crates/nimbus-storage/src/mysql/document_versions.rs` and tenant database initialization add provider-owned `document_versions` rows with `BIGINT UNSIGNED` sequence/time coordinates and live/tombstone payload checks. Direct commits and durable replay share the same document-version helpers. Fixture-gated tests: `mysql_document_versions_track_direct_write_history` and `mysql_document_versions_are_materialized_during_durable_recovery`. Generated-DDL regression coverage now also checks that MySQL's document-version boolean predicates do not collapse into invalid `TRUEAND`/`FALSEAND` tokens. |
| libSQL physical document-version rows | `crates/nimbus-storage/src/libsql/document_versions.rs` records remote-primary document versions for direct commits and durable replay. `crates/nimbus-storage/src/libsql/remote.rs` copies `document_versions` into the local SQLite replica snapshot cache so refreshes do not drop history. Fixture-gated tests: `libsql_document_versions_track_direct_write_history_and_snapshot_cache` and `libsql_document_versions_are_materialized_during_durable_recovery`. |
| Durable storage-format gates | `DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY` records the document-version storage format in provider metadata. redb, SQLite, Postgres, MySQL, and libSQL validate that marker before historical reads and before recording new version rows. Unknown future generations fail closed; embedded tests `redb_document_versions_reject_unknown_future_storage_format` and `sqlite_document_versions_reject_unknown_future_storage_format` corrupt the real metadata marker and prove rejection. |
| Version diagnostics | `StorageHealthDiagnostic` now includes `DocumentVersionStorageDiagnostic` with the document-version storage format marker, row count, minimum sequence, and maximum sequence. Embedded tests `redb_document_versions_storage_diagnostic_reports_format_and_range` and `sqlite_document_versions_storage_diagnostic_reports_format_and_range` prove the reported range across insert/update/delete histories. Fixture-gated provider tests `postgres_document_versions_storage_diagnostic_reports_format_and_range`, `mysql_document_versions_storage_diagnostic_reports_format_and_range`, and `libsql_document_versions_storage_diagnostic_reports_format_and_range` exercise the same provider diagnostic method when live fixtures are available. |
| Latest-path parity | Current reads still use existing current-row caches/tables for each backend. Focused tests assert the latest delete remains invisible through current `get` while historical lookup returns insert/update payloads and a delete tombstone at the matching sequences. |

SEQ3 is complete. Embedded redb/SQLite and Postgres/MySQL/libSQL all have
backend-owned physical document-version storage, durable document-version
format metadata gates, and version-count/range diagnostics for point reads.
Live external-provider conformance evidence now covers Postgres, MySQL, and
libSQL; the final Docker-backed `document_versions` lane passed all provider
tests rather than relying on skip-path evidence.

## Initial Verification Plan

- Add focused embedded tests for insert/update/delete historical point reads.
- Add table replacement tests proving old `TableId` document history does not
  leak into the replacement table.
- Add parity tests that current `get` agrees with the latest visible document
  version.
- Extend the SEQ verifier with SEQ3 source/proof/test evidence.

## Verification Evidence So Far

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo test -p nimbus-core document_history -- --nocapture` | Passed: `4 passed, 0 failed`, `116 filtered out`. Covers insert/update/delete point reads, table identity replacement isolation, latest-version parity within the oracle, duplicate sequence rejection, and format-generation fail-closed behavior. |
| `cargo test -p nimbus-storage redb_document_versions -- --nocapture` | Passed: `4 passed, 0 failed`, `261 filtered out`. Covers direct redb insert/update/delete history, durable append/recovery materialization, no pre-recovery historical visibility, document-version diagnostic count/range reporting, and fail-closed rejection of an unknown future document-version storage format marker. |
| `cargo test -p nimbus-storage sqlite_document_versions -- --nocapture` | Passed: `4 passed, 0 failed`, `261 filtered out`. Covers direct SQLite insert/update/delete history, durable append/recovery materialization, no pre-recovery historical visibility, document-version diagnostic count/range reporting, and fail-closed rejection of an unknown future document-version storage format marker. |
| `cargo test -p nimbus-storage document_versions -- --nocapture` | Passed with Docker-backed live external-provider fixtures: `17 passed, 0 failed`, `286 filtered out`. Covers redb, SQLite, Postgres, MySQL, and libSQL direct document-version history, durable-recovery materialization, and diagnostic count/range assertions without fixture skip paths. |
| `cargo test -p nimbus-storage tenant_init -- --nocapture` | Passed: `2 passed, 0 failed`, `265 filtered out`. Covers generated Postgres and MySQL tenant-init SQL for the document-version check-constraint tokenization regression caught by live Postgres. |
| `NIMBUS_TEST_POSTGRES_URL="host=/private/tmp/nimbus-seq3-pg.YHGBJP port=55432 user=postgres dbname=postgres" cargo test -p nimbus-storage postgres_document_versions -- --nocapture` | Passed after fixing generated DDL tokenization: `3 passed, 0 failed`, `262 filtered out`. A temporary local Postgres 17.9 fixture first exposed invalid `TRUEAND`/`FALSEAND` predicates in the `document_versions` check constraint; the rerun proves Postgres direct history, durable recovery, and diagnostics on a live provider. |
| `cargo check -p nimbus-core` | Passed. |
| `cargo check -p nimbus-storage` | Passed. |
