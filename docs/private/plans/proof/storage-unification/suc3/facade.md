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
- `sql/store_core.rs` = **1,490 lines**, of which **~1,149** is the single
  shared copy of that logic plus both trait declarations, and **341** is the
  facade macro (signatures and one-line forwards, no logic).
- Remaining insertions in the provider files are the two forwarding impl
  blocks (`SqlStoreCore` and `SqlWriteTransactionCore`), which are signatures
  only.

So the *logic* went from two copies to one, and every line of the new
plumbing is signature-shaped. Raw LoC is roughly flat because the plumbing
cost (facade + forwarding impls ≈ 660 lines) nearly offsets the ~800 lines of
body deduplication at two providers. The projected reduction materializes in
step 2: porting libsql and sqlite onto the same core reuses the 1,149-line
body once more per provider without growing it, and each added provider pays
only forwarding impls plus one facade invocation.

## Verification

Every command run with `set -o pipefail`. Fixtures were live for the provider
lanes; see the caveat below on the fixture-less run.

| Lane | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-storage` | 435 run, **435 passed**, 2 skipped |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-engine` | 659 run, **659 passed**, 5 skipped |
| Live PostgreSQL lane (official filter, storage + engine) | 76 run, **76 passed**, 1025 skipped |
| Live MySQL lane (official filter, storage + engine) | 46 run, **46 passed**, 1055 skipped |
| Live libsql lane — untouched-provider regression | 50 run, **50 passed**, 1051 skipped |
| Live `nimbus-system` provider trio (pg + mysql + libsql arms of the lane filter) | 3 run, **3 passed** |
| `cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings` | clean, exit 0 |
| `cargo fmt --all --check` | clean |
| `cargo check --workspace --all-targets` | clean |

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
