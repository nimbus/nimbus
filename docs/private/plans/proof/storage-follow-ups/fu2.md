# FU2 — PPSC Acknowledgement-Loss Arm Theft: Durable-Record Identity Through the Fault Interface

Owner plan: `docs/private/plans/storage-follow-ups-plan.md` (FU2).
Written against `main` @ `22c5cdd62`, rebased onto `main` @ `f94ed17dc`.
Branch: `codex/fu2-ppsc`.
Prior evidence this closes: `docs/private/plans/proof/storage-unification/suc3/facade.md`,
Step 2 "PPSC ack-loss arm theft" and Step 3 "Arm-Theft Fault Gating: Implemented,
Refuted, Reverted".

## The defect

`PpscStorageFaultInjector` arms `AcknowledgementLoss` as a one-shot fault keyed
on `(tenant, fault_point)`. The product checks
`StorageCommitAfterVisibilityBeforeReturn` unconditionally, once per commit, on
every dialect. Tenant identity therefore does not identify a transaction: every
concurrent same-tenant commit arrives at the same armed point at the same time,
and whichever gets there first consumes the arm. The scenario's real durable
batch then commits clean on retry and the seeded differential asserts a
crash-and-replay that correctly never happened — roughly 1 in 10 runs on the
libsql lane, also sighted on mysql.

The transactions that steal the arm are not exotic. Schedule-only execution
units, trigger outcomes, and the fenced durable batch itself all commit with
`commit == None`.

There are two distinct thieves, and only the first was known when FU2 was
ticketed. Both were caught in this lane:

1. **Record-less commits.** A same-tenant transaction that materializes no
   journal record at all reaches the boundary and consumes the arm.
2. **Durable-journal replay.** Recovery re-applies records that were made
   durable earlier, through the very same commit-sequence boundary, carrying a
   non-empty batch. This one survives a fix that only asks whether records are
   present, and it is what still failed the libsql differential after the first
   iteration of this work. Its backtrace is in the Evidence section below.

## Why this is not the refuted design

Step 3 of the SUC3 facade proof implemented, tested, and reverted the obvious
gate:

```rust
if commit.is_some() {
    backend.check_fault(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
}
```

`commit.is_some()` means "this transaction appended a commit entry", not "this
transaction changed something durable". The fenced durable batch is itself
`commit == None`, so the gate silenced the fault on its own target: seven
`nimbus-engine` tests failed deterministically, and the PPSC snapshot read
`visits: 0` — the arm was never even reached.

FU2 inverts where the decision lives. The product still checks
unconditionally, on every commit, exactly as before; nothing about which
boundaries are instrumented changes. What changes is that the check now
*carries the identity of what it is committing*, and the harness — which is the
only party that knows what it armed — decides whether this is the transaction
it meant. A fault adapter that arms nothing still sees identical behavior,
because the default trait bodies ignore `records`.

The distinction matters for coverage: under the refuted gate, a durable batch
could not be faulted at all. Under FU2 it is exactly the transaction that *can*
be, and the boundaries that used to steal the arm are the ones that now pass
through.

## Interface change

### One tenant-aware check, carrying records

`FaultInjector` (`crates/nimbus-storage/src/simulation/faults.rs`) had two
tenant-aware methods that could not be told apart — `check_for_tenant(point,
tenant)` and `check_for_durable_records(point, tenant, records)`, the latter
defaulting to the former, and the PPSC injector funnelling both into one
`check_tenant(point, tenant)`. The seam's own doc comment already described the
discrimination it was supposed to provide; nothing implemented it.

They are now one concept with two entry points, distinguished only by whether
the caller still holds a `TenantId`:

```rust
pub trait FaultInjector: Send + Sync {
    fn check(&self, point: FaultPoint) -> Result<()>;

    /// Checks a fault at a tenant-owned storage boundary, naming the durable
    /// journal records that boundary is about to make visible.
    fn check_for_tenant(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> { /* default: self.check(point) */ }

    /// The same check for a store that has already bound its tenant through
    /// `tenant_scoped_fault_injector` and therefore holds no `TenantId` at the
    /// call site — redb, SQLite and Memory reach their commit boundaries this
    /// way.
    fn check_durable_records(
        &self,
        point: FaultPoint,
        records: &[TenantEventRecord],
    ) -> Result<()> { /* default: self.check(point) */ }
}
```

`TenantScopedFaultInjector` forwards all three forms to
`inner.check_for_tenant(point, &self.tenant_id, records)`, passing `&[]` for the
point-only `check`. An empty slice is not an absence of information: it is the
positive statement "this boundary materializes no journal record", and it is
the whole discriminator.

### What a boundary may name: `DurableApplyKind`

Carrying records is not enough on its own, because "the records this call has
in hand" and "the records this boundary makes durable" are different sets, and
only the second one identifies a transaction. Recovery holds a full batch and
makes none of it durable — whatever appended those records already did that,
and no caller is waiting to acknowledge them.

Every backend's `recover_durable_journal` has the identical shape: read the
pending tail with `read_durable_journal_from`, then apply it. That uniformity
is what makes the distinction expressible once, in `nimbus-storage`, rather
than per dialect:

```rust
pub enum DurableApplyKind {
    ClientBatch,
    JournalReplay,
}

impl DurableApplyKind {
    /// The records this boundary makes durable, which is all the fault
    /// interface may see. A replay makes none, so a fault armed for a client
    /// batch cannot be consumed by a replay of an older one.
    pub fn newly_durable_records(self, records: &[TenantEventRecord]) -> &[TenantEventRecord] {
        match self {
            Self::ClientBatch => records,
            Self::JournalReplay => &[],
        }
    }
}
```

Each backend's apply-a-durable-batch entry point splits in two around it:
`apply_durable_records_batch` (the client path, unchanged for every caller) and
`replay_durable_records_batch` (recovery's only correct caller), both delegating
to one private body that differs solely in the kind it passes. For PostgreSQL
and MySQL the split is sharper still — the shared replay body simply omits the
`note_durable_records_for_fault` call, so replay reaches the identical SQL with
no identity attached. `SqlStoreCore` carries `replay_durable_records_batch`
alongside `apply_durable_records_batch`, so a new SQL dialect cannot compile
without deciding which of the two it is implementing.

### Threading it identically on every dialect

The three SQL dialects reach `StorageCommitAfterVisibilityBeforeReturn` through
one shared `sql_commit`, which sees only the backend — not the records. The
records are therefore stashed on the write transaction, at shared sites, and
composed into the check by a *provided* trait method so no dialect can quietly
fail to thread it:

```rust
pub(crate) trait SqlWriteBackend {
    /// Called from the shared apply and journal-batch paths only. A dialect must
    /// not call it: identity that one dialect records and another forgets is the
    /// drift these seams exist to prevent.
    fn note_durable_records_for_fault(&mut self, records: &[TenantEventRecord]);
    fn durable_records_for_fault(&self) -> &[TenantEventRecord];
    fn check_fault_for_records(&self, point: FaultPoint, records: &[TenantEventRecord]) -> Result<()>;

    /// Provided, and deliberately so: composing it here is what makes every
    /// dialect thread identical identity. Do not override it in a dialect.
    fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.check_fault_for_records(point, self.durable_records_for_fault())
    }
}
```

There are exactly two shared stash sites, both dialect-agnostic:

| Site | What it names |
| --- | --- |
| `sql/commit_effects.rs`, `DocumentWrites::PreparedDurableRecord` arm | the one prepared record this apply materializes |
| `sql/store_core.rs`, the append / apply / fenced-apply journal wrappers | the whole batch (the replay wrapper deliberately names nothing) |

Per backend:

| Backend | How records reach the commit-sequence check | Replay path |
| --- | --- | --- |
| PostgreSQL | `durable_records_for_fault: Vec<TenantEventRecord>` on the write transaction; `check_fault_for_records` → `fault_injector.check_for_tenant(point, tenant, records)` | `sql_store_replay_durable_records_batch` omits the stash |
| MySQL | identical to PostgreSQL | identical to PostgreSQL |
| libSQL replica | same field, replacing the ad-hoc `prepared_record_for_fault: Option<_>`; `check_fault_for_records` → the store's `check_durable_records_fault` | `apply_remote_batch(records, kind)` takes the kind; recovery passes `JournalReplay` |
| redb | `TenantWriteTransaction.durable_records_for_fault`, taken before the partial move in `commit_with_timestamp` and passed to `commit_write_txn_cancellable`; `commit_journal_txn` gained the same parameter | `replay_durable_records_batch` |
| SQLite | same field on `SqliteWriteTransaction`; all six journal checks and both commit checks switched to `check_durable_records(point, records)` | `replay_durable_records_batch` |
| Memory | `transact_durable_records(records, apply)`, with the old `transact` delegating with `&[]` | `replay_durable_records_batch` |

libSQL previously passed exactly one record via `slice::from_ref` at a single
site; PostgreSQL and MySQL reached the point through `check_for_tenant` with no
records in hand at all. That asymmetry is gone — the three dialects now hold
identical code at the seam.

Threading only the three SQL dialects would have been wrong. The embedded PPSC
lanes (memory, redb, sqlite) arm the same `AcknowledgementLoss` and assert it
fires, but reach the fault point through the record-less `check(point)` on a
tenant-scoped injector. A "fire only on non-empty records" harness rule would
have silenced those lanes. All six backends thread records for that reason.

## Harness change

`PpscStorageFaultInjector::check_tenant` takes `records` and applies a single
rule, after counting the visit:

```rust
current.visits = current.visits.saturating_add(1);
if records.is_empty() {
    return Ok(());
}
current.fires = current.fires.saturating_add(1);
```

One rule covers both thieves, because the product now reports them the same
way: a record-less commit has nothing to name, and a replay makes nothing
durable, so neither presents records. That the second is true of the product is
not something the harness can verify or recover; it is the product's side of
the contract, stated in `check_tenant`'s doc comment and pinned by the
storage-level tests named below. A newly instrumented boundary has to decide
which of the two it is.

Retries keep firing, which `ProviderTransient` requires: a batch retried after a
transient failure is still making its records durable, so it still presents
them, and it still fails until the scenario releases the fault.

`PpscStorageFaultSnapshot` semantics are now explicit and asymmetric on
purpose: `visits` counts every armed check that reached the fault's tenant and
point whether or not it fired, `fires` counts only the checks that actually
failed. `visits > fires` is the deflection count — the arm-theft attempts that
were turned away. Making deflections observable is what lets a test assert that
the fix is working rather than that nothing happened.

`nimbus-system`'s `ArmedProjectionAcknowledgementLoss` collapsed its two
overrides into one records-carrying `check_for_tenant` with behavior preserved:
a table- or epoch-targeted arm requires a matching durable record, so it passes
over a boundary carrying none (which is also how it declines an unrelated
concurrent same-tenant commit); a tenant-only arm still fires at that tenant's
first post-visibility boundary regardless of records.

## The watermark that was tried, measured, and discarded

Thief 2 invites an obvious harness-only fix, and this lane built it before
rejecting it. Keep a per-tenant durable watermark, updated on every
records-carrying check whether armed or not; freeze it at `arm()` time as
`armed_above`; fire only for a batch whose highest sequence exceeds it. It is
self-contained in `nimbus-testing`, it needs no product change beyond passing
records, and it passed its own unit test and eight loaded iterations.

It is wrong, and the acceptance loop caught it. The failing run recorded a
replay of sequence 9 stealing an arm frozen at `armed_above: 6` — a replayed
sequence *above* the watermark. Durability for a provider batch is established
out of process, on the remote primary; the local fault interface never observes
those boundaries, so a watermark accumulated from locally observed checks
cannot bound what recovery may later replay. The rule is unsound in exactly the
configuration PPSC exists to test, and it is unsound silently — it narrows the
race rather than closing it, which is the failure mode most likely to be
mistaken for a fix.

The lesson generalizes past this rule: the harness cannot reconstruct
durability from what it happens to see. Only the boundary knows whether it is
the one making records durable, which is why `DurableApplyKind` lives in the
product.

## U4 / U5 gates

Unchanged and green. The U4 journal-ownership gate pins the number of
*fault-point name* tokens per owner file, not method-name or argument counts:
`postgres/write_pipeline.rs` 4, `mysql/write_pipeline.rs` 2, `libsql/write.rs`
2. Adding a `records` argument to an existing check moves no token, and no
`StorageCommit*` token was added inside a provider directory — the
commit-sequence points remain owned solely by `sql/write_core.rs`. No pin was
edited.

The apply/replay split is the one change that could plausibly have moved a
token, so it was checked directly rather than assumed. `replay_durable_records_batch`
lands on the `SqlStoreCore` / `DurableJournal` seam, not at a fault-check site,
and the live token counts still read 4 / 2 / 2 against pins of 4 / 2 / 2. A
recursive scan confirms those three files are the only holders of any
`JOURNAL_FAULT_POINTS` token anywhere under `src/{postgres,mysql,libsql}`, and
`crates/nimbus-storage/src/tests/commit_path_ownership.rs` itself has an empty
diff against `origin/main`. `JOURNAL_OWNERS` therefore needs neither an edit nor
the loud justification the gate's failure message asks for.

## Evidence

### The second thief, caught in the act

The first iteration of this work threaded records and fired on any non-empty
batch. It still failed the libsql differential:

```
panicked at crates/nimbus-engine/src/tests/ppsc.rs:485:43:
PPSC atomic provider acknowledgement loss must require crash-and-replay;
fault snapshot: PpscStorageFaultSnapshot { active: false, visits: 1, fires: 1 }
```

`fires: 1` with the mutation nonetheless returning `Ok` is the signature of
theft by a *records-carrying* boundary: something consumed the arm, and it was
not the batch under test. A backtrace at the fire site named it exactly:

```
 3: PpscStorageFaultInjector::check_tenant                        faults.rs
 5: LibsqlReplicaTenantStore::check_durable_records_fault         libsql.rs:463
 6: LibsqlReplicaTenantStore::apply_remote_durable_records_batch  libsql.rs:1138
22: libsql::read::…::recover_durable_journal
23: nimbus_engine::persistence::tenant::journal::…::recover_durable_journal
```

### Where fires actually come from

Instrumenting every fire with its call site, over nine loaded libsql
iterations, captured 298 fires:

| Count | Fault | Site | Under recovery |
| --- | --- | --- | --- |
| 3 | AcknowledgementLoss | `apply_remote_durable_records_batch` | **yes** |
| 31 | AcknowledgementLoss | `commit_write_txn_cancellable` (redb reference lane) | no |
| 28 | AcknowledgementLoss | `fenced_append_and_apply_remote_durable_records_batch` | no |
| 124 | ProviderTransient | `commit_journal_txn` | no |
| 112 | ProviderTransient | `fenced_append_and_apply_remote_durable_records_batch` | no |

The three recovery fires correspond one-to-one with the three arm-theft
iterations in that run, and that path never fires legitimately: it is the only
row that is both rare and fatal.

The tally also rules out the blunter fix of suppressing records inside
`apply_remote_durable_records_batch` itself. The 31 redb fires are the
reference lane's *embedded* apply legitimately losing an acknowledgement, which
the differential asserts reconciles in place. The discriminator has to be the
caller, not the callee — which is what `DurableApplyKind` encodes.

Two cautions for anyone repeating the measurement. The instrumentation widens
the race (3 failures in 9 instrumented iterations against roughly 1 in 8
uninstrumented), so instrumented rates are useful for reproduction and must not
be quoted as the flake rate. And the differential runs each scenario on two
lanes — redb reference plus provider — so two ack-loss fires per tenant is the
correct count, not a duplicate.

### Flake statistics

The race only opens under CPU contention, so the arms were run on the libsql
lane (the flakiest) under an identical synthetic load of six spinning
processes. Each arm is a one-line ablation of the shipped code, so the arms
differ only in the rule under test (`scratchpad/fu2-arms.sh`):

| Arm | Rule under test | Iterations | Passed | Arm theft | Other |
| --- | --- | --- | --- | --- | --- |
| A | pre-FU2: harness ignores records, replay names records | 8 | 8 | 0 | 0 |
| B | harness rule active, replay still names records | 8 | 8 | 0 | 0 |
| C | shipped: harness rule active, replay names nothing | 8 | 8 | 0 | 0 |

**This battery is inconclusive and must not be read as a fix demonstration.**
Arm A is the pre-FU2 baseline and is the arm that was supposed to reproduce the
flake. It did not fire once. A battery whose positive control stays silent
cannot discriminate between the arms, so the identical tallies above carry no
information about arm C.

The ablations did apply — this is underpowering, not a broken harness. The
mechanism was checked before the result was accepted: no ablation-anchor
`AssertionError` appears in the runner's stderr (the anchors are asserted in
Python, and `fu2-arms.sh` runs under `set -uo pipefail` without `-e`, so a
missed anchor would have been silently skipped — it wasn't), the three arm logs
carry distinct checksums, and the source was verified restored to shipped form
afterwards. The arithmetic explains the rest: against the previously measured
loaded rate of roughly 1 failure in 8 iterations, the chance of drawing zero
failures in 8 trials is `0.875^8 ≈ 0.34`. A one-in-three null result is an
unremarkable draw, not a signal. Eight iterations per arm was simply too few;
separating a ~12% rate from zero with any confidence needs on the order of 24+
iterations per arm.

The reproduction evidence for the defect is therefore the instrumented run
recorded above — 3 arm-theft failures in 9 iterations, each classified by
backtrace, with the 298-fire tally naming `recover_durable_journal` →
`apply_remote_durable_records_batch` as the thief — and not this table. The
table is kept rather than dropped because a quietly discarded negative result is
exactly how an underpowered battery gets rerun and misread by the next person.

Failures are classified by signature, not by exit code: only a panic carrying
`must require crash-and-replay` counts as arm theft. That distinction earned
its keep — a first attempt at this measurement used sixteen spinners, drove the
load average past 70, and failed on `commit_faults.rs:36` ("did not reach
durable-before-publish within 5s"), an unrelated wall-clock deadline blown by
the stressor itself. It is recorded here because the same trap will catch the
next person: under a blunt enough stressor, everything fails, and a bare exit
code cannot tell you why.

On an unloaded host the race did not open at all: 27 libsql/mysql/postgres runs
with the pre-FU2 tenant+point keying produced zero failures, and a 96-snapshot
probe across all four backends recorded `visits: 1, fires: 1` every time — no
thief ever arrived. Reproducing this defect requires contention, so a quiet 20x
run is necessary evidence but not sufficient on its own — the loaded arms were
meant to supply the rest and, as recorded above, did not.

That leaves the case for the fix resting on three legs rather than four: the
instrumented reproduction that identified the thief by backtrace, the
deterministic regression tests below that pin both halves of the contract
without needing the race to open, and the 60-run acceptance sweep. What is
honestly missing is a powered loaded A/B contrast. It is worth having, it costs
roughly two hours at 24 iterations per arm, and it is the first thing to run if
this flake is ever seen again.

### Acceptance: seeded differentials, 20x

The acceptance run is `scratchpad/diff-loop.sh fu2-final-20x 20`: the external
provider fixtures are torn down and brought back up once, then twenty
iterations each run all three provider seeded journal differentials under the
`fu2-loop` nextest profile. The `external-provider` test group serializes them,
so each iteration exercises the three dialects back to back against the same
live fixtures.

| Dialect | Runs | Passed | Arm theft | Other failures |
| --- | --- | --- | --- | --- |
| `libsql_ppsc_seeded_journal_differential` | 20 | 20 | 0 | 0 |
| `mysql_ppsc_seeded_journal_differential` | 20 | 20 | 0 | 0 |
| `postgres_ppsc_seeded_journal_differential` | 20 | 20 | 0 | 0 |

Every iteration reported `iteration N rc=0` and
`Summary [...] 3 tests run: 3 passed, 665 skipped`; iteration wall times ranged
from 161s to 226s. Across the whole log
`grep -c 'must require crash-and-replay'` returns **0**, and no `FAIL` line
appears. Sixty differential runs, zero failures of any kind.

No source file was touched while the loop ran. That is a deliberate condition
of the measurement: a mid-loop edit triggers a cargo rebuild, so the later
iterations would be testing different code than the earlier ones. An earlier
attempt in this effort was invalidated exactly that way and had to be discarded.

### The arm still fires on its intended target

The 96-snapshot probe is also the product-side threading proof. Under the fix a
fault can only fire on a boundary that names newly durable records, so
`fires: 1` observed on **all four backends** — libsql, mysql, postgres and redb
— is direct evidence that records reach the check on every dialect. A dialect
that failed to thread them would report `fires: 0` and its ack-loss test would
fail.

| Backend | Armed windows observed | Snapshot |
| --- | --- | --- |
| redb | 48 | `active: false, visits: 1, fires: 1` |
| postgres | 16 | `active: false, visits: 1, fires: 1` |
| mysql | 16 | `active: false, visits: 1, fires: 1` |
| libsql | 16 | `active: false, visits: 1, fires: 1` |

### Deterministic regression tests

The race is reproducible only under load, so both halves of the contract are
pinned deterministically — the harness rule where it is decided, and the
product statement it rests on where it is made:

- `ppsc_storage_acknowledgement_loss_is_not_consumed_by_a_record_less_commit`
  (`nimbus-testing`) — three boundaries naming no durable record deflect
  (`visits: 3, fires: 0`, arm still active), then the durable batch fires.
- `redb_journal_replay_names_no_durable_records`,
  `memory_journal_replay_names_no_durable_records`,
  `sqlite_journal_replay_names_no_durable_records` (`nimbus-storage`) — a
  recording injector observes that `recover_durable_journal` still reaches the
  commit-sequence fault points (so the assertion is not vacuous) and names zero
  records at every one, while the append that made those records durable and a
  subsequent client batch both name theirs.

The harness test fails with the `records.is_empty()` rule removed; the storage
tests fail with `DurableApplyKind::JournalReplay` reporting its records. That
is what makes them regression tests rather than restatements.

### Suites

| Suite | Pre-rebase (base 22c5cdd62) | Post-rebase (base f94ed17dc) |
| --- | --- | --- |
| `nimbus-storage --features libsql,mysql,postgres` | 442 passed, 2 skipped | 447 passed, 2 skipped |
| `nimbus-testing` | 50 passed, 0 skipped | 50 passed, 0 skipped |
| `nimbus-engine` | 663 passed (4 slow), 5 skipped | 666 passed (4 slow), 5 skipped |
| `clippy -D warnings` (storage + engine + testing, featured, `--all-targets`) | clean | clean |
| `cargo fmt --all --check` | clean | clean |

The `+5` and `+3` are tests that arrived with the fifteen commits landed between
the two bases (FU6, FU10, FU11, FU12), not new tests from this change. Zero
failures in either column. The 20x acceptance sweep above was run pre-rebase;
it was not repeated, because the rebase changed no fault-path logic — see the
rebase note below.

The featured storage lane carries the surfaces this change could plausibly have
disturbed: the five `tests::async_faults::*` cancellation tests, the U4/U5
ownership gates in `tests::commit_path_ownership`, and the three new
`*_journal_replay_names_no_durable_records` recovery tests. The engine lane
carries the seeded differentials and the ack-loss reconciliation tests
(`postgres_schedule_only_execution_unit_reconciles_acknowledgement_loss`,
`postgres_trigger_outcome_reconciles_acknowledgement_loss_without_reexecution`)
— the two `commit == None` shapes that the refuted `commit.is_some()` gate would
have silently stopped covering.

Clippy did not pass first time, and the failure was in this change rather than
around it: `needless_lifetimes` on `DurableApplyKind::newly_durable_records`,
whose `<'a>` was written explicitly where elision expresses the same signature.
`self` is taken by value on a `Copy` enum, so `records` is the only input
reference and the output already binds to it. The lifetime was removed and the
three suites above were rerun against the corrected source, returning identical
counts; the numbers in this table come from that rerun, so they describe the
code that is actually committed.

### Pair interference: the FU10 sighting does not reproduce here

FU10 reported that `tests::ppsc::differential::redb_ppsc_seeded_journal_differential`
and `sqlite_ppsc_seeded_journal_differential` interfere with each other —
passing alone, failing when run together — pre-existing at base. That was worth
checking against this fix, because the two candidate explanations predict
different outcomes: genuine shared state between the tests would survive the
fix, whereas a load-sensitive intra-test race would not, since a sibling test is
simply another source of the contention the arm-theft race needs.

| Configuration | Runs | Tests per run | Passed | Failed |
| --- | --- | --- | --- | --- |
| `redb` alone | 3 | 1 | 3 | 0 |
| `sqlite` alone | 3 | 1 | 3 | 0 |
| both together | 5 | 2 | 10 | 0 |

Sixteen test executions across eleven runs, no failures in any configuration.
The interference does not reproduce on this branch.

Two honest limits on that verdict. First, absence over five paired runs does not
prove the interference is gone — the original sighting was itself intermittent,
and five runs cannot exclude a low-rate effect any more than eight could in the
arms battery above. Second, the reading that this fix explains it is supported
but not established: the two tests share no fault state and no on-disk path
(`new_embedded` builds a fresh `tempdir()` and a fresh
`PpscStorageFaultInjector::new()` per runner, `differential.rs:24-65`), which
rules out the shared-state explanation and leaves the load-sensitive one, which
is the shape this fix addresses. Recorded as: not reproducible post-fix, cause
consistent with the arm-theft race, not independently confirmed. If it is seen
again it should be reopened against the harness rather than assumed closed by
this ticket.

### Rebase onto current main

The work was written against `22c5cdd62` and rebased onto `f94ed17dc`, fifteen
commits later. Three conflicts, all resolved in favour of both sides:

- `libsql.rs` imports — FU6 moved `rebuild_sqlite_indexes_from_loaded_schema`
  into `sqlite::replica_cache`. Took main's path, kept this change's
  `DurableApplyKind` import.
- `sqlite/journal.rs` — FU6 relocated `reconcile_replica_durable_records_batch`
  wholesale into `sqlite/replica_cache.rs`, which this change had edited in its
  old location. Resolved to main's side (function gone from `journal.rs`), then
  the two edited lines re-applied in the new file. A diff of the two copies
  confirmed those two lines were the *only* difference, so nothing else was
  carried or dropped.
- `storage-follow-ups-plan.md` — took main's table wholesale (it carries the
  FU1-FU12 flips and the new FU13/FU14 rows) and re-applied only the FU2 row.

The re-applied lines name `records` at
`JournalAppendBeforeDurableFlush` / `JournalFlushBeforeVisibility` on the
replica-cache append. That is deliberate and matches every other journal
boundary: the reconciliation really does append new rows to the local
`commit_log`, so those records are what it is making locally durable. It is not
a `DurableApplyKind::JournalReplay` site — replay is the *recovery* path that
re-materializes records already present, and it is the only thing that names
nothing. `tests::sqlite_foundation::journal::sqlite_replica_journal_reconciliation_accepts_identical_overlap_and_missing_suffix`
covers these lines and passes.

No fault-path logic changed in the rebase, which is why the 20x acceptance sweep
was not repeated; the full featured battery, both U4 gates and the ack-loss
tests were rerun and are the post-rebase column above.

## Files changed

Product seam: `crates/nimbus-storage/src/simulation/faults.rs`,
`sql/write_core.rs`, `sql/commit_effects.rs`, `sql/store_core.rs`, `lib.rs`,
`simulation.rs`.
Dialects: `postgres.rs`, `postgres/read.rs`, `postgres/write.rs`,
`postgres/write_pipeline.rs`, `mysql.rs`, `mysql/read.rs`, `mysql/write.rs`,
`mysql/write_pipeline.rs`, `libsql.rs`, `libsql/read.rs`, `libsql/write.rs`,
`libsql/provider.rs`.
Embedded: `store.rs`, `store/journal.rs`, `store/write/transaction.rs`,
`store/write/store_entry.rs`, `sqlite.rs`, `sqlite/config.rs`,
`sqlite/journal.rs`, `sqlite/replica_cache.rs`, `sqlite/write.rs`, `memory/store.rs`, `memory/journal.rs`,
`memory/documents.rs`, `memory/provider.rs`.
Tests: `crates/nimbus-testing/src/ppsc/faults.rs`, `ppsc/tests.rs`,
`crates/nimbus-storage/src/simulation/tests.rs`,
`crates/nimbus-storage/src/tests/recovery.rs`,
`crates/nimbus-system/src/projection/reconciliation_tests.rs`.

## Review finding: memory dedup path named an unmaterialized record

The pre-merge autoreview surfaced (below its P0 threshold, verified real) that
`MemoryTenantStore::apply_prepared_write_batch` — newly record-naming in this
change — passed its prepared record to the commit-sequence fault points even
when `begin_scheduled_execution` deduplicated the delivery and the closure
returned `Ok(None)`. A deduplicated execution materializes nothing durable, so
the no-op could consume a one-shot fault armed for the batch that genuinely
commits: the same arm-theft class this change closes on the replay side. The
SQL core was verified unaffected — its dedup returns
`SkippedDuplicateExecution` before `note_durable_records_for_fault`.

Fix: `transact_admitted_durable_record` in `memory/store.rs` names the record
only when the closure admits the write. Regression test
`memory_deduplicated_prepared_write_names_no_durable_records` in
`tests/recovery.rs` asserts the admitted delivery names its record at
`StorageCommitAfterVisibilityBeforeReturn` and the deduplicated delivery still
reaches the fault points naming zero records.

Post-fix battery: nimbus-storage (libsql,mysql,postgres) 448 passed / 2
skipped; nimbus-testing 50/0; nimbus-engine 666 passed / 5 skipped; clippy
`-D warnings` storage+testing clean; fmt clean.
