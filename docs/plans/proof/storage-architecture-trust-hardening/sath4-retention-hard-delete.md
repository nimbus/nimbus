---
status: done
phase: SATH4
---

# SATH4 Retention Hard Delete

Destructive table cleanup now consults a typed `RetentionFloor` before store
APIs execute hard delete. Participants can pin a table identity or global
sequence for exported snapshots, transaction sessions, journal consumers,
embedded replicas, shadow materializers, or CDC/subscription consumers.

Evidence:

- `crates/nimbus-storage/src/retention.rs` defines `RetentionFloor`,
  `RetentionParticipant`, `RetentionPin`, and `HardDeleteDecision`.
- redb, SQLite, Postgres, MySQL, and libSQL hard-delete store APIs call the
  retention gate before physical cleanup.
- `hard_delete_denied_while_retention_floor_pins_table_identity`
- `retention_floor_survives_crash_recovery`
- `cargo test -p nimbus-storage --lib`: 245 passed, 2 ignored.
