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

# Step 3 — Commit Witness, Ownership Gate, Fault Gating

Three deliverables were dispatched: the U5 commit witness, the U4
commit-path ownership gate, and the arm-theft fault gating assigned by the
step-2 review. The first two shipped. **The third was implemented, refuted by
the engine suite, and reverted** — the section below is the evidence for that
call, because the refutation is the useful result.

## Arm-Theft Fault Gating: Implemented, Refuted, Reverted

### What was tried

The brief specified gating the post-visibility fault point on a visible commit:

```rust
if commit.is_some() {
    backend.check_fault(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
}
```

The stated rationale was that `StorageCommitAfterVisibilityBeforeReturn` models
a lost acknowledgement of a commit the caller must assume landed, so a
transaction that published nothing has no acknowledgement to lose — and that
checking it unconditionally let a no-op transaction consume the one-shot,
tenant-keyed arm meant for a concurrent durable batch.

The change was made, and three dialect regression tests were written and shown
to fail without it.

### Why it is wrong

`commit.is_some()` does not mean "this transaction changed something durable".
It means "this transaction appended a commit entry". Reading `sql_commit`:
`commit` is `None` whenever there is no prepared record *and* no buffered
tenant event. A transaction can write rows and still land there.

Three real cases do exactly that, and all three are load-bearing:

| Case | Writes durably | `commit` |
| --- | --- | --- |
| Schedule-only execution unit | scheduled-job rows | `None` |
| Trigger outcome without re-execution | outcome / dedup rows | `None` |
| Fenced durable journal batch (PPSC) | journal records, applied | `None` |

For each of these, an acknowledgement lost between visibility and return is
precisely the ambiguity the engine must escalate to crash-and-replay. Gating
the check on `commit.is_some()` silently removes fault coverage for all of
them. It does not merely fail to fix the arm theft; it disables the injection
site the arm was aimed at.

### Fail-after evidence

The full-workspace run with all three live fixtures surfaced seven
`nimbus-engine` failures. Re-run in isolation, away from suite load, all seven
reproduce deterministically:

```
Summary [81.134s] 11 tests run: 4 passed (2 slow), 7 failed, 657 skipped
```

```
FAIL mysql_ppsc_seeded_journal_differential
FAIL mysql ppsc_provider_takeover_extension_matches_postgres_mysql_and_libsql
FAIL postgres_schedule_only_execution_unit_reconciles_acknowledgement_loss
FAIL postgres_trigger_outcome_reconciles_acknowledgement_loss_without_reexecution
FAIL postgres_provider_publisher_ack_loss_is_classified_before_retry_fence
FAIL postgres_ppsc_seeded_journal_differential
FAIL postgres ppsc_provider_takeover_extension_matches_postgres_mysql_and_libsql
```

The panics name the mechanism directly:

```
the post-visibility acknowledgement-loss fault must fire exactly once
a lost provider acknowledgement must be terminally ambiguous: ()
PPSC atomic provider acknowledgement loss must require crash-and-replay;
  fault snapshot: PpscStorageFaultSnapshot { active: true, visits: 0, fires: 0 }
```

`visits: 0` is the decisive number. The PPSC arm was armed and never even
*visited*, which means the durable batch the arm targets is itself a
`commit == None` transaction. The premise of the brief fails on its own
intended target.

These seven were not attributed to the known flake. Only
`mysql_ppsc_seeded_journal_differential` was previously ticketed as an
arm-theft flake (step 2); the other six are new, deterministic, and
attributable to this one-line change alone.

### What was reverted

`sql_commit` is back to the unconditional check, with a comment recording why
it must stay unconditional. The three dialect regression tests
(`{postgres,mysql,libsql}_no_op_transaction_preserves_acknowledgement_loss_arm`)
and the shared `ArmedAcknowledgementLossFault` /
`exercise_no_op_transaction_preserves_acknowledgement_loss_arm` helpers were
removed with them: they assert the reverted behavior, so keeping them would
encode the defect as a requirement. `git diff` against `597d5d823` confirms the
three `journal.rs` files and the `tests.rs` helper region are byte-identical to
base; the only surviving `tests.rs` change is the `mod commit_path_ownership;`
line for the U4 gate.

### The real fix shape (not taken — needs a decision)

The arm theft is real, but neither half can discriminate today:

- **Product side.** The honest predicate is "this transaction performed at
  least one durable mutation", and no such signal exists at the `sql_commit`
  seam. Schedule-op, lease, and job-claim writes go through provider-specific
  inherent methods that are not on `SqlWriteBackend`, so a `mutated` flag would
  have to be set in each provider — reintroducing exactly the per-dialect
  divergence U4 exists to prevent, with a silent-fault-disable failure mode if
  any site is missed.
- **Harness side.** `PpscStorageFaultInjector::check_for_tenant` and
  `check_for_durable_records` both funnel into one `check_tenant`, so the
  injector cannot tell the two apart. A `records.is_empty()` guard would be a
  provable no-op: libsql always passes exactly one record via
  `std::slice::from_ref`, and PostgreSQL/MySQL reach the post-visibility point
  through `check_for_tenant` with no records in hand at all.

Making the harness half work therefore *requires* the product to pass durable
records (or an equivalent batch identity) into the post-visibility check on all
three dialects. That is a fault-interface change spanning
`nimbus-storage` and `nimbus-testing`, not a one-liner, and it is a design
decision above this step's scope. Recommended as its own ticket; the existing
`mysql_ppsc_seeded_journal_differential` flake ticket should be folded into it.

## U5 — The Commit Witness

`crates/nimbus-storage/src/sql/commit_effects.rs` adds `SqlCommitEffects` and
the single entry point `sql_apply_commit`. The struct has no `Default` and no
`Option` field, so a commit path must name every effect, including the ones it
does not perform:

```rust
pub(crate) struct SqlCommitEffects {
    pub(crate) dedup: ExecutionDedup,
    pub(crate) lease: LeaseEffect,
    pub(crate) trigger_origin: TriggerOriginEffect,
    pub(crate) commit_timestamp: CommitTimestampEffect,
    pub(crate) documents: DocumentWrites,
    pub(crate) schedule_ops: ScheduleOps,
    pub(crate) journal: JournalEffect,
    pub(crate) watermark: WatermarkEffect,
}

pub(crate) fn sql_apply_commit<T: SqlWriteTransactionCore>(
    transaction: &mut T,
    effects: SqlCommitEffects,
) -> Result<SqlCommitAdmission>;
```

Every "skip" is a named variant a reviewer can see: `LeaseEffect::NotFenced`,
`ExecutionDedup::NotDeduplicated`, `ScheduleOps::NoScheduleOps`,
`WatermarkEffect::NotAdvanced`,
`JournalEffect::CommitEntryFromBufferedWrites`,
`TriggerOriginEffect::TransactionDefault`,
`CommitTimestampEffect::ProviderAssigned`. Adding a field breaks all three
construction sites, which is the mechanism the brief asked for.

`sql_apply_commit` also fixes the effect *order* in one place — gate, fence,
documents, schedule rows, prepared record — where previously each path retyped
it. That ordering was already drifting: `insert_once` runs the dedup gate
before setting the commit timestamp while `insert_with_indexes_once_at` sets
the timestamp first.

Refactored onto it: `apply_prepared_write_batch`,
`fenced_apply_prepared_write_batch`, and
`apply_execution_unit_batch_with_origin`.

### Deviation 1: five enums, not the five named

The brief named document writes / version effects / journal / watermark /
lease. **Version and index effects are not separately supplied at this seam**,
so fields for them would be fiction: they are executed inside the document-write
statements (`apply_durable_record`, `apply_resolved_write` in `write_core.rs`),
which is precisely what keeps them in the same storage transaction as the
document row, per the atomicity invariant. All three paths would set them to
the same value, and `sql_apply_commit` would have nothing to do with them. Each
`DocumentWrites` variant documents which version and index effects it carries
instead. The witness carries eight fields, of which every one is executed.

### Deviation 2: the direct path is not on the witness

The brief listed a fourth path. It is excluded, for a type-system reason rather
than a scheduling one. The direct writes carry caller validators
(`FnOnce(&Document, &Document) -> Result<()>`) and return payloads that differ
by operation — `()` for insert, the removed `Document` for delete. Putting them
behind the same exhaustive, data-only enum requires either a `Default` bound on
the payload (which the brief forbids) or erasing the document strategy into a
boxed closure (which destroys the reviewer-visible variant that is the whole
point). The composite paths have no such problem: their payload is uniformly
`bool`.

The six direct `execute_write` sites are therefore unchanged. This is a
decision for the lead, not a deferral: extending the witness to them means
accepting one of those two costs.

### Coherence check

`DocumentWrites`, `JournalEffect` and `WatermarkEffect` are declared separately
for reviewer visibility but are not independent — the document strategy implies
the other two. `check_effect_coherence` rejects the four mismatched pairings
before any statement runs, rather than letting them produce a wrong commit
entry or a stalled watermark. Covered by
`effect_coherence_accepts_only_the_pairings_the_document_strategy_implies`,
which asserts both accepted pairings and all four rejections.

## U4 — Commit-Path Ownership Gate

`crates/nimbus-storage/src/tests/commit_path_ownership.rs` adds
`u4_commit_sequence_fault_points_live_only_in_the_shared_sql_core`. It walks
`src/{postgres,mysql,libsql}/` recursively and fails on any occurrence of
`StorageCommitBeforeVisibility` or `StorageCommitAfterVisibilityBeforeReturn`,
with a message naming `sql/write_core.rs` as where the check belongs. Needles
are matched without a path prefix, so `FaultPoint::X`, `crate::FaultPoint::X`
and a bare imported `X` are all caught.

Anti-vacuousness, following the established `reachability_lint.rs` idiom: a
scanned-file floor (50 files today, floor 45) and a positive check that both
needles still exist in `write_core.rs`, so the gate fails loudly if the commit
sequence moves rather than passing by scanning nothing.

Proof it bites — appending
`// GATE PROBE: FaultPoint::StorageCommitBeforeVisibility` to
`postgres/read.rs`:

```
U4 violation — a provider checks a write-transaction commit-sequence fault
point outside the shared core. ... ["postgres/read.rs checks
StorageCommitBeforeVisibility"]
Summary: 1 test run: 0 passed, 1 failed
```

Restored from a saved copy afterwards.

### Deviation 3: `Journal*` is not gated, and the brief's premise was wrong

The dispatch stated the fault points already live only in the shared core
post-step-2 and asked the gate to cover `FaultPoint::StorageCommit*` **and**
`Journal*`. `StorageCommit*` is indeed core-only. `Journal*` is not, and should
not be:

| location | points | why it is there |
| --- | --- | --- |
| `postgres/write_pipeline.rs` (4) | append, flush | pipelined append/apply pair; progress reported once both complete |
| `mysql/write_pipeline.rs` (2) | append, flush | separate statements; progress at batch admission |
| `libsql/write.rs` (2) | append, flush | append and flush to the primary are one statement batch |

Journal fault-point placement is a genuine dialect axis — it follows where each
dialect's journal write physically happens — not forked shared logic.
`SqlDurableJournalTransaction::append_and_apply_fenced_durable_batch` already
documents these accounting boundaries as intentionally not unified. Gating
`Journal*` would require allowlisting all three files, which enforces nothing.
The gate's doc comment records this and says to extend it if journal batching
is ever unified.

Two further `StorageCommitAfterVisibilityBeforeReturn` checks sit in
`libsql.rs` (the module root, outside the gated directories) on the replica's
remote durable-batch round-trips. These are a different surface from the write
transaction — they are not part of the `sql_commit` sequence the gate owns — so
leaving them outside its scope is correct rather than an exemption.

## Sub-task 4 — libsql Journal Fault Scoping: No Change Needed

The follow-up list carried "libsql `JournalFlushBeforeVisibility` tenant-scoped
vs siblings records-scoped" as an asymmetry to fix if base intent was clear.
Base intent is clear, and it points the other way — the description had the
asymmetry backwards:

| dialect | `JournalAppendBeforeDurableFlush` | `JournalFlushBeforeVisibility` |
| --- | --- | --- |
| postgres | tenant-scoped | tenant-scoped |
| mysql | tenant-scoped | tenant-scoped |
| libsql | records-scoped when a prepared record exists | tenant-scoped |

The flush point is **consistently tenant-scoped across all three dialects** —
correct, since a flush is a session-level event with no single record to
attribute it to. The outlier is libsql's *append*, which is records-scoped, and
that is a deliberate refinement: libsql's append happens on the write
transaction with a prepared record in hand, so it can discriminate, while
PostgreSQL and MySQL append inside a pipeline handling batches. This matches
base (`b4924264e`) exactly. **No code change; the follow-up item is closed as
correct-as-is rather than deferred.**

## Step 3 — Verification

All lanes run against the three live fixture containers
(`nimbus-external-provider-tests-{postgres,mysql,libsql}-1`) with
`NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1`, so a missing fixture panics
rather than skipping silently. Every command used `set -o pipefail` and its own
exit code was captured directly, never through a trailing `grep` or `tail`.

| Lane | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | rc 0 |
| Clippy | `cargo clippy -p nimbus-storage -p nimbus-engine -p nimbus-testing --all-targets -- -D warnings` | rc 0 |
| Storage suite | `cargo nextest run -p nimbus-storage` | 438 passed, 2 skipped, rc 0 |
| Focused: async faults, U4 gate, witness coherence | `-E 'test(async_faults) + test(commit_path_ownership) + test(commit_effects)'` | 7 passed, rc 0 |
| Engine ack-loss / PPSC set | `-p nimbus-engine -E 'test(ppsc_seeded_journal_differential) + test(ppsc_provider_takeover_extension) + test(reconciles_acknowledgement_loss) + test(ack_loss_is_classified_before_retry_fence)'` | 11 passed, rc 0 |
| libsql PPSC repeat | `libsql_ppsc_seeded_journal_differential` × 10, `--test-threads 1` | 10/10 passed, rc 0 each |

The libsql PPSC repeat is 10-for-10 green, but that is *not* evidence the arm
theft was fixed — nothing in this step changed arm consumption. It is the
pre-existing flake failing to reproduce in 10 attempts, which is consistent
with how it was characterised in step 2.

### The seven-failure detour

The first full-workspace run of this step was taken with the (now reverted)
`commit.is_some()` gating in place and reported 290 failures. Most were the
known bare-macOS `nimbus-runtime` `node_compat` lane, but seven were
`nimbus-engine` acknowledgement-loss tests — the exact semantics the change
touched. Those seven were **not** written off as suite-load flakes. Re-running
them in isolation reproduced all seven deterministically, which is what refuted
the change and produced the revert documented above. After the revert the same
filter is 11-for-11 green.

### sqlite untouched

```
git diff --name-only 597d5d823 -- \
  crates/nimbus-storage/src/sqlite crates/nimbus-storage/src/sqlite.rs \
  crates/nimbus-storage/src/async_storage/sqlite.rs \
  crates/nimbus-storage/src/tests/sqlite_foundation \
  crates/nimbus-storage/src/tests/sqlite_foundation.rs
```

Empty. All five real sqlite paths are byte-identical to base.

### Full workspace run

```
cargo nextest run --workspace --no-fail-fast
Summary [1301.398s] 6015 tests run: 5704 passed (10 slow, 5 leaky),
                    283 failed, 28 timed out, 322 skipped
```

The whole workspace was run rather than a name-filtered subset, because the U4
gate is a new fail-closed gate and a filtered run cannot show its blast radius.

Failure attribution, every non-`nimbus-runtime` failure named:

| Failing test | Crate | Attribution |
| --- | --- | --- |
| 610 `node_compat` fixtures + 28 timeouts | `nimbus-runtime` | Known bare-macOS lane; CI is the evidence for these. Was 612 on the previous run — noise, not signal. |
| `embedded_nodefull_anchor_installs_from_committed_blob` | `nimbus-runtime::embedded_anchor` | Fails identically on the previous run of this same tree; pre-existing, unrelated. |
| `opaque_internal_job_cannot_overtake_ordered_publisher` | `nimbus-engine` | Pre-existing flake — see below. |

`nimbus-storage` had **zero** failures. So did every other workspace crate.

#### The arm-selection failure is a pre-existing flake, proven on base

`tests::mutation_journal::arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher`
failed once with `journal.len()` of 3 against an expected 2. It was not written
off:

1. **It is non-deterministic.** 8 isolated runs on this tree: 1 failed, 7 passed.
2. **It reproduces on base.** The step-3 change was detached with
   `git stash push --include-untracked` (verified: `git diff --stat 597d5d823`
   empty), and the test run 12 times at base `597d5d823`: 1 failed, 11 passed —
   the same rate and the same assertion. The change was then restored, and all
   seven step-3 files verified byte-identical to copies saved beforehand.
3. **The change cannot reach it.** The test builds its engine with
   `Engine::new(path)`, which uses the sqlite-backed store. `SqlStoreCore` is
   implemented by exactly three types — `PostgresTenantStore`,
   `MySqlTenantStore`, `LibsqlReplicaTenantStore` — and the sqlite modules
   contain no reference to `SqlStoreCore`, `sql_store_core_facade`, or
   `sql::store_core`. There is no causal path from this step's edits to that
   test.
4. **It did not fail in the previous full run** of a strictly more perturbed
   tree.

Recommend a separate flake ticket: the extra journal record points at a
background commit racing `shutdown_trigger_candidates_for_testing`, which is
engine-side and unrelated to storage unification.

## Step 3 — Review Disposition

Structured review of `dfebd1a6f` returned three findings. Two accepted with
modification and fixed in the follow-up commit; one rejected.

### Accepted 1 — `commit_effects.rs` module doc overclaimed its coverage

The module doc opened by calling `sql_apply_commit` "one entry point that every
composite SQL commit path goes through". That is false under U8: the direct
path is excluded by decision, so the witness covers three of the four
commit-log paths, not all of them. Left as written, the doc would have led a
future reader to assume a new `SqlCommitEffects` field forces every commit path
to declare a position on it.

Rewritten to name the three witnessed paths explicitly
(`apply_prepared_write_batch`, `fenced_apply_prepared_write_batch`,
`apply_execution_unit_batch_with_origin` — verified as exactly the three
`SqlCommitEffects` construction sites in the crate), to state the U8 exclusion
with its rationale, and to make the trade explicit: **adding a field here does
not force direct-path declaration.** That cost is the accepted price of keeping
the variants reviewer-visible, and it is now visible too.

### Accepted 2 — `Journal*` needed a pin gate, not nothing

The step-3 gate banned `StorageCommit*` outside the shared core and left
`Journal*` entirely ungated, on the argument that an allowlist "enforces
nothing". The reviewer's allowlist shape is the better call: an exact-count pin
per owner file does enforce something real — that journal fault-point placement
cannot change silently — without pretending the placement is shared.

`u4_journal_fault_points_stay_with_their_pinned_owners` pins the three owners
inside the scanned provider directories:

| Owner (relative to `src`) | Pinned token count |
| --- | --- |
| `postgres/write_pipeline.rs` | 4 |
| `mysql/write_pipeline.rs` | 2 |
| `libsql/write.rs` | 2 |

Counted needles are all three journal fault points
(`JournalAppendBeforeDurableFlush`, `JournalFlushBeforeVisibility`,
`JournalDurableAppendBeforeApply`); all eight occurrences today are real check
sites, none in comments. `sql/write_core.rs` holds zero journal tokens and is
intentionally absent from the pin list — the shared commit sequence does not
observe the journal. `libsql.rs` at the module root stays outside the scan, as
it does for the sibling gate.

Three failure modes, each with its own message: a **new** file holding a
journal token, a **count drift** in a pinned file, and a **pinned owner that
the scan no longer finds** (file renamed or moved). The message states the gate
is a pin rather than a ban and tells the reader to update `JOURNAL_OWNERS` in
the same commit when the change is intentional. The `StorageCommit*`
ban-needles are unchanged, and both gates now share one scan helper with the
same 45-file vacuousness floor.

#### Fail-before evidence, both pin shapes

New unpinned file — one journal token appended to `postgres/read.rs`:

```
Unpinned: ["postgres/read.rs holds 1 journal fault-point token(s) but is not a pinned owner"]
Drifted: []
Summary: 1 test run: 0 passed, 1 failed
```

Count drift — one token appended to a pinned file:

```
Unpinned: []
Drifted: ["mysql/write_pipeline.rs holds 3 journal fault-point token(s), pinned at 2"]
Summary: 1 test run: 0 passed, 1 failed
```

Both probe files were restored from copies saved before the probe, never via
`git checkout -- <file>`; `git diff` against base for both is empty. With them
restored, both gates pass: `2 tests run: 2 passed`.

### Rejected — "the arm theft is left unresolved"

Correct as a statement of state, not a defect in this step. The arm theft is
real and remains open, but the fix assigned for it was refuted with
deterministic evidence (seven engine tests, `visits: 0` on the arm's own
target), and the reason no replacement shipped here is that neither the product
nor the harness half can discriminate without a fault-interface change spanning
`nimbus-storage` and `nimbus-testing`. That ticket is open and folds in the
`mysql_ppsc_seeded_journal_differential` flake. Shipping a second wrong gate to
avoid leaving the item open would trade a known-open bug for a silent loss of
fault coverage — the exact trade the first attempt made.

## Step 3 — Reviewer Outage Disclosure

After the fix round (commit 14d51dee0), the structured reviewer could not be
re-run: the Codex engine hit account credit exhaustion (usage limit, resets
2026-08-04 — probe transcript captured), and the Claude engine fails on this
host with the known reviewer-sandbox proxy-CA trust error ("Self-signed
certificate detected"). Per the review-outage policy this is an outage, not a
verdict. The branch's substantive review pass DID run (two findings, both
fixed with red-probed gates and full re-verification); the shipped delta
beyond that reviewed tree is exactly those two fixes. Steps 4–5 of this lane
will face the same outage; each will disclose it and lean on verification +
CI lanes until an engine returns.

# SUC3.1 Step 4 — Transaction-Half Twins

Brief: dedupe the postgres↔mysql transaction-half twins across five named
targets — scheduler transaction ops, table lifecycle, trigger invocations,
document/index-version orchestration, and the `load_schema` region in `read.rs`
— behavior-preserving, with dialect placeholders, locking, and type binding
staying put, and libsql included only where its code is genuinely the same
shape. Planner estimate ≈ −800.

Delivered: **net −366**. Three of the five targets landed in full, one landed
in half, and one was measured and declined. Every decline is enumerated below
with the numbers that produced it.

## What Changed

Five new dialect-free modules under `crates/nimbus-storage/src/sql/`, plus two
concepts folded into the existing `store_core` seam.

| New shared module | Lines | Replaces |
| --- | --- | --- |
| `sql/read_snapshot.rs` | 367 | the whole-tenant read snapshot in both `read.rs` |
| `sql/index_history.rs` | 315 | the historical index-scan family in both `index_versions.rs` |
| `sql/predicate.rs` | 246 | filter/ordering/prefix predicates in both `query_helpers.rs` |
| `sql/scheduler_core.rs` | 108 | the claim loop and running-job recovery in both `write.rs` |
| `sql/schema_events.rs` | 67 | both `write_schema_events.rs` (files deleted) |

| Provider file | Before | After |
| --- | --- | --- |
| `postgres/read.rs` | 912 | 621 |
| `mysql/read.rs` | 948 | 636 |
| `postgres/query_helpers.rs` | 271 | 106 |
| `mysql/query_helpers.rs` | 357 | 194 |
| `postgres/index_versions.rs` | 718 | 551 |
| `mysql/index_versions.rs` | 739 | 572 |
| `postgres/trigger_invocations.rs` | 214 | 143 |
| `mysql/trigger_invocations.rs` | 222 | 151 |
| `libsql/trigger_invocations.rs` | 195 | 120 |
| `postgres/resource_paths.rs` | 443 | 399 |
| `mysql/resource_paths.rs` | 487 | 443 |
| `postgres/write_schema_events.rs` | 40 | deleted |
| `mysql/write_schema_events.rs` | 40 | deleted |
| `postgres/write.rs` | 1408 | 1423 |
| `mysql/write.rs` | 1404 | 1420 |
| `libsql/write.rs` | 1219 | 1248 |

### (a) Scheduler transaction ops — landed, statement SQL held per-dialect

`sql/scheduler_core.rs` declares `SqlSchedulerTransaction: SqlWriteBackend` with
four statement hooks (`select_due_jobs`, `move_job_to_running`,
`load_running_jobs`, `move_job_to_pending`) plus a defaulted no-op
`mark_scheduler_changed`, and owns the orchestration in two free functions:

- `sql_claim_due_jobs` — the batch guard, the `max_jobs == 0` short-circuit, the
  per-job cancellation checks, and the "empty claim marks nothing changed and
  issues no move statements" rule.
- `sql_recover_running_jobs` — the `job.run_at = job.run_at.min(now)` rule and
  the twelve-line comment explaining it, which had drifted into three copies.

The claim **statement** stays dialect per the CO6 comment: PostgreSQL relies on
its per-tenant advisory transaction lock and issues a plain `SELECT`; MySQL
serializes claimers with `FOR UPDATE`. Both live inside each backend's own
`select_due_jobs`. Timestamp binding also stays at the SQL edge — PostgreSQL
converts fallibly through `i64_from_timestamp`, MySQL binds raw `u64`
microseconds.

libsql is **excluded here on evidence, not preference**: its scheduler methods
have the same shape but a different borrow structure (it re-acquires the session
inside each `block_on` future rather than holding one across the call) and its
statements are inline literals with no qualified table names. Forcing it through
the seam would mean rewriting its transaction plumbing, which is not
behavior-preserving.

### (c) Trigger invocations — landed, and libsql joined

Two dialect hooks (`materialize_trigger_invocations`, `save_trigger_invocation`)
were added to `SqlWriteTransactionCore` and four store-level wrappers to
`SqlStoreCore` — the plain pair plus the fenced pair, which carry the
lease-CAS-then-write ordering and the `FENCED_COMMITTER_LEASE_MARKER`
precondition. Row encoding and upsert syntax stay dialect-owned
(`ON CONFLICT ... DO UPDATE` vs `ON DUPLICATE KEY UPDATE`).

This is the one target where libsql was genuinely the same shape, and it joined:
`libsql/trigger_invocations.rs` dropped 77 lines against 2 added, with the two
dialect hook impls landing in `libsql/write.rs`. Because
`sql_store_core_facade!` is applied to all three stores, libsql picks up the
shared wrappers automatically.

`validate_fenced_committer_lease` was added as a **dialect method, not a
default**: PostgreSQL reuses the advancing CAS with an unchanged sequence, and
MySQL cannot, because a no-op `UPDATE` reports zero changed rows — it locks the
lease row instead.

### (d) Index-version orchestration — landed

`sql/index_history.rs` declares `SqlHistoricalIndexStore` with **one** hook,
`visible_historical_index_entries`, and defaults every entry point around it:
the eq / prefix / range / composite-range families, their paged forms, and the
single `historical_index_scan_page_for_plan` they all bottom out in. Those eight
entry points were byte-identical between the two stores; the family reduces to
plan-then-load-then-page, and only the load touches a database.

Per-backend by design: each store's own
`visible_historical_index_entries_for_tuple_bounds` (storage-format validation,
tuple-bound SQL, `ToSql` boxes vs `MySqlValue` binding, row decoding). sqlite is
out of scope; the libsql replica has no historical index-scan family at all, so
its exclusion is natural rather than forced.

`sql_historical_index_facade!` re-exposes the **six `pub`** entry points as
inherent methods, preserving each store's public API exactly. The two
`*_range_page_cancellable` forms are omitted: they are `pub(crate)`, the trait
defaults reach them directly, and a grep of every caller confirmed neither has a
caller outside the family on these two stores (the other hits are sqlite's,
libsql's, and the memory store's own copies).

**A facade-removal experiment was tried and reverted.** Deleting the 90-line
facade and importing the trait in the two provider test modules produced 10
dead-code errors covering the whole family plus each backend's private helpers,
because as `pub` inherent methods they had been public API and were never dead.
Removing the facade narrows the crate's public surface, which is outside a
behavior-preserving dedup. The facade stands.

### (e) `read.rs` — landed as the snapshot, plus the triplicated validators

The `~231-line identical region` in the brief measures, on a normalized diff, as
two runs: **150 lines** (pg 473–622 ⇄ my 196–345) and **101 lines**
(pg 122–222 ⇄ my 95–195), totalling 251.

The larger structure behind them is the read snapshot, and it moved:
`sql/read_snapshot.rs` now owns the materialized whole-tenant image and every
accessor over it — documents, schema, journal progress, table identities,
resource-path bindings, scheduled execution ids. `PostgresReadSnapshot` and
`MySqlReadSnapshot` are aliases for it. How the snapshot is *filled* stays per
backend (PostgreSQL uses a `read_only` transaction, MySQL a `REPEATABLE READ`
one). The resource-path accessors folded in here too, which is why both
`resource_paths.rs` shrank by 44.

`sql/predicate.rs` takes the dialect-free predicates that both `query_helpers.rs`
carried — `matches_filters`, `compare_values`, `document_matches_exact_prefix`,
`document_matches_range_bounds`, `index_fields_for_table_schema` — which had
drifted only in formatting (`Ordering` imported vs spelled out, `matches!` vs
`==`), never in behavior. It also takes the two index-scan argument validators,
`validate_index_prefix_len` and `validate_index_range_prefix`, whose **rejection
messages are observable** and were spelled out identically in both stores *and*
again in the shared read snapshot — three copies of one user-visible string is
exactly the drift this campaign exists to prevent.

**The 101-line run did not move, and cannot behavior-preservingly.** It is
entirely connection acquisition — `get`, `table_id`, `scan_table_*`,
`load_schema` — where every line is `provider.client().await?` against
`provider.conn().await?`, `&client` against `&mut conn`. Sharing it needs a
session abstraction over `&Client` and `&mut Conn`, which is a driver-lifetime
change, not a dedup. `load_schema` itself is three statements around a cache
check; the cache publish/read pair is already shared.

## Declined Targets, With Numbers

### (b) `table_lifecycle` — declined, line-additive

`postgres/table_lifecycle.rs` (446) and `mysql/table_lifecycle.rs` (445) have a
byte-identical store half (lines 6–45) and a transaction half that differs only
in session plumbing plus PostgreSQL's extra
`self.notification.schema_changed = true;` inside `hard_delete_table_identity`.

The seam it would need: 4 row hooks, 3 schema forwards, 2 defaulted markers — 9
hooks. Removing ~100 duplicated lines per backend costs ~60 lines of hook impls
per backend plus ~105 lines in the shared file. The store half nets ≈ −8. **Total
≈ +17 — line-additive, for a 9-hook indirection.** Declined.

### (d, document-version half) — declined, nothing to share

`postgres/document_versions.rs` (376) and `mysql/document_versions.rs` (401)
expose exactly two store methods each — `get_document_version_at` and
`document_version_storage_diagnostic` — and both are pure connection
acquisition wrapping an `_in_session` function. There is no shared orchestration
to extract: everything below them is dialect SQL. The index-version half of this
target is where the orchestration actually lived, and it landed (above).

## Divergences Found And Held

Behavior-preserving means these were found, documented, and **left alone**.

### LOUD: `has_scheduled_work` disagrees about disabled cron jobs — a real bug

PostgreSQL (`postgres/read.rs:6-16`) asks `table_has_rows_in_session(...,
"cron_jobs")` — any row. MySQL (`mysql/read.rs:584-600`) issues
`SELECT 1 FROM cron_jobs WHERE enabled = TRUE LIMIT 1`.

Every other backend agrees with PostgreSQL: `sqlite/read.rs:815-818`,
`libsql/read.rs:300-304`, `memory/scheduler.rs:240-244`. **MySQL is the sole
outlier of five.**

This is not cosmetic. A false `has_scheduled_work` means the tenant is never
loaded into the scheduler (`nimbus-engine/src/engine/scheduler/coordination.rs:96-98`
and `:205-222`). On MySQL, a tenant whose only scheduled work is a currently
disabled cron job is invisible to the scheduler, so re-enabling that cron job
does not wake it. Fixing this is a behavior change and belongs in its own
change with its own test; it is recorded here, not fixed here.

### Held per-dialect, with reasons

- `export_durable_journal_bootstrap` — PostgreSQL tears the snapshot/floor pair,
  MySQL captures both atomically. This is why the `journal_cursor_floor` field
  stayed with the MySQL store rather than moving into the shared snapshot;
  MySQL keeps its atomic pair via `read_snapshot_with_journal_floor`, and
  PostgreSQL keeps reading the floor separately.
- Snapshot transaction mode — PostgreSQL `read_only(true)`, MySQL not.
- Stream-limit conversion — hard error on PostgreSQL, silent clamp on MySQL.
- `field_type_for_table_schema` and `validate_durable_journal_stream_limit` —
  different observable error text per dialect; documented in
  `sql/predicate.rs`'s module doc rather than unified.
- `durable_record_changes_schema_cache` — semantically identical private replica
  at `sqlite/journal.rs:470`, left in place under the sqlite-untouched
  constraint and noted in `sql/schema_events.rs`.
- `cancel_scheduled_job` — PostgreSQL reads affected rows from `execute()`,
  MySQL uses `exec_drop` + `conn.affected_rows()`.
- `prune_document_versions_before_in_session` — two different algorithms;
  MySQL additionally reads `SELECT ROW_COUNT()` separately in
  `prune_index_versions_before_in_session`.
- MySQL's `index_versions` primary key is on the SHA-256 hash, PostgreSQL's on
  the raw tuple.
- PostgreSQL hand-rolls a `row_to_document` that already exists shared in
  `sql/row.rs` — a real duplicate, but repointing it changes decode behavior at
  the row edge and is not this step's mandate.

## LoC Delta

`git diff --numstat HEAD -- crates/nimbus-storage`:
**27 files changed, 1679 insertions(+), 2045 deletions(-)** — net **−366**.

| Bucket | Lines |
| --- | --- |
| Duplicated provider lines deleted | −2,037 |
| New shared modules (5 files) | +1,103 |
| Per-provider hook impls, facade invocations, import churn | +428 |
| Growth in existing shared files (`store_core` +129, `sql.rs` +10, `provider_impls` +9) | +148 |
| **Net** | **−366** |

The ≈ −800 estimate is not reachable behavior-preservingly, and the gap has one
cause, already diagnosed in step 1: **deduping N lines across exactly two
backends saves N but costs roughly 100 lines of fixed scaffolding** — trait
declaration, facade macro, module doc, and per-backend hook impls. The
`index_history` slice is the clearest instance: it removed 403 duplicated lines
across the two providers and netted ≈ −18, because preserving each store's
public API forces a facade whose size approaches the deduped body.

Every target that beat that ratio landed. Every target that did not was measured
and declined rather than shipped as a line-additive indirection. As in step 1,
the third backend is where this pays: the trigger-invocation seam already
absorbed libsql for +29 lines of hooks against −75 of duplication.

## Verification

All lanes re-run against the exact committed tree (not an earlier state), with
the three provider fixtures live: `nimbus-external-provider-tests-{postgres,mysql,libsql}-1`,
all `Up (healthy)`.

| Lane | Result |
| --- | --- |
| `cargo fmt --all --check` | RC=0 |
| `cargo check -p nimbus-storage --all-targets` | RC=0 — 0 errors, 0 warnings |
| `cargo nextest run -p nimbus-storage` | **439 run, 439 passed**, 2 skipped (17.7s) |
| `cargo nextest run -p nimbus-engine` | **663 run, 663 passed** (5 slow), 5 skipped (244.5s) |
| U4 gates (`-E 'test(commit_path_ownership)'`) | **2 run, 2 passed**, 439 skipped |
| PPSC libsql soak, 10× (`-E 'test(libsql_ppsc)'`) | **pass=10 fail=0**, each 2 run / 2 passed |
| `cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings` | RC=0 |
| sqlite untouched | `git diff --numstat HEAD -- 'crates/nimbus-storage/src/sqlite*'` → **0 files** |

Every command run with `set -o pipefail`; each battery was redirected to a log
file and its status read directly rather than through a pipe. (`${PIPESTATUS[0]}`
is bash-only and reports empty under this zsh shell — it produced blank RC
echoes earlier in this step until the pattern changed.)

**The U4 pins did not move.** `JOURNAL_OWNERS` needed no edit: the diff of
`crates/nimbus-storage/src/tests/commit_path_ownership.rs` against HEAD is
empty, and both gate tests pass unchanged. No pinned owner file moved in this
step.

Clippy's only two `^warning` lines are third-party — `brotli-decompressor` (3)
and `brotli` (16). Nothing in workspace code warns.

### A fixture-pollution failure, diagnosed and not papered over

The first engine run reported 4 failures — three in
`postgres_provider::lease_lifecycle` (`lease_renewal_ignores_backward_wall_clock_step`,
`lease_renewal_ignores_forward_wall_clock_step`,
`lease_renewal_shutdown_interrupts_monotonic_wait`) and
`ordered_arm::hot_tenant_provider_stall_does_not_block_other_tenant` — all with
`postgres error [SqlState(E42P06)]: schema "tenant_..." already exists`.

Cause: the shared PostgreSQL fixture had accumulated **1,549 leftover
`tenant_%` schemas** over its uptime, and the deterministic counter+hash naming
collided. Only the 4 colliding schema names were dropped — not all 1,549, since
other teammates may share the container. The final full engine run above is
663/663 with no filter, which is the evidence that this was fixture state and
not a code defect.

## Step 4 — Reviewer Outage Disclosure

The structured reviewer could not run for this step, as anticipated at the end
of step 3. The Codex engine remains in account credit exhaustion (usage limit,
resets 2026-08-04) and the Claude engine still fails on this host with the
reviewer-sandbox proxy-CA trust error. Per the review-outage policy this is an
outage, not a verdict.

The verification bar above therefore stands in for it, and this step was scoped
defensively in consequence: no behavior was changed anywhere, the one real
cross-dialect bug found (`has_scheduled_work`) was documented rather than fixed,
and three targets were declined with published numbers rather than shipped on
judgment that no second reader could check.
