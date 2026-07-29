# SUC4.1 — DynamoDB UpdateItem atomic read-modify-write

Branch: `codex/suc4a-dynamodb-rmw` (base `origin/main` @ `a082b9776`)
Crate: `crates/nimbus-dynamodb`

## Outcome

**The reported bug was already fixed on `main` before this task started.** It was
closed by `9b4f709ee` — "Close full codebase review findings (#231)", merged
2026-07-22. The full-review scout pass that produced the SUC4.1 pointers was
reading the review base (`9a40b60a4`, 2026-07-21), which predates that merge, so
every line reference in the task description describes code that no longer
exists.

This task therefore delivers **verification plus a regression test for the
uncovered half of the bug class**, not a second fix. Re-fixing would have meant
rewriting a correct implementation.

## The bug (pre-`9b4f709ee`)

`update_item` performed a read-modify-write with no shared conflict boundary:

1. `read_item(...)` — a plain committed read, outside any transaction.
2. `apply_update(...)` in memory.
3. `stream::execute_atomic_write_batch_with_streams(...)` — which began a *fresh*
   mutation execution unit and overwrote the **whole item** with the image
   derived from step 1.

The only guard was `WritePrecondition::exists(bool)`. Because the execution unit
in step 3 pins its snapshot *after* the step-1 read, a write that landed in
between was invisible: existence had not changed, so the precondition passed and
the stale full-item overwrite silently dropped the other writer's attributes.

`put_item` and `delete_item` shared the same existence-only pattern.

## Fail-before evidence

### 1. Both concurrency tests go RED on the pre-`#231` body

`update_item`'s body was temporarily reverted to the pre-`#231` non-transactional
read-modify-write (read outside a transaction, then
`execute_atomic_write_batch_with_streams`), leaving the tests untouched:

```
Starting 2 tests across 6 binaries (267 tests skipped)
    FAIL [0.172s] (1/2) commands::item::tests::concurrent_add_updates_retry_from_a_fresh_snapshot_without_lost_writes
    FAIL [0.172s] (2/2) commands::item::tests::concurrent_set_updates_on_distinct_attributes_both_survive
  Summary [0.175s] 2 tests run: 0 passed, 2 failed, 267 skipped

panicked at crates/nimbus-dynamodb/src/commands/item.rs:1271:14:
first update should retry and commit:
  TransactionConflictException("conflict: transaction conflict detected; retry the mutation")
```

Read this precisely: the failure is a **surfaced conflict**, not a silent lost
write. The `PREPARE_COMPLETE` pause sits *inside* the commit, after the execution
unit has pinned its snapshot, so the paused writer's read set does catch the
interleaved write — it just has no retry loop to recover with, and the error
reaches the caller. This proves the tests have teeth against the pre-fix code,
but it does not by itself exhibit the data loss.

### 2. The silent-loss window, demonstrated directly

The genuine loss window is between the adapter's read and the execution unit's
creation, which no fault label can pause. A temporary test reproduced that
sequence explicitly against the **fixed** tree, using the same helpers the
pre-fix code used (`read_item` then `atomic_overwrite` with
`WritePrecondition::exists(true)`):

```
PASS [0.268s] commands::item::tests::failbefore_stale_read_then_overwrite_silently_loses_a_concurrent_write
```

The test asserts `beta` is **absent** after the stale writeback, and it passed:
the concurrent writer's attribute was silently destroyed and the existence-only
precondition raised nothing. That is the bug class, reproduced deterministically.

This test asserts buggy behavior, so it is a demonstration rather than a
regression guard, and was removed before commit.

## Fix design (as landed in `#231`, verified here)

`execute_single_item_transaction` (`item.rs:46`) wraps each single-item
operation in an engine transaction session and retries the whole
read-modify-write on conflict:

- `begin_transaction_session(ReadWrite)` pins the snapshot **before** the read.
- The item read (`get_document_in_transaction`), `ConditionExpression`
  evaluation, update application, returned image, data write, and stream effects
  all happen inside that one session, so they share a conflict boundary.
- `commit_transaction_session` validates the tracked read set. A concurrent
  writer invalidates it and the commit fails with `Error::Conflict`.
- `single_item_transaction_should_retry` retries on retryable `Conflict`,
  `OutOfRetention`, and `AlreadyExists`, bounded by
  `MAX_SINGLE_ITEM_TRANSACTION_ATTEMPTS = 32`; exhaustion surfaces
  `TransactionConflictException`.

Note the OCC here is **read-set validation**, not
`WritePrecondition::update_time`. The task brief proposed `update_time`, but that
would be redundant: preconditions are evaluated against the execution unit's
pinned snapshot, not live state, so inside a session they are snapshot-local and
cannot detect a concurrent commit on their own. Read-set validation at commit is
what actually closes the race. The `exists(...)` preconditions are retained as a
cheap invariant check.

Conditional-write semantics stay separate from OCC retry, which is the subtle
part and is correct: `check_condition` runs against the transaction snapshot and
returns `ConditionalCheckFailedException` directly (rolling back, no retry). A
lost create race takes the other route — commit fails `AlreadyExists`, the loop
retries, the fresh snapshot now shows the item present, and `check_condition`
then fails the user's `attribute_not_exists(...)` on its own terms. So a genuine
conditional failure is never masked as a conflict, and a conflict is never
reported as a conditional failure.

`put_item` and `delete_item` were moved onto the same helper by `#231`;
`transact.rs` already used transaction sessions directly.

## What this task added

One regression test, `concurrent_set_updates_on_distinct_attributes_both_survive`
(`crates/nimbus-dynamodb/src/commands/item.rs`).

The pre-existing coverage (`concurrent_add_updates_...`) races two `ADD`
operations on one counter. `ADD` merges numerically, so a lost write there shows
up only as a wrong total. Racing `SET` on two **different** attributes is the
sharper probe: a wholesale snapshot writeback deletes the other attribute
outright, leaving no trace. Both tests use the `PREPARE_COMPLETE` commit-fault
pause for a deterministic interleaving rather than probabilistic looping, and
both assert `hit_count >= 3` so the retry is proven to have happened rather than
inferred from the final value.

## Verification

All commands run in `/Users/jack/src/github.com/nimbus/nimbus-suc4a` with
`set -o pipefail`.

| Gate | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb` | **269 tests run: 269 passed, 0 skipped** |
| `cargo clippy -p nimbus-dynamodb --all-targets -- -D warnings` | clean, exit 0 |
| `cargo fmt --all --check` | clean, exit 0 |

Test count: HEAD carried 268 (`267 passed, 1 skipped` with `soak` filtered out);
this change adds exactly one test, for 269.

## Deviations from the scout's description

- `update_item` is **not** at `item.rs:334` doing a non-transactional
  read-modify-write. It is at `item.rs:394` and already runs inside
  `execute_single_item_transaction`.
- `read_item` does not "discard the Document and keep only fields" in the write
  path. The write path uses `read_item_in_transaction`; the plain `read_item`
  survives only for `get_item`, `query.rs`, and `batch.rs`, all of which need
  fields only.
- The stale comments at `item.rs:110-114` and `153-155` claiming "the engine
  models only existence-level preconditions" no longer exist —
  `grep` for them across `crates/nimbus-dynamodb/src/` returns nothing. `#231`
  removed them.
- `map_conditional_write_error` is now `#[cfg(test)]`-only; the production
  conditional-failure path runs through `check_condition` inside the transaction.

## Open observation (not fixed, not the assigned bug class)

`batch_write_item` (`commands/batch.rs:132` and `:162`) still reads the prior
image with a plain `read_item` before each put/delete, outside any transaction.
This is **not** a lost-update defect: BatchWriteItem is explicitly non-atomic in
DynamoDB, and its Put and Delete are whole-item operations that do not merge with
prior state, so last-writer-wins is the correct semantic. The only staleness is
in the emitted **stream record** — a concurrent write between the read and the
commit can yield an `INSERT`/`MODIFY` classification or an `OldImage` that does
not match what the write actually replaced. That is a stream-fidelity gap worth
its own ticket, not a data-integrity one, and fixing it means routing batch
writes through `execute_single_item_transaction` too.
