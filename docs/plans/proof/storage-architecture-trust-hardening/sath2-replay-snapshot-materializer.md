---
status: done
phase: SATH2
---

# SATH2 Replay Snapshot Materializer

redb and SQLite replay now consume typed tenant events instead of reconstructing
metadata state from document writes only. Snapshot plus journal-tail rebuild
continues to match live state, including schema, lifecycle, scheduled execution,
and trigger-delivery cursor transitions.

Evidence:

- `crates/nimbus-storage/src/store/journal.rs` applies typed tenant events.
- `crates/nimbus-storage/src/sqlite/journal.rs` applies typed tenant events.
- `store::journal::tests::redb_tenant_event_journal_replays_mixed_history`
  verifies redb mixed replay.
- `tests::sqlite_foundation::journal::sqlite_tenant_event_journal_replays_mixed_history`
  verifies SQLite mixed replay.
- `cargo test -p nimbus-storage --lib`: 245 passed, 2 ignored.
