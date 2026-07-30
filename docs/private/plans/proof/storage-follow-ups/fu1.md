# FU1 — MySQL `has_scheduled_work` counted only enabled cron jobs

Branch `codex/fu1-mysql`, based on `origin/main` @ `22c5cdd62`.
Ticket: `docs/private/plans/storage-follow-ups-plan.md`, row FU1
(found in SUC3.1 step 4).

## The Bug

`MySqlTenantStore::has_scheduled_work` (`crates/nimbus-storage/src/mysql/read.rs`)
counted `scheduled_jobs` and `running_scheduled_jobs` rows unconditionally, but
filtered cron jobs:

```sql
SELECT 1 FROM <db>.cron_jobs WHERE enabled = TRUE LIMIT 1
```

The other five backends count any `cron_jobs` row:

| Backend | `has_scheduled_work` cron term |
| --- | --- |
| redb (`scheduler/inspection.rs`) | `table_has_entries_str(&read_txn, CRON_JOBS)` |
| memory (`memory/scheduler.rs`) | `!state.cron_jobs.is_empty()` |
| sqlite (`sqlite/read.rs`) | `table_has_entries(&self.conn, "cron_jobs")` |
| libsql (`libsql/read.rs`) | `table_has_entries_remote(&conn, "cron_jobs")` |
| PostgreSQL (`postgres/read.rs`) | `table_has_rows_in_session(&client, .., "cron_jobs")` |
| **MySQL (before)** | **`... WHERE enabled = TRUE`** |

`Engine`'s scheduler gates tenant load on this answer
(`crates/nimbus-engine/src/engine/scheduler/coordination.rs:98-100`, and the
async pair at `:207-224`). A MySQL tenant whose only scheduled work is a
currently-disabled cron job therefore never got loaded, so re-enabling that
job could not wake it — the tenant stayed dark until some other scheduler
state appeared.

`enabled` legitimately belongs to the *other* scheduler-inspection surface.
`next_scheduled_work_at` answers "when is the next due wake" and all six
backends filter disabled crons out of it. `has_scheduled_work` answers "does
this tenant own scheduler state at all", and a disabled cron job is state the
tenant owns.

## The Fix

`crates/nimbus-storage/src/mysql/read.rs` — the cron term becomes the same
`table_has_entries` call the other two tables already used, so all three
scheduler tables are probed identically:

```rust
Ok(
    table_has_entries(&mut conn, &database_name, "scheduled_jobs").await?
        || table_has_entries(&mut conn, &database_name, "running_scheduled_jobs")
            .await?
        || table_has_entries(&mut conn, &database_name, "cron_jobs").await?,
)
```

`next_scheduled_work_at` in the same file is untouched: it keeps
`WHERE enabled = TRUE`, matching the other five.

## The Regression Test

One generic exerciser pinned across **all six** backends rather than a
MySQL-only test, so the two questions cannot drift apart on one dialect
again. `crates/nimbus-storage/src/tests.rs` gains:

```rust
pub(crate) fn exercise_disabled_cron_job_still_reports_scheduled_work<S>(store: &S)
where
    S: crate::SchedulerStore + crate::SchedulerWriteStore,
```

The bounds are the two traits every production backend already implements
(`traits/provider_impls.rs` `impl_scheduler_store!`, `scheduler/write.rs`
`impl_scheduler_write_store!` / `impl_provider_scheduler_write_store!`), so the
body is backend-independent: save a cron job with `enabled: false` through
`SchedulerWrite::SaveCron`, then assert

- `has_scheduled_work()` is `true` (the bug),
- `next_scheduled_work_at()` is `None` (the contract that *does* honour
  `enabled`, asserted so a future "fix" cannot make the two agree by breaking
  the wrong one),
- and both fall back to empty after `SchedulerWrite::DeleteCron`.

Six wrappers call it, each named so its provider lane selects it
(`test(/^(tests::)?<provider>_/)`):

| Test | File |
| --- | --- |
| `tests::memory_conformance::redb_disabled_cron_job_still_reports_scheduled_work` | `tests/memory_conformance.rs` |
| `tests::memory_conformance::memory_disabled_cron_job_still_reports_scheduled_work` | `tests/memory_conformance.rs` |
| `tests::sqlite_foundation::scheduler::sqlite_disabled_cron_job_still_reports_scheduled_work` | `tests/sqlite_foundation/scheduler.rs` |
| `tests::libsql_provider::execution_units::libsql_disabled_cron_job_still_reports_scheduled_work` | `tests/libsql_provider/execution_units.rs` |
| `tests::mysql_provider::execution_units::mysql_disabled_cron_job_still_reports_scheduled_work` | `tests/mysql_provider/execution_units.rs` |
| `tests::postgres_provider::execution_units::postgres_disabled_cron_job_still_reports_scheduled_work` | `tests/postgres_provider/execution_units.rs` |

### Fail-before

Test added first, fix not yet applied, all three provider fixtures live
(`scripts/external-provider-fixture.sh up all`, PostgreSQL published on 55432
because 5432 was occupied locally):

```
$ cargo nextest run -p nimbus-storage --features libsql,mysql,postgres \
    -E 'test(disabled_cron_job_still_reports_scheduled_work)'
        PASS [   0.067s] (1/6) tests::memory_conformance::memory_disabled_cron_job_still_reports_scheduled_work
        PASS [   0.373s] (2/6) tests::sqlite_foundation::scheduler::sqlite_disabled_cron_job_still_reports_scheduled_work
        PASS [   0.560s] (3/6) tests::libsql_provider::execution_units::libsql_disabled_cron_job_still_reports_scheduled_work
        FAIL [   0.220s] (4/6) tests::mysql_provider::execution_units::mysql_disabled_cron_job_still_reports_scheduled_work
        PASS [   1.493s] (5/6) tests::memory_conformance::redb_disabled_cron_job_still_reports_scheduled_work
        PASS [   0.839s] (6/6) tests::postgres_provider::execution_units::postgres_disabled_cron_job_still_reports_scheduled_work
     Summary [   1.626s] 6 tests run: 5 passed, 1 failed, 441 skipped

    panicked at crates/nimbus-storage/src/tests.rs:108:5:
    a disabled cron job must count as scheduled work; hiding it strands the
    tenant unloaded so re-enabling the job can never wake it
```

Exactly one backend red, the five conforming ones green — the outlier shape
the ticket described.

### Fail-after

Same command, same live fixtures, after the one-line-shape fix:

```
     Summary [   1.292s] 6 tests run: 6 passed, 441 skipped
```

## Test-Lane Script Cleanup

`scripts/test-external-providers.sh` line 60 carried a dead second alternative
in the libSQL filter:

```
test(/^(tests::)?libsql_/) or test(/^(tests::)?libsql_replica_provider::/)
```

The module is `tests::libsql_provider`, not `tests::libsql_replica_provider`;
the first alternative already covers it (`libsql_provider` starts with
`libsql_`). The second alternative was removed.

Measured on this tree, over the two packages the differing clause spans:

```
$ cargo nextest list -p nimbus-storage -p nimbus-engine -E '<old filter clause>' | grep -c '^nimbus-'
52
$ cargo nextest list -p nimbus-storage -p nimbus-engine -E '<new filter clause>' | grep -c '^nimbus-'
52
$ cargo nextest list -p nimbus-storage -p nimbus-engine \
    -E 'test(/^(tests::)?libsql_replica_provider::/)' --no-tests pass | grep -c '^nimbus-'
0
```

The removed alternative selects zero tests on its own, so the union is
unchanged. The `package(nimbus-system)` clause of the lane filter is
byte-identical before and after and contributes the same single test, which is
the ticket's baseline of 52 (51 from this clause pre-change + 1 from
nimbus-system); this tree measures 52 for the clause because the new libSQL
regression test is one of them.

## Verification

| Gate | Result |
| --- | --- |
| Six-backend regression test, live fixtures | 6 tests run: 6 passed |
| MySQL provider lane, live fixture (`make test-external-provider PROVIDER=mysql`) | 48 tests run: 48 passed (2 slow), 1138 skipped, 106.087s |
| Scheduler + cron suite across all six backends, live fixtures | 39 tests run: 39 passed, 408 skipped |
| Provider-free build (`cargo check -p nimbus-storage`, no features) | clean |
| `cargo clippy -p nimbus-storage --features libsql,mysql,postgres --all-targets -- -D warnings` | clean |
| `make clippy` (workspace) | clean |
| `cargo fmt --all --check` | clean |

The scheduler suite is
`cargo nextest run -p nimbus-storage --features libsql,mysql,postgres
-E 'test(disabled_cron_job_still_reports_scheduled_work) or test(/scheduler/)
or test(/cron/)'`. It covers the pre-existing redb/memory/sqlite scheduler
tests, the provider execution-unit scheduler round-trips, and all six new
wrappers.

## The MySQL Lane's `nimbus-system` Flake

The first full MySQL lane run on this branch came back 47/48 with
`nimbus-system
projection::reconciliation_tests::projection_mysql_two_engine_takeover_rejects_late_old_document_schema_and_delete`
failing at `crates/nimbus-system/src/projection/reconciliation_tests.rs:280` —
the 10-second `wait_for_row_count` timeout inside
`assert_provider_restart_reconciles_scope`, with `visible_row=None` and every
mutation-journal frontier already at `SequenceNumber(2)`. It was attributed to
this change and then cleared. It is a pre-existing flake.

**The change cannot reach that test.** The only behaviour difference is the
`cron_jobs` probe, and it differs only when `cron_jobs` holds a row whose
`enabled` is false. Cron rows are written exclusively through
`CreateCronRequest` (`nimbus-engine/src/engine/scheduler/cron.rs`); the
projection test never creates one. Confirmed against the live fixture: every
`cron_jobs` table in every database the test left behind held zero rows, so
`has_scheduled_work` returns the same `false` on both sides of the change.

**A/B on two prebuilt binaries.** Per-arm batches confound the arm with machine
state, because each batch runs contiguously after its own rebuild. So both
`nimbus-system` test binaries were built once — identical except for the
`mysql/read.rs` hunk — and then run interleaved ABBA (`withfix, nofix, nofix,
withfix` per round, four rounds) from a clean fixture with no leftover
databases, with no `cargo` in the loop:

```
with-fix   8 runs: 8 passed
no-fix     8 runs: 7 passed, 1 failed
```

The single failure landed in the **no-fix** arm, panicking at the same
`reconciliation_tests.rs:280` with the same `visible_row=None`. That reproduces
the failure with the fix reverted, which is what settles attribution.

For the record, the confounded per-arm batches that preceded the A/B ran 20
with-fix runs (6 failures) against 14 no-fix runs (1 failure). Those failures
cluster in time — three consecutive at the head of one batch, one at the head of
another — which is the signature of an environmental spell, not of a code
difference, and the interleaved design is what removes it. The lane itself is
green at 48/48 with the fix applied.

The flake is a liveness race in the projection observer's post-restart
reconciliation: the scope is durable and the frontiers are caught up, but the
projected row does not publish within the test's 10-second budget. It lives
entirely in `nimbus-system` and touches no scheduler surface. It is worth its
own ticket; `ci-pr` runs with `retries = 0`, so it fails the MySQL lane outright
whenever it fires.
