# SUC3.1 Step 1 — Shared SQL Store Core (PostgreSQL + MySQL)

Branch `codex/suc3-provider-facade`, based on `origin/main` @ `c4adee822`.
Scope: PostgreSQL and MySQL only. libsql and sqlite are untouched, by design.

## What Changed

`postgres/write.rs` and `mysql/write.rs` each carried a near-identical
store-level wrapper layer above the shared in-transaction seam
(`sql/write_core.rs`): open a write transaction, run exactly one closure
against it, and shape the resulting `TenantWriteCommit` into a public entry
point. Both copies are replaced by one copy in `sql/store_core.rs`.

The new module owns three things:

1. **`SqlWriteTransactionCore`** — the transaction-concept seam. Required
   methods only (`begin_scheduled_execution`, `set_prepared_record`,
   `advance_fenced_committer_lease`, `apply_resolved_write`,
   `prune_retained_versions`, the scheduler and schema operations, …). Each
   backend's transaction type implements it by forwarding to the inherent
   method of the same name; inherent methods win method-call resolution, so
   the forwarding is not recursive. This mirrors the convention already used
   by `SqlWriteBackend`, which `SqlWriteTransactionCore` extends.
2. **`SqlStoreCore`** — the store seam. Six required methods per backend
   (the `execute_write`/`execute_write_cancellable` bridge, `retention_floor`,
   `pipeline_metrics`, and four journal reads) carry **40 default methods**
   holding the wrapper bodies: `apply_prepared_write_batch` and its fenced
   variant, retention watermarks and `compact_retained_versions`, PITR
   export/import plus the fenced import, schema replace/delete plus fenced
   variants, the durable-records append/apply pair plus both fenced forms,
   the seven scheduler wrappers, both execution-unit batch wrappers, and the
   full insert/update/delete family including the `once` and `once_at` forms.
3. **`SqlBlockingWriteExecutor<S>`** plus `sql_execute_read` /
   `sql_execute_read_cancellable` — the single copy of what
   `postgres/storage.rs` and `mysql/storage.rs` each duplicated: the
   semaphore-bounded `spawn_blocking` executors behind `TenantReadStorage` /
   `TenantWriteStorage`. `{Postgres,MySql}BlockingWriteExecutor` are now type
   aliases to it.

`{postgres,mysql}/backend.rs` lose their duplicate `expect_write_commit` and
`apply_schedule_ops_in_transaction` helpers, and
`{postgres,mysql}/trigger_invocations.rs` now take
`FENCED_COMMITTER_LEASE_MARKER` and `map_fenced_write_result` from
`sql/store_core.rs` instead of from their own provider's `write.rs`.

### What deliberately did not move

Dialect stays in each backend: SQL text and parameter binding, connection and
transaction types, how a transaction is begun (PostgreSQL takes an advisory
lock, MySQL retries a contended begin), the tokio-runtime bridge PostgreSQL
uses to reach its async driver, MySQL's microsecond lease conversion in
`mysql/committer_lease.rs`, and the per-dialect in-flight ceiling
(`POSTGRES_MAX_IN_FLIGHT_OPERATIONS = 2` vs `MYSQL_MAX_IN_FLIGHT_OPERATIONS = 1`,
never equalized).

`sql/write_core.rs` is byte-for-byte unchanged. It still owns
`BEGIN`/`COMMIT`/`ROLLBACK` and, in `sql_commit`, sole ownership of the two
commit-path fault points (`StorageCommitBeforeVisibility` and
`StorageCommitAfterVisibilityBeforeReturn`) — so fault-point placement and
behavior are identical for every provider. No shared path gained a
read-after-commit. `traits/provider_impls.rs` and nimbus-engine are unchanged.

## Deviations From The Brief

Six places where the "identical" code was not identical, or where the plan's
projection did not survive contact with the code.

### 1. The wrapper layer is public API, so a facade is required (largest deviation)

The brief treated the wrapper layer as internal to nimbus-storage. It is not:
nimbus-engine calls these methods **inherently on the concrete store types**
through its persistence dispatch — `crates/nimbus-engine/src/persistence/tenant/{writes,schema,journal,committer_lease}.rs`,
`engine/mutations/durable_batch.rs`, `engine/queries/journal.rs`,
`tenant/committer_lease.rs`, and `replica.rs` all do so, via
`match_tenant_persistence!` or a direct `Self::Postgres(store) => store.…`
arm.

Moving those names onto a `pub(crate)` trait therefore narrowed a public API.
The compiler surfaced this as a 38-error dead-code cascade (`-D unused`):
every default method reachable only from in-crate tests, plus every
transaction-side required method reachable only through those defaults, plus
both providers' now-orphaned inherent `set_prepared_record` /
`set_trigger_write_origin` / `set_commit_timestamp`. At HEAD these were `pub`
inherent methods on public structs, which is why dead-code never fired.

Resolution: `sql_store_core_facade!(<StoreType>)` in `sql/store_core.rs`
re-exposes all 40 wrappers as inherent `pub` methods on each store, each body
a single trait-qualified call. The public API is byte-identical, callers need
no import, and method resolution stays unambiguous where a store also
implements a provider trait with the same method name (`DurableJournal`,
`FencedDurableApply`). **Zero edits to nimbus-engine and zero edits to
`traits/provider_impls.rs`** — an earlier attempt that made the traits
`pub(crate)` and qualified the calls inside `impl_durable_journal!` was
reverted in favor of this.

The rejected alternative was making `SqlStoreCore` and
`SqlWriteTransactionCore` `pub` and re-exporting them from `lib.rs`. That
saves the facade's ~341 lines but forces `SqlWriteBackend` (the dialect seam
the brief wanted left alone) and `SqlWritePipelineMetrics` public to satisfy
`private_bounds`/`private_interfaces`, and it requires `use` lines in
nimbus-engine. Worse encapsulation for ~340 lines; not taken. It remains
available if SUC3 later wants it.

### 2. `execute_write` bounds and bridge differ

PostgreSQL requires `T: Send + 'static` and `Check: Fn() -> Result<()> + Send + 'static`
and bridges through `bridge_tokio_runtime`; MySQL's inherent methods are
unbounded and run inline. The trait adopts PostgreSQL's stricter bounds —
MySQL's looser method accepts them, not the reverse. Both stores keep their
own bridges as inherent methods, unrestructured.

### 3. Fenced append-and-apply has two different accounting boundaries

Not cosmetic. PostgreSQL runs one pipelined
`append_and_apply_durable_records_batch` and records pipeline progress
*after* it. MySQL records `pipeline_batch_admitted` at admission inside
`append_durable_records_batch_with_admission`, then applies in a separate
step. Unified as one trait method,
`append_and_apply_fenced_durable_batch(&mut self, records, on_pipeline_progress: &mut dyn FnMut())`,
with each provider invoking the callback at its own boundary. Both
behaviors are preserved exactly; the shared wrapper drives an
`Arc<AtomicBool>` progress flag either way.

### 4. `apply_execution_unit_batch_with_origin` cloned `trigger_write_origin` in different places

PostgreSQL cloned outside the closure, MySQL inside. Unified on
PostgreSQL's form, which the `'static` closure bound requires anyway.
Behavior identical.

### 5. `compact_retained_versions` used a non-`move` closure on MySQL

Unified on `move`. Both captures are `Copy`, so this is a no-op at runtime.

### 6. Cosmetic-only differences

`delete_cron_job(name.as_str())` vs `delete_cron_job(&name)`; a
`serde_json::Value` spelled through MySQL's local alias in one signature.
Unified on the explicit form.

## LoC Delta

`git diff --stat c4adee822..HEAD`: **12 files changed, 1818 insertions(+),
1665 deletions(-)** — net **+153 lines**.

| File | Before | After |
| --- | --- | --- |
| `sql/store_core.rs` | — | 1490 (new) |
| `postgres/write.rs` | 1902 | 1365 |
| `mysql/write.rs` | 1896 | 1366 |
| `postgres/storage.rs` | 214 | 106 |
| `mysql/storage.rs` | 214 | 106 |
| `postgres/backend.rs` | 1301 | 1277 |
| `mysql/backend.rs` | 1510 | 1486 |

The brief projected ≈ −1,150. That projection assumed no facade layer and no
per-backend forwarding impls. The measured accounting:

- Duplicated code deleted from the two providers: **1,331 lines**
  (537 + 530 in `write.rs`, 108 + 108 in `storage.rs`, 24 + 24 in `backend.rs`).
- `sql/store_core.rs` = **1,490 lines**, of which **~1,148** is the single
  shared copy of that logic plus both trait declarations, and **342** is the
  facade macro (lines 943–1284: a 9-line doc comment plus a 333-line
  `macro_rules!` block containing 40 `pub fn` signatures, each with exactly
  one trait-qualified forwarding call — declaration only, no logic).
- Remaining insertions in the provider files are the two forwarding impl
  blocks (`SqlStoreCore` and `SqlWriteTransactionCore`), which are signatures
  only.

So the *logic* went from two copies to one, and every line of the new
plumbing is signature-shaped. Raw LoC is roughly flat because the plumbing
cost (facade + forwarding impls ≈ 660 lines) nearly offsets the ~800 lines of
body deduplication at two providers.

### Cumulative plan target, recomputed

The −1,150 projection was a step-1 number, and it was wrong for step 1 because
it assumed the wrapper layer was internal. It is not wrong for the lane as a
whole — it was simply booked one step early. Two providers is the break-even
case for this refactor: you pay the shared body once and the plumbing twice.
Every provider after the second pays plumbing only.

Measured per-provider costs, which are what step 2 should be estimated from:

| Item | Cost |
| --- | --- |
| Shared body in `store_core.rs` (paid once, already paid) | ~1,148 |
| Facade macro (paid once, already paid) | 342 |
| Per-provider forwarding impls (`SqlStoreCore` + `SqlWriteTransactionCore`) | ~160 |
| Per-provider facade invocation | 1 |
| Per-provider duplicated wrapper layer deleted | −530 to −665 |

Step 1 therefore lands at **+153**: 1,490 fixed cost plus ~320 of forwarding,
against −1,331 deleted. Step 2 adds no fixed cost. Porting libsql and sqlite
pays ~160 lines of forwarding each and deletes each provider's own wrapper
layer, so each is worth roughly **−370 to −505**, putting the lane at
**≈ −590 to −860 cumulative** once all four SQL providers share the core.

That is below the original −1,150, and the gap is the facade: 342 lines that
exist only because the wrappers are engine-visible public API. Making the two
traits `pub` and re-exporting them from `lib.rs` would recover it, at the cost
of publishing `SqlWriteBackend` and `SqlWritePipelineMetrics` — see deviation 1.
The tradeoff is available at any time and gets cheaper to take in step 2, when
the same decision would otherwise be re-paid per provider.

## Verification

Every command run with `set -o pipefail`. Fixtures were live for the provider
lanes; see the caveat below on the fixture-less run.

| Lane | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-storage` | 435 run, **435 passed**, 2 skipped |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-engine` | 659 run, **659 passed**, 5 skipped — but see the flake below |
| Live PostgreSQL lane (official filter, storage + engine) | 76 run, **76 passed**, 1025 skipped |
| Live MySQL lane (official filter, storage + engine) | 46 run, **46 passed**, 1055 skipped |
| Live libsql lane — untouched-provider regression | 50 run, **50 passed**, 1051 skipped |
| Live `nimbus-system` provider trio (pg + mysql + libsql arms of the lane filter) | 3 run, **3 passed** |
| `cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings` | clean, exit 0 |
| `cargo fmt --all --check` | clean |
| `cargo check --workspace --all-targets` | clean |

### A pre-existing engine flake, bisected to base

Re-running the battery surfaced an intermittent failure that the first pass
did not hit:

```
FAIL nimbus-engine tests::mutation_journal::arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher
crates/nimbus-engine/src/tests/mutation_journal/arm_selection.rs:229:5
assertion `left == right` failed
  left: 3
 right: 2
```

Line 229 is `assert_eq!(journal.len(), 2)` — a third journal record appears
where the test expects two. It only fails under full-suite load; run alone
under `-E test(...)` it passes every time.

**It is not from this change, and that was measured, not inferred.** Detaching
the same worktree to base `c4adee822` and running the identical full engine
suite four times reproduced the identical assertion with the identical
`left: 3, right: 2` on run 1 of 4. On this branch the rate was 2 of 4. Both
samples are small and the difference between them is not meaningful; what
matters is that the failure exists at base.

The structural argument agrees: the test builds its engine through
`Engine::new`, which is `EmbeddedProviderKind::default()` = `Sqlite`. This
commit changes no line reachable from the SQLite provider — `src/sqlite/` is
diff-empty, `sql/write_core.rs` is diff-empty, and `sql/store_core.rs` is
referenced only by the PostgreSQL and MySQL stores.

The flake is out of scope for SUC3.1 and is not fixed here, but it is real and
should be tracked: a race in which a trigger or schema record lands in the
journal before the ordered publisher drains, despite the
`shutdown_trigger_candidates_for_testing` call at the top of the test.

### The fixture-less storage run is not provider evidence

Of the 435 tests in the fixture-less `nimbus-storage` run, **70 are the
PostgreSQL and MySQL provider suites, and they report PASS without touching a
server**: `support.rs::test_connection()` returns `None` when the fixture URL
env var is absent, and each test returns early. This is the documented
false-green trap. The provider evidence in this proof is the live-fixture
rows, not that run.

### How the live lanes ran

`scripts/external-provider-fixture.sh up all` with
`NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=55432` and
`NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=53306` (the default 5432 is taken by the
developer's persistent `compose.yaml` Postgres on this machine), then
`cargo nextest run --profile ci-pr --no-tests fail` with
`NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1` and the official per-provider
filters from `scripts/test-external-providers.sh`, split so that
`nimbus-storage`/`nimbus-engine` and `nimbus-system` ran as separate
invocations. Fixtures were torn down afterwards (`down all`).

`scripts/external-provider-fixture.sh run <provider>` itself could not be used
unmodified: its filter includes `package(nimbus-system)`, which pulls a
workspace-wide test build, and `nimbus-assets`' build script fails in a fresh
worktree until the UI and embedded-package payloads exist. Running
`npm install`, `npm run build -w nimbus-ui`, and
`npm run build:embedded-packages` cleared that, which is also what made the
workspace-wide `cargo check --all-targets` above possible.

Nothing was deferred to hosted CI. Hosted CI remains the merge source of
truth and re-runs these lanes in its service containers.

### sqlite / libsql / sqlite_foundation untouched

`git diff --stat c4adee822..HEAD -- crates/nimbus-storage/src/sqlite crates/nimbus-storage/src/libsql crates/nimbus-storage/tests/sqlite_foundation crates/nimbus-storage/src/tests/sqlite_foundation`
prints nothing: zero files changed under any of those paths. The live libsql
lane above passing 50/50 is the behavioral confirmation.

---

# SUC3.1 Step 2 — libsql Joins The Seam

Branch `codex/suc3-step2-libsql`, based on `origin/main` @ `b4924264e`
(step 1 is in main). Scope: libsql only. sqlite stays byte-untouched, and
PostgreSQL/MySQL change only where the shared seam had to grow to admit a third
dialect.

## What Changed

`libsql/write.rs` carried the same store-level wrapper layer that step 1
deleted from PostgreSQL and MySQL — roughly 680 lines of `execute_write(|tx|
…)` wrappers plus its own `map_fenced_write_result`, `expect_write_commit`,
`apply_schedule_ops_in_libsql_transaction`, and `apply_resolved_write`. All of
it is gone, replaced by `sql_store_core_facade!(LibsqlReplicaTenantStore)` and
three trait impls: `SqlStoreCore`, `SqlWriteTransactionCore`, and
`SqlWriteBackend`. `LibsqlReplicaWriteTransaction::commit` and `rollback` are
now two lines each, delegating to the shared `sql_commit` / `sql_rollback`.

| file | base | now | delta |
| --- | --- | --- | --- |
| `libsql/write.rs` | 1692 | 1219 | **−473** |
| `libsql.rs` | 1282 | 1280 | −2 |
| `libsql/backend.rs` | — | — | −15 |
| `sql/store_core.rs` | 1490 | 1528 | +38 |
| `sql/write_core.rs` | 315 | 368 | +53 |
| `postgres/write.rs` | 1365 | 1408 | +43 |
| `mysql/write.rs` | 1366 | 1404 | +38 |

Net across the nine production files: **751 insertions, 1069 deletions
(−318 lines)**. The new libsql test adds 135 lines, so the commit as a whole is
886/1069.

### Two seam changes libsql forced

1. **`commit_transaction` / `rollback_transaction` on `SqlWriteBackend`.**
   PostgreSQL and MySQL commit by issuing `batch_execute("COMMIT")` against a
   session. libsql owns a `libsql::Transaction` value that is *consumed* by
   `commit()`. `sql_commit` could not keep emitting a SQL string, so the
   commit and rollback verbs became seam methods. pg/mysql implement them with
   the same `batch_execute` they used before.

   One signature change falls out of this:
   `LibsqlReplicaWriteTransaction::rollback` was `pub fn rollback(mut self)`
   and is now `pub fn rollback(&mut self)`, matching the shape pg and MySQL
   already had. It has exactly one caller — the error arm of
   `libsql/write.rs:59`, which returns immediately after — and a second
   rollback would be a no-op anyway because `rollback_transaction` takes the
   `Option<Transaction>`.
2. **`pipeline_metrics` moved off `SqlStoreCore` into a new
   `SqlDurableJournalStore` supertrait**, implemented only by PostgreSQL and
   MySQL. The libsql store has no write-pipeline metrics object because it has
   no write pipeline: each durable-journal batch is a dedicated remote
   round-trip against the primary, not a replay through the write transaction.
   For the same reason libsql overrides the three store-level durable methods
   and does not implement `SqlDurableJournalTransaction`.

### What deliberately did not move

The remote-session layer (`libsql/remote.rs`), the replica cache and its
freshness machinery, the Hrana transport, and the three async remote
batch functions in `libsql.rs` are all unchanged. Per-dialect pipeline
constants are untouched. **No shared path gained a read-after-commit**: the
replica's Hrana session snapshot semantics mean a post-commit read can observe
an older snapshot, so `after_visibility` does local bookkeeping only — it
advances `required_cache_sequence` and sets `refresh_needed`, and reads
nothing.

## Behavior Changes

Three, all deliberate. The first was specified by the brief; the other two are
consequences of adopting shared code that was already correct for the other
dialects.

Fault semantics are otherwise byte-for-byte what libsql had before, which is a
constraint rather than an accident — see "Fault semantics held to base" below
for the two candidate changes that were implemented and then reverted.

### 1. libsql gains `FaultPoint::StorageCommitBeforeVisibility`

The replica's hand-rolled commit checked `JournalAppendBeforeDurableFlush`,
`JournalFlushBeforeVisibility`, and `StorageCommitAfterVisibilityBeforeReturn`,
but never `StorageCommitBeforeVisibility` — PostgreSQL, MySQL and SQLite all
had it. Joining `sql_commit` closes that gap.

New coverage:
`tests::libsql_provider::journal::libsql_pre_visibility_fault_rolls_back_and_leaves_the_store_writable`
arms the point through the store's own injector, asserts the fault is observed,
asserts the write is invisible and the remote durable head and commit log did
not move, and then asserts the store still commits a retry. Fail-before check:
with the single line `backend.check_fault(FaultPoint::StorageCommitBeforeVisibility)?`
removed from `sql_commit`, the test fails at the `expect_err`; restored from a
saved copy, it passes. The removal was an experiment on a saved copy and
`write_core.rs` was verified byte-identical afterwards.

Note that the assertion deliberately uses `latest_sequence()` and
`read_durable_journal_from`, not `journal_progress()`. `journal_progress`
mixes a remote `durable_head` with an `applied_head` read from the *local
replica cache*, so `applied_head` tracks cache freshness, not commit state: a
committed write leaves it stale until some later read crosses the barrier and
refreshes. That is pre-existing replica behavior, not a defect, but it makes
`journal_progress` the wrong oracle for "did this roll back".

### 2. A libsql cancel of a missing scheduled job now errors

`apply_schedule_ops_in_libsql_transaction` discarded the bool from
`cancel_scheduled_job`. The shared `apply_schedule_ops_in_transaction` raises
`Error::ScheduledJobNotFound(job_id)` instead. libsql now matches
PostgreSQL, MySQL and SQLite.

### 3. Errors before visibility roll the transaction back explicitly

The old code relied on `libsql::Transaction`'s drop. `sql_commit` calls
`sql_rollback` on the error path.

## Fault Semantics Held To Base

Two further changes were implemented, then reverted to hold libsql's existing
fault semantics exactly. Step 2's only sanctioned fault-semantics change is the
addition of `StorageCommitBeforeVisibility` in §1.

**`after_visibility` ordering.** Adopting `sql_commit` initially ran the
post-visibility hook *before* the `StorageCommitAfterVisibilityBeforeReturn`
check, so a fired fault still recorded the replica cache barrier. Base libsql
checks the fault first and skips the bookkeeping when it fires. `sql_commit`
now does the same, and carries a comment saying why: the point stands in for a
crash between visibility and return, which cannot have run local bookkeeping.
PostgreSQL, MySQL and SQLite are unaffected — their `after_visibility` is the
no-op default.

**`JournalFlushBeforeVisibility` scoping.** A single unified `check_fault` on
the libsql backend made every commit-path point records-scoped whenever the
transaction carried a prepared record. That matches base for
`JournalAppendBeforeDurableFlush` and `StorageCommitAfterVisibilityBeforeReturn`,
which base already records-scopes, but not for `JournalFlushBeforeVisibility`,
which base leaves tenant-scoped. `check_journal_append_faults` now calls the
store's tenant-scoped `check_fault` directly for the flush point. The asymmetry
looks unintentional and is worth revisiting, but not inside a refactor step.

Neither revert is observable through the PPSC injector: its
`check_for_durable_records` delegates straight to `check_tenant`, so
records-scoping and tenant-scoping are the same call for it today.

## Deviations From The Brief

**The engine's libsql special case was collapsed, which the brief did not ask
for.** `TenantPersistence::fenced_append_and_apply_durable_records_batch_cancellable`
had a libsql arm that called `check_cancel()` and then the *non*-cancellable
store method, because libsql had no cancellable variant. The new
`SqlStoreCore` impl supplies one whose body is the old wrapper verbatim with
`check_cancel()` moved inside, so the arm now matches PostgreSQL and MySQL.
This is the only nimbus-engine change in the commit (9 lines).

**One test was added.** The brief framed step 2 as a refactor plus one declared
behavior change, and did not ask for new tests. A newly reachable fault point
with no test exercising it is not a verified behavior change, so the test in §1
above was written to cover it.

## Verification

All commands run with `set -o pipefail`; none gate on a trailing `grep` or
`tail`. Docker was available, so all three provider fixtures ran locally
rather than deferring PostgreSQL and MySQL to hosted CI. Fixture ports were
overridden (`15432` PostgreSQL, `13306` MySQL, `18080`/`18081` libsql) because
a system PostgreSQL already holds 5432.

| lane | result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy -p nimbus-storage -p nimbus-engine --all-features --all-targets -- -D warnings` | RC=0 |
| storage + engine, **fixture-less** (`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`) | **1097 run, 1097 passed, 7 skipped**, RC=0 |
| storage + engine, **all three fixtures live** | **1097 run, 1097 passed, 7 skipped**, RC=0 (258s) |
| `tests::async_faults` | 5 run, 5 passed, RC=0 |
| `libsql_provider::journal` + `libsql_replica_provider::ppsc` | 18 run, 18 passed, RC=0 |
| `libsql_provider::journal` + `libsql_replica_provider`, 10× repeat | **10/10 clean, 31/31 each run**, RC=0 each |
| `bash scripts/check-docs.sh` | PASS — 108 pages link-clean |

The fixture-less run is recorded separately on purpose, and it is **not**
provider evidence: the disable flag makes each provider test print
`omitting … execution` and return, so those tests count as run without
touching a server. The all-fixtures row is the provider evidence.

One trap worth recording, because it cost a full suite run. Omitting the
disable flag does *not* silently skip the provider lanes — `provider_test_fixtures.rs:264`
panics with `tests require the pinned shared fixture; missing non-empty
environment variable(s): …`, producing 174 failures that look like a
regression across all three dialects at once. That fail-loud is the harness
enforcing "a skipped provider test is not a passing one". A fixture-less run
must pass the flag; a provider run must export
`NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1` plus the four URLs.

`cargo check --workspace --all-features --all-targets` does not complete on
this machine for reasons unrelated to the diff: `nimbus-runtime`'s build
script rejects the shared `target/debug/gn_out` prebuilt V8 as
non-pointer-compression, and `fuser` cannot find a system `fuse.pc`. The
scoped `-p nimbus-storage -p nimbus-engine --all-targets` check is green.

sqlite is byte-untouched. Note the paths: `nimbus-storage` has no `tests/`
directory at all — its test tree lives under `src/tests/` — so a diff naming
`crates/nimbus-storage/tests/sqlite_foundation.rs` is vacuously empty and
proves nothing. The five real paths:

```
$ git diff --stat b4924264e..HEAD -- \
    crates/nimbus-storage/src/sqlite \
    crates/nimbus-storage/src/sqlite.rs \
    crates/nimbus-storage/src/async_storage/sqlite.rs \
    crates/nimbus-storage/src/tests/sqlite_foundation \
    crates/nimbus-storage/src/tests/sqlite_foundation.rs
(no output)
$ … | wc -l
0
```

## The libsql PPSC Flake Is The Already-Ticketed Arm Theft

Repeat full-suite runs surfaced intermittent failures in the libsql PPSC
suite. Every one of them is the **same assertion** — `ppsc.rs:477`,
`PPSC atomic provider acknowledgement loss must require crash-and-replay;
fault snapshot: PpscStorageFaultSnapshot { active: false, visits: 1, fires: 1 }`
— carried by whichever seeded scenario happened to hit the timing window
(`libsql_ppsc_seed_97_diagnostic` in one run,
`libsql_ppsc_seeded_journal_differential` in another).

It did not reproduce on the final branch state: the 10× repeat of
`libsql_provider::journal` + `libsql_replica_provider` (which contains the PPSC
suite) was 10/10 clean at 31/31. Ten runs cannot clear a defect whose observed
rate is roughly 1 in 10, so this is consistency evidence, not an all-clear —
the argument that step 2 is not implicated is the structural one below, not
the run count.

This is a known defect, root-caused and ticketed on main at `2969a881e`
before step 2 began:

> **PPSC ack-loss arm theft (libsql lane flake root cause)** — SUC3.1 step-1
> CI + 40-run bisection — one-shot arm keys on tenant, so a concurrent
> `commit == None` transaction consumes it (unconditional
> `StorageCommitAfterVisibilityBeforeReturn` check), the real batch then
> commits clean on retry and the test asserts a crash that correctly never
> happened; fix: make `check_for_durable_records` discriminate on records and
> stop no-journal commits consuming the arm.

Step 2 cannot have caused or worsened it, and three independent checks say so:

1. **The new fault point is unreachable from PPSC.**
   `storage_fault_point_unchecked` maps `AcknowledgementLoss` to
   `StorageCommitAfterVisibilityBeforeReturn`, `ProviderTransient` to
   `JournalAppendBeforeDurableFlush`, and `DurableBeforePublish` /
   `PanicAfterDurable` to `None`. No `PpscInjectedFault` maps to
   `StorageCommitBeforeVisibility`, so it can neither be armed nor consume an
   arm.

2. **Records-scoping is a no-op for this injector.** The PPSC injector
   overrides `check_for_durable_records` but its body is `self.check_tenant(point, tenant_id)`
   — identical to the tenant-scoped path. So no scoping choice this step could
   have made, including the one it reverted, changes which arms are consumed.
   Discriminating on records is exactly the harness-side half of the ticket's
   fix, and it is still open.

3. **Visit counts are unchanged.** `sql_commit` checks
   `StorageCommitAfterVisibilityBeforeReturn` unconditionally once per commit,
   exactly as libsql's hand-rolled commit did. The journal points moved into
   `append_commit_entry` / `append_prepared_record`, whose only callers are the
   two call sites inside `sql_commit`, so those are still one visit per commit.

The failure also predates any libsql change. During the step-1 CI
investigation the same assertion in the same test appeared 1/10 on the step-1
branch and 0/10 both at its base and merged. The raw ratio looks incriminating
until you note that step 1's diff does not touch libsql at all (`d2a3596ff`:
"sqlite, and libsql are untouched") — it added the shared seam that libsql did
not yet use. A diff that cannot reach libsql's commit path cannot have caused a
libsql commit-path failure, which is what motivated the 40-run bisection the
ticket cites, and that bisection landed on arm theft rather than on either
diff.

**Verdict: pre-existing harness defect, already owned by the ticket above. Not
a step-2 regression, and not fixed here** — the recorded fix lands in
`nimbus-testing`'s PPSC injector and changes arm consumption for the
PostgreSQL and MySQL lanes too, which is a different change with its own blast
radius.

The unrelated `arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher`
load-flake (also ticketed) appeared once in the base sweep.

## Step 2 — Review Disposition

One P1: `sql_commit` runs `StorageCommitAfterVisibilityBeforeReturn`
unconditionally, including for `commit == None` transactions — the shape
behind the ticketed PPSC arm-theft flake. **Real, pre-existing, and
deliberately not fixed in this step**: base libsql behaves identically (the
step-2 contract was behavior preservation, and two of this step's own
fault-semantics drifts were reverted to honor it). The fix is assigned to
step 3, which owns commit-path fault-point placement (U4/U5): gate the
post-visibility fault (and records-scoped equivalents) on an actual visible
commit, alongside the harness half (PPSC `check_for_durable_records`
discriminating on records). Landing it there keeps step 2 reviewable as a
pure port and fixes the class once, in the code that will own it.
