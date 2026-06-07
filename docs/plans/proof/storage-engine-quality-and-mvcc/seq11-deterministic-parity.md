# SEQ11 Deterministic Parity

status: done

## Summary

`SEQ11` adds deterministic canonical digest parity for generated MVCC histories
across the embedded redb and SQLite backends. The test uses a shared manual
clock, stable `TableId`, shared document ids, and the same generated history so
backend divergence shows up as a real semantic mismatch instead of random test
noise.

The parity gate compares:

- latest materialized snapshot canonical fingerprints;
- selected midpoint and final PITR archive target fingerprints;
- restored replay fingerprints from the PITR archive;
- CDC/changefeed document-write sequence cuts from the initial bootstrap cursor.

## Confirmed Bug Fixed

The first parity run found a real redb/SQLite divergence: redb direct updates
patched fields without advancing `Document.update_time`, while SQLite,
Postgres, MySQL, and libSQL update paths advanced `update_time` from the
transaction clock. This caused identical generated histories to produce
different canonical snapshot fingerprints.

The redb direct update path now uses `Document::set_field(...)` for patch
application and sets `document.update_time = self.clock.now()` before
validation, persistence, and commit-log recording.

## Implementation Anchors

- `crates/nimbus-storage/src/tests/generated_history.rs`
  - `canonical_digest_generated_history_matches_redb_sqlite_pitr_cdc_and_rebuild_paths`
  - `collect_changefeed_document_sequences`
  - `export_point_in_time_restore_archive`
  - `import_point_in_time_restore_archive`
  - `stream_changefeed`
- `crates/nimbus-storage/src/store/journal_snapshot.rs`
  - `MaterializedJournalSnapshot::canonical_fingerprint`
- `crates/nimbus-storage/src/store/write/direct.rs`
  - redb direct update path advances `update_time` with the transaction clock.

## Verification

- `cargo test -p nimbus-storage canonical_digest_generated_history -- --nocapture`
  - result: `1 passed, 0 failed`
- `cargo test -p nimbus-storage generated_history -- --nocapture`
  - result: `9 passed, 0 failed`, `2 ignored`
- `cargo check -p nimbus-storage`
  - result: passed
