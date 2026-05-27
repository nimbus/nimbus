---
status: done
phase: SATH1
---

# SATH1 Tenant Event Journal

Nimbus now serializes `TenantEventRecord` as the durable tenant history record.
`TenantEventKind` covers document writes, schema changes, table lifecycle,
index lifecycle, scheduled execution markers, trigger-delivery cursors, and
barriers.

Evidence:

- `crates/nimbus-core/src/mutation.rs` defines `TenantEventRecord` and
  `TenantEventKind`.
- `crates/nimbus-storage/src/commit_log.rs` serializes/deserializes tenant
  event records.
- Storage write transactions record tenant events for schema, lifecycle,
  scheduler, trigger cursor, and document writes.
- `cargo test -p nimbus-storage tenant_event --lib`: 6 passed.
