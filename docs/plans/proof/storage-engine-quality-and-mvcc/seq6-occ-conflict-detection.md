# SEQ6 Transaction Sessions And Pending Writes

status: done

## Scope

SEQ6 integrates MVCC visibility with the existing server-owned transaction
session and `MutationExecutionUnit` path. It does not introduce a parallel OCC
engine. Transaction sessions still pin a begin `TenantPersistenceSnapshot`, use
the same read/write dependency sets, and commit through the existing
`apply_execution_unit_batch_with_origin(...)` storage transaction. SEQ6 adds an
explicit session staging API so pending writes live in the engine-owned
execution unit and are visible only through the active transaction token until
commit.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-core/src/transaction.rs`
- `crates/nimbus-core/src/error.rs`
- `crates/nimbus-engine/src/service/transactions.rs`
- `crates/nimbus-engine/src/service/execution_units/mod.rs`
- `crates/nimbus-engine/src/service/execution_units/reads.rs`
- `crates/nimbus-engine/src/service/execution_units/staging.rs`
- `crates/nimbus-engine/src/service/execution_units/batch.rs`
- `crates/nimbus-engine/src/service/execution_units/commit.rs`
- `crates/nimbus-engine/src/service/execution_units/tests.rs`
- `crates/nimbus-mongodb/src/commands/session.rs`
- `crates/nimbus-mongodb/src/commands/crud/mod.rs`
- `crates/nimbus-mongodb/src/commands/crud/filter.rs`
- `crates/nimbus-dynamodb/src/commands/transact.rs`
- `crates/nimbus-dynamodb/src/error.rs`
- `crates/nimbus-firebase/src/operations.rs`
- `crates/nimbus-firebase/src/errors.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Engine-owned pending writes | `Service::stage_atomic_write_batch_in_transaction(...)` stages an `AtomicWriteBatch` into the existing transaction session execution unit without committing it. |
| Transaction queries use the same overlay | `Service::query_documents_in_transaction(...)` routes simple `Query` reads through the pinned execution unit, matching the existing structured and point-read transaction paths. |
| Read-only sessions fail closed | Read-only transaction sessions reject staged writes with `InvalidInput` and remain rollbackable after the rejected stage attempt. |
| OCC remains on the existing path | Staged writes are still prepared by `MutationExecutionUnit::stage_atomic_write_batch(...)`, dependencies are tracked by existing point/query loaders, and `MutationExecutionUnit::commit(...)` continues to call `ensure_schema_unchanged(...)` and `ensure_no_conflicts(...)` before the single storage transaction. |
| MongoDB no longer buffers outside the engine | `SessionState` no longer owns a local `buffered_writes` list. Mongo CRUD writes with an active `lsid` now call `Service::stage_atomic_write_batch_in_transaction(...)`, Mongo finds/updates/deletes route query matching through `Service::query_documents_in_transaction(...)` / `get_document_in_transaction(...)` when an active token exists, and `findAndModify` return-new reads use the same transaction overlay after staged updates or upserts. |
| DynamoDB and Firebase error surfaces cover SEQ errors | DynamoDB maps `HistoricalRead` errors to `ValidationException`; Firebase maps them to `INVALID_ARGUMENT` / gRPC `InvalidArgument`, preserving typed fail-closed behavior for unsupported or expired historical features. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-engine transaction_session -- --nocapture` | Passed: `9 passed, 0 failed`, `266 filtered out`. Covers begin-snapshot point reads, staged write read-your-writes, outside invisibility before commit, conflict on concurrent document change, read-only stage rejection, expiry, rollback, principal mismatch, final-batch commit, and tracked-read conflict reporting. |
| `cargo test -p nimbus-mongodb transaction_ -- --nocapture` | Passed: `11 passed, 0 failed`, `254 filtered out`. Covers Mongo session lifecycle plus engine-staged transaction writes visible through the same `lsid`, `findAndModify` return-new update/upsert overlay reads, outside invisibility before commit, committed-on-commit behavior, and discarded-on-abort behavior. |
| `cargo test -p nimbus-dynamodb transact -- --nocapture` | Passed: DynamoDB transact unit lane `10 passed, 0 failed`, failure-injection lane `1 passed, 0 failed`. Covers repeatable `TransactGet`, atomic `TransactWrite`, condition cancellation without partial writes, stream records committed with data writes, and transaction conflict mapping. |
| `cargo test -p nimbus-dynamodb maps_each_core_error_class_to_the_expected_dynamodb_code -- --nocapture` | Passed: `1 passed, 0 failed`. Covers the new `HistoricalRead` error mapping to `ValidationException`. |
| `cargo test -p nimbus-firebase transaction -- --nocapture` | Passed: `7 passed, 0 failed`, `35 filtered out`. Covers Firestore transaction request parsing, transaction selectors, commit transaction bytes, batch-get transaction bytes, and unsupported aggregation transaction selector behavior. |
| `cargo test -p nimbus-firebase firebase_rest_error_maps_full_core_error_surface -- --nocapture` | Passed: `1 passed, 0 failed`. Covers the new `HistoricalRead` error mapping to `INVALID_ARGUMENT`. |
| `cargo test -p nimbus-server firebase_run_query_supports_transaction_selector_with_pinned_snapshot -- --nocapture` | Blocked by the pre-existing `nimbus-assets` UI build prerequisite: missing `packages/nimbus-ui/dist/index.html`. The same prerequisite also blocked `firebase_batch_get_accepts_active_transaction_tokens_and_rejects_inactive_ones`. |

## SEQ6 Closeout

SEQ6 is complete for the current engine and adapter surface. Pending writes now
live in the engine-owned transaction execution unit, reads through an active
session token observe that pending overlay, outside readers do not observe it
before commit, and commit still goes through existing OCC conflict detection and
one storage transaction. MongoDB moved off adapter-local buffered writes so its
transaction reads, writes, and `findAndModify` return-new responses share the
engine snapshot/overlay. DynamoDB and Firebase transaction surfaces remain on
the existing session manager and now compile against the full SEQ
historical-read error taxonomy.
