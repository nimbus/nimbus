# SIC6 — Physical SQLite durability faults

Base: `main` @ `a46c44e003ce1b2bbf94795ac4eed12c10c6313b` (SIC4 merged; SIC5 in
PR #289).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.

## 1. The gap

`nimbus-storage` could already fail a *logical* step. `FaultInjector` and
`FaultPoint` (`src/simulation/faults.rs`) let a test refuse a named point in the
write path, and `open_with_simulation` wires that into the SQLite store.

Nothing could fail a *physical* one. Every SQLite test ran against a device that
always accepted a write, always confirmed an `fsync`, and always took a
write-ahead log frame, and every process that opened a database also closed it.
So the durability contract — an acknowledged commit survives, an unacknowledged
one may not, and nothing in between is ever visible — had no test that could
observe it. The gap was not that a specific defect was suspected; it was that
this class of defect could not be caught at all.

## 2. What this adds

`crates/nimbus-storage/src/tests/sqlite_physical_durability.rs` and its
`fault_vfs` submodule, both under the crate's `#[cfg(test)] mod tests`
(`src/lib.rs:170`).

**The shim.** `fault_vfs` copies whatever VFS SQLite currently has as default,
overrides `xOpen`, and installs an `sqlite3_io_methods` table that delegates
every call to the real file. `xWrite` and `xSync` first consult one armed
global. Unarmed, the shim is a pass-through: it forwards to the VFS it captured
before registering, and a relaxed `AtomicBool` keeps the unarmed path off the
mutex entirely.

Arming is scoped two ways. A fault applies only to files whose path contains the
marker the test passed, which is the unique basename stem of the database under
test, so a fault can never reach another test's file even though the shim is the
process-wide default. And `arm` returns a guard that disarms on drop, so a
panicking test cannot leave a fault behind.

Three faults: `DiskFull` (`SQLITE_FULL` from a database or log write),
`SyncFailure` (`SQLITE_IOERR_FSYNC`), and `WalWriteFailure` (`SQLITE_IOERR_WRITE`
on the `-wal` file only, so a torn log is separable from a torn database).

**The rule.** `check_acknowledgement_survives` compares a reopened store against
the last result the store handed a caller, where that result is a durable head,
an applied head, and a `MaterializedPosition` — SIC4's digest-bound position, so
two databases at the same sequence with different content do not compare equal.
The rule permits losing an *unacknowledged* write, which is the entire point of
not acknowledging it. It forbids losing an acknowledged one, forbids reads
running ahead of durability, and forbids different state sitting at an unchanged
applied sequence.

**No production surface.** `sic6-no-production-surface.txt` records the check.
Every line is inside `#[cfg(test)] mod tests`. No
production module gains a switch, a trait method, a feature, or an environment
variable that can fail a physical operation, and `open_through_shim` opens the
store through the ordinary `SqliteTenantStore::open` the server uses.

## 3. The cases

| Case | Failure | What it pins |
| --- | --- | --- |
| `sqlite_disk_full_preserves_last_acknowledged_position` | before durable visibility | the rejected write leaves no trace: the reopened acknowledgement is byte-equal to the one before the fault |
| `sqlite_sync_failure_is_not_acknowledged` | after the bytes were written, before durability was confirmed | the caller is told the write failed; the reopened head is the acknowledged head or exactly the one unacknowledged commit past it, and recovery applies exactly what it found durable |
| `sqlite_wal_failure_never_exposes_partial_effects` | write-ahead log | a four-document `apply_resolved_write_batch` is all-visible or none-visible after recovery, never a subset |
| `sqlite_crash_after_durable_commit_recovers_matching_position` | process loss | a child process seeds five acknowledged commits, reports its acknowledgement, and is `SIGKILL`ed with the connection open — no unwinding, no `Drop`, no close, no checkpoint. The reopened database lands on the acknowledged position exactly. |

`physical_durability_checker_detects_a_broken_acknowledgement_rule` is the
mutation test the task requires. It drives three broken acknowledgements through
the checker and requires each to be rejected by name: acknowledged commits gone
(a real empty database compared against a real seeded acknowledgement), the same
applied sequence carrying different content (a second real database seeded with
different documents), and an applied head past the durable head.

## 4. Fail-before

`sic6-failbefore.txt`.

**A.** On `main` the four cases do not exist. Verifier condition 13 reports
`0/4 cases passed`.

**B — the rule is load-bearing.** `check_acknowledgement_survives` mutated to
return `Ok` unconditionally:

```
physical_durability_checker_detects_a_broken_acknowledgement_rule ... FAILED
  losing acknowledged commits must fail the rule: ()
```

**C — the faults are real.** `decide` mutated to pass every operation through:

```
sqlite_disk_full_preserves_last_acknowledged_position ... FAILED
sqlite_sync_failure_is_not_acknowledged ... FAILED
sqlite_wal_failure_never_exposes_partial_effects ... FAILED
test result: FAILED. 2 passed; 3 failed
```

The three armed cases stop passing, which is what proves they are driven by a
physical failure rather than by an incidental error. The crash case still passes
under C, correctly: it uses `SIGKILL`, not the shim.

**D.** Sources restored: `5 passed; 0 failed; 1 ignored`.

## 5. One thing the first draft got wrong

The write-ahead log case failed on the first run: the batch was acknowledged and
the fault never reached it. A SQLite connection binds its VFS when it opens, and
the shim installed itself lazily on the first `arm`. Whichever armed test ran
first had therefore already opened its store on the untouched default VFS, so no
fault could reach that connection — and which test that was depended on
scheduling order.

That is a property of the harness, not of the store, but it is exactly the shape
of bug that makes a fault test quietly vacuous: the case passes, the fault never
fires, and nobody notices. `install()` is now separate from `arm()`, every case
opens through `open_through_shim`, and every armed case asserts `fault_fired()`
so a fault that silently stops reaching SQLite fails the test instead of
passing it.

## 6. Verification

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage sqlite_physical_durability -- --nocapture` | 5 passed, 0 failed, 1 ignored |
| `cargo test -p nimbus-storage sqlite -- --nocapture` | 87 passed, 0 failed, 1 ignored |
| `cargo test -p nimbus-storage` | 313 passed, 1 failed — see below |
| `make verify-harness SURFACE=storage` | 1 passed, 0 failed, exit 0 |
| `cargo clippy -p nimbus-storage --all-targets` | clean |
| `cargo fmt --all --check` | clean |
| verifier | `sic6-verifier.txt` — condition 13 PASS; `Summary: 12 passed, 1 failed` |
| `make ci` | format, Clippy, deny, JavaScript checks and proof helpers green; the Rust workspace lane reports 7438 tests, 7437 passed, 1 failed — the same redb flake below |

The single verifier failure is condition 12, which belongs to SIC5 and is still
open as PR #289. It passed on the SIC5 branch. When both land, `main` carries the
matrix and this module together and the verifier reaches 13 passed, 0 failed.

The one test failure is
`redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`:

```
SEQ13 PITR export/import exceeded budget: 1.063382s > 1s
```

A wall-clock budget on the **redb** path, which shares no code with this change
— no SQLite connection, no VFS, no fault shim. Re-run three times in isolation:
FAILED, ok, ok. It is a timing flake under load, the same family as the
contention SIC4 recorded as blocker B2. The bound was not widened and the test
was not skipped.

The ignored case is `crash_child_writes_and_parks`, the child body the crash
test re-executes. It is `#[ignore]`d so it never runs in an ordinary pass, and
returns immediately when its environment variable is absent.
