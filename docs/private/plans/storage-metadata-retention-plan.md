# Storage Metadata Retention Plan

Status: `active` | Owner: this plan | Created: 2026-08-25.
Baseline: main @ cc7ae36a3c21bf7aa093c013f3025d074c679438.
Proof root: `docs/private/plans/proof/storage-metadata-retention/`.
Next action: execute SMR3 in the dedicated worktree. Add the bounded shipped
profile and one single-flight maintenance controller per loaded tenant, with
ordered finalization, cancellation, retry, diagnostics, metrics, and a manual
operator trigger.

## Outcome

> Nimbus retains a bounded, explicit window of document history, index history,
> CDC events, and point-in-time restore history. It advances each durable floor
> only after a materialized checkpoint proves that the dependent state can be
> rebuilt, publishes the checkpoint and deletion cut atomically, rejects every
> reader that crosses a published floor, and runs this maintenance in the
> production tenant lifecycle on every supported backend. A crash, concurrent
> append, stale cursor, or provider error can delay reclamation but cannot make
> retained history incomplete or make trimmed history appear valid.

## Architecture

Before:

```text
[tenant commits]
      |
      +----> [commit log from sequence 1 forever]
      +----> [document/index version tables forever]
      |
      +----> [PITR: empty sequence-0 snapshot + replay from genesis]

[RetentionFloor + resource watermarks + MVCC prune methods]
      |
      +----> tests and diagnostics only

durable_journal_cursor_floor() == 0 in production
```

After:

```text
[retention profile: document | index | CDC | PITR windows]
      |
      v
[candidate floors + active participant pins]
      |
      +----> [build checkpoint at candidate journal cut]
      |              |
      |              v
      |       [validate position and contiguous tail]
      |              |
      v              v
[tenant maintenance transaction / current committer lease]
      |
      +----> publish confirmed checkpoint
      +----> prune commit log only through confirmed checkpoint
      +----> preserve document anchors and live index intervals
      +----> publish exact durable floors and counts
      |
      v
[PITR: retained checkpoint + bounded journal tail]
[CDC/page reads: pre-read and post-read floor validation]
[old target/cursor: typed RetentionExpired]

[Engine tenant lifecycle]
      +----> bounded, single-flight maintenance
      +----> conservative durable window
      +----> separate aggressive in-memory conflict window
      +----> metrics, diagnostics, and operator controls
```

## Scope

- Owns the durable document-version, index-version, CDC-journal, and PITR
  retention contract.
- Owns a provider-neutral materialized retention checkpoint and its format,
  validation, crash behavior, and `MaterializedPosition` binding.
- Owns atomic checkpoint publication and commit-log pruning on memory, redb,
  SQLite, PostgreSQL, MySQL, and libSQL.
- Owns production Engine scheduling, single-flight execution, bounded policy,
  operator controls, diagnostics, metrics, and shutdown behavior.
- Owns trimmed-history errors and page-boundary validation for journal,
  changefeed, historical read, and PITR consumers.
- Owns retained PITR export and import from a nonzero materialized checkpoint.
- Owns the M11 checklist rule: a retained replay range distinguishes an empty
  logical change from unavailable replay data. A missing record never means
  both.
- Owns the M12 checklist rule where this plan edits shared metadata: the floor,
  checkpoint position, and deletion cut publish as one transaction and one
  observable state.
- Does not own blob liveness, blob sweeping, request orphans, or remote object
  reclamation. `blob-lifecycle-integrity-plan.md` owns those items.
- Does not own distributed consumer membership, cross-node checkpoint
  replication, mixed-version fleet rollout, or a consensus retention leader.
  `horizontal-scaling-plan.md` owns those items. This plan uses the current
  provider committer lease as single-tenant maintenance authority and leaves a
  typed seam for a future distributed authority.
- Does not own the process-local mutation conflict window. The archived PPSC
  plan owns its current time/size/frontier policy. This plan reports the two
  windows separately and does not make the durable floor as aggressive as the
  conflict window.
- Does not own tenant KV. NKV decides whether that journal-less plane is backed
  up or replicated.
- Non-goals: an epoch seal, a second client mutation path, per-object blob
  reference counts, a compatibility layer for unlaunched formats, or a generic
  distributed lease system.

## Promotion Gate

This plan is active because the owner started Band SA execution and SA4 is the
only active Band SA row. Promotion evidence:

1. Band SA records SA4 as a confirmed high launch-readiness gap.
2. The baseline commit and proof root are pinned.
3. Existing behavior was traced through all six backends, PITR, changefeed,
   historical reads, diagnostics, and the Engine lifecycle.
4. Convex's current retention manager was re-read from the updated local
   exemplar. Nimbus adopts separate document/index floors, confirmed-deleted
   ordering, and post-read validation without adopting Convex's distributed
   worker topology.
5. Every task below has falsifiable acceptance evidence and a pull-request
   boundary.

## Current Behavior At Promotion

- `RetentionFloor::gc_watermarks` computes resource-specific safe floors and
  routes pins by participant.
- `compact_retained_versions` prunes document and index versions atomically on
  redb, SQLite, PostgreSQL, MySQL, and libSQL. It preserves the document anchor
  at the floor and keeps live index intervals.
- `RetentionGcConfig::default()` is `retain_all`, and production Engine code
  never calls `compact_retained_versions`.
- Commit-log removal exists only in a memory-store test hook. Backend cursor
  floors are inferred from the oldest physical log row and therefore remain 0
  in production.
- Journal and changefeed page readers reject a cursor behind the floor before
  reading. PostgreSQL, MySQL, and remote libSQL do not yet prove a post-read
  floor check against a concurrent prune.
- PITR export always constructs an empty sequence-0 base and replays from
  genesis. Archive import explicitly rejects a nonempty base even though the
  lower rebuild primitive can restore one.
- Storage diagnostics expose version watermarks and pins, but they do not
  expose a confirmed checkpoint, journal rows pruned, lifecycle state, or the
  last maintenance outcome.

## Invariants

1. A commit-log row is deleted only after one validated materialized checkpoint
   at or after that row is durable.
2. The durable checkpoint, confirmed deletion cut, and physical journal prune
   are one atomic maintenance transaction. A crash observes all or none.
3. A checkpoint binds `applied_sequence`, `durable_head`, state contents, and
   `MaterializedPosition`. Nimbus validates that binding before publication and
   before restore.
4. The checkpoint cut cannot exceed the applied head. A durable-but-unapplied
   record remains in the journal.
5. The journal cut is the minimum safe floor required by CDC, PITR, exported
   snapshots, embedded replicas, shadow materializers, and active pins.
6. Document compaction preserves the newest version at or before the retained
   floor for each document. Index compaction removes only closed intervals that
   cannot be visible at a retained read.
7. A physical delete cannot advance a logical floor past its corresponding
   confirmed-deleted checkpoint.
8. Floors are monotonic. Recovery can retain extra history but cannot move a
   published floor backward or claim deleted history is available.
9. Every journal, changefeed, historical-read, and PITR page validates its
   required floor after the read. A long scan that crosses a concurrent floor
   fails instead of returning a torn result.
10. A replay range is contiguous from checkpoint + 1 through its declared
    target. An absent record is a retention or corruption error, never an empty
    logical change.
11. A target older than the published floor returns typed
    `HistoricalReadErrorKind::RetentionExpired`. Provider errors and format
    errors retain their own classifications.
12. Maintenance runs through the tenant's ordered internal maintenance route.
    Provider-backed work validates the current committer lease in the same
    transaction. Embedded work relies on the Engine process fence.
13. Maintenance is single-flight per tenant, bounded, cancellable on shutdown,
    and retry-safe. Failure records an outcome and retains data.
14. Shipped Nimbus uses an explicit bounded retention profile. `retain_all`
    remains an explicit operator choice, not an accidental lifecycle default.
15. The durable policy has distinct document-version, index-version, CDC, and
    PITR windows even when a preset assigns equal values.
16. The process-local conflict window remains separately configured and may be
    more aggressive than durable retention. It cannot advance a durable floor.
17. Metrics use bounded labels and never include tenant IDs, document IDs,
    table names, state bytes, or SQL text.
18. Every supported backend passes the same semantic model. An unavailable
    live provider lane is a visible qualification gap, not a silent pass.
19. No task adds a fourth client document mutation path.

## Decisions

### SMR-D1 Use a durable materialized checkpoint

- A bounded commit log needs a rebuild base. The current sequence-0 PITR base
  is not sufficient after truncation.
- The checkpoint advances incrementally from the prior checkpoint plus a
  contiguous journal prefix. It does not reconstruct schema or scheduled state
  from document-version tables.
- One retained checkpoint is sufficient for the first version because the
  journal window owns target granularity. Re-open only if measured checkpoint
  construction or export needs multiple generations.

### SMR-D2 Separate desired, confirmed, and physical floors

- The policy computes a desired floor.
- A validated durable checkpoint establishes the confirmed floor.
- Physical deletion can reach the confirmed floor and no farther.
- Diagnostics expose all three so an operator can distinguish policy lag from
  deletion lag.

### SMR-D3 Keep four durable windows

- Document versions, index versions, CDC, and PITR have separate policy fields.
- The physical journal retains the most conservative of CDC and PITR plus
  relevant participant pins.
- Registry and read-policy history stay clamped to the most conservative
  dependent document/index floor until a narrower proof exists.

### SMR-D4 Use existing single-tenant authority

- Embedded engines use the process fence landed by SA3.
- Provider engines execute finalization under the current committer lease and
  tenant transaction lock.
- Horizontal scaling can replace the authority source. It cannot weaken the
  checkpoint-before-delete contract.

### SMR-D5 Preserve typed fail-closed reads

- Readers can do an optimistic pre-check to avoid work.
- They must validate again after each page against the floor observed with the
  page or a newer authoritative floor.
- If a provider cannot supply an atomic page/floor view, a changed floor makes
  the page fail. Nimbus does not return a partial page.

### SMR-D6 Retain-all is explicit

- The storage library keeps an explicit retain-all constructor for tests,
  diagnostics, imports, and operators that accept unbounded growth.
- The shipped Engine profile is bounded. SMR0 ratifies its values from current
  benchmarks and records the storage-growth consequence.
- A zero window remains invalid.

## Status Ledger

| ID | Task | Status | Evidence |
| --- | --- | --- | --- |
| SMR0 | Baseline: pin the code inventory, create the proof root, author the contract verifier red, capture fail-before behavior, and ratify the bounded shipped profile. No production behavior changes. | `done` | PR #313 merged as `0ff18d1a7`. Verifier: `Summary: 7 passed, 11 failed`. Calibration: 2,049 sequences, 3.22 MB archive, 108 ms export, 2,560 MVCC rows pruned in 15 ms. Proof: `smr0-baseline.md`. |
| SMR1 | Add the provider-neutral checkpoint and retention-state contract; support a nonzero PITR base; implement crash-safe compaction for memory, redb, and SQLite. | `done` | PR #314 merged as `0d4b9a112`. Full storage library lanes: 379 passed and 3 ignored with default features, and 379 passed and 3 ignored without default features. Verifier: `Summary: 12 passed, 6 failed`. Autoreview rerun clean. Proof: `smr1-embedded-checkpoint.md`. |
| SMR2 | Implement lease-fenced checkpoint publication, journal pruning, and MVCC compaction parity for PostgreSQL, MySQL, and libSQL. | `done` | PR #317 merged as `f97b2db67`. Nine focused live provider tests passed: stale fencing, exact restart floors, injected and real-SQL rollback, MVCC pruning, and libSQL retained-base cache rebuild. Verifier: `Summary: 13 passed, 5 failed`. Autoreview clean. Proof: `smr2-provider-parity.md`. |
| SMR3 | Wire bounded single-flight maintenance into the production Engine tenant lifecycle with configuration, cancellation, retry, diagnostics, metrics, and an operator-triggered seam. | `in_progress` | |
| SMR4 | Make journal, changefeed, historical-read, bootstrap, and PITR consumers fail closed across trimmed history, including post-page validation and concurrent-prune tests. | `todo` | |
| SMR5 | Run the semantic/provider/benchmark matrix, update storage architecture and operating docs, and publish the final launch-readiness verdict. | `todo` | |
| SMR9 | After the final code pull request merges, archive this plan, retain its proof root, remove its active index entry, and close Band SA row SA4 with exact evidence. | `todo` | |

## Tasks

### SMR0 Baseline And Contract Verifier

- Problem: existing unit tests prove isolated watermark and MVCC mechanics but
  do not detect an unbounded production lifecycle, absent checkpoint, or
  unreachable physical floor.
- Owning seam and paths: `scripts/verify-storage-metadata-retention.sh`, this
  plan, and the proof root.
- Steps:
  1. Inventory every production call to retention, PITR, journal streaming,
     historical reads, and Engine tenant startup/shutdown.
  2. Add a structural/behavioral verifier with one condition for each contract
     cluster: policy, checkpoint, embedded parity, provider parity, Engine
     lifecycle, fail-closed consumers, diagnostics, and qualification.
  3. Record the exact red summary against the pinned baseline.
  4. Ratify the shipped sequence windows and maintenance threshold from the
     existing latest-path, historical-read, compaction, PITR, and storage-growth
     evidence. If evidence is insufficient, add a measurement case; do not pick
     a hidden constant.
- Acceptance: the verifier reports its exact failing condition count; proof
  names every existing satisfied guard and every missing lifecycle condition;
  no production Rust behavior changes.
- Fail-before: production has no checkpoint/prune API or lifecycle caller, the
  inferred journal floor stays 0, and PITR requires a sequence-0 base.
- Verification: `bash scripts/verify-storage-metadata-retention.sh` and
  `bash scripts/check-docs.sh`.
- Pull request: baseline verifier and proof only.

### SMR1 Embedded Checkpoint And Compaction

- Problem: commit-log deletion has no rebuild base or atomic confirmed cut.
- Owning seam and paths: `crates/nimbus-storage/src/retention.rs`,
  `store/journal_snapshot.rs`, redb store modules, SQLite modules, memory store,
  storage traits, and semantic tests.
- Steps:
  1. Define the retention profile, desired/confirmed/physical state, checkpoint
     format, summary, and validation errors.
  2. Rebuild a candidate checkpoint from the prior checkpoint plus a contiguous
     journal prefix and verify its `MaterializedPosition`.
  3. In one backend transaction, compare the expected prior checkpoint,
     publish the candidate, prune journal rows through its cut, and compact
     eligible document/index versions.
  4. Teach PITR archives and imports to use a validated nonzero base.
  5. Implement memory, redb, and SQLite parity with restart and fault tests.
- Acceptance: generated histories show that every target at or after the
  checkpoint restores to the same position; targets before it fail with
  `RetentionExpired`; faults before commit retain all history; faults after
  commit expose the checkpoint and pruned floor together; append concurrency
  cannot create a gap.
- Fail-before: the existing memory test hook can delete rows without a
  checkpoint, and the archive importer rejects every nonempty base.
- Verification: focused `nimbus-storage` retention/PITR tests, storage Clippy,
  format, the verifier, and Nimbus autoreview at `pre-pr`.
- Pull request: one embedded-contract PR.

### SMR2 Provider Parity And Lease Fencing

- Problem: supported providers need the same atomic contract and must not let a
  stale Engine generation prune shared tenant history.
- Owning seam and paths: SQL store core and transaction traits, PostgreSQL,
  MySQL, libSQL remote-primary/cache code, provider conformance fixtures, and
  committer-lease validation.
- Steps:
  1. Add concept-owned checkpoint persistence for each provider.
  2. Validate owner, epoch, and expected checkpoint state in the same
     transaction that publishes and prunes.
  3. Keep latest/applied sequence metadata independent of physical minimum log
     sequence.
  4. Refresh libSQL cache from the retained checkpoint plus tail when its local
     journal prefix no longer exists remotely.
  5. Prove rollback on injected and real provider errors.
- Acceptance: the semantic model passes on PostgreSQL, MySQL, and libSQL; a
  stale lease is fenced with zero deletion; restart retains exact floors; local
  cache refresh works after remote pruning; provider-specific errors do not
  become retention or corruption errors.
- Fail-before: no provider exposes physical pruning or a durable checkpoint;
  remote page reads can race a future prune.
- Verification: focused provider tests with explicit skip/fail output, storage
  Clippy in all feature graphs, format, verifier, and Nimbus autoreview.
- Pull request: one provider-parity PR.

### SMR3 Production Lifecycle

- Problem: safe storage methods do not bound growth unless production invokes
  them with an explicit policy and owns their lifecycle.
- Owning seam and paths: Engine persistence configuration, tenant load/runtime,
  internal committer route, background executor, diagnostics, metrics, CLI and
  server configuration surfaces.
- Steps:
  1. Add the bounded shipped profile and explicit retain-all override.
  2. Start one single-flight maintenance controller per loaded tenant.
  3. Prepare checkpoints off the mutation hot path and finalize through the
     ordered internal maintenance route.
  4. Cancel and drain maintenance on eviction, tenant deletion, and Engine
     shutdown.
  5. Expose manual trigger/result, desired/confirmed/physical floors, counts,
     lag, duration, failures, and next eligibility.
- Acceptance: deterministic-clock tests prove automatic advancement, no
  overlapping run, retry after failure, clean shutdown, and no hot-path wait
  below the eligibility threshold; shipped construction uses a bounded profile;
  retain-all is visible in diagnostics.
- Fail-before: Engine imports only `RetentionGcConfig` for latest PITR export
  with its retain-all default and has no compaction caller.
- Verification: focused Engine lifecycle tests, metrics tests, config tests,
  storage/Engine Clippy, format, verifier, and Nimbus autoreview.
- Pull request: one lifecycle PR.

### SMR4 Trimmed-History Consumer Safety

- Problem: a reader that validates only before a page can race deletion and
  return an incomplete range; every historical consumer must share one typed
  floor contract.
- Owning seam and paths: durable journal pages, changefeed, historical query
  pagination, bootstrap, PITR export, and provider read transactions.
- Steps:
  1. Return the authoritative floor with each page or validate it after the
     page read.
  2. Map behind-floor states to typed `RetentionExpired` across public engine
     surfaces.
  3. Validate contiguous sequence ranges and distinguish empty logical events
     from missing records.
  4. Add deterministic concurrent-prune tests at every page boundary and
     backend-specific transaction-shape tests.
- Acceptance: a page wholly inside the window succeeds; a cursor/target below
  the floor fails before data; a floor crossing during a long scan fails after
  the affected page; no consumer returns a gap, duplicate, or partial success.
- Fail-before: remote provider stream methods read floor and rows in separate
  statements without a post-read validation contract.
- Verification: focused historical/CDC/PITR tests, provider lanes, format,
  Clippy, verifier, and Nimbus autoreview.
- Pull request: one consumer-safety PR.

### SMR5 Qualification And Documentation

- Problem: retention changes storage size, write cost, restore cost, and
  provider operations; launch readiness needs measured evidence, not only unit
  tests.
- Owning seam and paths: storage benchmarks, provider harness, storage
  architecture, operating runbooks, proof root, and verifier.
- Steps:
  1. Run generated model histories, crash/restart faults, and all supported
     provider lanes.
  2. Measure latest-path impact, compaction throughput, checkpoint size/build
     cost, bounded PITR replay cost, and steady-state storage growth.
  3. Confirm the shipped profile against the ratified bounds or revise it with
     recorded evidence.
  4. Document operator controls, floor semantics, recovery, alerts, and
     horizontal-scaling handoff.
  5. Run the required repository gate and publish `SAFE` or `NOT SAFE` with
     every gap named.
- Acceptance: verifier is fully green; all available provider lanes pass;
  skipped live lanes are explicit; the measured profile keeps latest-path and
  storage-growth budgets; architecture and runbooks match code.
- Fail-before: no production lifecycle or bounded steady-state evidence exists.
- Verification: focused benchmarks, provider qualification, docs gates,
  `make ci`, and final Nimbus autoreview.
- Pull request: one qualification/docs PR if code or public docs change;
  private proof and ledger updates follow the repository's direct-to-main plan
  convention.

### SMR9 Cleanup

- Problem: a completed implementation plan must not remain an active source of
  truth.
- Owning seam and paths: this plan, its proof root, `plans/README.md`, and Band
  SA row SA4.
- Steps: confirm every code PR and required gate, mark SA4 done with exact
  evidence, archive this plan while retaining proof, remove the active index
  entry, and advance Band SA to its next eligible row.
- Acceptance: no active plan routes work to this file; SA4 is terminal; the
  archive and proof contain the completed record; Band SA has exactly one next
  `in_progress` row or is complete.
- Fail-before: this plan is active and SA4 is in progress.
- Verification: search for the plan slug, run docs gates, and inspect both
  ledgers.

## Verification Contract

- Every code task captures fail-before evidence before production edits.
- Focused tests cover success, boundaries, failure, crash/restart, recovery,
  concurrency, and provider parity at the owning seam.
- Use `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` only for an
  explicit embedded lane. Provider qualification must state configured,
  skipped, passed, and failed targets.
- Run `cargo fmt --all --check` and relevant Clippy before each pull request.
- Run `make ci` before SMR5 closeout and whenever a change crosses Engine,
  storage, provider, and public configuration seams together.
- Run Nimbus autoreview at `pre-pr` after the final code commit and checks for
  every substantive code pull request.
- Proof files name commands, exit status, test counts, skips, commits, pull
  requests, and remaining uncertainty.

## Goal

```text
Execute this plan to completion as SA4 inside the active Band SA goal. This is
a whole-plan goal, not a single-task goal. Read this plan fully, then read
AGENTS.md, docs/private/operating/verification.md,
docs/private/architecture/storage/README.md,
docs/private/plans/archive/storage-engine-quality-and-mvcc-plan.md,
docs/private/plans/proof/storage-engine-quality-and-mvcc/seq7-retention-gc.md,
the Band SA section of architecture-review-2026-07-plan.md, and the current
Convex retention sources under ~/src/github.com/get-convex/convex-backend.
Work in /Users/jack/src/github.com/nimbus/nimbus-worktrees/sa4-metadata-retention
and use dedicated codex/sa4-<task> branches for code tasks. Keep plan and proof
transitions on main. Chat history is not progress state. Resume from the status
ledger, execution log, proof files, and git state. If compaction happens,
continue from those sources rather than restarting. Loop: keep exactly one task
in_progress; capture fail-before evidence; implement at the concept-owned seam;
run focused verification; commit; run Nimbus autoreview for substantive code;
publish and merge the pull request; write exact proof; update this ledger and
Band SA on main; then advance the next task. Decide rather than ask. Mark a
wrong or already-satisfied task no-action with a one-line reason. Record a real
blocker and continue with the next eligible task. Binding constraints: all 19
invariants and every non-goal above; checkpoint before delete; fail closed on
trimmed history; preserve the three client mutation routes; do not edit the
dirty IMV7 worktree; do not expand BLI, NKV, or HS scope. Commit policy: one
reviewable pull request per SMR code task; plan/proof transitions are separate
direct-to-main docs commits; no AI attribution; no published-history rewrite.
Stop only at a valid stop state from the plans skill. Before stopping, update
both ledgers and the next action. The goal is met when SMR0-SMR5 are terminal,
the verifier and required repository gates pass, every supported backend has
explicit evidence, this plan is archived, and Band SA row SA4 is done.
```

## Execution Log

| Date | Item | Action | Evidence |
| --- | --- | --- | --- |
| 2026-08-25 | meta | promoted | User started Band SA execution. SA4 had no implementation-plan owner, so this active plan now owns checkpoint-backed journal retention, production MVCC compaction, bounded PITR, trimmed-history validation, and six-backend qualification. Baseline `cc7ae36a3`; no production behavior changed. |
| 2026-08-25 | SMR0 | completed | PR #313 merged as `0ff18d1a7`. The 18-condition verifier is red at `7 passed, 11 failed`; the indexed 2,049-sequence calibration measured 3.22 MB archive size, 108 ms export, and 15 ms compaction for 1,280 document plus 1,280 index rows. Ratified 100,000-sequence document/index/PITR windows, 50,000 CDC, and a 10,000-sequence maintenance step. No production behavior changed. |
| 2026-08-25 | SMR1 | started | Accepted the provider-neutral checkpoint and embedded atomic-compaction task. SMR1 is the only active task in this plan; SA4 remains the only active Band SA row. |
| 2026-08-26 | SMR1 | completed | PR #314 merged as `0d4b9a112`. The versioned checkpoint, embedded atomic compaction, nonzero-base PITR, sidecar-complete snapshot, generated-history proof, restart/fault/concurrency tests, full storage library lanes, and clean autoreview rerun satisfy SMR1. Verifier advanced from 7/11 to 12/6. |
| 2026-08-26 | SMR2 | started | Accepted provider parity and lease fencing. SMR2 is the only active task. The existing SQL transaction lease validator is the authority seam; an Engine-only precheck is insufficient. |
| 2026-08-26 | SMR2 | completed | PR #317 merged as `f97b2db67`. PostgreSQL, MySQL, and libSQL now publish checkpoints and delete journal/MVCC prefixes in one lease-fenced transaction. Nine focused live tests passed, including native provider-error rollback and libSQL cache rebuild. Verifier advanced from 12/6 to 13/5; Nimbus autoreview was clean. |
| 2026-08-26 | SMR3 | started | Accepted the bounded production lifecycle. SMR3 is the only active task. The Engine must own one single-flight controller per loaded tenant, finalize through the ordered maintenance authority, and cancel and drain on every tenant and Engine shutdown path. |
