# Plan: Runtime Capability And Adapter Boundary

Canonical execution control plane for restoring the intended
"adapters-as-shims over shared Neovex primitives" architecture after the
Firebase and Cloud Functions waves. This plan exists because the recent
multi-adapter work correctly promoted many shared data primitives, but one
important boundary was still crossed in the wrong direction: provider-specific
runtime compatibility shims were moved into `runtime_host/*`, where only
provider-neutral capabilities should live.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/testing/ci-failure-investigation.md`
- `docs/plans/archive/multi-adapter-boundary-hardening-plan.md`
- `docs/plans/runtime-provider-boundary-hardening-plan.md`
- `docs/plans/native-transport-evolution-plan.md`
- `crates/neovex-server/src/runtime_host/mod.rs`
- `crates/neovex-server/src/adapters/cloud_functions/runtime_api/firebase_admin/firestore.rs`
- `crates/neovex-server/src/adapters/cloud_functions/execution.rs`
- `crates/neovex-server/src/adapters/cloud_functions/http.rs`
- `crates/neovex-server/src/adapters/convex/host_bridge/db_ops/mod.rs`
- `docs/architecture/runtime/adapter-boundary.md`
- `packages/codegen/src/cloud_functions.mjs`
- the current git worktree on `2026-04-26`

---

## Status

- **Status:** `done`
- **Primary owner:** completed adapter/runtime boundary baseline
- **Completed on:** `2026-04-26`
- **Execution order:** completed before any promotion of
  `docs/plans/native-transport-evolution-plan.md`
- **Verification posture:** each roadmap item must record focused verification
  before closing; docs-only reasoning is not sufficient because this plan is
  correcting a real ownership leak in live code

## Why This Exists

Neovex now has multiple adapter families, which was the point of the Firebase
and Cloud Functions work. That effort successfully promoted many reusable
database and trigger primitives into `neovex-core`, `neovex-engine`, and
`neovex-storage`.

The remaining problem is higher in the stack:

- a runtime compatibility shim for `firebase-admin/firestore` now lives under
  `crates/neovex-server/src/runtime_host/`,
- that same shim still depends directly on `ConvexHostBridge` and
  `ConvexRuntimeResponseEnvelope`,
- and the "shared" runtime host path is still implemented as a thin Convex
  wrapper instead of a truly server-owned capability surface.

That is the opposite of the intended architecture.

The correct model is:

1. shared Neovex primitives live in core, engine, storage, and provider-neutral
   server capability layers
2. adapters own provider-specific shims, including runtime API compatibility
   shims
3. adapters translate provider contracts into shared capabilities
4. shared capability layers do not become provider-named just because more than
   one adapter uses them

This plan fixes that boundary explicitly before another cross-cutting wave such
as native transport evolution.

## Relationship To Other Plans

- `docs/plans/archive/multi-adapter-boundary-hardening-plan.md`
  is the completed follow-up that fixed principal propagation, stock-compat
  truth, WebSocket prelaunch cleanup, and the first runtime-host extraction
  step. This plan corrects the remaining mistake in that extraction: moving a
  provider shim instead of only moving primitives.
- `docs/plans/runtime-provider-boundary-hardening-plan.md`
  is the completed runtime/provider cleanup baseline for typed host ABI payloads
  and provider-owned persistence capabilities. This plan continues that same
  philosophy inside the server runtime-host layer.
- `docs/plans/native-transport-evolution-plan.md`
  remains deferred. It should not be promoted while the adapter/runtime seam is
  still confused, because transport work should build on clean ownership
  boundaries rather than adding another cross-cutting layer first.

## Scope

This plan covers:

- the canonical distinction between adapter-owned runtime compatibility shims
  and provider-neutral runtime capabilities
- extraction of a provider-neutral runtime-host capability surface from the
  current Convex-backed implementation
- relocation of the covered `firebase-admin/firestore` runtime shim back under
  adapter ownership
- removal of direct `ConvexHostBridge`, `ConvexRegistry`, and
  `ConvexRuntimeResponseEnvelope` leakage from shared runtime-host modules
- narrowing the Convex host bridge so it adapts shared capabilities instead of
  serving as the de facto shared runtime-host API
- docs and focused proof updates needed to keep later adapter and transport
  work honest

This plan does not cover:

- new Firebase or Cloud Functions product breadth
- new Firestore network transport work
- Firebase WebChannel support
- new native transport codec or WebTransport work
- a redesign of the runtime ABI payload family unless it is directly required
  to restore the intended adapter/runtime boundary

## Control Plan Rules

1. Adapters own provider-specific shims.
   This includes runtime API compatibility shims such as
   `firebase-admin/firestore`, not just HTTP, gRPC, or WebSocket transport
   handlers.

2. `runtime_host/*` owns only provider-neutral capabilities.
   Shared runtime-host modules may execute document reads, staged writes,
   session validation, read tracking, and similar primitives, but they must not
   encode Firebase-, Firestore-, or Convex-specific compatibility contracts.

3. Shared runtime-host modules may not depend directly on adapter-owned types.
   If a module under `runtime_host/*` imports `ConvexHostBridge`,
   `ConvexRegistry`, or other adapter-owned compatibility types, the seam is
   still wrong.

4. Translation and execution are separate concerns.
   Provider shims translate provider payloads, paths, and result shapes into
   generic capability calls. Shared capability code executes the primitive
   behavior and returns provider-neutral results.

5. No namespace laundering.
   Moving provider-specific code into a shared folder does not make it shared.
   Code is shared only when its inputs, outputs, dependencies, and names are
   provider-neutral.

6. Pre-launch breaking cleanup is preferred.
   Delete the confused boundary and replace it directly instead of preserving
   intermediate compatibility layers.

## Current Assessed State

- The covered `firebase-admin/firestore` runtime shim now lives under
  `crates/neovex-server/src/adapters/cloud_functions/runtime_api/firebase_admin/firestore.rs`,
  which is the correct ownership layer for provider-specific runtime
  compatibility translation.
- The reusable document-read, staged-write, standalone-write, and runtime
  session-validation primitives now live in
  `crates/neovex-server/src/runtime_host/capabilities.rs` and are generic over
  a narrow `RuntimeCapabilityHost` trait instead of depending directly on
  `ConvexHostBridge`.
- `crates/neovex-server/src/runtime_host/mod.rs`,
  `runtime_host/abi/document_calls.rs`, and `runtime_host/responses.rs` now
  define server-owned runtime-host scope, invocation, context, host-call
  execution, and neutral response-envelope helpers without importing
  `ConvexHostBridge`, `ConvexRegistry`, or `ConvexRuntimeResponseEnvelope`.
- `crates/neovex-server/src/adapters/cloud_functions/host_bridge.rs` now owns
  the Cloud Functions runtime bridge and adapts the shared runtime-host
  capabilities plus the adapter-owned `firebase-admin/firestore` shim.
- `crates/neovex-server/src/adapters/convex/host_bridge/db_ops/mod.rs` no
  longer dispatches `FirebaseAdminFirestore*` host calls. Convex host-bridge
  roots now own Convex behavior only plus explicit adaptation to shared
  provider-neutral capabilities.
- `packages/codegen/src/cloud_functions.mjs` exposes covered
  `firebase-admin/firestore` compatibility to user code. That confirms the shim
  is real and adapter-owned, not a shared primitive.
- The architecture goal is not "no provider-specific code in the server." The
  goal is "provider-specific code stays in adapters, and adapters are built on
  provider-neutral primitives."

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow docs or naming slice |
| `M` | 12k-18k tokens, 8-14 files | one ownership boundary plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer extraction or relocation |
| `XL` | 28k-40k tokens, 18-30 files | only for decomposition or full boundary closeout; split if possible |

Rule:

- if execution needs more than the estimated band for an item, split the item
  in this plan before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `RCAB1` | Boundary contract baseline and naming rules | — | `S` | `done` |
| `RCAB2` | Provider-neutral runtime capability surface | `RCAB1` | `L` | `done` |
| `RCAB3` | Relocate `firebase-admin/firestore` shim under adapter ownership | `RCAB1`, `RCAB2` | `L` | `done` |
| `RCAB4` | Remove Convex type leakage from `runtime_host/*` | `RCAB2`, `RCAB3` | `L` | `done` |
| `RCAB5` | Narrow Convex host bridge to adapter-owned concerns | `RCAB2`, `RCAB4` | `L` | `done` |
| `RCAB6` | Runtime-host naming, module-layout, and envelope cleanup | `RCAB3`, `RCAB4`, `RCAB5` | `M` | `done` |
| `RCAB7` | Focused proofs for Cloud Functions, Convex, and shared runtime capabilities | `RCAB3`, `RCAB4`, `RCAB5`, `RCAB6` | `L` | `done` |
| `RCAB8` | Closeout docs, plan index, AGENTS, and native-transport gate refresh | `RCAB7` | `S` | `done` |

## Roadmap

### RCAB1 — Boundary Contract Baseline And Naming Rules

Goal: document the correct ownership model in a way that later refactors can
follow consistently.

- Publish one concise reference for the adapter/runtime boundary, likely under
  `docs/architecture/runtime/`, that states:
  - adapters own provider-specific runtime compatibility shims
  - `runtime_host/*` owns provider-neutral capabilities only
  - shared modules may not carry provider names unless a strong exception is
    recorded
  - shared modules may not depend on adapter-owned bridge types
- Record the current anti-patterns explicitly:
  - `runtime_host/firestore_admin.rs`
  - `RuntimeHostBridge` wrapping `ConvexHostBridge`
  - Convex dispatch of `FirebaseAdminFirestore*` host calls
- Make the target end-state concrete enough that later items can be judged
  against it without reopening the architecture debate

Completion gate:

- there is one canonical written contract for this boundary and the plan’s next
  items can point to it directly

### RCAB2 — Provider-Neutral Runtime Capability Surface

Goal: extract the actual reusable primitives that provider shims should call.

- Identify the narrow primitive operations that the current Firestore admin shim
  actually needs, such as:
  - document get by path or locator
  - staged write-batch execution against an active mutation execution unit
  - standalone write-batch execution outside one
  - invocation/session validation
  - principal and tenant access
  - read tracking
- Implement these as server-owned runtime capability modules under
  `runtime_host/` with provider-neutral names
- Decide which data crosses that seam:
  - likely Neovex document locators, field maps, atomic write batches, and
    provider-neutral result structs
  - not Firestore document paths, database IDs, or provider-shaped JSON
    envelopes

Completion gate:

- a provider-neutral capability surface exists and shared runtime-host code no
  longer needs provider-specific names to express the primitive behavior

### RCAB3 — Relocate `firebase-admin/firestore` Shim Under Adapter Ownership

Goal: move provider-specific runtime compatibility code back to the adapter
layer where it belongs.

- Move the current `firebase-admin/firestore` translation logic out of
  `runtime_host/*`
- Rehome it under adapter ownership, likely beneath
  `adapters/cloud_functions/` or an adapter-owned runtime-API sub-tree
- Keep Firestore-specific responsibilities there:
  - runtime payload decoding
  - Firestore path and database parsing
  - Firestore-shaped result encoding
  - covered behavior boundaries and error text
- Make that shim call the new provider-neutral runtime capability surface
  instead of directly depending on Convex host-bridge internals

Completion gate:

- there is no provider-specific Firestore admin shim left under
  `runtime_host/*`, and Cloud Functions clearly owns the
  `firebase-admin/firestore` compatibility layer

### RCAB4 — Remove Convex Type Leakage From `runtime_host/*`

Goal: make `runtime_host/*` actually shared at the type and dependency level.

- Stop importing `ConvexHostBridge`, `ConvexRegistry`,
  `ConvexHostBridgeScope`, `ConvexHostBridgeInvocation`, and
  `ConvexRuntimeResponseEnvelope` inside shared runtime-host modules
- Replace the current thin wrapper approach with server-owned shared runtime
  host types or traits that are not Convex-branded
- Remove the current `RuntimeHostEnvironment::Shared` path that still
  synthesizes a `ConvexRegistry`
- Decide whether the current response envelope is actually:
  - a shared runtime ABI envelope that should be renamed and rehomed, or
  - an adapter-specific envelope that should stay inside the Convex adapter

Completion gate:

- `runtime_host/*` depends only on server-owned shared runtime-host types and
  provider-neutral primitives

### RCAB5 — Narrow Convex Host Bridge To Adapter-Owned Concerns

Goal: make Convex an adapter implementation on top of the shared runtime
capability surface instead of the accidental shared runtime-host owner.

- Remove Firebase or Cloud Functions compatibility dispatch from Convex-owned
  host-bridge modules
- Make the Convex host bridge implement or delegate to the shared runtime
  capability surface where reuse is legitimate
- Keep Convex-specific query-builder, scheduler, and Convex contract lowering
  under Convex adapter ownership

Completion gate:

- Convex host-bridge roots own only Convex behavior plus explicit adaptation to
  shared capabilities; they no longer act as a carrier for another adapter’s
  runtime compatibility layer

### RCAB6 — Runtime-Host Naming, Module-Layout, And Envelope Cleanup

Goal: make the final module tree reflect the real architecture instead of the
intermediate extraction history.

- Rename or reshuffle modules so:
  - `runtime_host/*` reads as provider-neutral capability code
  - adapter-owned runtime shims live under adapter namespaces
  - any shared runtime response envelope has a server-owned neutral name
- Remove transitional names that imply shared ownership where the code is still
  provider-specific
- Keep composition roots thin and concept-owned children explicit

Completion gate:

- the final module tree makes the adapter versus primitive distinction obvious
  without needing historical context

### RCAB7 — Focused Proofs For Cloud Functions, Convex, And Shared Runtime Capabilities

Goal: prove the boundary change instead of only making it look cleaner.

- Add focused tests that prove:
  - Cloud Functions `firebase-admin/firestore` still works end to end through
    the adapter-owned shim
  - shared runtime capabilities work without Convex-namespaced bridge types
  - Convex runtime behavior still works after the host-bridge narrowing
  - the runtime-host layer no longer depends on adapter-owned types in the
    touched surfaces
- Prefer ownership-local proofs and focused lanes over one broad end-to-end
  rerun

Completion gate:

- focused verification exists for the relocated shim, the new shared
  capabilities, and the narrowed Convex boundary

### RCAB8 — Closeout Docs, Plan Index, AGENTS, And Native-Transport Gate Refresh

Goal: leave the repo pointing at the corrected baseline.

- Update `docs/plans/README.md` and any relevant docs so later agents do not
  treat the old boundary as canonical
- Update `AGENTS.md` only as needed to point future adapter/runtime boundary
  work at the completed baseline instead of the wrong historical layer
- Refresh `docs/plans/native-transport-evolution-plan.md` so it reflects this
  cleanup as a prerequisite baseline

Completion gate:

- the repo’s control-plane docs all point at the corrected adapter/runtime
  boundary

## Verification Expectations

Each implementation item should record focused verification before it closes.
Expected lanes include the narrowest proofs that match the touched surface, for
example:

- `cargo test -p neovex-server cloud_functions --lib`
- focused Convex host-bridge lanes under `cargo test -p neovex-server ...`
- `cargo check -p neovex-server`
- `cargo fmt --all --check`
- focused `@neovex/codegen` tests when the Cloud Functions runtime shim surface
  changes

Use broader workspace verification only after the focused boundary proofs are
green.

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-26 | plan authoring | `done` | Created this control-plane plan after the architecture review clarified that `firestore_admin.rs` is a provider-specific runtime compatibility shim, not a shared primitive module. Registered the plan as the next active owner for adapter/runtime boundary cleanup before native transport promotion. No code verification ran because this pass only authored the plan and updated plan-control docs. |
| 2026-04-26 | RCAB1 | `done` | Published `docs/architecture/runtime/adapter-boundary.md` as the canonical ownership reference for adapter-owned runtime shims versus provider-neutral `runtime_host/*` capabilities, and linked it from `docs/README.md`. This records the exact anti-patterns that the runtime-capability wave is correcting without reopening the architecture debate each pass. Verification: reviewed the new reference doc and docs index entries with focused `sed` and `rg` checks. |
| 2026-04-26 | RCAB2 | `done` | Added `crates/neovex-server/src/runtime_host/capabilities.rs` with a provider-neutral `RuntimeCapabilityHost` trait plus shared document-read, async document-read, write-batch execution, and session/cancellation validation helpers. `ConvexHostBridge` now implements that narrow trait, and the Firestore admin shim consumes the generic helpers instead of directly expressing the primitive behavior in Convex terms. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`; `cargo test -p neovex-server adapters::convex::tests::authorization --lib`. |
| 2026-04-26 | RCAB3 | `done` | Moved the covered `firebase-admin/firestore` runtime shim out of `runtime_host/*` and into `crates/neovex-server/src/adapters/cloud_functions/runtime_api/firebase_admin/firestore.rs`, with a new adapter-owned runtime API module tree under `adapters/cloud_functions/`. `runtime_host/*` now retains only the provider-neutral capability layer while the Cloud Functions adapter clearly owns the provider-specific runtime translation shim. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`; `cargo test -p neovex-server adapters::convex::tests::authorization --lib`. |
| 2026-04-26 | RCAB4 | `done` | Replaced the Convex-backed shared wrapper in `runtime_host/mod.rs` with server-owned `RuntimeHostScope`, `RuntimeHostInvocation`, and `RuntimeHostContext` types, and added provider-neutral `runtime_host/abi/document_calls.rs` plus `runtime_host/responses.rs` so shared runtime-host code no longer imports `ConvexHostBridge`, `ConvexRegistry`, or `ConvexRuntimeResponseEnvelope`. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`; `cargo test -p neovex-server adapters::convex::tests::authorization --lib`. |
| 2026-04-26 | RCAB5 | `done` | Narrowed the Convex host bridge back to adapter-owned concerns: Convex runtime-backed invocation now constructs `ConvexHostBridge` directly, while Cloud Functions owns `adapters/cloud_functions/host_bridge.rs` and `adapters/convex/host_bridge/db_ops/mod.rs` no longer dispatches `FirebaseAdminFirestore*` host calls. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`; `cargo test -p neovex-server adapters::convex::tests::authorization --lib`. |
| 2026-04-26 | RCAB6 | `done` | Landed the naming and module-layout cleanup that makes the final tree read honestly: shared runtime primitives now live under neutral `runtime_host/*` modules, provider-specific `firebase-admin/firestore` translation lives under `adapters/cloud_functions/runtime_api/firebase_admin/firestore.rs`, and Cloud Functions now owns its own adapter bridge root. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`. |
| 2026-04-26 | RCAB7 | `done` | Closed the focused proof bundle for the corrected boundary: Cloud Functions compatibility still passes through the adapter-owned Firestore shim, Convex authorization lanes still pass after the bridge narrowing, and the shared runtime-host path compiles and runs without adapter-owned bridge types in the touched surfaces. Verification: `cargo fmt --all --check`; `cargo check -p neovex-server`; `cargo test -p neovex-server cloud_functions --lib`; `cargo test -p neovex-server adapters::convex::tests::authorization --lib`. |
| 2026-04-26 | RCAB8 | `done` | Refreshed the control-plane docs to treat this plan as the latest completed adapter/runtime ownership baseline: updated `docs/plans/README.md`, `AGENTS.md`, `docs/architecture/runtime/adapter-boundary.md`, and `docs/plans/native-transport-evolution-plan.md` so future work starts from the corrected boundary instead of the intermediate extraction history. Verification: focused `sed` / `rg` review of the updated plan index, AGENTS references, runtime-boundary reference, and native-transport gate text. |
