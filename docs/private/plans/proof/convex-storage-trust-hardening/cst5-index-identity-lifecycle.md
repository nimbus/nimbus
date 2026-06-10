status: done
date: 2026-05-27
phase: CST5

# CST5 Index Identity And Lifecycle Proof

## Decision

Nimbus adopted the Convex pattern that an index has a stable logical identity
separate from its public schema name, but narrowed the implementation to the
current Nimbus storage model.

- Public APIs still name indexes by developer-facing `index.name`.
- Core schema now carries `IndexId` plus `IndexState`.
- `IndexState` has `pending`, `backfilling`, `enabled`, and `deleting`.
- Queries only resolve `enabled` indexes.
- Writes maintain `backfilling` and `enabled` indexes so an online backfill can
  catch up without exposing a half-built index to query planning.
- Unchanged public index definitions reconcile to their previous `IndexId`
  during schema replacement. Same public name with different indexed fields gets
  a fresh identity.

This gives Nimbus the enterprise-trust property we wanted from Convex's index
registry without importing Convex's full index table/backfill worker
architecture in this phase.

## Backend Evidence

| Backend | Evidence |
| --- | --- |
| redb | Secondary index keys now use `TableId + IndexId + encoded_value + DocumentId`. Scan paths resolve the public name through stored schema and reject indexes unless `IndexState::Enabled`. Rebuild/maintenance filters on `is_maintained()`. |
| SQLite | Physical SQL index names are derived from `IndexId`, not public table/index names. Query field resolution uses only queryable indexes. |
| Postgres | Physical SQL index names are derived from `IndexId`. Creation skips non-maintained lifecycle states, and query planning resolves only queryable indexes. |
| MySQL | Physical SQL index names are derived from `IndexId`; generated columns are driven by maintained indexes. Query field lookup uses only enabled indexes. |
| libSQL | Remote schema replacement reconciles index metadata before persisting replicated schema JSON. |

## Behavioral Proof

- `IndexDefinition::new(...)` creates an enabled index with a fresh stable
  `IndexId`.
- `IndexDefinition::with_state(...)` allows pending/backfilling/enabled/deleting
  definitions.
- Duplicate `IndexId` values in one table schema are rejected.
- Backfilling indexes are physically maintained but not queryable until state is
  changed to enabled.
- Replacing a schema with the same public index name and fields preserves the
  stored `IndexId`, even when the incoming schema was rebuilt from a fresh
  manifest or fixture.
- Runtime index read dependencies now carry both public `index_name` context and
  stable `index_id`.

## Verification

- `cargo check -p nimbus-core -p nimbus-storage -p nimbus-engine -p nimbus-server --all-targets`
  passed.
- `cargo test -p nimbus-core schema --lib`: 11 passed.
- `cargo test -p nimbus-core dependency --lib`: 9 passed.
- `cargo test -p nimbus-storage index --lib`: 25 passed.
- `cargo test -p nimbus-server read_tracking --lib`: 5 passed.
- `cargo fmt --all --check` passed.
