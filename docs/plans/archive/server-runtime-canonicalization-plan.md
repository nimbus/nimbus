# Plan: Server Runtime Canonicalization

Canonical execution control plane for the next post-Firebase, post-Cloud
Functions architecture wave. This plan exists because the completed trust and
boundary hardening work left Nimbus in a much healthier state, but the current
review still found several important seams that are less canonical, less
modular, or less enterprise-trustworthy than the pre-launch repo should accept:

- server-owned application auth is still lifecycle-coupled to `ConvexRegistry`
  instead of existing as its own first-class server subsystem
- the Cloud Functions `firebase-admin/firestore` runtime shim still bounces
  typed host payloads through `serde_json::Value`, and its async write path is
  not truly async
- the core document model cannot yet express durable last-update metadata,
  limiting truthful cross-adapter read contracts and stronger audit semantics
- high-churn public switchboards remain above the repo’s “needs justification”
  modularity band
- Convex host-call dispatch still repeats the same sync / cancellable / async
  trees manually
- boundary docs have already drifted slightly behind the landed code layout

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/testing/ci-failure-investigation.md`
- `docs/architecture/runtime/adapter-boundary.md`
- `docs/architecture/server/auth-runtime-trust.md`
- `docs/adapters/firebase/auth-contract.md`
- `docs/adapters/firebase/compatibility.md`
- `docs/adapters/cloud-functions/compatibility.md`
- `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`
- `docs/plans/archive/adapter-runtime-trust-hardening-plan.md`
- the current git worktree on `2026-04-26`

---

## Status

- **Status:** `done`
- **Primary owner:** completed server/runtime canonicalization baseline
- **Completed on:** `2026-04-26`
- **Activation gate:** the runtime-capability and trust-hardening plans were
  complete before this wave started
- **Execution order:** completed before any future activation of
  `docs/plans/native-transport-evolution-plan.md`
- **Verification posture:** each implementation item records focused proof
  lanes because this plan changed live server/runtime contracts

## Why This Exists

The adapter and runtime boundary work succeeded in the most important way:
adapters mostly act like adapters now, and the shared data path is much closer
to provider-neutral primitives. The remaining seams are narrower, but they are
also more foundational:

- auth should be its own server subsystem
- runtime API shims should stay typed all the way down
- document lifecycle metadata should be canonical instead of inferred ad hoc
- the highest-churn public surfaces should be smaller and clearer
- host-call dispatch should not require triplicate hand-maintained switchboards

This is exactly the kind of cleanup Nimbus should do pre-launch while direct
breaking changes are still preferred over compatibility layers.

## Relationship To Other Plans

- `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`
  is the completed baseline that corrected the main ownership mistake:
  provider-specific runtime shims stay in adapters and `runtime_host/*` stays
  provider-neutral.
- `docs/plans/archive/adapter-runtime-trust-hardening-plan.md`
  is the completed baseline for server-owned auth, fail-closed callable auth,
  provider-family seams, truthful first-slice Firestore-admin metadata, shared
  runtime bootstrap, and the last trust-oriented clippy cleanup.
- `docs/plans/native-transport-evolution-plan.md`
  remains deferred until explicitly promoted. This plan is now part of the
  completed baseline that native transport work should build on.

## Scope

This plan covers:

- server-owned application auth lifecycle independence
- typed provider runtime-API shims without JSON bounce
- truly async/cancellable runtime write capabilities
- canonical durable document lifecycle metadata
- ownership-based modular splits for the highest-churn public surfaces
- reduced host-bridge dispatch repetition
- boundary-doc sync after the code moves
- focused proofs and closeout

This plan does not cover:

- new Firebase surface breadth
- new Cloud Functions surface breadth
- MongoDB adapter implementation work
- native transport evolution
- new storage provider topology work

## Control Plan Rules

1. Server-owned subsystems should not hide inside one adapter’s lifecycle.
   Shared auth may still consume adapter-provided config today, but the server
   should own activation, rotation, and observation explicitly.

2. Typed runtime payloads should stay typed.
   Provider-specific runtime shims may translate shaped payload structs into
   shared capability inputs, but they should not serialize to JSON and parse
   back just to cross an internal boundary.

3. Async runtime paths should actually be async.
   If a runtime API is surfaced as async/cancellable, its write path should use
   explicit async/cancellable capability execution instead of routing back to
   the sync helper.

4. Durable metadata should be canonical at the core layer.
   If adapters need truthful `update_time` or equivalent read-side lifecycle
   data, the answer should come from shared document metadata rather than
   transient adapter-local inference.

5. Composition roots must stay thin.
   Public switchboards may coordinate, but new logic should move into
   concept-owned children instead of accumulating in one root file.

6. Dispatch repetition is a design smell.
   When sync, cancellable, and async runtime host-call paths all repeat the
   same variant routing manually, prefer a narrower shared dispatch seam.

7. Docs must trail code by hours, not weeks.
   Boundary references are only useful if they match the landed module layout.

## Current Assessed State

- `AppState` owns application-auth verifier lifecycle explicitly instead of
  deriving it from `ConvexRegistry`.
- Cloud Functions `firebase-admin/firestore` host-call dispatch stays typed
  end to end and uses explicit async runtime write helpers.
- `nimbus-core::Document` now stores both durable `creation_time` and
  `update_time`, and covered adapters expose truthful shared lifecycle
  metadata.
- `crates/nimbus-server/src/adapters/cloud_functions/http.rs` and
  `packages/firebase/src/firestore.ts` are both back under the repo’s
  preferred composition-root modularity band.
- Convex host-bridge document/query dispatch now routes through a narrower
  family-owned dispatch seam instead of three mostly parallel trees.
- `docs/architecture/runtime/adapter-boundary.md` matches the landed
  `runtime_host/abi/document_calls.rs` layout.

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow docs or cleanup slice |
| `M` | 12k-18k tokens, 8-14 files | one ownership seam plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer extraction or contract fix |
| `XL` | 28k-40k tokens, 18-30 files | only for decomposition or metadata refactors; split if possible |

Rule:

- if execution needs more than the estimated band for an item, split the item
  in this plan before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `SRC1` | Canonical baseline and review findings ledger | — | `S` | `done` |
| `SRC2` | Server-owned application auth lifecycle independence | `SRC1` | `L` | `done` |
| `SRC3a` | Typed Cloud Functions Firestore-admin runtime shim | `SRC1` | `L` | `done` |
| `SRC3b` | Truly async/cancellable runtime write capabilities | `SRC3a` | `L` | `done` |
| `SRC4` | Canonical durable document lifecycle metadata | `SRC3b` | `XL` | `done` |
| `SRC5` | Cloud Functions HTTP composition-root split | `SRC1` | `L` | `done` |
| `SRC6` | Firebase JS Firestore public-surface split | `SRC1` | `XL` | `done` |
| `SRC7` | Convex host-call dispatch consolidation | `SRC2`, `SRC3b` | `L` | `done` |
| `SRC8` | Boundary-doc sync and stale reference cleanup | `SRC2`, `SRC3a`, `SRC7` | `S` | `done` |
| `SRC9` | Focused proofs, control-plane closeout, and native-transport gate refresh | `SRC4`, `SRC5`, `SRC6`, `SRC7`, `SRC8` | `M` | `done` |

## Roadmap

### SRC1 — Canonical Baseline And Review Findings Ledger

Goal: turn the review fallout into one explicit execution baseline before more
code moves.

- publish the remaining auth, runtime, metadata, modularity, and dispatch
  issues as one settled control-plane baseline
- record which findings are correctness risks, which are ownership problems,
  and which are modularity follow-ons
- refresh repo guidance so later implementation points to one agreed owner

Completion gate:

- this plan and repo guidance clearly state the remaining seams and later items
  can execute without reopening the architecture review debate

### SRC2 — Server-Owned Application Auth Lifecycle Independence

Goal: make shared application auth a first-class server subsystem instead of a
derived property of `ConvexRegistry`.

- remove `AppState::from_config` seeding auth verifier state from
  `convex_registry`
- make auth verifier activation an explicit server-owned config surface
- keep Convex auth-provider parsing and bearer verification logic adapter-owned
  where appropriate, but stop making auth lifecycle ownership implicit

Completion gate:

- server-owned auth state no longer derives from `ConvexRegistry` as a hidden
  side effect, and deploy/start flows set auth lifecycle state explicitly

### SRC3a — Typed Cloud Functions Firestore-Admin Runtime Shim

Goal: keep provider runtime-API shims typed instead of bouncing through JSON.

- remove the internal `serde_json::to_value` / `from_value` roundtrip from
  `firebase-admin/firestore` host-call dispatch
- keep provider-specific request semantics under the Cloud Functions adapter
- preserve the adapter/runtime boundary while making the shim cheaper and
  easier to reason about

Completion gate:

- the Cloud Functions Firestore-admin shim remains adapter-owned and typed all
  the way down

### SRC3b — Truly Async/Cancellable Runtime Write Capabilities

Goal: make async runtime writes use explicit async/cancellable capability
 execution.

- introduce async/cancellable shared runtime write helpers for staged or
  standalone write-batch execution
- route async Firestore-admin writes through those helpers instead of the sync
  path
- preserve cancellation and mutation-session semantics explicitly

Completion gate:

- async runtime write APIs no longer just call the sync helper

### SRC4 — Canonical Durable Document Lifecycle Metadata

Goal: make last-update metadata a core primitive instead of an adapter-local
 limitation.

- decide the canonical shared metadata shape for durable creation/update
  lifecycle information
- thread it through core, storage, engine write outcomes, and covered adapter
  read contracts
- update any adapter responses that can now expose truthful lifecycle metadata

Completion gate:

- covered adapters can expose truthful durable lifecycle metadata from shared
  core state rather than transient inference

### SRC5 — Cloud Functions HTTP Composition-Root Split

Goal: shrink the Cloud Functions HTTP surface to a thinner composition root.

- split callable and plain HTTP exposure handling into concept-owned children
- keep tenant resolution, response shaping, and CORS rules clear but separated
- leave the root as a router/composition boundary rather than a behavior sink

Completion gate:

- `crates/nimbus-server/src/adapters/cloud_functions/http.rs` is below the
  1,500-line threshold or justified by a strong ownership exception recorded in
  this plan

### SRC6 — Firebase JS Firestore Public-Surface Split

Goal: reduce the remaining public-surface switchboard in the first-party
 Firebase package.

- split the remaining query/transaction/transport coordination out of
  `packages/firebase/src/firestore.ts`
- keep the public API clear while pushing behavior into concept-owned children
- preserve type surface and selftest coverage while shrinking the root

Completion gate:

- `packages/firebase/src/firestore.ts` is below the 1,500-line threshold or
  justified by a strong ownership exception recorded in this plan

### SRC7 — Convex Host-Call Dispatch Consolidation

Goal: reduce the triple-dispatch repetition in the Convex host bridge.

- narrow the shared dispatch seam for sync / cancellable / async host-call
  routing
- keep adapter-owned Convex behavior under the Convex adapter, but remove
  avoidable repetition
- preserve explicit unsupported handling for adapter-owned runtime APIs that
  should not route through Convex host calls

Completion gate:

- the Convex host bridge no longer requires largely parallel hand-maintained
  sync/cancellable/async trees for the same host-call families

### SRC8 — Boundary-Doc Sync And Stale Reference Cleanup

Goal: leave the docs matching the landed code.

- refresh runtime-boundary and trust docs after the code moves
- remove stale path references and outdated active-plan wording
- keep `AGENTS.md` aligned with the new active owner while the plan is in
  progress

Completion gate:

- boundary docs and repo guidance reflect the landed module layout accurately

### SRC9 — Focused Proofs, Control-Plane Closeout, And Native-Transport Gate Refresh

Goal: leave the repo pointing at the corrected canonical baseline.

- add focused tests that prove:
  - auth lifecycle remains server-owned and deploy-safe
  - typed runtime shims still execute correctly
  - async runtime writes preserve cancellation and write semantics
  - document lifecycle metadata is truthful where exposed
  - modular splits preserve public behavior
- update `docs/plans/README.md`, `AGENTS.md`, and any affected reference docs
- refresh `docs/plans/native-transport-evolution-plan.md` if its promotion
  guidance changes after this wave closes

Completion gate:

- focused verification and docs both point at the corrected canonical baseline

## Verification Expectations

Each implementation item should record focused verification before it closes.
Expected lanes include the narrowest proofs that match the touched surface, for
example:

- `cargo test -p nimbus-server cloud_functions --lib`
- focused Firebase lanes under `cargo test -p nimbus-server ...`
- focused Convex auth/runtime lanes under `cargo test -p nimbus-server ...`
- `cargo check -p nimbus-server`
- `cargo fmt --all --check`
- `cargo clippy -p nimbus-server --lib --tests -- -D warnings`
- focused `npm run typecheck --workspace @nimbus/firebase`
- focused `npm run test --workspace @nimbus/firebase`

Use broader workspace verification only after the focused proofs are green.

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-26 | plan authoring | `done` | Created this control-plane plan from the architecture and modularity review after the completed trust/boundary waves. No code verification ran in the authoring pass because this step only established the control plane. |
| 2026-04-26 | `SRC1` | `done` | Promoted this plan into `docs/plans/README.md` as the active owner and updated `AGENTS.md` so post-Firebase / post-Cloud-Functions auth/runtime/modularity follow-ons now point here instead of only at the completed trust baseline. |
| 2026-04-26 | `SRC2` | `done` | `AppState` no longer infers `application_auth_verifier` from `convex_registry`; router composition explicitly seeds the verifier at startup, and deploy activation continues to update it explicitly. Focused proofs: `cargo test -p nimbus-server state::tests --lib`, `cargo test -p nimbus-server deploy_ --lib`, `cargo test -p nimbus-server convex_runtime_query_rejects_invalid_bearer_token --lib`, `cargo test -p nimbus-server firebase_rest_commit_and_batch_get_respect_bearer_principal --lib`, and `cargo check -p nimbus-server`. |
| 2026-04-26 | `SRC3a` + `SRC3b` | `done` | Removed the internal `serde_json::Value` bounce from the Cloud Functions `firebase-admin/firestore` host-call dispatcher and introduced `execute_atomic_write_batch_async(...)` in the shared runtime-host capability seam so async Firestore-admin writes no longer just fall back to the sync batch helper. Focused proof: `cargo test -p nimbus-server cloud_functions --lib`. |
| 2026-04-26 | `SRC4` | `done` | Added durable `update_time` metadata to `nimbus-core::Document`, threaded it through engine write preservation plus SQLite/Postgres/MySQL/LibSQL persistence paths, and updated covered Firebase and Cloud Functions read contracts to expose truthful shared lifecycle metadata. No-op overwrites now preserve `update_time` so trigger suppression semantics remain correct. Focused proofs: `cargo check -p nimbus-core -p nimbus-storage -p nimbus-engine -p nimbus-server`; `cargo test -p nimbus-server cloud_functions --lib`. |
| 2026-04-26 | `SRC5` | `done` | Split callable-specific Cloud Functions HTTP behavior into `adapters/cloud_functions/http/callable.rs`, leaving `http.rs` as a thinner composition root under the 1,500-line threshold while preserving the same tenant-resolution, CORS, and runtime invocation behavior. Focused proof: `cargo test -p nimbus-server cloud_functions --lib`. |
| 2026-04-26 | `SRC6` | `done` | Extracted the remaining Firebase Firestore public-surface helpers into `packages/firebase/src/internal/firestore-helpers.ts`, shrinking `packages/firebase/src/firestore.ts` back under the 1,500-line threshold without changing the public API surface. Focused proofs: `npm run typecheck --workspace @nimbus/firebase`; `npm run build --workspace @nimbus/firebase`; `npm run test --workspace @nimbus/firebase`. |
| 2026-04-26 | `SRC7` | `done` | Consolidated the Convex host-bridge triple-dispatch into family-owned enums under `adapters/convex/host_bridge/db_ops/dispatch.rs`, leaving `db_ops/mod.rs` as a narrower adapter-owned dispatch root with explicit unsupported handling for adapter-owned host calls. Focused proofs: `cargo test -p nimbus-server adapters::convex::tests::authorization --lib`; `cargo test -p nimbus-server adapters::convex::tests::cancellation --lib`; `cargo test -p nimbus-server cloud_functions --lib`; `cargo check -p nimbus-server`. |
| 2026-04-26 | `SRC8` | `done` | Refreshed the canonical boundary docs and stable completed baselines so the runtime-host ABI layout, composition-root thresholds, and completed canonicalization wave all match the landed code instead of the pre-split plan state. Focused proof: targeted `sed`/`rg` review across `docs/architecture/runtime/adapter-boundary.md`, `docs/plans/README.md`, `AGENTS.md`, and `docs/plans/native-transport-evolution-plan.md`. |
| 2026-04-26 | `SRC9` | `done` | Closed the control plane by moving this plan out of the active index, rewriting `AGENTS.md` to treat it as a completed baseline instead of an active owner, and refreshing native-transport guidance so future work starts from the corrected server/runtime/auth baseline. Focused proofs: `cargo fmt --all --check`; targeted `rg` review of plan-index and AGENTS references. |
