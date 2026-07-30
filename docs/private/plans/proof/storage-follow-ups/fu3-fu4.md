# FU3 + FU4 + FU-P1..P4 — DynamoDB batch stream fidelity, policy-aware paging, and GetRecords authorization

Owning plan: `docs/private/plans/storage-follow-ups-plan.md` (FU3, FU4, FU-P1, FU-P2, FU-P3, FU-P4).
Prior evidence: `proof/storage-unification/suc4/dynamodb-rmw.md` (FU3 finding),
`proof/storage-unification/suc5/principal.md` (FU4 finding).
Branch `codex/fu3-dynamo`, base `origin/main @ 22c5cdd62`.

Six concepts, six commits: the batch transaction (FU3), the starting-at scan
paging (FU4), stream-record read authorization (FU-P1, raised as a P1 on review
of the first two), real lifecycle timestamps on stored images (FU-P2, raised as
a P2 on review of FU-P1), the three review findings against the GetRecords path
(FU-P3 — page-fill amplification, no-op MODIFY reconstruction, and timing sleeps
in the new tests), and the store-read ceiling FU-P3 documented without enforcing
(FU-P4).

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
  `denies_everything`, `depends_on_document_timestamps` (removed again by FU-P2),
  and `allows(&Document)`.
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

Two fail-closed cases fell out of this commit:

- **Rules naming `_creationTime` / `_updateTime`.** A document rebuilt from an
  image carried no engine lifecycle timestamps, so rather than authorize against
  an invented value, `depends_on_document_timestamps()` withheld the record.
  **FU-P2 below removes this branch entirely** by persisting the real timestamps
  with each image; it is described here because it is what FU-P1 shipped and what
  FU-P2's fail-before reverts to.
- **An event carrying neither image** is unreadable rather than public. This one
  survives FU-P2 unchanged.

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

## FU-P2 — the timestamp fail-closed branch made the authorization contract knowingly incomplete

### What was wrong

FU-P1's `depends_on_document_timestamps()` withheld any record whose table read
rule named `_creationTime` or `_updateTime`. Withholding is safe against
disclosure, but a stream iterator is consumed as it is read: a withheld record
still advances `next_sequence`, so those records were **permanently skipped**,
not deferred. A perfectly ordinary policy — `_creationTime > 0`, or any rule that
merely *mentions* a lifecycle field — silently emptied the stream forever, and
the reason was that the adapter had thrown the answer away rather than that the
caller was unauthorized.

### The fix — persist the real times, evaluate every rule

`crates/nimbus-dynamodb/src/commands/stream.rs`

- New `DocumentTimes { created, updated }` records a source document's engine
  lifecycle stamps, and `StoredEvent` gained `old_image_times: Option<DocumentTimes>`.
  The field is serialized unconditionally (no `serde(default)`, no
  `skip_serializing_if`): it is `None` exactly when there is no old image, so an
  event that has an old image and no times is **corrupt, not old**, and reading
  one fails with `InternalServerError` instead of authorizing against absent
  metadata. Pre-launch, changing the stored format directly is the instruction;
  there is no shim and no migration.
- New `OldImage { item, times }` replaces the bare `Item` in `ChangeEvent` and
  `StreamChange`, so the two travel together and a capture site *cannot* record
  an image without its metadata. `OldImage::of(Option<&Document>)` is the single
  constructor; all nine capture sites across `item.rs`, `batch.rs`,
  `transact.rs`, and `ttl.rs` now go through it.
- `RecordAuthorization::documents_for` rebuilds every image the event carries as
  a real `Document` with real times, and `allows` evaluates the rule against each.
  The `depends_on_document_timestamps` branch is deleted; so are
  `TIMESTAMP_FIELDS` and `names_timestamp_field()` in
  `crates/nimbus-engine/src/engine/queries/authorization.rs`.

**The both-images conservative rule is unchanged**, and so is the
neither-image withholding.

### Where the new image's `_updateTime` comes from

The brief's premise held for the old image but not the new one. A capture site
has the *prior* document in hand, so `creation_time` and `update_time` are
available there — but the new image's `_updateTime` is the mutation's **commit
timestamp**, which `assign_commit_timestamp` does not assign until after the
event payload has been built. It is not knowable at capture, and no amount of
write-path plumbing makes it so without changing when the clock is read.

It did not need plumbing. The event document is created in the **same
`AtomicWriteBatch`** as the data write, so the engine stamps both from one commit
timestamp — which makes the event document's own `creation_time` exactly the new
image's `_updateTime`. `read_events_from` recovers it into a `#[serde(skip)]`
`committed_at` field. The new image's `_creationTime` is then the old document's
when the write replaced one, and `committed_at` when it created one — the same
inheritance the engine applies when it stamps an update.

This is an identity derived by reading the write path, so it is pinned by a
test rather than trusted: `reconstructed_image_times_match_the_engine_document_times`
writes an item twice and asserts the reconstructed times equal the times the
engine actually holds for the stored document, having first asserted that the two
writes landed in different milliseconds so the comparison can distinguish them.

### A prerequisite defect in `nimbus-core` — lifecycle rules denied every row

Writing the first timestamp-referencing test surfaced a bug **outside** the
adapter, and fixing it was load-bearing rather than incidental scope.

`AccessPredicate::compile_read_filter` pushed any `DocumentField op Literal` pair
down as a planner `Filter`. But a planner filter is resolved by
`matches_filters` (`nimbus-storage/src/sql/predicate.rs`) via
`Document::get_field`, which reads the **stored field map** — where `_id`,
`_creationTime`, and `_updateTime` never appear. A pushed-down lifecycle filter
therefore matched nothing, so a read policy of `_creationTime > 0` denied **every
row of every scan and query**.

Left unfixed this would have inverted the property under test: streams would have
returned records for a policy under which the table itself returned nothing —
strictly *more* permissive than the source table, the opposite of what FU-P1 and
FU-P2 exist to establish.

`crates/nimbus-core/src/auth/access.rs` now gates pushdown on
`is_pushdown_document_field()`, which excludes the three metadata names; such
predicates stay **residual** and are answered by the per-document rule evaluation
every read path already applies (`document_field_value` resolves them from the
document header). No existing test pinned the broken behaviour.

### Fail-before (RED)

The eight source files were stashed with `git stash push -- <paths>`, leaving
both test files in place; afterwards `git stash pop` restored them and all ten
modified files were verified byte-identical against copies saved beforehand
(`RESTORE_OK=1`). `git checkout -- <file>` was not used: it restores from HEAD
and would have destroyed the FU3/FU4/FU-P1 work still uncommitted in these files.

```
$ NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-core --lib auth::
  FAILED auth::tests::read_rule_keeps_lifecycle_metadata_predicates_residual
    panicked at crates/nimbus-core/src/auth/tests.rs:92
  test result: FAILED. 5 passed; 1 failed

$ NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1     cargo test -p nimbus-dynamodb --test principal_authorization
  FAIL get_records_fills_pages_under_a_timestamp_referencing_policy
    principal_authorization.rs:586  left: []  right: ["i0", "i2"]
  FAIL get_records_withholds_a_straddling_record_under_a_timestamp_policy
    principal_authorization.rs:644  left: []  right: ["y"]
  FAIL get_records_evaluates_lifecycle_times_against_the_real_document_history
    principal_authorization.rs:755  left: []  right: ["a", "b"]
  test result: FAILED. 8 passed; 3 failed
```

Every RED is `left: []` — the fail-closed branch withholding the whole stream,
which is precisely the incompleteness this commit removes. The three in-crate
`stream/tests.rs` additions fail to *compile* against the old format, which is
expected and is why they are not listed as assertion failures.

### Tests added

`crates/nimbus-dynamodb/src/commands/stream/tests.rs`

- `reconstructed_image_times_match_the_engine_document_times` — the identity
  above, against the engine's own stored document.
- `a_record_carrying_neither_image_is_withheld` — the surviving fail-closed case.
  It first asserts a *real* record is allowed under the same policy, so the
  withheld verdict is about the missing images and not about a policy that denies
  everything.
- `an_old_image_without_lifecycle_times_is_rejected` — a `Some(old_image)` with
  `None` times is corrupt and errors rather than authorizing.

`crates/nimbus-dynamodb/tests/principal_authorization.rs` — the paging and
straddle bodies were extracted into `run_paging_scenario` / `run_straddle_scenario`
so each runs under two policies: the original, and the same policy with a
`_creationTime > 0` term appended. The added term must change nothing, which is
the assertion that records now flow under a timestamp-referencing rule.

- `get_records_evaluates_lifecycle_times_against_the_real_document_history` is
  the discriminating one. Its policy is `_creationTime == _updateTime` — both
  sides `DocumentField`, answerable only against real times. A `> 0` probe cannot
  do this job: under placeholder zeros every image compares equal and passes,
  whereas under this rule a placeholder implementation returns all three records.
  Item `a` is created and then modified, item `b` is created and left alone, and
  `OWNER_KEY` must receive `["a", "b"]` — a's INSERT and b's INSERT, with a's
  MODIFY withheld because its new image was updated after it was created.

Three things make that test unable to pass vacuously:

- The two writes to `a` are separated by 5ms, because commit timestamps are
  milliseconds and `max(now, previous)`, so same-millisecond writes would share a
  stamp.
- Before any policy is set, the stream is read once and asserted to carry
  `["a", "a", "b"]`. Were the MODIFY simply absent, the filtered expectation
  would be identical — this is what makes the filtered read a statement about
  authorization.
- A mirror `Scan` under the same policy must return `Count == 1`. The stream
  verdict is only meaningful if the table reaches the same one.

That mirror assertion is also what caught a **wrong assumption in the test
itself**, and it is worth recording. The first version wrote `{"pk": "a"}` twice
with identical content; the Scan returned 2, not 1. An empirical probe (scan
documents printed beside `Engine::get_document`, then the same run with the
second write changed) showed why: **a PutItem that rewrites identical content
leaves the document unmodified, `update_time` and all**, so `a` had never been
modified and the rule correctly admitted it. The authorization path was right and
the test was wrong; the fix was to make the second write a real modification, not
to weaken the assertion.


## FU-P3 — three findings from the review of FU-P2

The review of FU-P2 accepted three findings against the GetRecords path. All
three are fixed here in one commit: they are one review pass over one surface.

### (1) The page-fill loop permitted severe request amplification

Filling a page walks *past* records the caller may not read, so the loop needed
a bound. The bound it had was `MAX_EVENTS_EXAMINED = 10 * MAX_GET_RECORDS`, a
constant tied to the maximum page size rather than to what the caller asked
for. That made a dense run of withheld events into an amplification lever: a
caller polling for a **single** record could induce a scan of up to 10,000
stored events, and because each poll re-scans from its own iterator, it could
do so on every poll. Reading one event at a time — `wanted` is the number of
records still needed — that is also up to 10,000 store reads for one call.

The budget is now relative to the requested limit:

```rust
const EVENT_EXAMINATION_AMPLIFICATION: usize = 4;
...
let examination_budget = limit.saturating_mul(EVENT_EXAMINATION_AMPLIFICATION);
while records.len() < limit && examined < examination_budget {
```

**The events ceiling.** One call examines at most
`EVENT_EXAMINATION_AMPLIFICATION * limit` stored events. The worst case across
all callers is `4 * MAX_GET_RECORDS` = 4,000 events, and reaching it requires
asking for the largest page DynamoDB allows — a caller asking for one record
gets a budget of four.

This bounds the events read but *not* the number of store reads spent reading
them, which is the gap FU-P4 closes.

Returning a short page is correct here in a way it would not be for a
limit-bearing scan: GetRecords always returns an advanced `NextShardIterator`
that has walked past every event the fill consumed, so a short page means "poll
again" rather than "the stream is drained" and the consumer still drains the
stream at a bounded rate per poll.

### (2) A no-op MODIFY reconstructed the wrong `_updateTime`

FU-P2 stamped the reconstructed new image with the event's commit timestamp
unconditionally. That is right for a real modification and wrong for a write
that changes nothing — and this repo's own FU-P2 investigation had already
recorded that a PutItem rewriting identical content leaves `update_time`
untouched. The consequence is a reconstruction that disagrees with what a table
read returns, so a lifecycle rule reaches a different verdict for the record
than for the item it describes: under `_creationTime == _updateTime` the item
is admitted and its MODIFY record is withheld.

**Where the suppression actually happens.** This was located rather than
guessed. It is not in the storage backends —
`nimbus-storage/src/sqlite/write.rs:583` and
`nimbus-storage/src/memory/documents.rs:372` both assign `update_time`
unconditionally. It is in the execution-unit write path, which is the path the
DynamoDB adapter uses, at
`crates/nimbus-engine/src/engine/execution_units/batch.rs:857`:

```rust
fn preserve_document_lifecycle_times(existing, current, update_time) {
    if let Some(existing) = existing {
        current.creation_time = existing.creation_time;
        current.update_time =
            if existing.fields == current.fields && existing.typed_fields == current.typed_fields {
                existing.update_time
            } else {
                update_time
            };
```

So the engine's rule is exactly field-map equality **and** typed-field-map
equality. That makes this a predicate to mirror, not a heuristic to invent.

**Why capture, not reconstruction.** Reconstruction can compare the two stored
images, but that is not the same predicate: it cannot see the replaced
document's typed fields, and the stored old image is a `fields → item → fields`
round trip of the real one. Both gaps fail in the *disclosure* direction — they
would report "unchanged" where the engine stamped a new time, admitting a
record the table read denies. Capture has no such gap: it holds the replaced
`Document` and the fields about to be written. `OldImage` now carries them, and

```rust
fn retains_update_time(&self, new_fields: &Map<String, Value>) -> bool {
    self.typed_fields_empty && self.fields == *new_fields
}
```

mirrors the engine rule directly (every DynamoDB write sets an empty
typed-field map, so a replaced document that had typed fields is necessarily
modified). `event_fields` records the answer as `new_image_retained_update:
Option<u64>` — `Some(retained_time)` for a lifecycle no-op, `None` when the new
image takes the commit timestamp, which `committed_at` already recovers on
read.

This is team-lead's "better" option and it is **not** circular. Capture never
needs to know the commit timestamp: in the retained case the resulting
`_updateTime` is the old one, which capture has; in every other case the reader
derives it from the event document's own creation stamp.

### (3) Raw separation sleeps replaced with bounded semantic waits

Three `std::thread::sleep(Duration::from_millis(5))` calls stood between writes
that had to land in different milliseconds. Five milliseconds is a guess about
a clock: a coarse or adjusted clock, or a loaded machine, can defeat it, and
the test then fails despite correct behavior.

Both files now wait on the observable quantity with a 5s deadline and a loud
assertion on timeout. The in-crate helper waits past a stamp read from the
stored document itself:

```rust
fn wait_for_commit_clock_past(stamp: Timestamp) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while now_millis() <= stamp.0 {
        assert!(Instant::now() < deadline, "...");
        std::thread::yield_now();
    }
}
```

The integration test has no direct document access, so
`wait_for_next_commit_millisecond()` marks the clock immediately after a write
returns — at or past that write's stamp — and waits for the clock to pass it.
Commit stamps are `max(now, previous)`, so a wall clock past the mark puts
every later commit past it too. The existing diagnostic assertions (the
`Scan` `Count` mirror, the `assert_ne!` on the stored document's stamps) are
kept: they are what fails loudly if a clock never advances.

### Fail-before (RED)

Both production behaviors were reverted at once — `updated: event.committed_at`
and `EVENT_EXAMINATION_AMPLIFICATION = 10 * MAX_GET_RECORDS` — and the three
new tests run against that tree:

```
Summary [0.841s] 3 tests run: 0 passed, 3 failed, 289 skipped
  FAIL principal_authorization get_records_admits_a_no_op_rewrite_the_table_read_still_admits
  FAIL commands::stream::tests::a_no_op_write_reconstructs_the_retained_update_time
  FAIL commands::stream::tests::a_small_page_request_is_bounded_when_records_are_withheld
       assertion `left == right` failed: a one-record poll must examine 10000
       stored events, not walk the whole run of 40 withheld ones
         left: 40
        right: 10000
```

The amplification failure is the clearest statement of the finding: under the
old cap a one-record poll walked every one of the 40 withheld events, and would
have walked 10,000 had they been there.

`stream.rs` was restored from a scratchpad copy and verified by SHA-256
(`8238ef88…`, `RESTORE_OK=1`), not with `git checkout`.

### Tests added

`crates/nimbus-dynamodb/src/commands/stream/tests.rs`:

- `a_small_page_request_is_bounded_when_records_are_withheld` — 40 events, all
  withheld by a real per-document predicate, polled with `Limit: 1`. Asserts the
  page is empty *and* that the returned iterator advanced by exactly
  `EVENT_EXAMINATION_AMPLIFICATION`. Pinning the advance, not just the page
  size, is what makes this a statement about work performed.
- `a_no_op_write_reconstructs_the_retained_update_time` — writes identical
  content twice in different milliseconds, asserts the stored document's stamps
  are still equal, that the rewrite still emitted a MODIFY, and that **both**
  reconstructed images carry the stored document's times.

`crates/nimbus-dynamodb/tests/principal_authorization.rs`:

- `get_records_admits_a_no_op_rewrite_the_table_read_still_admits` — the
  converse of `get_records_evaluates_lifecycle_times_against_the_real_document_history`.
  Under `_creationTime == _updateTime` the `Scan` mirror must return `Count` 2
  and the stream must return `["a", "a", "b"]`, so record and item verdicts are
  asserted to agree rather than each being checked alone.

Two helpers were generalized rather than duplicated: `restrict_events_reads`
and the new `withhold_events_reads` now share `set_events_read_rule`.

### One incidental doc correction

`StoredEvent`'s comment said keys and images are stored "in AttributeValue
wire-JSON". They are not — `event_fields` stores `item_to_fields(...)` output,
the same field encoding the data path writes, and `shape_record` re-encodes on
the way out. The stale comment cost real time during this work (it is why the
image-equality question had to be re-derived from the code), so it is corrected.

## FU-P4 — the store-read ceiling was documented but not enforced

### What was wrong

FU-P3 bounded the *events* one GetRecords call may examine and claimed a second
bound alongside it: at most `EVENT_EXAMINATION_AMPLIFICATION` store reads. The
loop did not enforce that. Each read was sized by the output slots still to
fill:

```rust
let wanted = (limit - records.len()).min(examination_budget - examined);
```

`limit - records.len()` is small exactly when the page is nearly full, so a
caller who arranges for a first window to fill all but one slot and follows it
with events they may not read drains the budget one event per store read. With
`Limit=1000`, 999 authorized records in the first window and a withheld tail,
that is one 1,000-event read followed by ~3,000 single-event reads: **~3,001
backend scans for one request**, repeatable by replaying the same iterator. An
authenticated caller with write access to their own table can arrange the
distribution deliberately.

The events budget held throughout — the pathology was never in how much data
was read, only in how many round trips it took.

### The fix

Size each read by the budget remaining rather than by the slots remaining:

```rust
let wanted = limit.min(examination_budget - examined);
```

Every iteration but the last now spends a full `limit` of budget, so the read
count is `ceil(budget / limit)` = `EVENT_EXAMINATION_AMPLIFICATION`, by
construction and independent of how the caller distributes authorized,
withheld, and expired events.

The cost is that the final read may fetch events the page has no room for.
Those are simply not consumed: `next_sequence` advances only through the events
the loop walked, `reclaim_expired_events` still receives `&window[..consumed]`,
and the next poll reads them again. Over-reading cannot skip a record, and it
cannot reclaim one the iterator has not passed.

Both properties are now stated at `EVENT_EXAMINATION_AMPLIFICATION` as enforced
invariants, with the reason the slot-sized form fails recorded there — it is
the form a reader would otherwise reach for as the tighter one.

### Making the ceiling observable

A read-count bound argued from the shape of a loop is the kind of claim that
was already wrong once here. `read_events_from` now increments a thread-local
counter under `#[cfg(test)]`, and `take_store_reads()` returns and clears it.
Thread-local rather than a global: a GetRecords call runs entirely on its
caller's thread, so parallel tests cannot pollute each other's count, which
makes the probe correct under bare `cargo test` as well as nextest.

### Fail-before (RED)

`wanted` was reverted to the slot-sized form with the new test in place:

```
$ NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-dynamodb --lib \
    -- commands::stream::tests::the_store_read_ceiling commands::stream::tests::a_small_page
test commands::stream::tests::a_small_page_request_is_bounded_when_records_are_withheld ... ok
test commands::stream::tests::the_store_read_ceiling_holds_when_a_page_stalls_one_slot_short ... FAILED
  assertion `left == right` failed: one call must spend its examination budget
  in 4 store reads; 31 means the reads were sized by the page slots left, not
  by the budget
    left: 31
   right: 4
test result: FAILED. 1 passed; 1 failed
```

31 = 1 + 30, the degeneration exactly: one full-sized read, then the remaining
30 budget spent one event at a time. Scaled from `limit=10` to `limit=1000`
that is the 3,001 the finding names.

Note the other test **passes** under the defect. `a_small_page_request_is_bounded_when_records_are_withheld`
polls for one record with everything withheld, and with one output slot the two
sizings coincide. A uniformly-withheld run cannot expose this; the finding
lives in the *mixed* distribution, which is why the new test exists rather than
an assertion added to the old one.

`stream.rs` was restored from a scratchpad copy and verified by SHA-256
(`72770c26…`, `RESTORE_OK=1`), not with `git checkout`.

### Tests added

`crates/nimbus-dynamodb/src/commands/stream/tests.rs`:

- `the_store_read_ceiling_holds_when_a_page_stalls_one_slot_short` — the
  adversarial distribution at `limit=10`: nine authorized events, then 40
  withheld ones, so the first window leaves exactly one slot to fill. Asserts
  the store-read count is exactly `EVENT_EXAMINATION_AMPLIFICATION`, that the
  page is the nine-record short page, and that the iterator advanced over all
  40 consumed events.
- `a_small_page_request_is_bounded_when_records_are_withheld` also gained a
  read-count assertion, so both bounding tests now pin work performed rather
  than only results returned.

Discriminating authorized from withheld events within one stream needed a
policy on something other than the primary key, so `set_events_read_rule` takes
a field name and the new `put_tagged` / `admit_only_readable_events` pair writes
and matches a `tag` attribute. The literal is AttributeValue wire JSON
(`{"S": "read"}`) because that is how `item_to_fields` persists attributes —
the same encoding the FU-P3 doc correction was about.

## Verification

All commands run with `set -o pipefail` so the recorded status is the command's,
not `tail`'s. macOS, `stable-aarch64-apple-darwin`.

The battery was run five times: after FU3 + FU4, after FU-P1, after FU-P2,
after FU-P3, and after FU-P4. The table records the **final** run, over all six
concepts.

Note on the idiom: `${PIPESTATUS[0]}` is bash-only and expands to nothing under
this shell (zsh), which prints an empty `RC=` that reads like a missing result.
`set -o pipefail` plus plain `$?` is the form used here.

| Command | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb` | **293 run, 293 passed, 0 skipped** — `DDB_RC=0` |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-core` | **194 run, 194 passed, 0 skipped** — `CORE_RC=0` |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-engine` | **665 run, 665 passed (2 slow), 5 skipped** — `ENGINE_RC=0` |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-server -E 'test(dynamodb) or test(ddb)'` | **15 run, 15 passed, 590 skipped** — `SERVER_RC=0` |
| `make clippy` (workspace, `-D warnings`) | `CLIPPY_RC=0` |
| `cargo fmt --all --check` | `FMT_RC=0` |

The dynamodb count moves 281 → 283 → 289 → 292 → 293: FU-P1 replaced one test
with three, FU-P2 adds three in-crate plus three integration tests, FU-P3 adds
two in-crate plus one integration test, and FU-P4 adds one in-crate test.

FU-P3 widened the lint lane from three named crates to `make clippy` over the
whole workspace. The FU-P2 change to read-filter compilation is on every
adapter's read path, so a crate-scoped lint was narrower than the change.

**nimbus-core joins the battery from FU-P2 on**, and not as a formality. That
commit changes read-filter compilation, which every scan and query on every
adapter goes through — the blast radius is the whole read path rather than the
DynamoDB surface, so the core and engine suites are the load-bearing evidence
here, not the adapter's.

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
- The FU-P2 engine run showed **1 failure**, a *third* test in the same
  subsystem:
  `tests::mutation_journal::durable_outcomes::trigger_cursor_unreadable_progress_evicts_and_replays`.
  It passed 5/5 in isolation, and the full suite was then rerun against the
  **byte-identical tree** and came back 665/665. Same tree, different result — so
  it is nondeterministic by direct evidence, and no revert was needed to
  attribute it. The circumstances say why: a sibling worktree's `nextest` was
  saturating the machine during the failing run, and the two flakes above are the
  same subsystem under the same kind of load. Third sighting; FU5/FU9 should be
  scoped to cover it.

### Restoration discipline

All five fail-before captures reverted production code temporarily, and so did
two of the three flake attributions. Every time, the files were restored from
copies saved to the session scratchpad and then compared byte-for-byte — never
with `git checkout -- <file>`, which restores from HEAD and would have destroyed
the rest of the uncommitted work in these files.

FU-P2's revert used `git stash push -- <the eight source paths>`, leaving both
test files in the tree, and `git stash pop` to restore. Ten files were then
verified byte-identical against copies saved beforehand (`RESTORE_OK=1`); the
stash was chosen over per-file copying because eight interdependent files had to
move together and back together.

FU-P3's revert was a single file (`stream.rs`) carrying both reverted
behaviors, so a plain scratchpad copy sufficed; restoration was confirmed by
SHA-256 equality with the pre-probe file rather than by `cmp` alone.

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
