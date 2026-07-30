# Storage Unification And Carry-Over Closeout Control Plane

Status: `active — SUC0–SUC2 + SUC4 complete (PRs #248–#253); SUC3.1 steps 1-5 merged (PRs #254/#257-#260); step 6 (test dedupe) in_progress; SUC5.1 complete (PR #255); SUC6.1 complete (PR #256); SUC6.2 closed by U7; SUC6.3 closed`

Owner: this plan, and no other plan

Provenance: the 2026-07-29 storage architecture review; the PR #241 audit's
deferred F3 finding (D10 in the archived SQLite campaign); the archived
`archive/sqlite-write-throughput-optimization-plan.md` standing limitations
(open-loop companion, resource-binding candidate, D15 follow-up); the July 21
full review's three confirmed HIGH bugs.

Proof root: `proof/storage-unification/`

## Mission

1. Make storage atomicity structurally enforceable and put every
   journal-sequence-consuming write on a real commit path (the objects-on-
   read-executor bypass is the sharpest known defect).
2. Collapse the triplicated provider layer onto one provider-agnostic facade
   with sqlite as the proven reference implementation, fixing the three live
   divergences immediately as spot-fixes.
3. Close every carry-over item from the SQLite campaign and the July 21
   review: the full-CI retrospective (main is RED at plan-creation time),
   the three HIGH bugs, the DynamoDB principal, the open-loop latency
   companion, the resource-binding candidate, and the formal hot-key/OCC
   closure.

## Non-Negotiable Invariants

- Storage atomicity: document write, index effects, durable record, and
  applicable watermarks stay in one storage transaction per route.
- Client document mutations keep exactly three Engine-owned routes; this
  campaign REDUCES bypasses, never adds one.
- WAL + `synchronous=FULL` stays; `synchronous=NORMAL` remains a rejected
  durability shortcut (not an optimization lever) unless the owner opens an
  explicit product-durability decision outside this plan.
- No storage-format change.
- Measurement rules inherit the archived campaign's contract: same-session
  paired A/B, whole-run retention of rejected runs, D18 lane-scoped
  admissibility where a lane's CV is intrinsically dispersed, D19 OPS=700
  N=1-rung stabilizer, no cross-session pooling.
- Merge policy: owner-approved fast loop (verify locally, merge on
  surface-matched key lanes) WITH the SUC0 correction: the full-CI bill is
  triaged at phase boundaries, not deferred to campaign end.

## Durable Status Ledger

| Phase/task | Status | Scope | Gate | Proof | Next action |
| --- | --- | --- | --- | --- | --- |
| SUC0.1 Main full-CI triage | `complete` (PR #248 / `97f6d134b`) | attribute every red lane on `main` (CI, Coverage, Node Compatibility failing at plan creation) to campaign merges vs pre-existing; fix-forward campaign breakage immediately | main required lanes green or every red attributed non-campaign with owner sign-off | `proof/storage-unification/suc0/ci-triage.md` | complete: flush test fixed; Coverage downstream; Node Compat pre-campaign (open, non-campaign) |
| SUC0.2 Fail-before inventory | `complete` (PR #249 / `02aa024e4`) | reproduce the objects race shape, the two engine commit-sequence transcriptions, and the three provider divergences as failing/characterizing tests | each defect has a committed fail-before artifact | `suc0/fail-before.md` | after SUC0.1 |
| SUC1.1 Provider divergence spot-fixes | `complete` (PR #249) | libsql missing fault-injection point; lease validation unified to milliseconds (postgres+libsql convention; mysql converts at the edge); mysql length guard propagated to postgres | conformance harness asserts parity across all providers | `suc1/` | small PRs; may land during SUC0.2 |
| SUC2.1 CommitTransaction witness | `complete` (PR #250) | witness type threading document/version/index/journal/watermark effects so a provider omitting one cannot compile; unify the two engine transcriptions of the queued commit sequence under one compiler-linked definition | engine transcription count = 1 (durable_batch core; two serial-arm drifts fixed); provider-side witness rides with SUC3 per U5 | `suc2/witness.md` | after SUC0 |
| SUC2.2 Objects/KV/scheduler/trigger onto a real commit path | `complete` (PR #250) | object manifests and multipart, KV, scheduler-state, and trigger-cursor writes leave `TenantPointWrite`-on-read-executor; sequence assignment fenced through the committer or an explicitly serialized internal path; publication/subscription classification made explicit per event kind | audit F3 closed (fail-before 3/4 RED, all GREEN; A/B N=256 ratio 0.9982 PASS; per-kind classification recorded; scheduler/trigger already fenced, KV consumes no sequences) | `suc2/commit-path.md` | after SUC2.1 |
| SUC3.1 Provider facade extraction | `in_progress` (steps 1-2 merged, PRs #254/#257: SqlStoreCore + shared executor + libsql on the seam incl. shared sql_commit and its new StorageCommitBeforeVisibility; steps 1-5 all merged: shared store core, libsql on the seam, U5 witness + U4 gates, transaction-half twins, per-provider feature gates (default=[] embedded; loud uncompiled-provider failures; hakari excludes storage+crypto). Step 6 (conformance test dedupe, waves 1-3 + conditional 4, committer_lease wrapper model, filter-count baseline 77/47/52) in_progress toward the LoC acceptance | delete the ~855-line triplicated blocks into one facade; sqlite (with #244/#245 optimizations) is the reference; provider feature gates so embedded builds/measures in isolation | ~−1,700 LoC net; conformance harness green for all providers; sqlite paired A/B unchanged | `suc3/facade.md` | after SUC2; sqlite fork is canonical, not blocked-on |
| SUC4.1 DynamoDB non-atomic RMW (HIGH) | `complete` (PR #251; fix landed earlier in #231, verified + regression-pinned; follow-up ticket: batch_write_item stream-record staleness) | atomic read-modify-write with fail-before race test | `suc4/dynamodb-rmw.md` | independent; parallel-capable |
| SUC4.2 Firestore Timestamp+GeoPoint writes (HIGH) | `complete` (PR #253; #231 pre-fixed document fields, this closed array transforms + query contract + wire canonicalization; 7-pass review trail in proof) | rejected typed writes accepted with round-trip fidelity | conformance + adapter tests | `suc4/firestore-types.md` | independent; parallel-capable |
| SUC4.3 Egress HTTPS CONNECT rule (HIGH) | `complete` (PR #252; fix landed earlier in #231 — gate deferral + forced interception — verified + regression-pinned) | method-path rule denying all CONNECT fixed with policy tests | egress policy suite + proxy integration test | `suc4/egress-connect.md` | independent; parallel-capable |
| SUC5.1 Real DynamoDB principal | `complete` (PR #255; found worse than planned: split system/anonymous execution + a real read-authz bypass in the id-prefix scan reachable via DynamoDB Query — all closed; fail-before 4/5 RED) | end DynamoDB executing as `system()`; storage-specific half only (generic `TenantBindingRegistry` stays out of scope) | requests carry a real principal; authz tests | `suc5/principal.md` | after SUC4.1 |
| SUC6.1 Open-loop latency companion | `complete` (PR #256; minicloud gated evidence: p99 ≈3.0ms @25% / ≈3.6–3.9ms @50% CO-free; 75% fails stability gate; recurring ≥50% burst/shed finding) | constant-rate below-saturation harness (e.g. 50%/75% of measured N=256 capacity), coordinated-omission-free percentiles; publishes the campaign's standing prerequisite | accepted CV-gated runs at two rates; doc stating what latency claims are now supportable | `suc6/open-loop.md` | needs quiet windows |
| SUC6.2 Resource-binding cleanup decision | `complete — rejected by U7 gate amendment (owner may override)` | measure the 2.3%-of-guarded candidate on current main; implement only if ≥3% safe end-to-end (expected: reject) | decision row either way | `suc6/binding-decision.md` | measurement-only first |
| SUC6.3 Hot-key/OCC formal closure | `complete` (decision row below) | close D15's Engine-owned follow-up: record moot-by-SWT2 with the +190% valid-pair evidence, or open a successor item if any regression is ever measured | decision row; D15 thread ended | `suc6/hotkey-closure.md` | decision-only |

## Explicitly Rejected

- Weakening `synchronous=FULL` (standing).
- A benchmark-only or adapter-local fast path around the committer.
- Treating the Convex `authorize_silo_selection` review item as open — #239
  landed production call sites (`http_actions/dispatch.rs:64`,
  `handlers/registry_auth.rs:132`); recorded here as verified-closed.
- Re-running any archived SQLite-campaign work (see archive plan; PASS).

## Worktree, PR, And Measurement Protocol

Inherit the archived SQLite campaign's protocol verbatim (fresh sibling
worktrees `nimbus-suc-<phase>` from clean `origin/main`, one concept per PR,
fail-before evidence, structured autoreview before push, immediate merge on
surface-matched key lanes per the owner's fast-loop policy, rejected-run
retention, ledger discipline, remove only your own worktree/branch after
merge). Performance-sensitive phases (SUC2.2, SUC3.1, SUC6.*) use the
canonical CRUD/layered/hot-key protocols with D18/D19 as recorded.

## Follow-Up Tickets

| Ticket | Source | Notes |
| --- | --- | --- |
| DynamoDB `batch_write_item` stream-record staleness | SUC4.1 verification | reads prior images outside a transaction; not a lost-update bug (BatchWriteItem is non-atomic by contract); affects stream INSERT/MODIFY classification and OldImage freshness; fix = route through `execute_single_item_transaction` |
| Type-aware typed-operand query comparison | SUC4.2 review | filters/cursors compare against the plain projection, so typed operands stay rejected (collision + ordering hazards pinned by tests); accepting them needs StoredValue-aware comparison and index-order design. Supersedes the earlier RFC3339-projection-ordering note: range operands are rejected outright now |
| Policy-aware filter-then-fill paging for `scan_documents_by_id_starting_at_cancellable` | SUC5.1 | limit-bearing scan stays policy-blind; sole caller is the adapter-owned `_ddb_stream_*` sidecar so no current exposure; policy awareness needs filter-then-fill paging |
| PPSC ack-loss arm theft (libsql lane flake root cause) | SUC3.1 step-1 CI + 40-run bisection | one-shot arm keys on tenant, so a concurrent `commit == None` transaction consumes it (unconditional `StorageCommitAfterVisibilityBeforeReturn` check), the real batch then commits clean on retry and the test asserts a crash that correctly never happened; the step-3 product-half attempt (gate on `commit.is_some()`) was REFUTED deterministically (7 engine tests; the fenced durable batch is itself `commit == None`); real fix = durable-record identity through the fault interface across nimbus-storage/nimbus-testing (own ticket; folds the mysql PPSC flake). The libsql flush-scoping asymmetry claim was backwards — resolved, not a bug |
| MySQL `has_scheduled_work` filters cron_jobs on `enabled = TRUE` | SUC3.1 step-4 twin survey | sole outlier of five backends; a MySQL tenant whose only work is a disabled cron job is never loaded by the scheduler and re-enabling does not wake it (engine/scheduler/coordination.rs gating); behavior change — needs its own fix + test |
| `arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher` load-flake | observed 3× this campaign | pre-existing (#226/#229 era); fails only under heavy parallel multi-crate load; 30/30 clean isolated, passes full single-crate runs; timing-sensitive not-finished assertion |

## Decision Log

| ID | Date | Decision | Evidence / consequence |
| --- | --- | --- | --- |
| U1 | 2026-07-29 | SUC0.1 gates all phases | main full CI red at plan creation (CI, Coverage, Node Compatibility); the fast-merge bill is triaged first, not last |
| U2 | 2026-07-29 | Facade extraction treats sqlite as reference, after the witness | the sqlite fork is merged and A/B-proven; the witness shapes the facade API, so SUC2 precedes SUC3 |
| U3 | 2026-07-29 | Lease durations unify to milliseconds | postgres and libsql already use millis; mysql converts at its SQL edge; conformance test pins the unit |
| U4 | 2026-07-29 | Fault-point exact-parity assertion deferred to SUC3 | the site survey shows three structurally different flows; SUC3's facade deletes the triplication that permits drift, making per-provider parity tests redundant rather than writing them against code scheduled for deletion |
| U5 | 2026-07-29 | Provider-side CommitTransaction witness rides with SUC3 | the witness requires touching all four providers' apply paths — code SUC3.1 deletes; the facade's single apply signature (every effect a required argument) is the witness (same rationale as U4) |
| U6 | 2026-07-29 | D15 hot-key/OCC follow-up closed as moot-by-SWT2 | SWT2's committer changes lifted valid-pair throughput +190% on the hot-key lane with no measured OCC regression across the campaign's accepted runs (archive: sqlite-write-throughput plan, D15/D18 rows); no Engine-owned successor item; reopen only if a future lane measures an OCC hot-key regression |
| U7 | 2026-07-29 | SUC6.2 completion gate amended: reject-by-attribution, no re-measurement | the candidate (D17 `binding`, 0.108ms = 2.3% of guarded) has an end-to-end ceiling <1%, below the CV≤10% paired protocol's resolution; the full ablation already failed the ≥3% bar (3.8%/2.6%). Flagged for owner override; the SWT4 ablation branch can be rebuilt on current main if the literal gate is preferred |
| U8 | 2026-07-30 | Direct SQL write path stays off the U5 witness | both encodings that could include it (a `Default` bound or boxed-closure erasure) destroy the witness's reviewer-visible-variant purpose; 3 of 4 commit-log paths are compiler-linked; the direct path keeps per-operation validators. Module doc names the trade |

## Open Blockers

| Blocker | Owner | Unblock condition | Status |
| --- | --- | --- | --- |
| All phases gated on SUC0.1 | SUC0.1 | main required lanes green or reds attributed non-campaign with owner sign-off | open |
| SUC6.1/6.2 need quiet-host windows | SUC6 | same discipline as the archived campaign | open |
