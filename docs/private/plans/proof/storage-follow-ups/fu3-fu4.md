# FU3 + FU4 + FU-P1 — DynamoDB batch stream fidelity, policy-aware paging, and GetRecords authorization

Owning plan: `docs/private/plans/storage-follow-ups-plan.md` (FU3, FU4, FU-P1).
Prior evidence: `proof/storage-unification/suc4/dynamodb-rmw.md` (FU3 finding),
`proof/storage-unification/suc5/principal.md` (FU4 finding).
Branch `codex/fu3-dynamo`, base `origin/main @ 22c5cdd62`.

Three concepts, three commits: the batch transaction (FU3), the starting-at scan
paging (FU4), and stream-record read authorization (FU-P1, raised as a P1 on
review of the first two).

## FU3 — `batch_write_item` read the prior image outside the write's transaction

### What was wrong

`batch_write_item` computed each op's stream record from a `read_item` taken on
its own snapshot, then applied the write through a *separate*
`execute_atomic_write_batch_with_streams` call. Nothing tied the image the
record described to the state the write actually replaced.

This is not a lost-update bug. BatchWriteItem is non-atomic by contract and
Put/Delete are whole-item, so last-writer-wins on the item itself is correct.
What the split window corrupts is the **emitted stream record**: its
INSERT/MODIFY classification and its `OldImage`.

### The fix

Each op now runs through `execute_single_item_transaction` — the same helper
`put_item`/`delete_item`/`update_item` use post-#231 — with the prior image read
by `read_item_in_transaction` *inside* that transaction:

- `crates/nimbus-dynamodb/src/commands/batch.rs` — Put and Delete arms.
- `crates/nimbus-dynamodb/src/commands/item.rs` — `SingleItemTransactionPlan`,
  `execute_single_item_transaction`, and `read_item_in_transaction` widened from
  private to `pub(crate)` so `batch.rs` can drive them. No behavior change.

One transaction **per op**, not per batch, so BatchWriteItem semantics are
preserved: ops stay independent, there is no cross-item atomicity, and
`UnprocessedItems` stays empty. The Put arm carries
`WritePrecondition::exists(old.is_some())` and the Delete arm
`WritePrecondition::exists(true)`, which makes the snapshot's existence
assumption a cheap in-transaction invariant on top of read-set validation.

### Fail-before (RED)

`batch.rs` was temporarily reverted to the pre-fix non-transactional arms with
the new tests in place (the fixed file was restored afterwards from a saved
copy under the session scratchpad — never `git checkout --`, per CLAUDE.md).

```
$ NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb \
    -E 'test(batch_put_stream_record_reflects_the_image_it_actually_replaced) or
        test(batch_delete_stream_record_carries_the_image_it_actually_removed) or
        test(batch_write_ops_stay_independent_of_each_other)'
  FAIL [0.252s] (1/3) commands::batch::tests::batch_put_stream_record_reflects_the_image_it_actually_replaced
    panicked at crates/nimbus-dynamodb/src/commands/batch.rs:550:14:
    batch put should retry and commit: TransactionConflictException("conflict: transaction conflict detected; retry the mutation")
  FAIL [0.255s] (2/3) commands::batch::tests::batch_delete_stream_record_carries_the_image_it_actually_removed
    panicked at crates/nimbus-dynamodb/src/commands/batch.rs:626:14:
    batch delete should retry and commit: TransactionConflictException("conflict: transaction conflict detected; retry the mutation")
  PASS [0.435s] (3/3) commands::batch::tests::batch_write_ops_stay_independent_of_each_other
  Summary [0.439s] 3 tests run: 1 passed, 2 failed, 278 skipped
PIPELINE_RC=100
```

**Be precise about what the RED shows.** With the concurrent write landing while
the batch is paused at `PREPARE_COMPLETE`, the pre-fix code does not silently
emit a stale record — it fails the whole BatchWriteItem with
`TransactionConflictException`, because the untransacted path has no retry. The
pure staleness interleaving (the concurrent commit landing *between* the
untransacted `read_item` and the *start* of the write transaction, so the write
never conflicts and the stale `StreamChange` is emitted as-is) is real and
reachable in production, but is not reachable with the fault labels available:
`PREPARE_COMPLETE` fires inside the commit, after the transaction has pinned its
snapshot, and there is no hook between the read and the transaction's start.

Both outcomes have the same root cause and the same fix, and the post-fix
assertions pin the property the finding cares about: the batch op conflicts,
retries, **re-reads inside the new transaction**, and re-derives its record from
what it actually replaced. A retry that reused the stale `StreamChange` would
fail these tests exactly as the pre-fix code does.

### Tests added (`crates/nimbus-dynamodb/src/commands/batch.rs`)

Records are read back through the real client surface — DescribeStream →
GetShardIterator(TRIM_HORIZON) → GetRecords — not by peeking at the event store.

- `batch_put_stream_record_reflects_the_image_it_actually_replaced` — arms
  `PREPARE_COMPLETE`, runs a batch Put of `{pk:x, v:2}` on a worker thread, waits
  for the pause, creates `x=1` concurrently, releases. Asserts exactly two
  records; the one carrying `NewImage.v == 2` must be `MODIFY` with
  `OldImage.v == 1`; and `x == 2` afterwards, so last-writer-wins is intact.
- `batch_delete_stream_record_carries_the_image_it_actually_removed` — seeds
  `y=1`, arms the same pause, runs a batch Delete of `y`, writes `y=2`
  concurrently, releases. Asserts the REMOVE record's `OldImage.v == 2` (the
  image it removed, not the stale `1`) and that `y` is gone.
- `batch_write_ops_stay_independent_of_each_other` — a two-op batch whose second
  Put is missing its partition key returns `ValidationException` while the first
  item stays applied. This is the guardrail on the semantics the fix must not
  change.

## FU4 — the limit-bearing starting-at scan was policy-blind

### What was wrong

SUC5.1 made `scan_documents_by_id_prefix` apply `ReadAuthorization`, but left
`scan_documents_by_id_starting_at_cancellable` alone: it is limit-bearing, and
post-filtering one page of `limit` documents hands a restricted caller a short
page it cannot distinguish from the end of the range. There was no current
exposure — the sole caller is the adapter-owned `_ddb_stream_*` sidecar — but
the seam invited exactly that misuse.

### The fix

`crates/nimbus-engine/src/engine/queries/query_api.rs` — the method takes a
`&PrincipalContext` and does filter-then-fill paging:

- `ReadAuthorization::for_table(schema.get_table(table), principal)`, with
  `impossible` short-circuiting to an empty page (same shape as the prefix scan).
- The loop keeps fetching from the store until `limit` **authorized** documents
  are collected or the range is exhausted, so withheld documents do not consume
  page slots.
- Termination: the store scan is inclusive of its start id, so a refill re-reads
  the document it resumed from. Each round asks for one extra document and drops
  that repeat, which makes every non-exhausted round net-positive. (`\0` as an
  exclusive successor key was rejected — Postgres text cannot carry NUL.)
- `check_cancel` is called at the top of every round and passed into each store
  scan, so cancellation is honoured across refills, not just once.

### Which principal the sidecar passes

Verified rather than assumed: before this change the API had **no** principal
parameter at all — the read went straight to the store. The sole caller,
`read_events_from` in `crates/nimbus-dynamodb/src/commands/stream.rs`, now
passes `adapter_principal()`, which preserves that behavior and matches every
other `_ddb_stream_*` / `_ddb_streamseq_*` access per SUC5.1. Passing the
caller's principal here would put the *user* table's read policy in front of the
adapter's own change-capture bookkeeping.

### Fail-before (RED)

The method body was temporarily reverted to the pre-change policy-blind single
fetch (new signature retained so the new tests compile), then restored from the
saved copy.

```
$ cargo nextest run -p nimbus-engine \
    -E 'test(engine_read_policy_fills_limited_pages_of_the_id_starting_at_scan) or
        test(engine_id_starting_at_scan_without_a_policy_is_a_plain_limited_range_read)'
  FAIL [0.606s] (1/2) tests::policy::engine_read_policy_fills_limited_pages_of_the_id_starting_at_scan
    panicked at crates/nimbus-engine/src/tests/policy.rs:616:5:
    assertion `left == right` failed: the limit must be filled with authorized documents, skipping withheld ones
      left: ["doc-00", "doc-01"]
     right: ["doc-00", "doc-02"]
  PASS [1.361s] (2/2) tests::policy::engine_id_starting_at_scan_without_a_policy_is_a_plain_limited_range_read
  Summary [1.365s] 2 tests run: 1 passed, 1 failed, 668 skipped
PIPELINE_RC=100
```

The pre-change scan returned `doc-01` — a document the policy withholds from
that principal. So the RED here is a read-policy bypass, not merely a truncated
page: the seam handed back an unauthorized document. The unguarded-table test
passes in both states, which is its job — it pins that adding authorization did
not change the no-policy behavior.

### Tests added

`crates/nimbus-engine/src/tests/policy.rs`:

- `engine_read_policy_fills_limited_pages_of_the_id_starting_at_scan` — eight
  documents `doc-00..doc-07` under `read_only_owner_policy()` with owners
  alternating so that no page-sized raw fetch ever yields a full page of
  authorized rows. With `limit = 2`: `doc-00` returns `["doc-00","doc-02"]` (a
  filled page, not the single row that survives filtering the first two);
  resuming at `doc-03` returns `["doc-04","doc-06"]`, so the cursor still means
  what it did; `doc-07` returns empty, the only legitimate short page; and a
  stranger and `PrincipalContext::anonymous()` each get nothing.
- `engine_id_starting_at_scan_without_a_policy_is_a_plain_limited_range_read` —
  with no policy the scan is still a plain range read: the limit caps the page,
  `start_id` is inclusive, a limit past the end returns the rest, and `limit == 0`
  reads nothing.

`crates/nimbus-dynamodb/tests/principal_authorization.rs` initially gained
`get_records_reads_the_adapter_owned_event_store_as_the_adapter`, which asserted
that `OWNER_KEY` and `OTHER_KEY` both saw the same three captured events while
`Scan` still returned `Count == 0` for `OTHER_KEY`. The test pinned the
pre-existing behavior faithfully — and review then found that behavior is itself
a read-policy bypass. FU-P1 below replaces that test with the corrected contract.

## FU-P1 — GetRecords was a side door around the source table's read policy

### What was wrong

A stream record carries the item contents that changed, so returning one
discloses what a read of the source table would. GetRecords returned every
captured event to any authenticated tenant key. With `NEW_AND_OLD_IMAGES` that
hands a caller the full contents of items the table's read policy withholds from
it.

DynamoDB gets away with the equivalent because a stream is its own IAM resource:
the stream ARN is granted separately, so stream access is a deliberate act. Nimbus
has no stream-permission surface — an access key's rights come from its tenant
binding plus the table policies — so there is nothing between an authenticated
key and the records. The table's read rule has to hold at the record-return
boundary or it does not hold at all.

### The fix

`crates/nimbus-dynamodb/src/commands/stream.rs` — a new `RecordAuthorization`
resolves the source table's read rule for `caller_principal(context)` once per
GetRecords call, and `get_records` fills its page only with records that rule
allows.

The sidecar's own storage access is unchanged: the `_ddb_stream_*` /
`_ddb_streamseq_*` event store stays adapter-owned and is still read as
`adapter_principal()` (FU4 above). Authorization is layered at the point records
are returned, not pushed down into the bookkeeping read — putting the user
table's policy in front of the adapter's change-capture store would break the
sidecar itself.

`nimbus-engine` gained the seam this needs, since `ReadAuthorization` is
`pub(crate)`:

- `DocumentReadFilter` (`engine/queries/authorization.rs`) — a table's read rule
  resolved for one principal, applicable to documents a caller assembles itself
  rather than reads through a scan. It exposes `is_unrestricted`,
  `denies_everything`, `depends_on_document_timestamps`, and `allows(&Document)`.
- `Engine::document_read_filter(tenant, table, principal)`
  (`engine/queries/query_api.rs`) — resolves one, entering the tenant operation
  guard the same way the scans do.

The policy knowledge stays in the engine; the judgment that *these* documents are
reconstructed stays in the adapter.

### (a) Which check applies to a record

The source table's read rule, evaluated against the caller, with each captured
image reconstructed as the document the table would have held:
`fields_to_item` + `item::primary_key_id(item, key_schema)` reproduce the id the
write path derived, and the image is already stored in the data path's field
encoding, so `_id` and every stored field are faithful.

**OLD vs NEW image — the conservative rule, as directed: withhold unless every
image the event carries is authorized.** A MODIFY that moves an item between
owners has one image the caller may read and one it may not, and either half
reveals the other because the record pairs them. Withholding the whole record is
the only answer that does not leak, and the only one that stays correct as the
view type changes.

Authorization uses the **stored** images even when the configured view type would
not return them. `StoredEvent` always holds full old/new images and `shape_record`
applies the view type at read time, so this is available under KEYS_ONLY too — and
a KEYS_ONLY record still names an item that changed, which is itself item-level
information, so it is held to the same standard.

Two fail-closed cases fall out and are deliberate:

- **Rules that can depend on `_creationTime` / `_updateTime`.** A document rebuilt
  from an image carries no engine lifecycle timestamps. Rather than authorize
  against an invented value, `depends_on_document_timestamps()` withholds. This
  gap was not in the ticket; it is real, and failing closed is the only safe
  reading. (`TIMESTAMP_FIELDS` in the engine names the reserved fields, so the
  adapter does not have to know them.)
- **An event carrying neither image** is unreadable rather than public.

A DynamoDB-surfaced table stores every attribute as AttributeValue wire JSON, so
a `DocumentField` read rule on such a table compares against `{"S": "..."}`, not
a bare scalar. The test helper documents this — a policy written against the bare
value silently matches nothing and would make the tests pass for the wrong reason.

### (b) DescribeStream and GetShardIterator

Confirmed to carry nothing item-level, and left unfiltered:

- **DescribeStream** returns shape, status, view type, table name, and key
  schema. That is table metadata — attribute *names*, never values — and a caller
  that can address the table through the same tenant binding already sees all of
  it from DescribeTable.
- **GetShardIterator** returns a position, not content. Holding an iterator grants
  nothing, because GetRecords authorizes every record it hands back. The one
  observable is that `LATEST` resolves the stream's high-water sequence, which
  counts changes without describing any; that is a metadata-grade signal of the
  same kind DescribeTable's item count already gives.

Both now carry a doc paragraph saying so, so the asymmetry with GetRecords reads
as a decision rather than an oversight.

### Filter-then-fill paging

Same shape as FU4, with the differences the stream seam forces:

- The fill loop keeps reading windows until `limit` **authorized** records are
  collected or the store is drained, so withheld records do not consume page
  slots.
- `next_sequence` advances **per consumed event**, not to the end of the fetched
  window. Advancing per window would skip unreturned tail events once the limit
  filled mid-window — the cursor hazard that makes the naive version lose records.
- Expired-event reclamation is limited to `&window[..consumed]`, keeping deletion
  behind the iterator so nothing is dropped that a later poll still has to walk
  past.
- Termination differs from FU4's. FU4 must fill the page exactly, because a short
  page from a limit-bearing scan is indistinguishable from the end of the range.
  GetRecords always returns an advanced `NextShardIterator`, so a short page is a
  signal to poll again — which makes it safe to bound the work at
  `MAX_EVENTS_EXAMINED = 10 * MAX_GET_RECORDS` rather than scan unboundedly past
  withheld records. The multiple lets a heavily filtered stream still fill a full
  page in one call.

### Fail-before (RED)

`stream.rs` was reverted to `git show HEAD:...` — a self-consistent whole file,
because `-D unused` makes a hand-edited revert with orphaned imports a build
error, not a test failure — with the new tests in place. The engine additions
stayed, being `pub` and therefore not dead code. It was then restored from the
saved copy and `diff`-verified.

```
$ NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb \
    -E 'test(get_records_authorizes_returned_records_against_the_source_table) or
        test(get_records_fills_pages_from_the_records_the_caller_may_read) or
        test(get_records_withholds_a_record_whose_images_straddle_the_policy)'
  FAIL get_records_authorizes_returned_records_against_the_source_table
    panicked at crates/nimbus-dynamodb/tests/principal_authorization.rs:499:
    a caller the table's read policy withholds items from must not receive those
    items back as stream records: ["a", "b", "c"]
  FAIL get_records_fills_pages_from_the_records_the_caller_may_read
    panicked at crates/nimbus-dynamodb/tests/principal_authorization.rs:546:
      left: ["i0", "i1"]
     right: ["i0", "i2"]
  FAIL get_records_withholds_a_record_whose_images_straddle_the_policy
    panicked at crates/nimbus-dynamodb/tests/principal_authorization.rs:585:
      left: ["x", "x", "y"]
     right: ["y"]
  Summary 3 tests run: 0 passed, 3 failed
PIPELINE_RC=100
```

The third RED is the one that matters most. An implementation authorizing only
the NEW image would *pass* the first two tests and still leak: `x` is created
owned by `OTHER_KEY` and then handed to `OWNER_KEY`, so its MODIFY has a NEW
image `OWNER_KEY` may read and an OLD image it may not. `left: ["x", "x", "y"]`
is the unauthorized version returning both of x's records.

### Tests (`crates/nimbus-dynamodb/tests/principal_authorization.rs`)

All three drive the real client surface — DescribeStream → GetShardIterator
(TRIM_HORIZON) → GetRecords — under `read_only_owned_by(key)`, a policy that
admits one access key *and* requires `owner == that key`.

- `get_records_authorizes_returned_records_against_the_source_table` —
  `OWNER_KEY` sees `["a","b","c"]`; `OTHER_KEY` sees none. `assert_ne!` on the
  returned iterator proves withheld records still advance it, so a poller does not
  loop forever on records it will never receive, and `Scan["Count"] == 0` for
  `OTHER_KEY` proves the policy is real rather than the table being empty.
- `get_records_fills_pages_from_the_records_the_caller_may_read` — six items
  `i0..i5` with alternating owners and `Limit = 2`, so no raw window of two ever
  holds two authorized records. Pages are `["i0","i2"]`, then `["i4"]`, then
  empty: the limit is filled from authorized records across refills, and the
  cursor still means what it did.
- `get_records_withholds_a_record_whose_images_straddle_the_policy` — the
  both-images rule. `OWNER_KEY` must see exactly `["y"]`, while
  `Scan["Count"] == 2` proves both items are currently readable — the withheld
  record is withheld for its OLD image alone.

## Verification

All commands run with `set -o pipefail` so the recorded status is the command's,
not `tail`'s. macOS, `stable-aarch64-apple-darwin`.

The battery was run twice: once after FU3 + FU4, and again after FU-P1. The
table records the **final** run, over all three concepts.

| Command | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb` | **283 run, 283 passed, 0 skipped** — `PIPELINE_RC=0` |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-engine` | **665 run, 665 passed (1 slow), 5 skipped** — `ENGINE_RC=0` |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-server -E 'test(dynamodb) or test(ddb)'` | **15 run, 15 passed, 590 skipped** — `PIPELINE_RC=0` |
| `cargo clippy -p nimbus-dynamodb -p nimbus-engine --all-targets -- -D warnings` | `CLIPPY_RC=0` |
| `cargo fmt --all --check` | `FMT_RC=0` (after applying `cargo fmt --all`) |

The dynamodb count moves 281 → 283: FU-P1 replaces one test with three.

The server lanes include `tests::dynamodb_wire::dynamodb_wire_streams_event_delivery`
and `nimbus-server::dynamodb_spec dynamodb_tenant_admission_uses_provider_lifecycle`,
so change capture is exercised end-to-end through the wire, not only in-crate.

### Which lanes actually ran

- The **17 `tests::postgres_provider::*` engine tests need a live Postgres** and
  are not runnable here. A first engine run without
  `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` failed all 17, and their
  CPU load also pushed `redb_ppsc_seeded_journal_differential` and
  `sqlite_ppsc_seeded_journal_differential` past nextest's 135s slow-timeout.
  With the env var set — the same one `make ci`'s workspace lane uses — those 17
  skip and both PPSC differentials pass (78.9s and 56.9s). **Hosted CI's service
  containers are the evidence for the Postgres lanes; they were not proven here.**
- A post-`cargo fmt` engine rerun showed **1 failure**:
  `tests::mutation_journal::arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher`.
  This is **FU5**, the flake already ticketed in this plan. It was confirmed
  pre-existing rather than assumed: the working tree was stashed
  (`git stash push -u -- crates/`) and the same test failed identically on the
  unmodified base commit. The tree was then restored and every one of the six
  touched files verified byte-identical against copies saved before the stash.
- The first post-FU-P1 engine run also showed **1 failure**, a *different* test:
  `tests::mutation_journal::publisher_observers::projection_provider_schema_refresh_waits_for_journal_frontier`
  (`publisher_observers.rs:190`, `left: SequenceNumber(2)` / `right:
  SequenceNumber(1)`). Same discipline, not assumption: three isolated reruns gave
  FAIL, PASS, PASS, so it is flaky; then `crates/` was stashed and the test run
  **eight** times against the tree without the FU-P1 change, where it failed
  **1 of 8** with the identical assertion. Pre-existing, and a second journal-
  publication flake alongside FU5's — worth its own ticket. The tree was restored
  and all seven touched files verified byte-identical. The final full engine run
  above is green at 665/665.

### Restoration discipline

All three fail-before captures reverted production code temporarily, and so did
both flake attributions. Every time, the file was restored from a copy saved to
the session scratchpad and then `diff`-verified — never with
`git checkout -- <file>`, which restores from HEAD and would have destroyed the
rest of the uncommitted work in these files.

The FU-P1 revert used `git show HEAD:crates/nimbus-dynamodb/src/commands/stream.rs`
rather than hand-editing the changed arms back. Under `-D unused` a partial
revert leaves orphaned imports, which is a **build** error — the tests would not
have run at all, and a build failure is not a fail-before.

### Environment note

The first `nimbus-server` build failed twice for reasons unrelated to the code:
the worktree had no `node_modules` (`nimbus-assets` requires the built operator
UI, so `npm install` + `npm run build -w nimbus-ui` were needed), and then the
volume hit 100% with 262Mi free, producing
`rustc-LLVM ERROR: IO failure on output stream: No space left on device`.
Reclaiming ~4.4G of regenerable caches (`go clean -cache`, Homebrew/pip/node-gyp
caches, `~/.cargo/registry/cache`) let macOS release its purgeable reserve,
restoring 147Gi. No other worktree's `target/` was touched — four sibling
follow-up worktrees were actively building at the time.
