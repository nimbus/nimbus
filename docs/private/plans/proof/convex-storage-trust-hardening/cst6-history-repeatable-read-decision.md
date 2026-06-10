status: done
date: 2026-05-27
phase: CST6
posture: intentionally_latest_row

# CST6 History And Repeatable-Read Decision

## Decision

Nimbus is not adopting Convex's full MVCC document/index-row layout in this
phase.

The storage contract is intentionally:

- latest-row document and index storage for serving reads
- atomic document/index/commit-log writes
- an ordered `DurableMutationRecord` commit log as the authoritative logical
  history
- materialized snapshot plus journal-tail rebuild for restore, replay, and
  downstream consumers
- pinned transaction/session snapshots where Nimbus exposes a transaction API

That means Nimbus can reconstruct materialized state at a sequence boundary,
prove durable ordering, and keep transaction-session reads repeatable, but it
does not claim arbitrary historical reads from every table/index row the way a
full MVCC engine would.

## Why Not Full Convex MVCC Now

Convex's MVCC design is valuable because it powers historical reads,
subscription correctness, retention, and backfill behavior inside Convex's
database architecture. Nimbus currently gets the guarantees it exposes through
a smaller contract:

- The commit log records logical writes with previous/current document
  snapshots.
- Storage transactions keep document writes, index effects, and journal append
  atomic.
- Materialized snapshots record applied and durable boundaries, then reject an
  incomplete journal tail instead of silently rebuilding partial state.
- Transaction sessions read from their begin snapshot rather than drifting to
  later latest-row state.

Adopting full MVCC would add retention policy, compaction, historical index
selection, and backend-specific versioned-row implementations before Nimbus has
a product requirement that needs arbitrary time-travel reads.

## Verified Guarantees

| Guarantee | Evidence |
| --- | --- |
| Atomic mutation effects | Execution-unit batch tests cover document writes, resource-path binding changes, and commit-log effects as one storage transaction for SQLite, Postgres, and MySQL. |
| Durable replay | Durable-journal recovery tests cover Postgres, MySQL, and libSQL applying pending records from the authoritative journal. |
| Snapshot plus stream rebuild | redb and SQLite materialized snapshot tests prove snapshot plus journal tail rebuild matches live state and rejects incomplete tails. |
| Point-in-time rebuild | redb materialized snapshot tests can stop rebuild at a target sequence. |
| Repeatable transaction reads | Engine transaction-session tests prove point reads stay on the begin snapshot. |

## Verification

- `cargo test -p nimbus-storage materialized_snapshot --lib`: 5 passed.
- `cargo test -p nimbus-storage durable_journal_recovery --lib`: 3 passed.
- `cargo test -p nimbus-storage execution_unit_batch_persists --lib`: 3 passed.
- `cargo test -p nimbus-engine transaction_session_point_reads_stay_on_the_begin_snapshot --lib`:
  1 passed.
