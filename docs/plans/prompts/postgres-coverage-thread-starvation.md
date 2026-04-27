# Postgres Coverage Thread Starvation

## Status

Resolved on 2026-04-15.

- `crates/neovex-storage/src/postgres.rs` now bridges sync Postgres reads and
  writes back into Tokio with `tokio::task::block_in_place` on multi-thread
  runtimes instead of spawning a fresh OS thread per bridged operation.
- The dedicated bridge-thread fallback remains only for current-thread runtimes,
  where `block_in_place` would panic.
- `crates/neovex-engine/src/tests/postgres_provider.rs` now pins the
  128-round CRUD regression to `worker_threads = 2` so the local/runtime shape
  matches the constrained GitHub Actions coverage runner.
- `.github/workflows/ci.yml` now provisions explicit Postgres, MySQL, and
  libsql fixtures via explicit `NEOVEX_*` fixture env vars, so the
  coverage lane runs all three external provider suites without skip filters or
  silent testcontainer fallback.

## Compact prompt

Use this when running `/compact` before starting the work:

```
Preserve: historical context for the resolved Postgres coverage-thread
starvation bug. The original issue was the Postgres storage bridge
creating a nested spawn_blocking → std::thread::spawn →
Handle::block_on chain for journaled mutations, which hung the
128-round CRUD test under cargo llvm-cov on 2-vCPU CI runners. The
fix switched the Postgres bridge to block_in_place on multi-thread
Tokio runtimes and re-enabled tests::postgres_provider in coverage.
Read docs/prompts/postgres-coverage-thread-starvation.md for details if
the regression reappears.
```

---

## Session prompt

```
Read docs/prompts/postgres-coverage-thread-starvation.md for the
historical Postgres coverage-thread-starvation fix, then verify whether
the regression has reappeared. If it has, focus on the Postgres runtime
bridge and the 128-round CRUD coverage test on a 2-worker Tokio runtime.
```

---

## Full context

### Symptom

The test `typed_postgres_config_keeps_sequence_heads_in_sync_across_repeated_direct_crud`
in `crates/neovex-engine/src/tests/postgres_provider.rs:225` hangs
indefinitely under `cargo llvm-cov` on GitHub Actions CI runners
(ubuntu-latest, 2 vCPUs). It passes locally (macOS, 10+ cores) both
with and without coverage. MySQL and libsql equivalent tests pass fine.

The test does 128 rounds of insert → update → delete (384 total mutations)
against a Postgres testcontainer, each going through the full
`Service::apply_mutation` → journal worker → Postgres storage path.

### Current workaround

Resolved for the external SQL provider suites. The coverage CI step now
provisions explicit provider fixtures and runs the provider suites without skip
filters:

The coverage CI step runs:
```yaml
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

The provider tests continue to support container-backed local fallback when no
explicit fixture envs are set, but the coverage lane now declares
`NEOVEX_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1` so missing explicit fixtures fail
honestly instead of silently downgrading to skip-and-pass behavior.

### Root cause analysis

The per-mutation write path creates a deeply nested blocking chain:

```
1. Test calls service.insert_document_async()
   → enqueues mutation to journal queue
   → waits on oneshot channel for response

2. Journal worker (tokio task) drains queue
   → tokio::task::spawn_blocking(process_queued_mutation_batch)
     [crates/neovex-engine/src/service/mutations/journal.rs:87]

3. Inside spawn_blocking: process_queued_mutation_batch()
   → holds std::sync::Mutex (lock_mutation_sequence)
   → calls runtime.store methods which call...

4. PostgresTenantStore::execute_write() [postgres.rs:507]
   → detects tokio runtime context (TokioRuntimeHandle::try_current())
   → spawns std::thread::spawn to escape the runtime [postgres.rs:527]

5. Inside std::thread::spawn:
   → begin_write_transaction_cancellable()
   → acquire_tenant_lock() [postgres.rs:2417]
   → self.block_on(async { pg_advisory_xact_lock }) [postgres.rs:2408]
   → Handle::block_on(future) — blocks the OS thread waiting for
     the tokio runtime to poll the Postgres connection future
```

For 384 mutations, this creates 384 × (1 spawn_blocking + 1 std::thread::spawn)
= 768 thread creations, each doing Handle::block_on which parks the thread
waiting for the tokio runtime.

Under `llvm-cov` coverage instrumentation (documented 20-40x slowdown,
see taiki-e/cargo-llvm-cov#376), each operation takes dramatically
longer, causing:
- The spawn_blocking pool to fill with threads waiting on Handle::block_on
- The std::thread::spawn threads to compete for OS scheduling
- The tokio runtime worker threads (only 2 on CI) to be starved
  servicing the parked Handle::block_on futures

This matches the documented tokio deadlock pattern from
tokio-rs/tokio#3717: "If your spawn_blocking task cannot complete
until some other spawn_blocking task completes, then this can cause a
deadlock given enough concurrency."

### Why MySQL doesn't hang

MySQL has only 2 engine-level tests. Postgres has 4, including the
128-round CRUD test that doesn't exist for MySQL. It's not a
MySQL vs Postgres behavioral difference — the heavy test simply
doesn't exist for MySQL.

### Desired fix

Implemented on 2026-04-15 by replacing the Postgres runtime bridge's
per-operation thread spawn with `block_in_place` on multi-thread runtimes and
keeping the existing bridge-thread fallback only for current-thread runtimes.
That removes the hot `spawn_blocking -> std::thread::spawn -> Handle::block_on`
path from the multi-thread coverage lane while preserving sync call support for
current-thread runtimes.

Eliminate the nested blocking chain. The Postgres write path should not
require spawn_blocking → std::thread::spawn → Handle::block_on. Options:

**Option A: Make the write path fully async**

Change `PostgresTenantStore::execute_write` and the write transaction
methods to be async. This eliminates the need for spawn_blocking and
std::thread::spawn entirely. The journal worker would need to be
restructured to process batches asynchronously instead of via
spawn_blocking.

**Option B: Use a dedicated blocking runtime for Postgres**

Instead of Handle::block_on on the main runtime, create a dedicated
single-threaded tokio runtime for Postgres blocking operations. This
avoids competing with the main runtime's worker threads.

**Option C: Pool the write threads**

Instead of spawning a new std::thread per write, use a small fixed
thread pool (e.g., 2-4 threads) with a channel for write requests.
This bounds thread creation and avoids the scheduling pathology.

### Key constraints

- The mutation path invariant: every mutation flows through
  `Service::apply_mutation`. Do not create a separate code path.
- Storage atomicity: document write, index effects, and commit log
  append must remain a single storage transaction.
- The `lock_mutation_sequence` mutex must be held during batch
  processing to maintain sequence ordering.
- The `pg_advisory_xact_lock` is required for cross-process tenant
  write serialization.

### Reference files

1. `crates/neovex-engine/src/service/mutations/journal.rs` — journal
   worker, spawn_blocking call (line 87), batch processing loop
2. `crates/neovex-engine/src/service/mutations/direct/store.rs` — 
   synchronous store mutation wrappers, lock_mutation_sequence usage
3. `crates/neovex-storage/src/postgres.rs` — PostgresTenantStore,
   execute_write (line 507), std::thread::spawn escape (line 527),
   block_on (line 1308), write transaction (line 1714), advisory lock
   (line 2417)
4. `crates/neovex-engine/src/tests/postgres_provider.rs` — the hanging
   test (line 225), test infrastructure (line 1073)
5. `crates/neovex-engine/src/tenant/mutation_facade.rs` —
   lock_mutation_sequence (line 85)
6. `.github/workflows/ci.yml` — coverage step with current --skip
   workaround

### External references

- [taiki-e/cargo-llvm-cov#376](https://github.com/taiki-e/cargo-llvm-cov/issues/376) —
  20-40x slowdown under coverage instrumentation
- [tokio-rs/tokio#3717](https://github.com/tokio-rs/tokio/discussions/3717) —
  spawn_blocking + block_on deadlock patterns
- [testcontainers/testcontainers-java#5978](https://github.com/testcontainers/testcontainers-java/issues/5978) —
  Postgres container intermittent failures on GitHub Actions
