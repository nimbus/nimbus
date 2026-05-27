status: done
date: 2026-05-27
phase: CST7

# CST7 Diagnostics And Summary Proof

## Decision

Nimbus exposes table identity through read-only diagnostics, not through mutable
table-catalog helpers.

The public storage DTO is:

`TableIdentityDiagnostic { table_name, table_id, state, backend_layout, document_count, summary_status }`

`TableCatalog*` helper types remain crate-private/test-only internals.

## Implemented Surface

- `TableIdentityDiagnostic` reports the public table name, stable `TableId`,
  lifecycle `TableState`, backend physical-layout posture, document count when
  available, and explicit summary status.
- `TableBackendLayout` distinguishes redb keyspace layout,
  shared-documents-by-table-id SQL layout, and libSQL replica shared layout.
- `TableSummaryStatus` distinguishes exact document counts from unsupported
  summaries.
- redb diagnostics count documents by physical `table_id` key prefix.
- SQLite diagnostics count documents by `documents.table_id`.
- Postgres and MySQL snapshots expose diagnostics from their repeatable-read
  snapshot data.
- libSQL replica diagnostics reuse the active local SQLite cache and report the
  libSQL replica layout.

## Non-Exposure Proof

- `crates/nimbus-storage/src/lib.rs` exports `TableIdentityDiagnostic`,
  `TableIdentitySnapshotEntry`, `TableBackendLayout`, and `TableSummaryStatus`.
- It does not export `TenantTableCatalog`, `TableCatalogEntry`, or
  `TableCatalogKey`.
- The mutable catalog helper types in `table_identity.rs` are `#[cfg(test)]`
  and `pub(crate)`, so app developers and operators cannot construct catalog
  mutations through the storage public API.

## Verification

- `cargo check -p nimbus-storage --all-targets` passed.
- `cargo test -p nimbus-storage table_identity_diagnostics --lib`: 2 passed.
