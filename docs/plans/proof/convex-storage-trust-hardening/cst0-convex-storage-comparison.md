# CST0 Convex Storage Comparison

Date: 2026-05-27

## Source Scope

Local Convex source:

- `/Users/jack/src/github.com/get-convex/convex-backend/crates/value/src/table_mapping.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/value/src/document_id.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/common/src/persistence.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/common/src/bootstrap_model/tables.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/table_registry.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/writes.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/reads.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/write_log.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/snapshot_manager.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/indexing/src/index_registry.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/database/src/table_summary.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/sqlite/src/lib.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/postgres/src/sql.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/mysql/src/sql.rs`

Nimbus source compared:

- `crates/nimbus-core/src/types.rs`
- `crates/nimbus-core/src/mutation.rs`
- `crates/nimbus-core/src/dependency.rs`
- `crates/nimbus-storage/src/table_identity.rs`
- `crates/nimbus-storage/src/store/table_catalog.rs`
- `crates/nimbus-storage/src/keys.rs`
- `crates/nimbus-storage/src/index/keyspace.rs`
- `crates/nimbus-storage/src/sqlite.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `crates/nimbus-storage/src/mysql.rs`
- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-server/src/execution/read_tracking/`

## Findings

### Stable Table Identity

Convex has stable internal table identity. `TableMapping` maps active public
names to internal `TabletId` and table numbers, while preserving tablet-to-name
history for inactive/deleted tablets whose names may conflict with active
tables.

Nimbus now has the essential equivalent for current storage needs:
`TableId` plus a per-tenant `table_catalog` and durable `WriteOp.table_id`.
This is a welcome addition, not over-defense. It prevents table-name reuse from
being the only storage identity.

### SQL Physical Layout

Convex SQL persistence uses shared physical tables keyed by internal table
identity. SQLite documents are keyed by `(ts, table_id, id)`, and Postgres/MySQL
use equivalent versioned rows plus split index-key storage.

Nimbus's shared SQL layout keyed by `(table_id, id)` is aligned with that
pattern. Convex does not require UUID-named physical SQL tables as the default
answer.

### Table Lifecycle

Convex models table lifecycle with active, hidden, and deleting states. Hidden
tables support snapshot import while another table with the same public name is
active. Deleting tables stop accepting new documents and are hard-deleted after
retention permits it.

Nimbus currently has stable identity but no explicit table lifecycle. This is
acceptable while there is no explicit table-drop/import/rename API, but it is
the next correctness boundary before those features can be trusted.

### Document Identity

Convex separates public/developer IDs from storage IDs. Developer IDs carry a
table number, internal IDs carry `TabletId`, and resolved IDs bind both views.

Nimbus storage is safe because documents are keyed by `(TableId, DocumentId)`,
but `DocumentId` itself is a plain validated string. The Convex adapter should
validate table-bearing ID semantics at the boundary so an ID for one table
cannot silently target another table in Convex-compatible code.

### Read Dependencies And Subscriptions

Convex tracks reads and writes by internal table/index identity. Write-log
refresh and subscription invalidation check interval overlap on those internal
names.

Nimbus dependency tracking remains table-name based in core/server read
tracking. That is enough before table lifecycle exists, but it must move to
`TableId` before explicit drop/recreate/rename because a same-name new table
must not satisfy old dependencies.

### Index Lifecycle

Convex has an `_index` registry with stable index IDs and pending/enabled
state. It can reason about index metadata dependencies and backfill/activation
separately from public index names.

Nimbus has simpler schema/index metadata. Enterprise-safe online index changes
should add stable index identity and lifecycle before exposing index evolution
that can race with reads, writes, restore, or replay.

### MVCC And History

Convex stores document and index history as timestamped rows, maintains
repeatable snapshots, and uses write logs for conflict detection and
subscription refresh. Nimbus stores latest rows plus a durable commit log.

This is not automatically a Nimbus bug. It is a product/guarantee decision.
CST6 must either adopt a narrow history/repeatable-read guarantee where Nimbus
needs it or document why atomic latest-row storage plus commit-log replay is
the intended contract.

### Diagnostics And Summaries

Convex persists table summaries keyed by table identity. This gives operators
and internal systems stable count/shape/size information.

Nimbus should expose read-only table identity diagnostics and, where cheap and
accurate, table summaries. Mutable table-catalog constructors should remain
internal storage details.

## Adoption Map

| Pattern | Nimbus posture |
| --- | --- |
| Stable table identity | Adopted; CST1 closes proof/debt. |
| Shared SQL tables keyed by table identity | Adopted; CST8 locks matrix tests. |
| Table lifecycle states | Adopt before table drop/import/rename. |
| Table-bearing Convex document IDs | Adopt at Convex boundary and internal resolved-ID layer. |
| TableId-based read dependencies | Adopt before lifecycle features. |
| Stable index identity/lifecycle | Adopt before online index evolution. |
| Full Convex MVCC | Decide explicitly; adopt only if product guarantees require it. |
| Table summaries | Adopt as read-only diagnostics where useful. |

## Immediate Debt Changes

- Mark stale `S-004` and `A-003` done because the table-catalog implementation
  now exists. CST1 still owns the final per-backend proof bundle for that
  already-landed implementation.
- Add CST-owned debt rows for table lifecycle, table-aware document identity,
  table-id dependency tracking, index lifecycle, history/repeatable-read
  posture, and table diagnostics.
