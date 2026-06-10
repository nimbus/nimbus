# Plan: Repo Architecture And Seam Hardening

Canonical execution control plane for the next repo-wide architecture wave.
This plan exists because the latest full-subsystem review found a smaller set
of higher-value seams that still keep Nimbus from feeling fully canonical,
fully modular, and fully enterprise-trustworthy pre-launch:

- Firebase application auth still accepts JSON-object bearer payloads as
  authenticated emulator principals with no explicit server-side emulator gate
- the Cloud Functions `firebase-admin/firestore` async write path is not truly
  async/cancellable once execution begins
- provider-specific Firebase admin host-call variants still live in the
  generic `nimbus-runtime` host ABI
- durable document `update_time` now exists in core, but native and Convex
  read surfaces still do not expose that lifecycle metadata canonically
- the structured-query engine root remains above the repo's hard modularity
  threshold
- the Cloud Functions codegen/runtime-bundle root still owns too many
  different concern families in one file

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
- `docs/plans/archive/server-runtime-canonicalization-plan.md`
- the current git worktree on `2026-04-26`

---

## Status

- **Status:** `done`
- **Primary owner:** repo-wide post-canonicalization architecture hardening
- **Activation gate:** immediate; the adapter/runtime, trust, and
  canonicalization baselines are already complete
- **Execution order:** this plan precedes any new repo-wide architecture,
  auth/runtime, or native-transport activation wave that would build on the
  same seams
- **Verification posture:** every implementation item must record focused
  proof lanes because this plan changes live auth, runtime, metadata, engine,
  and codegen contracts

## Why This Exists

The earlier Firebase, Cloud Functions, runtime-boundary, and server
canonicalization waves improved the architecture substantially. The remaining
issues are fewer, but they are more fundamental:

- one trust boundary still fails open in practice
- one "async" runtime path still behaves like sync work in disguise
- one provider-specific contract still leaks into the generic runtime ABI
- one shared lifecycle primitive is not actually canonical across all public
  read surfaces
- two high-churn ownership roots still need decomposition

This is the kind of cleanup Nimbus should finish pre-launch while breaking
changes are preferred over compatibility theater.

## Relationship To Other Plans

- `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`
  is the completed baseline for provider-neutral runtime-host capabilities and
  adapter-owned provider shims.
- `docs/plans/archive/adapter-runtime-trust-hardening-plan.md`
  is the completed baseline for server-owned auth, fail-closed callable auth,
  truthful first-slice lifecycle metadata, shared runtime bootstrap, and the
  main post-Firebase trust cleanup.
- `docs/plans/archive/server-runtime-canonicalization-plan.md`
  is the completed baseline for typed runtime shims, durable lifecycle
  metadata in core, composition-root cleanup, and narrowed host-call dispatch.
- `docs/plans/native-transport-evolution-plan.md`
  remains deferred. Future transport work should not bypass the remaining auth,
  ABI, metadata, engine, and codegen seams tracked here.

## Scope

This plan covers:

- explicit server-side Firebase emulator-auth gating
- truly async/cancellable Firestore-admin write capability execution
- removal of provider-specific Firebase admin variants from the generic
  runtime ABI
- canonical lifecycle metadata exposure across native, Convex, Firebase, and
  Cloud Functions read surfaces
- ownership-based decomposition of the structured query engine root
- ownership-based decomposition of Cloud Functions codegen/runtime bundle
- focused proofs, docs sync, and closeout

This plan does not cover:

- new Firebase surface breadth
- new Cloud Functions surface breadth
- MongoDB adapter implementation work
- new storage-provider topology work
- native transport evolution

## Control Plan Rules

1. Trust boundaries must fail closed by default.
   If a bearer token is presented on an application-auth surface, the server
   must either verify it under the active contract or reject it explicitly.
   Emulator-only shortcuts must require explicit server-side opt-in.

2. "Async/cancellable" must mean service-owned async/cancellable execution.
   `spawn_blocking` wrappers are not the canonical answer for runtime write
   capabilities when the service already owns async/cancellable mutation paths.

3. Provider-specific runtime contracts must not leak into generic runtime ABI
   surfaces.
   The generic runtime crate may carry typed ABI structures, but adapter- or
   provider-family-specific operations should live behind a narrower server or
   adapter seam instead of becoming first-class global host-call variants.

4. Durable lifecycle metadata must be canonical across read surfaces.
   Once `update_time` exists in shared document state, all covered read
   surfaces should either expose it truthfully or explicitly document why they
   do not.

5. Composition roots and engine roots must stay ownership-driven.
   Large files are allowed only when they tell one coherent story. When a file
   mixes lowering, validation, projection, dispatch, manifest reading, bundle
   generation, and provider shims, it must split by owned concept.

6. Docs must stay aligned with the landed baseline.
   Reference docs and AGENTS guidance should point at the active plan while the
   wave is in progress, and move back to completed-baseline language only after
   closeout.

## Current Assessed State

- Firebase routes resolve verified bearer tokens through shared application
  auth, but JSON-object emulator tokens still authenticate without an explicit
  server-side emulator gate.
- Cloud Functions `firebase-admin/firestore` async writes still route through a
  blocking helper instead of a native async/cancellable capability path.
- `nimbus-runtime::host` still includes `FirebaseAdminFirestore*` host-call
  variants even though that surface is provider-specific rather than generic.
- Shared document state now stores durable `update_time`, but native Nimbus and
  Convex read projections still do not expose it.
- `crates/nimbus-engine/src/service/queries/structured.rs` is still above the
  repo's 2,000-line decomposition threshold.
- `packages/codegen/src/cloud_functions.mjs` is still above the "needs
  justification" band and mixes too many ownership stories.

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow docs or trust-contract slice |
| `M` | 12k-18k tokens, 8-14 files | one ownership seam plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer extraction or contract fix |
| `XL` | 28k-40k tokens, 18-30 files | only for decomposition items; split if possible |

Rule:

- if execution needs more than the estimated band for an item, split the item
  in this plan before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `RASH1` | Baseline and findings ledger | — | `S` | `done` |
| `RASH2` | Firebase emulator-auth trust boundary | `RASH1` | `L` | `done` |
| `RASH3` | True async/cancellable Firestore-admin writes | `RASH1` | `L` | `done` |
| `RASH4` | Remove provider-specific Firebase admin operations from generic runtime ABI | `RASH1`, `RASH3` | `XL` | `done` |
| `RASH5` | Canonical lifecycle metadata exposure across read surfaces | `RASH1` | `L` | `done` |
| `RASH6` | Structured query engine ownership split | `RASH1`, `RASH5` | `XL` | `done` |
| `RASH7` | Cloud Functions codegen/runtime-bundle ownership split | `RASH1`, `RASH4` | `XL` | `done` |
| `RASH8` | Focused proofs, docs closeout, and baseline refresh | `RASH2`, `RASH3`, `RASH4`, `RASH5`, `RASH6`, `RASH7` | `L` | `done` |

## Roadmap

### RASH1 — Baseline And Findings Ledger

Goal: turn the full-subsystem architecture review into one explicit execution
owner before more code moves.

- publish the trust, ABI, metadata, engine, and codegen findings as one
  settled control-plane baseline
- refresh repo guidance so later work points at one active owner instead of a
  stack of completed baselines
- record which items are correctness risks, which are ownership problems, and
  which are decomposition follow-ons

Completion gate:

- the plan and repo guidance clearly state the remaining repo-wide seams and
  later items can execute without reopening the architecture review debate

### RASH2 — Firebase Emulator-Auth Trust Boundary

Goal: stop treating arbitrary JSON-object bearer payloads as authenticated
principals unless the server is explicitly in the covered emulator mode.

- decide the clean pre-launch contract for Firebase emulator/mock-user auth
- require explicit server-side opt-in for JSON-object `mockUserToken`-style
  bearers, or reject them directly when the gate is absent
- keep verified bearer-token behavior unchanged for covered Firebase routes
- update compatibility/auth docs so the public story matches the new trust
  boundary exactly

Completion gate:

- Firebase routes no longer authenticate arbitrary JSON bearer payloads by
  default, and focused tests prove the gated versus ungated behavior

### RASH3 — True Async/Cancellable Firestore-Admin Writes

Goal: make Cloud Functions Firestore-admin async writes use a native
async/cancellable service path.

- add shared async/cancellable runtime-host helpers for atomic write-batch
  execution instead of routing through the sync helper in a blocking task
- preserve runtime mutation-session, cancellation, and trigger/write-origin
  semantics explicitly
- route the Cloud Functions `firebase-admin/firestore` async write surface
  through that new capability path

Completion gate:

- Firestore-admin async writes no longer just wrap the sync helper and can
  honor the shared async/cancellable runtime contract

### RASH4 — Remove Provider-Specific Firebase Admin Operations From Generic Runtime ABI

Goal: stop making the generic runtime crate own Firebase-admin-specific
host-call operations.

- decide the narrowest canonical shape for provider-specific runtime ABI
  extension without re-entangling adapters
- move Firebase-admin-specific operation naming and payload ownership out of
  the generic `nimbus-runtime` host surface
- keep the runtime crate zero-workspace-dependency while tightening provider
  neutrality

Completion gate:

- generic runtime host ABI no longer names Firebase-admin-specific operations
  as first-class global variants

### RASH5 — Canonical Lifecycle Metadata Exposure Across Read Surfaces

Goal: make durable `update_time` a truthful, shared public read contract.

- decide the canonical external shape for read-side lifecycle metadata on
  native, Convex, Firebase, and Cloud Functions surfaces
- thread the already-landed core metadata through the missing read projections
- preserve any provider-specific naming while keeping the underlying metadata
  source canonical

Completion gate:

- covered read surfaces expose durable lifecycle metadata consistently from the
  same shared document source

### RASH6 — Structured Query Engine Ownership Split

Goal: decompose the structured query engine root by owned concept.

- split lowering, validation, projection, ordering/cursor handling, and
  service entrypoint concerns into concept-owned children
- keep the composition root thin and explicit
- preserve structured-query semantics and focused proofs during the split

Completion gate:

- `crates/nimbus-engine/src/service/queries/structured.rs` is back below the
  repo threshold or justified only as a thin ownership root

### RASH7 — Cloud Functions Codegen/Runtime-Bundle Ownership Split

Goal: decompose the Cloud Functions codegen root into clearer owned seams.

- split project/app detection, artifact assembly, shared generated runtime
  source, and provider-specific shim generation into concept-owned modules
- keep the composition root thin and explicit
- preserve the current generated contract and selftest coverage

Completion gate:

- `packages/codegen/src/cloud_functions.mjs` is back below the repo threshold
  or justified only as a thin ownership root

### RASH8 — Focused Proofs, Docs Closeout, And Baseline Refresh

Goal: close the wave cleanly once the remaining seams are fixed.

- rerun focused Rust and JS proof lanes for auth, runtime, metadata, engine,
  and codegen work
- refresh AGENTS, plan index, and any touched reference docs so they describe
  the landed baseline accurately
- move this plan out of the active-owner slot once the work is complete

Completion gate:

- focused verification is green, docs match the landed architecture, and this
  plan can become a completed baseline

## Execution Log

| Date | Item | Status | Notes | Verification |
| --- | --- | --- | --- | --- |
| 2026-04-26 | `RASH1` | `done` | Converted the full-subsystem architecture review into one active control-plane owner, registered it in `docs/plans/README.md`, and updated `AGENTS.md` so the remaining auth, ABI, lifecycle, engine, and codegen seams now have one explicit execution owner instead of relying on a stack of completed baselines. | `git diff -- AGENTS.md docs/plans/README.md docs/plans/repo-architecture-and-seam-hardening-plan.md`; `rg -n "repo-architecture-and-seam-hardening-plan" AGENTS.md docs/plans/README.md`. |
| 2026-04-26 | `RASH2` | `done` | Removed implicit Firebase mock-user bearer authentication from the default server contract. JSON-object emulator bearers now authenticate only when `FirebaseConfig` explicitly enables emulator mock-user-token auth; otherwise Firebase routes fail closed through the shared application-auth verifier path. Updated the Firebase auth, compatibility, migration, and browser `Listen` docs to match, and added focused REST plus WebSocket `Listen` proofs for ungated-versus-gated behavior while keeping verified bearer auth green. | `cargo fmt --all --check`; `cargo test -p nimbus-server firebase_mock_user_token_requires_explicit_server_opt_in --lib`; `cargo test -p nimbus-server firebase_listen_websocket_mock_user_token_requires_explicit_server_opt_in --lib`; `cargo test -p nimbus-server firebase_rest_commit_and_batch_get_respect_bearer_principal --lib`. |
| 2026-04-26 | `RASH3` | `done` | Removed the hidden `spawn_blocking` fallback from the shared runtime-host async batch helper. Async atomic write batches now require an active mutation execution unit instead of silently creating a blocking fallback, which matches the covered Cloud Functions mutation invocation model and keeps the async Firestore-admin write path honest. Added a lightweight regression guard so runtime-host capability code cannot quietly reintroduce `spawn_blocking`. | `cargo fmt --all --check`; `cargo test -p nimbus-server async_runtime_integration_removes_hot_path_blocking_adapters --lib`; `cargo test -p nimbus-server cloud_functions --lib`. |
| 2026-04-26 | `RASH4` | `done` | Replaced the four Firebase-admin-specific runtime host-call variants with one provider-neutral `RuntimeExtensionCall` ABI shape in `nimbus-runtime`. The runtime crate now owns only the generic async extension lane, while the Cloud Functions adapter owns the `cloud_functions` extension namespace, the `firebase_admin.firestore.*` operation strings, and the typed payload decoding/dispatch. Convex-side host-call contract and unsupported-adapter handling were updated to keep the adapter boundary explicit instead of carrying Cloud Functions-specific operation names in generic runtime or Convex bridge enums. | `cargo fmt --all --check`; `cargo check -p nimbus-runtime -p nimbus-server`; `cargo test -p nimbus-runtime host_call --lib`; `cargo test -p nimbus-server cloud_functions --lib`; `cargo test -p nimbus-server adapters::convex::tests::contracts --lib`; `npm run test --workspace @nimbus/codegen`. |
| 2026-04-26 | `RASH5` | `done` | Made durable `update_time` part of the shared external document projection instead of leaving it trapped behind provider-specific read paths. `Document::to_json()` and `into_json()` now expose `_updateTime`, shared auth/system-field lookups understand it, generated schema document types include it, and focused native plus Convex-facing tests/docs were updated so read surfaces derive lifecycle metadata from the same shared document source. Firebase and Cloud Functions keep their provider-specific `updateTime` / `update_time_ms` naming on top of that same durable field. | `cargo fmt --all --check`; `cargo check -p nimbus-core -p nimbus-server`; `cargo test -p nimbus-core document_to_json_includes_system_fields --lib`; `cargo test -p nimbus-server convex_query_returns_documents_as_plain_json --lib`; `cargo test -p nimbus-server query_endpoint_returns_filtered_results --lib`; `npm run test --workspace @nimbus/codegen`. |
| 2026-04-26 | `RASH6` | `done` | Split the structured-query engine root into concept-owned children. `structured.rs` is now a 522-line composition root over shared types and `Service` entrypoints, with lowering/index-validation preparation moved into `structured/prepare.rs`, ordering/cursor/finalization logic moved into `structured/finalize.rs`, and the large inline proof slab moved into `structured/tests.rs`. The result keeps the structured query contract intact while putting the highest-churn ownership clusters behind explicit module seams. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p nimbus-engine`; `cargo test -p nimbus-engine structured --lib`. |
| 2026-04-26 | `RASH7` | `done` | Split the Cloud Functions codegen/runtime-bundle root into project detection, bundling, and generated-runtime ownership seams. `packages/codegen/src/cloud_functions.mjs` is now a 194-line composition root; project/app detection lives in `cloud_functions/project.mjs`, esbuild virtual-module assembly lives in `cloud_functions/bundle.mjs`, and generated runtime/shim source generation lives in `cloud_functions/runtime_sources.mjs`. This preserves the existing artifact contract and selftest coverage while making the composition root thin and explicit. | `npm run test --workspace @nimbus/codegen`; `npm run typecheck --workspace @nimbus/codegen`. |
| 2026-04-26 | `RASH8` | `done` | Closed the wave after focused proof lanes passed. Refreshed `docs/plans/README.md` and `AGENTS.md` so this plan is no longer advertised as an active execution owner and instead serves as the latest completed repo-wide architecture/seam hardening baseline. Kept the earlier runtime/auth/canonicalization plans as supporting completed baselines and left future repo-wide seam work gated on promoting a new active control plan rather than silently reusing this finished one. | `git diff -- AGENTS.md docs/plans/README.md docs/plans/repo-architecture-and-seam-hardening-plan.md`; `rg -n "repo-architecture-and-seam-hardening-plan" AGENTS.md docs/plans/README.md`. |
