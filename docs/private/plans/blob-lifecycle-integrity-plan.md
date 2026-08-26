# Blob Lifecycle Integrity Plan

Status: `proposed` | Owner: this plan | Created: 2026-08-19
Baseline: main @ bdda5da944e244f1643b21c6ac699391c9b40d83
Proof root: `docs/private/plans/proof/blob-lifecycle-integrity/`
Next action: land this plan and its rebased index on main. Keep it `proposed`
while IMV executes. After IMV reaches a valid stop, promote this plan and run
BLI0 to create the proof root and nine-condition red verifier.

## Outcome

> Nimbus reclaims content-addressed object bytes only from a complete live-root
> decision. An S3 request cannot delete a tenant hash that another manifest,
> multipart upload, retained snapshot, or in-flight write still needs. A single
> node reclaims unrooted bytes automatically on a bounded schedule, and an
> operator can run the same complete-root decision on demand. Unused bytes stay
> readable and safe between sweeps.

## Architecture

Before:

```text
[S3 request]
     | put bytes                         | delete, replace, reject, or abort
     v                                   v
[BlobStore: tenant-wide hash] <----- [request-local release(hash)]
     ^                                   |
     |                                   v
[other object with same hash]       [hash becomes unreadable]

[object_gc_roots: manifest + multipart extraction]
     |
     +---- test composition only ----> [BlobGc]
```

After:

```text
[S3 request] ---- put under intent pin ----> [BlobStore: immutable CAS]
     |                                               |
     +---- commit or reject metadata ----------------+
                                                     |
                    [storage-backed live root cut]   |
                    manifests + multipart + snapshots
                                 |                   |
                                 v                   v
                         [CompositeBlobRoots] --> [BlobGc]
                                                   |
                                                   v
                                      [GC-owned reclaim + compaction]

S3 never owns destructive reclamation. A failed request leaves an unrooted
blob for the grace window and the next complete-root sweep.
```

## Scope

- Owns: destructive byte reclamation, object roots, intent pins, and
  object-plane `BlobGc` composition.
- Owns: the automatic single-node schedule and explicit local GC command.
- Owns: an engine-owned opaque multipart expected-state type that validates
  exact-one clause cardinality before the commit authority receives a write.
- Owns: the SIC1 retained-blob erratum and the tracked storage seams
  specification.
- Does not own: `MaterializedPosition`, PITR target validation, canonical value
  encoding, or Merkle verification. The completed
  `archive/incremental-materialized-verification-plan.md` record owns those
  delivered contracts.
- Does not own: cluster-wide root authority. `horizontal-scaling-plan.md` owns
  the Raft-committed root set and distributed automatic sweep policy.
- Non-goals: per-object reference counts and synchronous S3 reclamation. The
  plan also excludes new blob formats, cross-tenant deduplication, and automatic
  cloud object deletion.

## Promotion gate

Promote this plan to `active` only when every item holds:

1. The owner accepts GC as the only object-byte reclamation authority.
2. The dedicated worktree is clean except for this plan and its index edit.
3. Each implementation task has one pull request boundary, except BLI4, which
   documents two ordered pull requests.
4. The manual object GC command reports a dry run by default and requires an
   explicit execute option.
5. The owner approves the five-minute default interval.
6. The owner approves five-second and 256 MiB soft node-wide cycle budgets.

BLI0 runs first after promotion and creates the proof root and the
nine-condition red verifier.

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| BLI0 | Pin the baseline, create the proof root, author the nine-condition verifier red, inventory every production reclamation caller, and capture shared-hash fail-before evidence. No production behavior changes. | `todo` | |
| BLI1 | Move destructive reclaim behind its owning lifecycle seam and remove every request-local S3 release path. | `todo` | |
| BLI2 | Compose complete roots and atomic intent pins into `BlobGc`, recheck each pin before reclaim, and add one explicit local GC command. | `todo` | |
| BLI3 | Replace raw multipart clause collections with an opaque engine-owned exact-one expected-state type; keep condition and successor-revision decisions at the commit authority. | `todo` | |
| BLI4 | Make sweep and compaction resumable and budget-aware, then drive them automatically under one fair node-wide schedule that reports a starved cycle as no-progress. | `todo` | |
| BLI5 | Reconcile governing docs, define the sweep operating policy, add the SIC1 erratum, and publish final proof. | `todo` | |
| BLI9 | After the final pull request merges, archive this plan, retain its proof root, and remove its active index entry. | `todo` | |

## Coordination

- This plan wins for object byte liveness and reclamation.
- `storage-seams-architecture.md` governs Seam A and `BlobGc` behavior. BLI5
  force-tracks that specification and reconciles its release terminology.
- The archived storage integrity plan remains historical evidence. BLI5 adds
  an erratum instead of rewriting its execution log.
- The IMV plan can run independently. If both plans edit the archived SIC
  record, the second plan rebases and preserves both errata.
- Successor row RR30 in `architecture-review-2026-07-plan.md` owns the broader
  `IMVR1` audit of storage durable outcomes and provider-capability types. BLI3
  owns only the multipart exact-one expected-state type. It cannot close RR30.
- Plan and index edits land directly on main. The IMV plan lands first. This
  plan then rebases its index edit. Code tasks keep their worktree and pull
  request boundaries.
- BLI4 owns the automatic single-node sweep and its fixed defaults. BLI5 owns
  its operating guide and capacity warning. The horizontal scaling plan owns
  distributed automatic sweep policy after a committed root authority
  activates.

## Invariants

1. A content hash is tenant-wide. One manifest never owns it exclusively.
2. `BlobStore::put` remains idempotent and stores equal bytes once per tenant.
3. An S3 request cannot invoke destructive reclamation.
4. Only `BlobGc` can reclaim ordinary object bytes.
5. The live set is the union of committed manifests, open multipart uploads,
   retained snapshots, backup holds, and write-intent pins.
6. A sweep uses one storage-consistent root cut and the existing append-position
   boundary. It rechecks the release guard and the hash's live pin immediately
   before each reclaim.
7. A failed or rejected request leaves no metadata effect. Its unrooted bytes
   become eligible only after the grace window.
8. Reference counts can report or accelerate work. They cannot authorize
   reclamation.
9. A multipart put carries an opaque engine-owned expected-state value whose
   constructor accepts exactly one clause. The commit authority decides the
   condition and publishes its successor revision.
10. An unavailable provider or host check is `UNVERIFIED`, never green.
11. A new or deduplicated put holds an intent pin before byte admission
    returns. The pin remains held through the final metadata outcome.
12. A single node sweeps automatically on a bounded schedule. Nimbus retains
    unused bytes and reports capacity growth between sweeps instead of risking
    data loss.
13. The automatic sweep never waits for a request-owned writer lock. It holds
    that lock only for brief atomic commits, not root scans or pack copies.
14. A cycle that exhausts its budget without reclaiming is no-progress, not
    success. A starved sweep and a healthy sweep never report the same
    state.

## Findings ledger

| ID | Classification | Evidence | Owning task |
|---|---|---|---|
| BLIF1 | P1 / confirmed | Equal bytes under keys A and B share one tenant hash. Deleting or replacing A calls `release(hash)` and makes B unreadable. | BLI1 |
| BLIF2 | P1 / confirmed | S3 delete, replacement, rejection, abort, and multipart cleanup decide liveness from one request-local object or upload, on both the S3 wire surface and the Convex file-storage surface (`crates/nimbus-s3/src/convex.rs`), where `ctx.storage.store` deduplicates by construction. | BLI1 |
| BLIF3 | P1 / confirmed | `object_gc_roots` and `BlobGc` exist, but production code never composes the object root provider into a sweep. | BLI2 |
| BLIF4 | P1 / confirmed | S3 puts do not hold the existing intent-pin registry across byte put and metadata commit. A deduplication hit returns the original append position and the original write timestamp with no pin, so it defeats both the position and age grace arms. | BLI2 |
| BLIF5 | P3 / confirmed | Multipart revision validation runs only when the caller supplies exactly one clause. Other cardinalities bypass the guard. | BLI3 |
| BLIF6 | P2 / confirmed | The governing storage seams specification is local-only and predates the landed pin registry, append-position seal, and automatic sweep posture. The SIC invariant set never contained a blob-liveness invariant for delete, replacement, or cleanup, so the shared-hash defect closed out of scope rather than falsely verified. Invariant 6 itself is rejected-condition-scoped and remains satisfied. | BLI5 |
| BLIF7 | P3 / race refuted | Writable server and CLI backup handles take the same exclusive root lock. A second process refuses with `Busy`; no unpinned backup can coexist with the automatic sweep. | BLI4 |

## Decisions

### BLI-D1 Separate byte access from reclamation

- The request-facing `BlobStore` capability owns immutable put and read
  behavior.
- A crate-owned reclamation capability serves `BlobGc`, compaction, recovery,
  and repair code that already proved its own liveness contract.
- Re-open only if a second non-GC consumer proves complete live-root authority.

### BLI-D2 Leave request orphans for GC

- A rejected, failed, replaced, deleted, completed, or aborted S3 request does
  not call the reclamation capability.
- The grace window and intent pin protect pre-commit bytes. A later sweep
  removes bytes that no complete root set names.
- Re-open only if the byte plane stops deduplicating by tenant-wide hash.

### BLI-D3 Use the existing root and pin machinery

- Keep `BlobGc`, `CompositeBlobRoots`, `BlobPinRegistry`, the append-position
  boundary, backup holds, release guards, and compaction.
- Extend the existing `object_gc_roots` extraction into one storage-backed
  object lifecycle composition. Do not add a parallel collector.
- Make byte admission return with a live intent pin on both the new-write and
  deduplication-hit paths. Recheck that hash's pin immediately before reclaim.
- Re-open only if a provider cannot supply a storage-consistent root cut.

### BLI-D4 Sweep automatically and expose a manual command

- Run one node-wide sweep every five minutes after an initial five-minute
  delay. Skip missed ticks and forbid overlapping cycles.
- Limit each cycle to five seconds and 256 MiB of payload read-plus-write I/O.
  Check both soft limits between atomic release or pack-copy operations. Permit
  one atomic operation to overrun so a large blob or pack cannot starve.
- Use a five-minute cadence. This is five times the DynamoDB adapter TTL
  sweeper default. `DEFAULT_TTL_SWEEP_INTERVAL` is 60 seconds in
  `crates/nimbus-dynamodb/src/config.rs`. It is the only periodic sweep today.
  The I/O budget covers one read-plus-rewrite of the 128 MiB default pack.
- Add one operator command that reports a dry run by default. Require explicit
  execution for an out-of-band sweep.
- Both paths use the same complete-root decision, and both refuse a live writer
  lock or an incomplete root source.
- The existing exclusive root lock is the coordination contract with CLI
  backup and restore. A CLI backup refuses with `Busy` while the server owns
  the root. A server owner also refuses while the CLI holds the root. The
  automatic sweep and an offline CLI backup never coexist on one root.
- Re-open the defaults when BLI4 measurements show request interference or GC
  debt growth. `horizontal-scaling-plan.md` owns distributed scheduling.

### BLI-D5 Make multipart expected state exact by construction

- Adapters parse wire clauses and pass them to the engine-owned fallible
  constructor. They cannot populate fields or pass a raw clause collection
  across the engine write seam.
- An engine-owned opaque type validates exactly one
  `ObjectUploadExpectedState`. Its fields stay private, and no unchecked or
  default constructor exists.
- The commit authority evaluates the validated value against its own read and
  decides the outcome before sequence assignment.
- Re-open only if the multipart protocol no longer requires one observed state
  to fence each put.

## Rejected designs

- Check every other manifest before each S3 release. Rejected because it races
  later metadata commits and duplicates `BlobGc` root policy.
- Add a per-object reference count. Rejected because crash or metadata drift
  can make the count authorize data loss.
- Keep `release` public and rely on comments. Rejected because the current bug
  came from treating the method as request cleanup.
- Run GC from each S3 mutation. Rejected because request latency and liveness
  cannot depend on a tenant-wide scan.
- Delete unique-looking failed uploads immediately. Rejected because content
  addressing cannot prove that another key did not publish the same hash.

## Test matrix

| Dimension | Required cases |
|---|---|
| Whole object | same bytes at two keys, deduplication retry, replacement, delete, rejected condition, ambiguous error |
| Multipart | duplicate part bytes, omitted part, completion, abort, concurrent upload, exact clause cardinality |
| Root source | committed manifest, chunked manifest, open upload, retained snapshot, backup hold, intent pin |
| Sweep timing | zero grace, nonzero grace, new put, deduplication hit, deduplication hit past age grace, pin acquired before reclaim, commit during enumeration, process reopen |
| Sweep driver | five-minute interval, initial delay, missed tick, overlap, disable, live writer, incomplete roots, budget, restart, tenant fairness, CLI backup root-lock refusal |
| Placement | local pack, encrypted local pack, placement wrapper, erasure repair ownership |
| Outcome | rooted retained, pinned retained, grace retained, orphan reclaimed, compaction report |

## Verifier contract

BLI0 creates `docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.
It prints one line for each fixed condition and ends with
`Summary: N passed, M failed`.

| Conditions | Contract | Terminal owner |
|---|---|---|
| 1 | Request-facing byte access exposes no destructive object cleanup path | BLI1 |
| 2-3 | Shared whole, chunk, and multipart hashes survive delete, replacement, rejection, completion, and abort | BLI1 |
| 4-5 | One root cut composes every retention source, and new or deduplicated puts remain pinned through metadata outcome and reclaim recheck | BLI2 |
| 6 | The explicit local GC command reports dry-run and execute outcomes | BLI2 |
| 7 | An opaque engine-owned multipart expected-state value rejects zero or multiple clauses; the commit authority enforces the condition and successor revision | BLI3 |
| 8 | The automatic sweep is fair and resumable, compacts outside request locks, refuses incomplete roots, honors its soft cycle budgets, reports a starved cycle as no-progress rather than success, and cannot coexist with an offline CLI backup on one root | BLI4 |
| 9 | The tracked governing specification and SIC1 erratum match shipped behavior | BLI5 |

The completion gate requires `Summary: 9 passed, 0 failed`.

## Tasks

### BLI0 Baseline and red verifier

- Problem: the campaign needs fixed proof of the shared-hash deletion. It also
  needs proof that object GC has no production composition.
- Owning seam and paths: this plan, its proof root,
  `crates/nimbus-blob/src/store.rs`, `crates/nimbus-blob/src/gc.rs`,
  `crates/nimbus-object-storage/src/gc.rs`, `crates/nimbus-s3/src/service.rs`,
  `crates/nimbus-s3/src/object_io.rs`, and `crates/nimbus-s3/src/convex.rs`.
- Steps:
  1. Pin main and record unrelated dirty state.
  2. Inventory every production `BlobStore::release` caller and classify its
     liveness authority.
  3. Author the fixed nine-condition verifier.
  4. Add review-only shared-hash probes for whole, chunked, multipart, and
     Convex file-storage paths.
  5. Add a probe for the original timestamp and position on a deduplication
     hit. This state defeats both grace arms.
  6. Record that `object_gc_roots_provider` has test-only consumers.
  7. Remove every review-only probe after the proof captures its output.
- Acceptance: the verifier reports its exact red baseline. The proof names
  every production reclaim caller. Four shared-hash probes and the
  deduplication-grace probe fail before the fix. `git diff -- crates packages`
  is empty when the task closes.
- Fail-before: `PUT A=X`, `PUT B=X`, and `DELETE A` make `GET B` fail. Equivalent
  replacement and multipart cleanup cases also make another live root fail. A
  Convex `ctx.storage.store` of duplicate bytes followed by one handle's
  delete makes the other handle unreadable. A deduplication hit returns a blob
  that both grace arms already treat as old.
- Verification:
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.
  `rg -n '\.release\(' crates/nimbus-blob crates/nimbus-s3 crates/nimbus-object-storage`.
  `git diff -- crates packages`.

### BLI1 Reclamation ownership and S3 safety

- Problem: the public byte-plane capability lets request code destroy a
  tenant-wide hash without complete root authority.
- Owning seam and paths: `crates/nimbus-blob/src/store.rs`, the local,
  encrypted, placement, erasure, memory, and object-store implementations,
  `crates/nimbus-s3/src/service.rs`, `crates/nimbus-s3/src/object_io.rs`,
  `crates/nimbus-s3/src/convex.rs`, and S3 tests.
- Steps:
  1. Separate immutable byte access from the crate-owned reclamation
     capability.
  2. Route `BlobGc`, compaction, repair, and recovery through the reclamation
     capability.
  3. Remove S3 and Convex file-storage manifest, upload, rejection,
     replacement, and error cleanup calls that reclaim bytes.
  4. Leave failed request bytes unrooted for the lifecycle protocol.
  5. Add whole, chunked, multipart, and Convex file-storage shared-hash
     regressions.
- Acceptance: `shared_blob_survives_other_key_delete`,
  `shared_blob_survives_other_key_replacement`,
  `shared_blob_survives_rejected_condition`,
  `shared_blob_survives_convex_storage_delete`, and
  `shared_part_survives_completion_and_abort_cleanup` pass. Production
  `nimbus-s3` contains no call to the reclamation capability on any surface,
  including the Convex file-storage module. Existing blob repair and GC tests
  stay green.
- Fail-before: use the BLI0 probes.
- Verification:
  `cargo test -p nimbus-s3 shared_blob -- --nocapture`.
  `cargo test -p nimbus-blob gc -- --nocapture`.
  `cargo test -p nimbus-blob erasure -- --nocapture`.
  `cargo test -p nimbus-object-storage object_gc -- --nocapture`.
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.

### BLI2 Object lifecycle composition and operator command

- Problem: the root extractor, pin registries, and collector exist. No
  production composition supplies one complete object root cut to a sweep.
- Owning seam and paths: `crates/nimbus-object-storage/src/gc.rs`, object
  storage resolver composition, `crates/nimbus-blob/src/gc.rs`,
  `crates/nimbus-s3/src/backend.rs`, S3 put and multipart paths, and
  `crates/nimbus-cli/src/object_storage.rs`.
- Steps:
  1. Build one storage-backed root source from a repeatable materialized cut.
  2. Include committed whole and chunked manifests, open multipart uploads,
     retained snapshot roots, backup holds, and write-intent pins.
  3. Make byte admission return with an intent pin on new writes and
     deduplication hits. Hold it through the final metadata outcome.
  4. Recheck the hash's shared pin registry immediately before each reclaim.
  5. Compose the root source and shared registries into the existing `BlobGc`.
  6. Add a dry-run local GC command. Require an explicit execute option for a
     sweep and refuse incomplete roots or a live writer lock.
  7. Report each retention class, swept count, and compaction outcome.
- Acceptance: `object_gc_retains_every_live_root`,
  `object_gc_reclaims_only_past_grace_orphans`,
  `object_put_intent_pin_survives_concurrent_sweep`,
  `deduplicated_put_is_pinned_before_admission_returns`,
  `deduplicated_put_past_age_grace_survives_sweep`,
  `pin_acquired_during_sweep_blocks_reclaim`,
  `object_written_during_root_cut_survives_zero_grace`, and
  `object_storage_gc_command_requires_execute` pass. A command integration test
  shows dry-run state unchanged and execute state reclaimed after the grace
  rule permits it.
- Fail-before: only tests instantiate `object_gc_roots_provider`. S3 puts hold
  no intent pin across metadata commit. Deduplication hits keep the original
  append position and timestamp without a pin. Both grace arms treat the
  in-flight blob as old.
- Verification:
  `cargo test -p nimbus-object-storage object_gc -- --nocapture`.
  `cargo test -p nimbus-s3 intent_pin -- --nocapture`.
  `cargo test -p nimbus-cli object_storage_gc -- --nocapture`.
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.

### BLI3 Multipart clause cardinality

- Problem: multipart revision validation silently skips zero or multiple
  expected-state clauses. A raw collection lets future callers bypass the
  exact-one contract again.
- Owning seam and paths: `crates/nimbus-engine/src/engine/objects.rs`,
  `crates/nimbus-s3/src/backend.rs`, and object metadata tests.
- Steps:
  1. Add an engine-owned opaque multipart expected-state type with private
     fields and a fallible constructor from parsed clauses.
  2. Reject zero or multiple clauses in that constructor. Expose no unchecked
     or default construction path.
  3. Make `TenantObjectMeta::put_multipart_upload_conditional`,
     `ObjectMetaWrite::PutMultipart`, and the committer accept the validated
     type, not a raw vector or slice.
  4. Keep the condition decision at the commit authority against its own read.
  5. Keep the successor revision check for the one accepted clause.
  6. Update adapter callers and API documentation to state the cardinality.
- Acceptance: `multipart_expected_state_has_no_public_fields`,
  `multipart_put_rejects_zero_expected_clauses`,
  `multipart_put_rejects_multiple_expected_clauses`, and
  `multipart_put_requires_successor_revision` pass. `commit_meta_write`
  accepts only the validated exact-one type for multipart puts. The public
  engine write seam also accepts only that type. Every production adapter
  parses clauses but delegates validation and the outcome decision to the
  engine and commit authority.
- Fail-before: zero and multiple clauses reach `commit_meta_write` without the
  successor revision guard.
- Verification:
  `cargo test -p nimbus-engine multipart_upload -- --nocapture`.
  `cargo test -p nimbus-s3 multipart -- --nocapture`.
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.

### BLI4 Bounded automatic single-node sweep

- Problem: current GC scans and compacts the full store under one lock. A timer
  cannot make that work bounded or request-safe.
- Owning seam and paths: `crates/nimbus-blob/src/gc.rs`,
  `crates/nimbus-blob/src/local.rs`,
  `crates/nimbus-object-storage/src/gc.rs`,
  `crates/nimbus-server/src/adapters/s3/listener.rs` resolver lifecycle,
  `crates/nimbus-cli/src/object_storage.rs` configuration surface, and
  `crates/nimbus-cli/src/object_storage/backup_restore.rs` coordination.
- Steps:
  1. Add a resumable `BlobGc` cycle with a cursor and explicit soft budgets.
     Re-enumerate one complete root cut before each resumed destructive pass.
  2. Replace whole-store lock-held compaction with pack-granular copy-on-write.
     Copy outside the state lock and commit only if the source entries match.
  3. Use a try-lock for each destructive commit. Yield the cycle when a request
     owns the writer lock.
  4. Drive all resolved local-pack tenants in round-robin order under one
     node-wide budget. Persist no tenant IDs in metric labels.
  5. Start after five minutes and repeat every five minutes. Skip missed ticks
     and forbid automatic or manual overlap.
  6. Stop after five seconds or 256 MiB of payload I/O. Allow one atomic pack or
     blob operation to overrun, then record the overrun and stop.
  7. Refuse a cycle when any root source is incomplete and report `UNVERIFIED`.
  8. Let an operator disable the schedule and fall back to the BLI2 command.
  9. Emit bounded metrics for cycles, debt, I/O, skips, refusals, and overruns.
  10. Classify a budget-exhausted cycle with no reclaim as no-progress.
  11. Alarm on consecutive no-progress cycles. Record whether root enumeration
      or destructive work consumed the budget.
  12. Document why each resumed pass re-enumerates roots despite the
      append-position boundary.
  13. Preserve the existing exclusive root-lock contract for CLI backup and
      restore.
  14. Prove that the CLI refuses with `Busy` while the server owns the root.
  15. Prove that server startup refuses while the CLI holds the root.
  16. Do not add a second cross-process hold protocol.
- Pull request boundary: this task may use two ordered pull requests. The first
  owns bounded GC and compaction primitives. The second owns the driver.
- Acceptance: `automatic_sweep_reclaims_orphans_without_operator_action`,
  `automatic_sweep_yields_to_live_writer_lock`,
  `automatic_sweep_refuses_incomplete_root_source`,
  `automatic_sweep_honors_cycle_budget`,
  `automatic_compaction_copies_outside_the_store_lock`,
  `automatic_sweep_resumes_after_restart`,
  `automatic_sweep_is_fair_across_tenants`,
  `automatic_sweep_never_reclaims_a_pinned_or_rooted_hash`,
  `automatic_sweep_makes_forward_progress_at_100k_objects`,
  `cli_backup_refuses_while_automatic_sweep_owner_is_live`,
  `automatic_sweep_owner_refuses_while_cli_backup_holds_root`,
  `budget_exhausted_with_zero_reclaim_reports_no_progress`, and
  `disabled_schedule_leaves_manual_command_authoritative` pass. The
  forward-progress rung uses 100,000 unique 4 KiB objects with 10% unrooted
  bytes. This exceeds three default pack targets and proves cursor progress
  under the five-second and 256 MiB soft budgets. Metrics carry no tenant,
  document, table, or key labels.
- Fail-before: reclamation runs only when an operator invokes the BLI2 command,
  so an unattended node grows without bound. A budget-starved cycle would also
  report the same counters as a healthy one.
- Verification:
  `cargo test -p nimbus-object-storage automatic_sweep -- --nocapture`.
  `cargo test -p nimbus-server object_storage_sweep -- --nocapture`.
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.

### BLI5 Closeout, governing docs, and SIC erratum

- Problem: the local-only storage specification omits the corrected reclamation
  contract. The archived SIC claim omits the audit failure mechanism.
- Owning seam and paths:
  `/Users/jack/src/github.com/nimbus/nimbus/docs/private/plans/storage-seams-architecture.md`,
  `docs/private/operating/verification.md`,
  `docs/private/plans/archive/storage-integrity-contracts-plan.md`, this plan,
  and its proof root.
- Steps:
  1. Force-track the governing storage seams specification.
  2. State that request consumers cannot reclaim and that `BlobGc` owns the
     complete-root decision and destructive operation.
  3. Update the specification's `BlobGc` status. Record the implemented pin
     registry and append-position seal. Record BLI4's default local-pack sweep.
  4. Document the local GC command, dry-run behavior, explicit execute option,
     root sources, grace, pins, root-lock refusal, and reports.
  5. Document the interval, time budget, I/O budget, overrun rule, warning, and
     disable option.
  6. Document the CLI backup root-lock refusal contract.
  7. Route distributed automatic sweeps to the horizontal scaling plan after
     it supplies committed cluster root authority.
  8. Add a dated SIC1 erratum that records the coverage gap.
  9. State that rejected-condition-scoped invariant 6 remains satisfied.
  10. State that SIC lacked a blob-liveness invariant for request cleanup.
  11. Name the shared-hash mechanism and the BLI proof root.
  12. Run focused checks, required CI, docs checks, and the pre-PR review gate.
- Acceptance: the verifier reports `Summary: 9 passed, 0 failed`. Git tracks
  `storage-seams-architecture.md`. The SIC archive keeps its original log. The
  dated erratum records a coverage gap, not a false closure.

  The specification records the pin, seal, and automatic sweep status. The
  operating guide gives one exact dry-run command and one exact execute
  command. It also states the interval, cycle budget, capacity warning,
  root-lock refusal, and distributed scheduling owner.
- Fail-before: `.gitignore` excludes the specification, and its `BlobGc` status
  predates the landed pin registry and seal. No SIC invariant covers blob
  liveness on delete or replacement, and no production sweep runs without an
  operator.
- Verification: `git ls-files docs/private/plans/storage-seams-architecture.md`.
  `cargo fmt --all --check`. `make clippy`. `make ci`.
  `bash scripts/check-docs.sh`.
  `bash docs/private/plans/proof/blob-lifecycle-integrity/verify.sh`.
  `nimbus-autoreview --gate pre-pr --mode auto`.

### BLI9 Cleanup

- Problem: a merged campaign must not remain an active control plane.
- Owning seam and paths: this plan, its proof root, and
  `docs/private/plans/README.md`.
- Steps:
  1. Confirm the final pull request merge and every terminal ledger row.
  2. Move this plan to `docs/private/plans/archive/` with the merge date and
     pull request range.
  3. Retain the proof root and remove the active index entry.
  4. Confirm that `horizontal-scaling-plan.md` retains distributed automatic
     sweep ownership. Route any other residual to one named successor plan.
- Acceptance: repository search finds only the archive record, retained proof,
  SIC erratum, and named successor references.
- Fail-before: not applicable because the merge triggers cleanup.
- Verification: `rg -n 'blob-lifecycle-integrity' docs/private/plans`.

## Goal

```text
Execute docs/private/plans/blob-lifecycle-integrity-plan.md to
completion. This is a whole-plan goal, not a single-task goal. Read the
plan fully, then read README.md, ARCHITECTURE.md,
docs/private/plans/README.md,
docs/private/plans/archive/storage-integrity-contracts-plan.md,
/Users/jack/src/github.com/nimbus/nimbus/docs/private/plans/storage-seams-architecture.md,
docs/private/operating/verification.md,
docs/private/adapters/convex/ai-guidelines.md,
crates/nimbus-blob/src/store.rs,
crates/nimbus-blob/src/gc.rs, crates/nimbus-blob/src/pins.rs,
crates/nimbus-blob/src/local.rs,
crates/nimbus-object-storage/src/gc.rs,
crates/nimbus-object-storage/src/backup.rs,
crates/nimbus-s3/src/backend.rs, crates/nimbus-s3/src/service.rs,
crates/nimbus-s3/src/object_io.rs, crates/nimbus-s3/src/convex.rs,
crates/nimbus-engine/src/engine/objects.rs,
crates/nimbus-cli/src/object_storage.rs, and
crates/nimbus-cli/src/object_storage/backup_restore.rs. Work in
/Users/jack/src/github.com/nimbus/nimbus-worktrees/blob-lifecycle-integrity
on branch codex/blob-lifecycle-integrity. Chat history is not progress
state. Resume from the status ledger, the execution log, and git state.
If compaction happens, continue from the plan and git state rather than
restarting. Loop: keep one task in_progress, implement at the owning
seam, capture fail-before evidence, run the verification commands,
commit the work per the commit policy, write the proof file, append the
execution log with the work commit, mark the task terminal with
evidence, commit the plan update the same way, then advance to the next
task. Decide rather than ask. Mark a wrong or already-satisfied task
no-action with a one-line reason. Record a blocker and continue with the
next eligible task. Binding constraints: S3 never owns destructive
reclamation, BlobGc uses a complete root cut, failed writes become
grace-protected orphans, new and deduplicated byte admissions return with
a live pin, intent pins span byte admission through metadata outcome,
BlobGc rechecks each hash pin before reclaim, no refcount authorizes deletion,
automatic sweep and compaction are resumable and copy outside request locks,
the automatic sweep and an offline CLI backup cannot coexist on one root,
the node-wide cycle starts after five minutes and uses five-second and 256 MiB
soft budgets with one atomic-operation overrun, multipart puts carry exactly
one opaque engine-owned expected-state value validated from exactly one clause,
adapters cannot pass raw clauses across the engine write seam, the commit
authority decides the condition, and unavailable checks are UNVERIFIED.
Commit policy: one reviewed pull request per task, with separate work and
proof commits when practical. Run the Nimbus pre-PR autoreview gate after
final checks for each substantive code pull request. Stop only at a valid
stop state from the plans skill. Before you stop, update the ledger and
the log, and record the next action in the status line. The goal is met
when BLI0 through BLI5 are terminal, the verifier reports Summary: 9
passed, 0 failed, required checks are recorded, every retained object
survives the shared-hash corpus, and the final pull request is ready to
merge.
```

## Execution log

Append rows at the end. This section stays last.

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-19 | meta | authored | Proposed plan created from the PR #287 and SIC campaign review. Baseline `8348556754446f5cd0f35a10619fa9169e45e2f2`. Verified that `object_gc_roots_provider` exists from RFS6 but has test-only consumers. No implementation started. |
| 2026-08-19 | meta | refined | Added atomic pins for new and deduplicated byte admissions, a per-hash pre-reclaim pin check, the accepted capacity-growth consequence, the single-node manual policy, the horizontal-scaling automation owner, and the direct-main plan merge order. No implementation started. |
| 2026-08-19 | meta | refined | Owner required automatic single-node reclamation inside this plan rather than deferring scheduling. Added BLI4 automatic sweep, renumbered closeout to BLI5, bounded invariant 12, added invariant 13, and raised the verifier to nine conditions. No implementation started. |
| 2026-08-19 | meta | refined | Found that current `BlobGc::sweep` scans all entries and compacts the full store under one lock. Expanded BLI4 to own resumable GC, pack-granular copy-on-write compaction, node-wide tenant fairness, and proposed five-minute, five-second, and 256 MiB soft defaults. Awaiting owner approval. No implementation started. |
| 2026-08-19 | meta | refined | Rebased the baseline onto `5fb9284cf7e313cfc0a4901835d7bd6144e297c8`. No implementation started. |
| 2026-08-19 | meta | refined | Owner approved the five-minute interval, five-second time budget, 256 MiB I/O budget, and single atomic-operation overrun. Added invariant 14 and a BLI4 no-progress guard so a budget-starved cycle cannot report the same state as a healthy one. No implementation started. |
| 2026-08-19 | meta | refined | Strengthened BLI3 with an opaque engine-owned exact-one multipart expected-state type. Adapters parse clauses, the validated type reaches the committer, and the commit authority decides the condition and successor revision. Confirmed that repository-wide opaque outcomes and provider-capability sets remain routed as IMVR1 rather than claimed by BLI. No implementation started. |
| 2026-08-19 | meta | refined | Bound IMVR1 to actual successor ledger row RR30 in the active architecture-review plan. RR30 activates after IMV1 and BLI3 merge. BLI cannot close it. No implementation started. |
| 2026-08-19 | meta | refined | Applied the required Fable corrections. Named the Convex file-storage surface in BLIF2, BLI0, and BLI1. Recorded that a deduplication hit defeats both grace arms. Anchored BLI-D4 to the DynamoDB adapter TTL sweeper default. Reframed BLIF6 and the SIC1 erratum as a coverage gap. Reconciled the specification's pin, seal, and automatic-sweep status. No implementation started. |
| 2026-08-19 | BLI4 | refined | Replaced the proposed backup hold choice with the existing exclusive root-lock refusal contract. Bound forward progress to 100,000 unique 4 KiB objects with 10% unrooted bytes, which spans more than three default pack targets. No implementation started. |
| 2026-08-19 | meta | corrected | Applied the second contract audit. Removed BLI0 from the promotion gate and sequenced it first after promotion. Made BLI4's two ordered pull requests the explicit gate exception. Moved the status ledger before Coordination for cold resume. No implementation started. |
| 2026-08-20 | meta | rebased | Rebased onto `bdda5da94` after IMV landed and became active. Preserved both adjacent index entries. Recent main merges changed no BLI-owned blob path. No implementation started. |
