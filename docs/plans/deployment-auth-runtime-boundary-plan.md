# Plan: Deployment, Auth, And Runtime Boundary Canonicalization

Canonical execution control plane for the next repo-wide server/runtime
architecture wave. This plan exists because the latest full-repo review found
that the remaining high-value seam issues are no longer about raw adapter
breadth; they are about how live deployment state, application auth lifecycle,
the shared runtime ABI, and one still-broad JS compatibility surface are owned.

The current code is substantially healthier than before the Firebase and Cloud
Functions waves, but these seams still keep the architecture from feeling fully
canonical and fully idiomatic pre-launch:

- deploy-time live state is still activated through several independent cells
  instead of one immutable deployment snapshot
- application auth is server-owned in API shape, but still lifecycle-coupled to
  Convex deployment state and Convex-centric operator messaging
- the shared runtime ABI still uses Convex `CtxDb*` names as the generic
  document capability lane reused by Cloud Functions
- `packages/firebase/src/firestore.ts` is below the hard threshold now, but it
  still owns too many public-surface stories in one root
- top-level architecture docs have already drifted behind the landed lifecycle
  metadata contract

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/reference/reliability-posture.md`
- `docs/reference/ci-failure-investigation.md`
- `docs/reference/runtime-adapter-boundary.md`
- `docs/reference/server-auth-runtime-trust.md`
- `docs/reference/firebase-auth-contract.md`
- `docs/reference/firebase-compatibility.md`
- `docs/reference/cloud-functions-compatibility.md`
- `docs/plans/repo-architecture-and-seam-hardening-plan.md`
- `docs/plans/server-runtime-canonicalization-plan.md`
- the current git worktree on `2026-04-26`

---

## Status

- **Status:** `done`
- **Primary owner:** repo-wide deployment/auth/runtime boundary canonicalization
- **Activation gate:** immediate; the earlier runtime, trust, and seam-hardening
  baselines are complete
- **Execution order:** this plan precedes any new repo-wide server/runtime seam
  wave that would touch deploy activation, application auth lifecycle, or the
  generic runtime ABI
- **Verification posture:** every implementation item must record focused proof
  lanes because this wave changes live deploy activation, auth, runtime
  dispatch, and public SDK composition roots

## Why This Exists

Neovex now has the right high-level layering:

- `core` owns shared semantics
- `engine` owns execution and coordination
- `storage` owns persistence providers
- `server` owns transport, deployment, and runtime integration
- adapters mostly behave like adapters

The remaining problems are narrower and more structural:

- live deployment activation should be atomic
- shared auth should be an explicit server subsystem instead of a side effect
  of one adapter
- generic runtime capability lanes should use generic names
- public compatibility roots should keep shrinking toward thin composition
  surfaces
- architecture docs should not trail the landed contract

These are exactly the kinds of changes Neovex should make before launch while
breaking changes are still preferred over compatibility layers.

## Relationship To Other Plans

- `docs/plans/runtime-capability-adapter-boundary-plan.md`
  is the completed baseline that restored the core adapter/runtime ownership
  rule: provider shims stay in adapters and `runtime_host/*` stays
  provider-neutral.
- `docs/plans/adapter-runtime-trust-hardening-plan.md`
  is the completed trust and auth baseline for server-owned application auth,
  fail-closed callable auth, and truthful first-slice lifecycle metadata.
- `docs/plans/server-runtime-canonicalization-plan.md`
  is the completed canonicalization baseline for typed runtime shims, durable
  `_updateTime`, composition-root cleanup, and narrowed host-call dispatch.
- `docs/plans/repo-architecture-and-seam-hardening-plan.md`
  is the completed repo-wide baseline for explicit Firebase emulator-auth
  gating, provider-neutral runtime extension cleanup, canonical lifecycle
  metadata exposure, and the last engine/codegen decomposition wave.

This plan is a follow-on baseline, not a restart of those earlier waves.

## Scope

This plan covers:

- atomic deploy-time activation of auth, adapter registries, and related live
  server state
- application-auth lifecycle and operator-story independence from Convex
- generic naming and dispatch for the shared runtime document capability ABI
- ownership-based decomposition of the remaining Firebase JS Firestore public
  root
- architecture and baseline doc sync after the code moves
- focused proofs and closeout

This plan does not cover:

- MongoDB adapter implementation work
- new Firebase surface breadth
- new Cloud Functions surface breadth
- new storage-provider topology work
- native transport evolution

## Control Plan Rules

1. Live deployment activation must be atomic.
   Request paths should observe one coherent deployment snapshot instead of a
   partially updated mix of auth, registries, and generation counters.

2. Application auth is a server subsystem.
   Adapters may provide verifier config or provider logic, but auth lifecycle,
   activation, and operator messaging should not be implicitly owned by one
   adapter.

3. Shared runtime ABI names should describe shared capabilities.
   If Cloud Functions and future adapters reuse a runtime document capability
   lane, its ABI should not remain branded as Convex `ctx.db.*` internals.

4. Composition roots should stay thin.
   Once helpers and concept-owned children exist, public roots should keep
   moving toward API composition and away from broad multi-story ownership.

5. Docs must stay aligned with the landed contract.
   Reference docs and `AGENTS.md` should point at the active owner while the
   wave is in progress and move back to completed-baseline language only after
   closeout.

## Current Assessed State

- `AppState` still owns separate `ActiveConvexRegistry`,
  `ActiveApplicationAuthVerifier`, `ActiveCloudFunctionsRegistry`,
  `ActiveFirebaseConfig`, and `ActiveDeployGeneration` cells instead of one
  coherent deployment snapshot.
- deploy activation updates auth, Convex registry, Cloud Functions registry,
  trigger registrations, and generation in sequence instead of swapping one
  already-built deployment view.
- the server-auth API is shared, but router/deploy flows still derive auth
  lifecycle from Convex registry activation and retain Convex-centric operator
  guidance.
- the shared runtime ABI document lane still uses `CtxDbGet` /
  `CtxDbInsert` / `CtxDbPatch` / `CtxDbDelete` names.
- `packages/firebase/src/firestore.ts` is smaller than before, but still owns
  bootstrap, refs, query factories, CRUD orchestration, transactions, and
  watch entrypoints together.
- `ARCHITECTURE.md` still describes only `_id` and `_creationTime` as canonical
  document system fields even though `_updateTime` is now part of the shared
  contract.

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow docs or guidance slice |
| `M` | 12k-18k tokens, 8-14 files | one ownership seam plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer contract or activation refactor |
| `XL` | 28k-40k tokens, 18-30 files | only for ABI rename or composition-root splits; split if possible |

Rule:

- if execution needs more than the estimated band for an item, split the item
  in this plan before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `DARB1` | Baseline, findings ledger, and active-guidance refresh | — | `S` | `done` |
| `DARB2` | Atomic active-deployment snapshot for live server state | `DARB1` | `XL` | `done` |
| `DARB3` | Application-auth lifecycle independence and operator-story cleanup | `DARB2` | `L` | `done` |
| `DARB4` | Generic runtime document ABI naming and dispatch | `DARB1` | `XL` | `done` |
| `DARB5` | Firebase JS Firestore public-root ownership split | `DARB1` | `L` | `done` |
| `DARB6` | Docs sync, focused proofs, and closeout baseline refresh | `DARB2`, `DARB3`, `DARB4`, `DARB5` | `L` | `done` |

## Roadmap

### DARB1 — Baseline, Findings Ledger, And Active-Guidance Refresh

Goal: turn the latest architecture review into one explicit owner before more
server/runtime boundary work lands.

- publish the deploy/auth/runtime-ABI/public-root findings in one plan
- refresh `docs/plans/README.md` so this wave is discoverable as active
- update `AGENTS.md` so broad deploy/auth/runtime boundary work points at this
  active owner instead of only completed baselines

Completion gate:

- repo guidance clearly identifies this plan as the active owner for this seam
  wave

### DARB2 — Atomic Active-Deployment Snapshot

Goal: replace the current multi-cell live state activation path with one
coherent deployment snapshot.

- introduce a single active deployment state that owns the live Convex
  registry, Cloud Functions registry, Firebase config, auth verifier, and
  deployment generation together
- make deploy/start flows build the next deployment state first, then swap it
  atomically
- keep trigger registration and runtime-executor installation aligned with the
  newly active deployment instead of with partially updated sub-cells

Completion gate:

- request paths read one coherent deployment snapshot and deploy activation no
  longer exposes mixed-generation live state

### DARB3 — Application-Auth Lifecycle Independence And Operator Story

Goal: make application auth explicitly server-owned instead of implicitly
derived from Convex lifecycle.

- stop `RouterBuildConfig::with_convex(...)` from silently becoming the only
  auth lifecycle path
- make deploy/start flows set application auth explicitly as part of deployment
  activation
- remove Convex-centric operator guidance from shared auth failures where the
  server contract is broader than Convex alone

Completion gate:

- application auth has an explicit server-owned lifecycle and shared operator
  errors no longer imply that Convex is always the owning source

### DARB4 — Generic Runtime Document ABI Naming And Dispatch

Goal: make the shared runtime document capability lane generic in both name and
ownership.

- replace `CtxDbGet` / `CtxDbInsert` / `CtxDbPatch` / `CtxDbDelete` at the
  generic runtime ABI boundary with generic document-operation names
- keep Convex adapter behavior intact through adapter-owned translation where
  necessary, but stop using Convex-branded names as the shared runtime ABI
- update runtime tests, server dispatchers, and Cloud Functions host-bridge
  routing to the new generic contract

Completion gate:

- the shared runtime ABI describes generic document capability operations, and
  Convex-specific naming remains adapter-owned instead of generic-runtime-owned

### DARB5 — Firebase JS Firestore Public-Root Ownership Split

Goal: finish shrinking the Firebase Firestore root into a thinner composition
surface.

- separate the remaining public API clusters into concept-owned children
- keep `firestore.ts` focused on stable exports and thin top-level composition
- preserve package ergonomics while reducing future drift pressure in the main
  root

Completion gate:

- `packages/firebase/src/firestore.ts` owns one coherent public composition
  story and sheds at least one remaining concept family into owned children

### DARB6 — Docs Sync, Focused Proofs, And Closeout

Goal: leave the repo with an accurate baseline and proof bundle after the code
moves.

- update `ARCHITECTURE.md` and any affected reference docs to match the landed
  deployment/auth/runtime contract
- run focused Rust and JS proof lanes for each touched seam
- move this plan from active owner to completed baseline language in the repo
  guidance once all items are done

Completion gate:

- docs match the landed code, focused proof lanes are recorded, and guidance
  no longer advertises this plan as active

## Execution Log

- `2026-04-26`: Plan created from the latest full-repo architecture review.
  `DARB1` complete. `DARB2` starts immediately.
- `2026-04-27`: `DARB2` and `DARB3` completed. `AppState` now activates one
  coherent `DeploymentState` snapshot instead of several live cells, request
  paths read auth/config/registries from that snapshot, and application auth is
  now wired explicitly by server build/deploy flows instead of piggybacking on
  `with_convex(...)`. Verified with `cargo check -p neovex-server`,
  `cargo test -p neovex-server state --lib`,
  `cargo test -p neovex-server cloud_functions --lib`,
  `cargo test -p neovex-server adapters::convex::tests::authorization --lib`,
  `cargo test -p neovex-server firebase_rest_commit_and_batch_get_respect_bearer_principal --lib`,
  `cargo test -p neovex-server local_server_security --lib`,
  `cargo test -p neovex-server deploy --lib`, and
  `cargo test -p neovex-server firebase_auth_and_availability --lib`.
- `2026-04-27`: `DARB4` completed. The shared runtime ABI document lane now
  uses provider-neutral `DocumentGet` / `DocumentInsert` / `DocumentPatch` /
  `DocumentDelete` names, Convex keeps `convex.ctx.db.*` only at the adapter
  contract edge, Cloud Functions routes the generic payloads directly, and the
  runtime bootstrap source now issues the renamed generic host ops. Verified
  with `cargo check -p neovex-runtime -p neovex-server`,
  `cargo test -p neovex-runtime host_call --lib`,
  `cargo test -p neovex-server adapters::convex::tests::contracts --lib`, and
  `cargo test -p neovex-server cloud_functions --lib`.
- `2026-04-27`: `DARB5` and `DARB6` completed. The remaining Firestore public
  model implementation family moved into
  `packages/firebase/src/internal/firestore-models.ts`, shrinking
  `packages/firebase/src/firestore.ts` from 1,470 lines to 1,089 lines so the
  root now acts more like a public composition surface. Architecture and
  boundary docs were updated to reflect `_updateTime`, active deployment
  snapshots, and the generic document runtime ABI, and repo guidance was moved
  from active-owner language to completed-baseline language. Verified with
  `npm run typecheck --workspace @neovex/firebase`,
  `npm run build --workspace @neovex/firebase`,
  `npm run test --workspace @neovex/firebase`, and
  `cargo fmt --all --check`.
