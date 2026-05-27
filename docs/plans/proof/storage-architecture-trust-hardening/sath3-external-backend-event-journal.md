---
status: done
phase: SATH3
---

# SATH3 External Backend Event Journal

Postgres, MySQL, and libSQL now append typed tenant-event records and replay
mixed histories through backend-owned SQL layouts in one transaction. External
replay applies table lifecycle, schema/index state, document writes, scheduler
dedup markers, and trigger-delivery cursors.

Evidence:

- `crates/nimbus-storage/src/postgres/backend.rs`
- `crates/nimbus-storage/src/mysql/backend.rs`
- `crates/nimbus-storage/src/libsql/backend.rs`
- `postgres_tenant_event_journal_replays_mixed_history`
- `mysql_tenant_event_journal_replays_mixed_history`
- `libsql_tenant_event_journal_replays_mixed_history`
- `cargo test -p nimbus-storage tenant_event --lib`: 6 passed.
