# Storage Integrity Contracts Plan

Status: `active` | Owner: this plan | Created: 2026-08-18
Baseline: main @ `8877eaff43a36d9606a1feaa0ab31d0377539d9d`
Proof root: `proof/storage-integrity-contracts/`
Next action: SIC1 carries typed object expected-state conditions into the tenant commit authority.

## Outcome

> Nimbus evaluates every conditional metadata mutation at its authoritative
> commit boundary, makes every cross-cutting effect explicit on every writer,
> and binds exported state to one canonical sequence-and-digest position.

## Architecture

Before:
```text
[protocol consumer: may decide a condition from a pre-read]
    -> [engine commit authority: serializes writes but receives no condition]
    -> [storage commit plans: composite writers are witnessed, but direct/internal writers vary]
    -> [provider transaction: fences owner, epoch, and expected durable head]
    -> [materialized artifacts: sequences plus several fingerprint representations]
    -> [conformance/fault gate: strong logical coverage with physical failure gaps]
```

After:
```text
[protocol consumer: translates wire policy into a typed expected-state condition]
    -> [engine commit authority: reads, decides, and sequences under one authority]
    -> [storage commit plans: every writer declares every cross-cutting effect]
    -> [provider transaction: atomically fences owner, epoch, and expected durable head]
    -> [materialized artifacts: one canonical {applied sequence, logical digest} position]
    -> [conformance/fault gate: provider parity plus physical SQLite failure evidence]
```

## Scope

- Owns: typed object conditions at the tenant committer. It also owns explicit
  writer effects, one materialized position, provider qualification, and SQLite faults.
- Consumes: the U5/U8 shared SQL core, deterministic simulation, provider
  scenarios, and the engine durable-outcome classifier.
- Does not own: cluster placement or object-store lease authority.
  `horizontal-scaling-plan.md` owns lineages, seals, and reader negotiation.
- Does not own: node-level restore/compaction budgets, attribution changes,
  adapter differential tests, or LTX/Litestream implementation.
- Non-goals: The plan does not replace the logical journal with WAL/LTX.
  It does not require object storage or add a fourth client mutation route.
  It does not generalize ETags, hash full state per write, or add format shims.

## Promotion Gate

Promote only after the owner approves the plan. Confirm that no active plan
edits the same seams. Protect unrelated dirty files. Start SIC0 on a fresh
`codex/sic-sic0` branch. The proof root and fixed verifier contract exist.

## Invariants

1. The logical tenant journal remains authoritative for every provider.
2. Queued, direct, and execution-unit remain the only client mutation routes.
   object metadata remains an internal route.
3. Document, index, version, and journal effects commit in one transaction.
4. The authoritative committer decides a condition before sequence assignment.
5. Provider writes atomically fence owner, epoch, and expected durable head.
6. A rejected condition has no sequence, journal, fan-out, or retained-blob effect.
7. Ambiguous outcomes use durable-head recovery, never conflict translation.
8. Every writer declares every effect without defaults, optional silence, or opaque callbacks.
9. A position binds applied sequence to a versioned digest. Durable head stays separate.
10. The digest is provider-independent, ordering-stable, and never page-based.
11. `nimbus-core` stays zero-I/O. `nimbus-runtime` keeps zero workspace deps.
12. Unavailable provider or host lanes are `UNVERIFIED`, never green.

## Findings Ledger

| ID | Classification | Evidence | Owning task |
|---|---|---|---|
| F1 | critical / confirmed | S3 preconditions are decided before unconditional metadata put. | SIC1 |
| F2 | high / confirmed | Multipart read-modify-write can lose concurrent parts. | SIC2 |
| F3 | medium / confirmed | U5 covers three composite paths, not direct/internal writers. | SIC3 |
| F4 | medium / confirmed | Storage and engine duplicate snapshot hashing. Shadow binds only sequence. | SIC4 |
| F5 | medium / confirmed | Provider guarantees have no complete qualification matrix. | SIC5 |
| F6 | medium / high confidence | Physical SQLite durability cases are not a named gate. | SIC6 |
| F7 | deferred / confirmed | Seals and reader floors have no active consumer. | `horizontal-scaling-plan.md` |

## Decisions

| ID | Decision and evidence | Re-open condition |
|---|---|---|
| SIC-D1 | Carry conditions into `TenantObjectMeta`'s existing committer actor. Keep ETag policy in S3 and add no raw provider CAS. | Object metadata can bypass the actor or expected-head fence. |
| SIC-D2 | Preserve U8's ban on defaults and opaque validators while adding a compile or non-vacuous structural all-writer gate. | A clearer typed direct-write encoding is proved. |
| SIC-D3 | Consolidate existing fingerprints into one storage-owned position computed at artifact boundaries, not per write. | Boundary hashing misses its measured budget. |
| SIC-D4 | Defer external lineages, seals, and reader-fleet protocols to HS. Current PITR already fails closed on exact versions. | Another external artifact consumer activates first. |

## Coordination

- This plan wins for conditional metadata, commit-effect declarations,
  materialized positions, provider qualification, and SQLite fault evidence.
- `storage-seams-architecture.md` remains governing. SIC7 reconciles it.
- `horizontal-scaling-plan.md` wins for cluster authority, lineages, seals, and
  reader-fleet rollout. Archived storage plans remain historical evidence.

## Verifier Contract

SIC0 creates `proof/storage-integrity-contracts/verify.sh`. It reports one line
per fixed condition and ends `Summary: N passed, M failed`.

| Conditions | Contract | Terminal owner |
|---|---|---|
| 1–5 | condition crosses `S3ObjectMeta`. Actor decides. Sequential and concurrent probes pass. Rejection has no effects | SIC1 |
| 6 | concurrent multipart writes preserve every accepted part | SIC2 |
| 7–8 | all writers are inventoried. Effects cannot be omitted through defaults, options, or opaque callbacks | SIC3 |
| 9–11 | storage owns one digest. Divergence/order tests pass. All materialized consumers use the position | SIC4 |
| 12 | every provider has a complete, non-skipping semantic qualification row | SIC5 |
| 13 | test-only disk-full, sync/WAL, and process-loss cases preserve the last acknowledged position | SIC6 |

SIC0 records a red baseline. SIC1 through SIC5 make conditions 1–12 green in
order. SIC6 and SIC7 require `Summary: 13 passed, 0 failed`.

## Status Ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| SIC0 | Baseline: pin execution HEAD, author the 13-condition verifier red, inventory every writer and current fingerprint consumer, and capture fail-before evidence. No production behavior changes. | `done` | `proof/storage-integrity-contracts/sic0.md`. Verifier `Summary: 0 passed, 13 failed`, exit 1. Both fail-before probes reproduced. `git diff -- crates packages` empty. Work commit `a2f34aec6`. |
| SIC1 | Carry object expected-state conditions into the tenant commit authority and close concurrent `PutObject` precondition races. | `in_progress` | |
| SIC2 | Apply atomic expected-state updates to multipart metadata and complete condition-failure cleanup and provider parity. | `todo` | |
| SIC3 | Make cross-cutting commit effects explicit on every client and internal writer without defaults or opaque callbacks. | `todo` | |
| SIC4 | Consolidate canonical fingerprints into one storage-owned materialized position and propagate it through every materialized artifact consumer. | `todo` | |
| SIC5 | Establish the complete provider semantic qualification matrix and shared conformance scenarios. | `todo` | |
| SIC6 | Add test-only physical SQLite durability fault evidence with no production fault seam. | `todo` | |
| SIC7 | Reconcile governing architecture, run closeout gates, and publish final proof. | `todo` | |
| SIC9 | Cleanup after the final pull request merges: archive this plan and keep its proof root because Nimbus retains completed campaign evidence. | `todo` | Trigger: merge of the final SIC pull request. |

## Tasks

### SIC0 Baseline and red verifier

- Problem: the campaign needs a pinned execution baseline, a complete writer
  census, and non-vacuous fail-before evidence.
- Owning seam and paths: this plan and the proof root. The inventory covers
  `sql/commit_effects.rs`, `tests/commit_path_ownership.rs`, and engine objects.
  It also covers callers for writes, durable batches, scheduler, triggers,
  restore, object metadata, and replica-cache reconciliation.
- Steps:
  1. Record current `main` HEAD and protect unrelated worktree changes.
  2. Create the 13-condition verifier with fixed, non-vacuous source and test
     checks.
  3. Inventory the three client mutation paths and every non-client storage
     writer named by repository instructions.
  4. Capture the concurrent conditional-put and multipart lost-update
     fail-before results without changing production behavior.
  5. Record the current fingerprint producers and consumers.
- Acceptance: `verify.sh` evaluates exactly 13 conditions, reports at least one
  failure, and `sic0.md` names every writer and fingerprint consumer with file
  paths. No production source changes appear in the task diff.
- Fail-before: `conditional_put_if_none_match_is_linearizable` and
  `concurrent_upload_parts_preserve_all_accepted_parts` fail against the
  baseline, or the proof records `UNVERIFIED` with the exact harness blocker.
- Verification: `bash docs/private/plans/proof/storage-integrity-contracts/verify.sh`.
  `git diff -- crates packages` must be empty for this task.

### SIC1 Atomic object conditions

- Problem: `PutObject` evaluates ETag preconditions before it enters the one
  serialized commit authority, so two concurrent requests can both succeed.
- Owning seam and paths: `crates/nimbus-storage/src/traits/object_metadata.rs`,
  `crates/nimbus-engine/src/engine/objects.rs`, `crates/nimbus-s3/src/backend.rs`,
  `crates/nimbus-s3/src/service.rs`, and `crates/nimbus-s3/src/tests.rs`.
- Steps:
  1. Define a typed object expected-state condition and a typed committed or
     rejected outcome. Keep S3 ETag parsing and response mapping in `nimbus-s3`.
  2. Carry the condition through `S3ObjectMeta` and `TenantObjectMeta` into the
     committer actor.
  3. Evaluate it against the actor's current document before sequence
     assignment. Preserve provider owner, epoch, and expected-head fencing.
  4. Map condition rejection to the correct S3 response and release the new
     blob only when no committed manifest retains it.
  5. Add sequential and concurrent conformance probes.
- Acceptance: `conditional_put_probe_create_reject_update_reject_stale` passes.
  `conditional_put_if_none_match_is_linearizable` admits one of 100 claimants.
  Rejection leaves both heads unchanged and publishes no observer event.
  Cleanup preserves bytes that the winning manifest retains.
- Fail-before: use the SIC0 concurrent probe and record both successful writes
  at one logical key.
- Verification: `cargo test -p nimbus-s3 conditional_ -- --nocapture`.
  `cargo test -p nimbus-engine object_meta -- --nocapture`.
  `cargo test -p nimbus-storage object_meta -- --nocapture`.
  the plan verifier must report conditions 1–5 green.

### SIC2 Multipart conditional state

- Problem: multipart part writes use read-modify-write outside the committer,
  so concurrent accepted parts can overwrite each other.
- Owning seam and paths: object multipart DTOs and conditions in
  `crates/nimbus-storage/src/traits/object_metadata.rs`, the object committer in
  `crates/nimbus-engine/src/engine/objects.rs`, and multipart operations in
  `crates/nimbus-s3/src/service.rs` and `tests.rs`.
- Steps:
  1. Carry the observed multipart state or revision as an expected-state
     condition into the committer.
  2. On a conflict, reload and retry only the pure merge operation within a
     bounded policy. Never retry an ambiguous durable outcome as a conflict.
  3. Make abort and completion reject stale upload state without losing
     accepted parts or retaining superseded blobs.
  4. Run the same object metadata scenarios over embedded and available remote
     provider fixtures.
- Acceptance: `concurrent_upload_parts_preserve_all_accepted_parts` retains
  every distinct accepted part. Same-part races have one documented winner.
  Stale completion and abort return conflicts with no sequence or leak.
  Provider-backed runs preserve lease and head fencing.
- Fail-before: use SIC0's synchronized two-part overwrite reproduction.
- Verification: `cargo test -p nimbus-s3 multipart -- --nocapture`.
  `cargo test -p nimbus-engine objects -- --nocapture`.
  `cargo test -p nimbus-storage object_meta -- --nocapture`.
  the plan verifier must report conditions 1–6 green.

### SIC3 Exhaustive commit-effect ownership

- Problem: U5 covers three composite SQL paths. Direct and internal writers can
  omit a new effect without a compile or structural failure.
- Owning seam and paths: storage commit effects, direct write modules, and the
  commit-path ownership tests. Engine-owned internal writers are also in scope.
- Steps:
  1. Turn SIC0's writer census into one checked ownership matrix.
  2. Name the stable commit concepts.
  3. Require each writer to declare admission, lease, condition, document,
     index, version, scheduler, trigger, journal, watermark, and outcome effects.
  4. Preserve concept-specific plans where one universal type would require a
     default, optional silence, or opaque validator.
  5. Add a verifier mutation test that omits one fixture effect.
- Acceptance: a new effect or writer fails compilation or the ownership test.
  Every matrix row must declare a decision. The matrix names all client and
  internal writers. U5 coherence tests remain green. Witnesses use no silent default.
- Fail-before: add a fixture-only effect that the current direct path omits and
  record that the baseline U5 checks remain green.
- Verification: `cargo test -p nimbus-storage commit_path_ownership -- --nocapture`.
  `cargo test -p nimbus-storage commit_effects -- --nocapture`.
  `cargo test -p nimbus-engine mutation -- --nocapture`.
  the plan verifier must report conditions 1–8 green.

### SIC4 Canonical materialized position

- Problem: Nimbus has strong fingerprints, but storage and engine own parallel
  canonicalizers, and some persisted checkpoints bind only a sequence.
- Owning seam and paths: storage journal snapshots, journal streams,
  materializers, and provider snapshot exports. Engine verification, replicas,
  and diagnostics are consumers.
- Steps:
  1. Define one storage-owned materialized position with applied sequence,
     digest format, and logical state digest. Keep durable head separate.
  2. Define canonical ordering and digest inputs for table identities, schema,
     documents, and scheduled execution IDs.
  3. Replace the duplicate engine canonicalizer with the storage contract.
  4. Bind PITR targets, journal bootstraps, shadow checkpoints and recovery,
     replica comparison, and diagnostic reports to the position.
  5. Use a clean pre-launch format change. Do not add legacy decoding shims.
- Acceptance: `same_sequence_different_state_has_different_materialized_position`,
  `logical_order_does_not_change_materialized_position`,
  `shadow_recovery_rejects_wrong_checkpoint_digest`, and
  `pitr_import_rejects_wrong_target_digest` pass. Only one canonical digest
  implementation remains. Provider exports of equal logical state produce the
  same position.
- Fail-before: alter snapshot content without changing its sequence and show
  that the current shadow manifest accepts the sequence-only checkpoint.
- Verification: `cargo test -p nimbus-storage materialized_position -- --nocapture`.
  `cargo test -p nimbus-storage journal_snapshot -- --nocapture`.
  `cargo test -p nimbus-engine verification -- --nocapture`.
  the plan verifier must report conditions 1–11 green.

### SIC5 Provider semantic qualification

- Problem: fixtures and comments distribute the provider guarantees. No one
  artifact states the complete semantic contract.
- Owning seam and paths: storage provider scenarios, SQL pair scenarios,
  provider fixture modules, diagnostics, and the
  provider-selection runbook.
- Steps:
  1. Define a fixed qualification matrix.
  2. Include atomic effects, fences, conditions, progress, recovery, isolation,
     and position parity in the matrix.
  3. Route each provider through shared scenarios instead of copying expected
     behavior into provider tests.
  4. Make the matrix non-vacuous with an explicit provider roster and feature
     state. Mark unavailable external dependencies `UNVERIFIED`.
  5. Expose the qualified semantic profile in diagnostics only where an
     operator needs it. Do not add per-request probing.
- Acceptance: memory, redb, and SQLite report all required rows green. Available
  libSQL, PostgreSQL, and MySQL fixtures run the same rows. One missing scenario
  fails `provider_contract_matrix_is_complete`. Skipped tests prove no guarantee.
- Fail-before: remove one current provider scenario registration in a fixture
  and prove the baseline has no complete-matrix failure.
- Verification: `cargo test -p nimbus-storage provider_contract_matrix -- --nocapture`.
  `cargo test -p nimbus-storage provider_scenarios -- --nocapture`.
  available external-provider commands from
  `docs/private/operating/verification.md`. The plan verifier must report
  conditions 1–12 green.

### SIC6 Physical SQLite durability faults

- Problem: logical fault points do not prove behavior for disk-full, failed
  sync/WAL operations, or process loss at the SQLite boundary.
- Owning seam and paths: SQLite test support, storage fault tests, and the verification
  harness. Production modules must contain no new injectable fault control.
- Steps:
  1. Add a test-only VFS or subprocess harness that can fail bounded physical
     durability operations deterministically.
  2. Cover failure before durable visibility, failure after durability before
     acknowledgement, disk-full, WAL/checkpoint failure, and process loss.
  3. Reopen the database and compare its durable head, applied head, and
     materialized position with the last acknowledged result.
  4. Add a mutation test that proves the physical checker detects a deliberately
     broken acknowledgement rule.
- Acceptance: `sqlite_disk_full_preserves_last_acknowledged_position`,
  `sqlite_sync_failure_is_not_acknowledged`,
  `sqlite_crash_after_durable_commit_recovers_matching_position`, and
  `sqlite_wal_failure_never_exposes_partial_effects` pass. The production
  binary has no new fault configuration or VFS selection surface.
- Fail-before: SIC0 records these cases as absent or `UNVERIFIED`. The current
  logical tests miss the deliberately broken acknowledgement fixture.
- Verification: focused physical-fault test command recorded by SIC0 with a
  bounded timeout. `cargo test -p nimbus-storage sqlite -- --nocapture`.
  `make verify-harness SURFACE=storage`. The plan verifier must report
  `Summary: 13 passed, 0 failed`.

### SIC7 Architecture and closeout

- Problem: the governing storage documents and release gates must state the
  landed contracts and preserve the deferred HS handoffs.
- Owning seam and paths: `docs/private/plans/storage-seams-architecture.md`.
  `docs/private/architecture/storage/persistence-engine-baseline.md`.
  `docs/private/architecture/time-and-ordering.md`.
  `docs/private/operating/verification.md`, this plan, and its proof root.
- Steps:
  1. Reconcile the governing specs with the condition, effect, position, and
     provider-qualification contracts.
  2. Record epoch lineages, seals, and reader-first format rollout as binding HS
     prerequisites, not implemented SIC work.
  3. Run the complete verifier, focused suites, workspace gates, and the Nimbus
     pre-PR autoreview gate.
  4. Write `sic7.md` with the branch, commits, pull requests, exact test counts,
     skipped dependencies, and remaining uncertainty.
- Acceptance: every non-cleanup row is terminal. The verifier reports
  `Summary: 13 passed, 0 failed`. `make ci` and autoreview are green.
  Governing docs name one owner for each deferred item.
- Fail-before: search finds the adapter pre-read model or sequence-only recovery.
  It also finds the U8 gap without a successor gate.
- Verification: `bash docs/private/plans/proof/storage-integrity-contracts/verify.sh`.
  `cargo fmt --all --check`. `make clippy`. `make ci`.
  `autoreview --gate pre-pr --mode auto` after final checks and commit.

### SIC9 Cleanup after merge

- Problem: a merged plan must not remain an active control plane.
- Owning seam and paths: this plan, `proof/storage-integrity-contracts/`, and
  `docs/private/plans/README.md`.
- Steps:
  1. Confirm that the final SIC pull request merged.
  2. Set every row terminal.
  3. Move this plan to `docs/private/plans/archive/`.
  4. Set status `complete` with the pull request range and date.
  5. Keep the proof root and replace the active index entry.
  6. Route each residual item to its owner.
- Acceptance: the active index has no entry for this file. The archive is
  complete, and the proof root remains. Each residual item has one owner.
- Fail-before: not applicable because the final merge is the trigger.
- Verification: search the plans index and repository for
  `storage-integrity-contracts-plan`. Confirm only the archive, proof, and
  retrospective references remain.

## Goal

```text
Execute docs/private/plans/storage-integrity-contracts-plan.md to
completion. This is a whole-plan goal, not a single-task goal. Read the
plan fully, then read: AGENTS.md, ARCHITECTURE.md,
docs/private/plans/README.md,
docs/private/plans/storage-seams-architecture.md,
docs/private/architecture/storage/persistence-engine-baseline.md,
docs/private/architecture/time-and-ordering.md,
docs/private/operating/verification.md,
crates/nimbus-storage/src/sql/commit_effects.rs, and the current task's
owning files. Work in /Users/jack/src/github.com/nimbus/nimbus. Use one
branch per substantive task named codex/sic-<lowercase-task-id>, cut from
updated main after the preceding task merges. Chat history is not progress
state. Resume from the status ledger, the execution log, and git state. If
compaction happens, continue from the plan and git state rather than
restarting. Loop: keep one task in_progress, implement at the owning seam,
capture fail-before evidence, run the verification commands, commit the
work per the commit policy, write the proof file, append the execution log
with the work commit, mark the task terminal with evidence, commit the plan
update the same way, then advance to the next task. Decide rather than ask.
Mark a wrong or already-satisfied task no-action with a one-line reason.
Record a blocker and continue with the next eligible task. Binding
constraints: preserve all twelve invariants and every non-goal. Do not add
a fourth client mutation route, raw provider CAS escape hatch, physical
journal authority, or compatibility shim. Commit policy: preserve unrelated
work. One coherent work commit per task unless the row records an approved
mechanical split. Record each ledger transition in a separate plan commit.
Open one pull request per substantive task. Run the Nimbus pre-PR autoreview
gate after final checks and the work commit. Stop only at a valid stop state
from the plans skill. Before you stop, update the ledger and the log, and
record the next action in the status line. The goal is met when SIC0 through
SIC7 are terminal, the verifier reports Summary: 13 passed, 0 failed, make
ci and the pre-PR review gate are green, every required provider lane is
green or explicitly UNVERIFIED, the final pull request merges, and SIC9
archives the plan with no unowned residual work.
```

## Execution Log

Append rows at the end. This section stays last.

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-18 | meta | authored | Proposed plan created from the celld storage review and current Nimbus storage inspection. Baseline `8877eaff43a36d9606a1feaa0ab31d0377539d9d`. No implementation started. |
| 2026-08-18 | SIC0 | done | Verifier `verify.sh` authored red: `Summary: 0 passed, 13 failed`, exit 1 at `49884476d`. Census in `proof/storage-integrity-contracts/sic0.md` names three client routes, three composite `SqlCommitEffects` sites, eleven non-client writer families, five fingerprint producers, and seven consumer sites. Fail-before: both concurrent `If-None-Match: *` creates accepted; both concurrent `UploadPart` calls accepted with one part lost. Probes removed by saved-copy restore; `git diff -- crates packages` empty. Docs gate PASS. Commit `a2f34aec6`. Docs-only task, so no pull request per repository convention. |
