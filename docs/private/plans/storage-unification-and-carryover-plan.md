# Storage Unification And Carry-Over Closeout Control Plane

Status: `active — SUC0–SUC2 complete (PRs #248–#250); SUC4.1+4.3 complete (PRs #251, #252, both pre-fixed by #231); SUC4.2 in review`

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
| SUC3.1 Provider facade extraction | `planned` | delete the ~855-line triplicated blocks into one facade; sqlite (with #244/#245 optimizations) is the reference; provider feature gates so embedded builds/measures in isolation | ~−1,700 LoC net; conformance harness green for all providers; sqlite paired A/B unchanged | `suc3/facade.md` | after SUC2; sqlite fork is canonical, not blocked-on |
| SUC4.1 DynamoDB non-atomic RMW (HIGH) | `complete` (PR #251; fix landed earlier in #231, verified + regression-pinned; follow-up ticket: batch_write_item stream-record staleness) | atomic read-modify-write with fail-before race test | `suc4/dynamodb-rmw.md` | independent; parallel-capable |
| SUC4.2 Firestore Timestamp+GeoPoint writes (HIGH) | `in_progress` (`codex/suc4b-firestore-types`) | rejected typed writes accepted with round-trip fidelity | conformance + adapter tests | `suc4/firestore-types.md` | independent; parallel-capable |
| SUC4.3 Egress HTTPS CONNECT rule (HIGH) | `complete` (PR #252; fix landed earlier in #231 — gate deferral + forced interception — verified + regression-pinned) | method-path rule denying all CONNECT fixed with policy tests | egress policy suite + proxy integration test | `suc4/egress-connect.md` | independent; parallel-capable |
| SUC5.1 Real DynamoDB principal | `planned` | end DynamoDB executing as `system()`; storage-specific half only (generic `TenantBindingRegistry` stays out of scope) | requests carry a real principal; authz tests | `suc5/principal.md` | after SUC4.1 |
| SUC6.1 Open-loop latency companion | `planned` | constant-rate below-saturation harness (e.g. 50%/75% of measured N=256 capacity), coordinated-omission-free percentiles; publishes the campaign's standing prerequisite | accepted CV-gated runs at two rates; doc stating what latency claims are now supportable | `suc6/open-loop.md` | needs quiet windows |
| SUC6.2 Resource-binding cleanup decision | `planned` | measure the 2.3%-of-guarded candidate on current main; implement only if ≥3% safe end-to-end (expected: reject) | decision row either way | `suc6/binding-decision.md` | measurement-only first |
| SUC6.3 Hot-key/OCC formal closure | `planned` | close D15's Engine-owned follow-up: record moot-by-SWT2 with the +190% valid-pair evidence, or open a successor item if any regression is ever measured | decision row; D15 thread ended | `suc6/hotkey-closure.md` | decision-only |

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

## Decision Log

| ID | Date | Decision | Evidence / consequence |
| --- | --- | --- | --- |
| U1 | 2026-07-29 | SUC0.1 gates all phases | main full CI red at plan creation (CI, Coverage, Node Compatibility); the fast-merge bill is triaged first, not last |
| U2 | 2026-07-29 | Facade extraction treats sqlite as reference, after the witness | the sqlite fork is merged and A/B-proven; the witness shapes the facade API, so SUC2 precedes SUC3 |
| U3 | 2026-07-29 | Lease durations unify to milliseconds | postgres and libsql already use millis; mysql converts at its SQL edge; conformance test pins the unit |
| U4 | 2026-07-29 | Fault-point exact-parity assertion deferred to SUC3 | the site survey shows three structurally different flows; SUC3's facade deletes the triplication that permits drift, making per-provider parity tests redundant rather than writing them against code scheduled for deletion |
| U5 | 2026-07-29 | Provider-side CommitTransaction witness rides with SUC3 | the witness requires touching all four providers' apply paths — code SUC3.1 deletes; the facade's single apply signature (every effect a required argument) is the witness (same rationale as U4) |

## Open Blockers

| Blocker | Owner | Unblock condition | Status |
| --- | --- | --- | --- |
| All phases gated on SUC0.1 | SUC0.1 | main required lanes green or reds attributed non-campaign with owner sign-off | open |
| SUC6.1/6.2 need quiet-host windows | SUC6 | same discipline as the archived campaign | open |
