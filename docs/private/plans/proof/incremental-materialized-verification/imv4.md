# IMV4 Writer-Owned Delta Proof

Date: 2026-08-21.

## Fail-before

Storage had no contract that joined a verification root to the applied journal
prefix. A caller could observe a durable record before its materialized effects.
The SIC writer census did not classify each route for verification. Snapshot
replacement and libSQL replica refresh could leave a retained root tied to
replaced state. `AGENTS.md` described object metadata as a raw
`TenantPointWrite` route. The route uses a sequenced internal committer.

Verifier condition 9 failed before IMV4.

## Contract

`nimbus-storage` now owns a session-local `MaterializedVerificationTracker`.
It starts from one validated materialized snapshot and uses the same canonical
leaf writers as `MaterializedPosition`. There is no second stored-value
normalizer. Raw deltas and their leaf payloads are private to the storage
concept.

The tracker accepts a record only after the caller applies that record to
the state under verification. It validates record integrity first. It then
requires the next contiguous sequence, applies every exact delta, and publishes
the new sequence only after all tree updates succeed. A valid replay at or
below the current sequence keeps the root. A corrupt record, sequence gap,
unrepresentable lifecycle effect, or tree error discards the index. The normal
storage write still succeeds when the tracker discards this disposable index.

Exact deltas cover table identity, schema, documents, and scheduled execution.
Document insert, update, and delete and schema set and delete match a full
snapshot rebuild. Table lifecycle invalidates because hard deletion can remove
an unbounded document family that the event does not enumerate. Index and
trigger events do not change a root-covered state family.

The tracker retains the validated table-identity map from its bootstrap cut.
The exact decoder mirrors the provider's default-table transition. A write to a
staged hidden identity removes that hidden leaf. It moves the prior active
identity to deleting and installs the referenced identity as active. It then
changes the document leaf.

A deleting identity cannot accept a write. An unknown
identity can enter the map only through a creation event. A table ID and
logical-name disagreement invalidates the tracker.

Redb, memory, and SQLite snapshot restore publish an opaque process-local
generation change. Their rebuild and PITR paths compose through the checked
restore path. A replacement guard keeps each complete mutation window
non-current. This includes in-place libSQL catch-up and the full cache swap. A
bounded session cannot capture a passing generation during replacement.

The materialized serving trees remain derived read caches. A structural gate
rejects verification-root authority in the storage materializer and the engine
materialized-read cache.

## Writer inventory

The checked SIC matrix contains 54 logical writer rows. IMV4 assigns exactly
one verification effect to each row, in the same name and order:

| Effect | Rows | Meaning |
| --- | ---: | --- |
| `ExactAppliedRecord` | 32 | A successfully applied journal record contains exact canonical deltas. |
| `Invalidate` | 4 | The route changes covered state but cannot describe the complete leaf change. |
| `DurableOnly` | 1 | The route appends durability and must not advance an applied root. |
| `NoMaterializedState` | 17 | The route does not change a root-covered state family. |

The verification census also names nine replacement paths outside the SIC
`SqlStoreCore` census. These are redb, memory, and SQLite restore and rebuild.
The census also includes SQLite replica durable append and both libSQL refresh
paths. Eight paths invalidate. One path is durable-only. The gate checks that
each symbol still exists.

The gate also checks that each invalidating replacement
publishes the generation change directly or delegates to the checked restore
path.

The inventory includes all three client mutation routes, schema, scheduler,
trigger, PITR, object metadata, table lifecycle, and replica refresh. The object
route regression reads the sequenced durable record from the internal committer,
applies its delta after the commit, and matches a full post-commit rebuild.

## Acceptance evidence

The focused materialized-verification lane reports 22 passed tests. It covers
contiguous advance, failed apply, duplicate replay, corrupt duplicate, and gap
invalidation. It also covers durable-before-apply separation, local-provider
apply order, document and schema delta parity, lifecycle invalidation, and local
replacement generations. Two review regressions cover hidden-lineage writes and
the complete replacement mutation window. The hidden-lineage regression stages
a real redb identity and applies a durable record. It exports the provider
snapshot and proves incremental root parity for the active, hidden, deleting,
and document effects.

The writer-ownership lane reports 6 passed tests. Its scan finds 52
`SqlStoreCore` methods, 26 direct writers, 44 writers in the core, and 54 matrix
rows with 10 external rows. The object route and libSQL replacement regressions
each report one passed test. The durable-journal lane reports 9 passed tests.

The full default-feature storage lane reports 359 passed tests and 3 expected
ignored tests. It includes all local provider and physical-durability tests.

The unqualified engine mutation command first selected four remote-provider
tests and reported 251 passed and 4 failed. Each failure named a missing fixture
environment variable. The runbook-safe command then omitted those fixtures and
reported 255 passed tests. This omission is explicit and is not provider
evidence. IMV6 owns those provider lanes.

The fixed verifier reports:

```text
PASS  9. every materialized writer updates or invalidates after apply
Summary: 9 passed, 7 failed
```

The seven remaining failures are the planned IMV5 through IMV7 conditions.

Commands:

```text
cargo test -p nimbus-storage materialized_verification -- --nocapture
cargo test -p nimbus-storage commit_path_ownership -- --nocapture
cargo test -p nimbus-storage durable_journal -- --nocapture
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage
cargo test -p nimbus-storage --features libsql libsql_replica_refresh_invalidates_stale_verification_root -- --nocapture
cargo test -p nimbus-engine object_manifest_commit_updates_verification_root -- --nocapture
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-engine mutation -- --nocapture
bash docs/private/plans/proof/incremental-materialized-verification/verify.sh
```
